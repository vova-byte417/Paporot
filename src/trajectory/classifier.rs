//! PhaseClassifier trait + RuleBasedClassifier。
//!
//! 将 BehaviorTrace 中的 tool_calls 分类为语义阶段。
//! trait 设计支持未来插拔 LLM / Embedding 分类器。

use serde::{Deserialize, Serialize};

use super::types::{PhaseSegment, ToolIndexInfo};
use crate::trace::types::BehaviorTrace;

/// 将 BehaviorTrace 的 tool_calls 分类为语义阶段。
pub trait PhaseClassifier: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn classify(&self, trace: &BehaviorTrace) -> Vec<PhaseSegment>;
}

/// 阶段映射：阶段名 → 匹配的 tool 名称列表。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseMapping {
    pub phase: String,
    pub tool_names: Vec<String>,
}

/// 基于规则的分类器。
pub struct RuleBasedClassifier {
    pub rules: Vec<PhaseMapping>,
    pub default_phase: String,
}

impl RuleBasedClassifier {
    pub fn new(rules: Vec<PhaseMapping>, default_phase: String) -> Self {
        Self { rules, default_phase }
    }

    /// 使用内置默认规则创建。
    pub fn default() -> Self {
        Self {
            rules: vec![
                PhaseMapping {
                    phase: "定位问题".into(),
                    tool_names: vec![
                        "read", "grep", "glob", "search_codebase",
                        "web_search", "web_fetch", "ls", "list",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                },
                PhaseMapping {
                    phase: "实施修改".into(),
                    tool_names: vec![
                        "write", "edit", "search_replace", "delete_file",
                        "bash", "run_command",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                },
                PhaseMapping {
                    phase: "验证".into(),
                    tool_names: vec![
                        "test", "cargo", "check", "lint", "clippy",
                        "build", "compile",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                },
                PhaseMapping {
                    phase: "提交".into(),
                    tool_names: vec![
                        "commit", "git", "push", "pull_request",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                },
            ],
            default_phase: "其他".into(),
        }
    }

    pub fn default_with_en() -> Self {
        Self {
            rules: vec![
                PhaseMapping {
                    phase: "locate".into(),
                    tool_names: vec![
                        "read", "grep", "glob", "search_codebase",
                        "web_search", "web_fetch", "ls", "list",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                },
                PhaseMapping {
                    phase: "modify".into(),
                    tool_names: vec![
                        "write", "edit", "search_replace", "delete_file",
                        "bash", "run_command",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                },
                PhaseMapping {
                    phase: "verify".into(),
                    tool_names: vec![
                        "test", "cargo", "check", "lint", "clippy",
                        "build", "compile",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                },
                PhaseMapping {
                    phase: "commit".into(),
                    tool_names: vec![
                        "commit", "git", "push", "pull_request",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                },
            ],
            default_phase: "other".into(),
        }
    }

    /// 查找 tool_name 对应的 phase label。
    pub fn find_phase(&self, tool_name: &str) -> String {
        for rule in &self.rules {
            if rule.tool_names.iter().any(|n| n == tool_name) {
                return rule.phase.clone();
            }
        }
        self.default_phase.clone()
    }

    /// 公共 fallback 版本，允许调用方指定自定义 default phase。
    pub fn find_phase_fallback(&self, tool_name: &str, fallback: &str) -> String {
        for rule in &self.rules {
            if rule.tool_names.iter().any(|n| n == tool_name) {
                return rule.phase.clone();
            }
        }
        fallback.to_string()
    }
}

impl PhaseClassifier for RuleBasedClassifier {
    fn name(&self) -> &str {
        "rule_based"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn classify(&self, trace: &BehaviorTrace) -> Vec<PhaseSegment> {
        if trace.tool_calls.is_empty() {
            return Vec::new();
        }

        let mut segments: Vec<PhaseSegment> = Vec::new();
        let mut current_phase = self.find_phase(&trace.tool_calls[0].tool_name);
        let mut current_indices: Vec<ToolIndexInfo> = Vec::new();

        for (i, tc) in trace.tool_calls.iter().enumerate() {
            let phase = self.find_phase(&tc.tool_name);

            if phase != current_phase {
                // 阶段切换：保存当前段，开始新段
                if !current_indices.is_empty() {
                    segments.push(PhaseSegment {
                        label: std::mem::replace(&mut current_phase, phase.clone()),
                        tool_indices: std::mem::take(&mut current_indices),
                    });
                    current_phase = phase;
                }
            }

            current_indices.push(ToolIndexInfo {
                index: i,
                tool_name: tc.tool_name.clone(),
            });
        }

        // 处理最后一段
        if !current_indices.is_empty() {
            segments.push(PhaseSegment {
                label: current_phase,
                tool_indices: current_indices,
            });
        }

        segments
    }
}

// ─── 测试 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::types::{ToolCall, TokenUsage, TraceSource};

    fn make_tool(name: &str, id: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            tool_name: name.into(),
            args: serde_json::json!({}),
            timestamp: "2026-06-12T10:00:00Z".into(),
            duration_ms: 100,
            result_id: None,
        }
    }

    fn make_trace(tools: Vec<ToolCall>) -> BehaviorTrace {
        BehaviorTrace {
            id: "trace_001".into(),
            session_id: "sess_001".into(),
            prompt: "do something".into(),
            tool_calls: tools,
            observations: vec![],
            final_output: "done".into(),
            token_usage: TokenUsage::default(),
            started_at: "2026-06-12T10:00:00Z".into(),
            finished_at: "2026-06-12T10:01:00Z".into(),
            source: TraceSource::Captured {
                agent_name: "test".into(),
            },
            tags: vec![],
            capability_ids: vec![],
            deleted: false,
        }
    }

    #[test]
    fn test_classify_empty_trace() {
        let classifier = RuleBasedClassifier::default();
        let trace = make_trace(vec![]);
        let segments = classifier.classify(&trace);
        assert!(segments.is_empty());
    }

    #[test]
    fn test_classify_single_tool() {
        let classifier = RuleBasedClassifier::default();
        let trace = make_trace(vec![make_tool("read", "call_001")]);
        let segments = classifier.classify(&trace);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].label, "定位问题");
        assert_eq!(segments[0].tool_indices.len(), 1);
        assert_eq!(segments[0].tool_indices[0].tool_name, "read");
    }

    #[test]
    fn test_classify_phase_transition() {
        let classifier = RuleBasedClassifier::default();
        let trace = make_trace(vec![
            make_tool("read", "c1"),
            make_tool("grep", "c2"),
            make_tool("edit", "c3"),      // phase switch here
            make_tool("write", "c4"),
            make_tool("test", "c5"),      // phase switch here
        ]);
        let segments = classifier.classify(&trace);
        assert_eq!(segments.len(), 3, "Expected 3 phases: locate, modify, verify");
        assert_eq!(segments[0].label, "定位问题");
        assert_eq!(segments[1].label, "实施修改");
        assert_eq!(segments[2].label, "验证");
    }

    #[test]
    fn test_classify_unknown_tool() {
        let classifier = RuleBasedClassifier::default();
        let trace = make_trace(vec![make_tool("mystery_tool", "c1")]);
        let segments = classifier.classify(&trace);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].label, "其他");
    }

    #[test]
    fn test_classifier_name_version() {
        let classifier = RuleBasedClassifier::default();
        assert_eq!(classifier.name(), "rule_based");
        assert_eq!(classifier.version(), "1.0.0");
    }

    #[test]
    fn test_classify_consecutive_same_phase() {
        let classifier = RuleBasedClassifier::default();
        let trace = make_trace(vec![
            make_tool("read", "c1"),
            make_tool("grep", "c2"),
            make_tool("ls", "c3"),
        ]);
        let segments = classifier.classify(&trace);
        assert_eq!(segments.len(), 1, "Consecutive same-phase tools stay in one segment");
        assert_eq!(segments[0].tool_indices.len(), 3);
    }

    #[test]
    fn test_classify_alternating_phases() {
        let classifier = RuleBasedClassifier::default();
        let trace = make_trace(vec![
            make_tool("read", "c1"),
            make_tool("edit", "c2"),
            make_tool("grep", "c3"),
            make_tool("write", "c4"),
        ]);
        let segments = classifier.classify(&trace);
        assert_eq!(segments.len(), 4, "Each phase switch creates new segment");
        assert_eq!(segments[0].label, "定位问题");
        assert_eq!(segments[1].label, "实施修改");
        assert_eq!(segments[2].label, "定位问题");
        assert_eq!(segments[3].label, "实施修改");
    }

    #[test]
    fn test_english_classifier() {
        let classifier = RuleBasedClassifier::default_with_en();
        let trace = make_trace(vec![
            make_tool("read", "c1"),
            make_tool("edit", "c2"),
            make_tool("test", "c3"),
            make_tool("commit", "c4"),
        ]);
        let segments = classifier.classify(&trace);
        assert_eq!(segments.len(), 4);
        assert_eq!(segments[0].label, "locate");
        assert_eq!(segments[1].label, "modify");
        assert_eq!(segments[2].label, "verify");
        assert_eq!(segments[3].label, "commit");
    }
}
