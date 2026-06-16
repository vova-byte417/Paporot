//! P1 Feature Extractor: 从 BehaviorStateGraph + ToolLog 提取统计特征。
//!
//! 产出: tool/state histogram, transition counts, 三层 entropy。
//! 复用 P0 的 StateFeatures 特征空间，但生成纯测量值（无阈值判断）。
//!
//! D4: tool_entropy → phase_entropy → transition_entropy 是三分辨率分解。

use std::collections::HashMap;

use crate::trace::types::ToolCall;
use crate::trajectory::types::{BehaviorStateGraph, TransitionEvent};

/// 从 P0 图中提取的统计特征快照。
#[derive(Debug, Clone)]
pub struct FeatureSnapshot {
    /// 归一化工具直方图: tool_category → frequency
    pub tool_histogram: HashMap<String, f32>,
    /// 归一化状态分布: state_phase → frequency
    pub state_histogram: HashMap<String, f32>,
    /// 转移计数矩阵: (from_state, to_state) → count
    pub transition_counts: HashMap<(String, String), u32>,
    /// 工具序列熵: H(tool category sequence) — raw event disorder (D4)
    pub tool_entropy: f32,
    /// 状态路径熵: H(state bigram sequence) — path disorder (D1)
    pub phase_entropy: f32,
    /// 转移结构熵: H(aggregated edge distribution) — topology disorder (D4)
    pub transition_entropy: f32,
    /// 每状态工具数均值
    pub avg_tools_per_state: f32,
    /// 状态数
    pub state_count: usize,
    /// 工具总数
    pub total_tools: usize,
}

impl FeatureSnapshot {
    /// 从 BehaviorStateGraph + ToolCall 序列提取所有特征。
    pub fn from_graph_and_tools(graph: &BehaviorStateGraph, tools: &[ToolCall]) -> Self {
        let state_count = graph.states.len();
        let total_tools = graph.total_tools;

        // ── tool histogram ──
        let mut tool_hist: HashMap<String, f32> = HashMap::new();
        for state in &graph.states {
            for (tool, &freq) in &state.features.tool_histogram {
                *tool_hist.entry(tool.clone()).or_insert(0.0) += freq;
            }
        }
        if state_count > 0 {
            let n = state_count as f32;
            for v in tool_hist.values_mut() {
                *v /= n;
            }
        }

        // ── state histogram ──
        let mut state_hist: HashMap<String, f32> = HashMap::new();
        for state in &graph.states {
            *state_hist.entry(state.primary_phase.clone()).or_insert(0.0) += 1.0;
        }
        if state_count > 0 {
            let n = state_count as f32;
            for v in state_hist.values_mut() {
                *v /= n;
            }
        }

        // ── transition counts ──
        let mut transition_counts: HashMap<(String, String), u32> = HashMap::new();
        for ev in &graph.event_log {
            let key = (ev.from.clone(), ev.to.clone());
            *transition_counts.entry(key).or_insert(0) += 1;
        }

        // ── tool_entropy: H(tool category sequence) ── (D4)
        let tool_entropy = compute_tool_entropy(tools);

        // ── phase_entropy: H(state bigram sequence) ── (D1)
        let phase_entropy = compute_phase_entropy(&graph.event_log);

        // ── transition_entropy: H(aggregated edge distribution) ── (D4)
        let transition_entropy = compute_transition_entropy(&graph.event_log);

        // ── avg tools per state ──
        let avg_tools_per_state = if state_count > 0 {
            total_tools as f32 / state_count as f32
        } else {
            0.0
        };

        FeatureSnapshot {
            tool_histogram: tool_hist,
            state_histogram: state_hist,
            transition_counts,
            tool_entropy,
            phase_entropy,
            transition_entropy,
            avg_tools_per_state,
            state_count,
            total_tools,
        }
    }

    /// 将 tool histogram 转为有序 Vec<f32>（按 P0 的 tool_category 顺序）。
    pub fn tool_distribution_vec(&self) -> Vec<f32> {
        let categories = ["locate", "modify", "verify", "commit", "other"];
        categories
            .iter()
            .map(|c| self.tool_histogram.get(*c).copied().unwrap_or(0.0))
            .collect()
    }

    /// 将 state histogram 转为有序 Vec<f32>。
    pub fn state_distribution_vec(&self) -> Vec<f32> {
        let phases = ["locate", "modify", "verify", "commit", "other"];
        phases
            .iter()
            .map(|p| self.state_histogram.get(*p).copied().unwrap_or(0.0))
            .collect()
    }
}

/// 工具类别映射（与 P0 features.rs 保持一致）。
fn tool_category(name: &str) -> String {
    match name {
        "read" | "grep" | "glob" | "search_codebase" | "web_search" | "web_fetch"
        | "ls" | "list" => "locate".into(),
        "write" | "edit" | "search_replace" | "delete_file" | "bash" | "run_command" => {
            "modify".into()
        }
        "test" | "cargo" | "check" | "lint" | "clippy" | "build" | "compile" => "verify".into(),
        "commit" | "git" | "push" | "pull_request" => "commit".into(),
        _ => "other".into(),
    }
}

/// D4: H(tool category sequence) — raw event level.
fn compute_tool_entropy(tools: &[ToolCall]) -> f32 {
    if tools.is_empty() {
        return 0.0;
    }

    let mut cat_counts: HashMap<String, u32> = HashMap::new();
    for tc in tools {
        let cat = tool_category(&tc.tool_name);
        *cat_counts.entry(cat).or_insert(0) += 1;
    }

    let n = tools.len() as f32;
    let mut entropy = 0.0_f32;
    for count in cat_counts.values() {
        let p = *count as f32 / n;
        if p > 0.0 {
            entropy -= p * p.log2();
        }
    }
    entropy
}

/// D1: H(state bigram sequence) from TransitionEventLog.
fn compute_phase_entropy(events: &[TransitionEvent]) -> f32 {
    if events.is_empty() {
        return 0.0;
    }

    let mut bigram_counts: HashMap<(&str, &str), u32> = HashMap::new();
    for ev in events {
        *bigram_counts.entry((&ev.from, &ev.to)).or_insert(0) += 1;
    }

    let n = events.len() as f32;
    let mut entropy = 0.0_f32;
    for count in bigram_counts.values() {
        let p = *count as f32 / n;
        if p > 0.0 {
            entropy -= p * p.log2();
        }
    }
    entropy
}

/// D4: H(aggregated edge distribution) — ignores temporal order.
fn compute_transition_entropy(events: &[TransitionEvent]) -> f32 {
    if events.is_empty() {
        return 0.0;
    }

    // Aggregate: from→to edge count distribution
    let mut edge_counts: HashMap<(&str, &str), u32> = HashMap::new();
    for ev in events {
        *edge_counts.entry((&ev.from, &ev.to)).or_insert(0) += 1;
    }

    let n = events.len() as f32;
    let mut entropy = 0.0_f32;
    for count in edge_counts.values() {
        let p = *count as f32 / n;
        if p > 0.0 {
            entropy -= p * p.log2();
        }
    }
    entropy
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::types::BehaviorState;
    use crate::trajectory::types::StateFeatures;

    fn make_graph(num_states: usize, num_tools: usize, num_events: usize) -> BehaviorStateGraph {
        let mut states = Vec::new();
        let phases = ["locate", "modify", "verify", "commit"];
        for i in 0..num_states {
            let phase = phases[i % phases.len()];
            let mut tool_hist = HashMap::new();
            tool_hist.insert(phase.to_string(), 1.0);
            let mut phase_dist = HashMap::new();
            phase_dist.insert(phase.to_string(), 1.0);
            states.push(BehaviorState {
                id: format!("s{}", i),
                phase_dist,
                primary_phase: phase.to_string(),
                features: StateFeatures {
                    tool_histogram: tool_hist,
                    ..Default::default()
                },
                stability_score: 0.8,
                tool_range: (i * 2, i * 2 + 2),
            });
        }

        let mut event_log = Vec::new();
        for i in 0..num_events.min(num_states * num_states) {
            let from_idx = i % num_states;
            let to_idx = (i + 1) % num_states;
            event_log.push(TransitionEvent {
                from: format!("s{}", from_idx),
                to: format!("s{}", to_idx),
                trigger_tool: "edit".into(),
                timestamp: i as u64 * 100,
            });
        }

        BehaviorStateGraph {
            trace_id: "t1".into(),
            session_id: "s1".into(),
            states,
            event_log,
            edges: Vec::new(),
            total_tools: num_tools,
        }
    }

    fn tc(name: &str, id: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            tool_name: name.into(),
            args: serde_json::json!({}),
            timestamp: "now".into(),
            duration_ms: 100,
            result_id: None,
        }
    }

    #[test]
    fn test_feature_snapshot_basic() {
        let graph = make_graph(4, 24, 4);
        let tools = vec![
            tc("read", "1"),
            tc("edit", "2"),
            tc("test", "3"),
            tc("commit", "4"),
        ];
        let snap = FeatureSnapshot::from_graph_and_tools(&graph, &tools);
        assert_eq!(snap.state_count, 4);
        assert_eq!(snap.total_tools, 24);
        assert!(snap.phase_entropy > 0.0);
        assert!(snap.transition_entropy > 0.0);
        assert!(snap.tool_entropy > 0.0);
    }

    #[test]
    fn test_feature_snapshot_empty() {
        let graph = make_graph(0, 0, 0);
        let snap = FeatureSnapshot::from_graph_and_tools(&graph, &[]);
        assert_eq!(snap.state_count, 0);
        assert_eq!(snap.tool_entropy, 0.0);
        assert_eq!(snap.phase_entropy, 0.0);
        assert_eq!(snap.transition_entropy, 0.0);
    }

    #[test]
    fn test_tool_entropy_uniform() {
        let tools = vec![
            tc("read", "1"),
            tc("edit", "2"),
            tc("test", "3"),
            tc("commit", "4"),
        ];
        // 4 different categories → max entropy = log2(4) = 2.0
        let e = compute_tool_entropy(&tools);
        assert!((e - 2.0).abs() < 0.01, "Expected ~2.0, got {}", e);
    }

    #[test]
    fn test_tool_entropy_single_category() {
        let tools = vec![tc("read", "1"), tc("read", "2"), tc("read", "3")];
        let e = compute_tool_entropy(&tools);
        assert!((e - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_phase_entropy_linear() {
        let events = vec![
            TransitionEvent { from: "s0".into(), to: "s1".into(), trigger_tool: "e".into(), timestamp: 0 },
            TransitionEvent { from: "s1".into(), to: "s2".into(), trigger_tool: "e".into(), timestamp: 1 },
            TransitionEvent { from: "s2".into(), to: "s3".into(), trigger_tool: "e".into(), timestamp: 2 },
        ];
        let e = compute_phase_entropy(&events);
        // 3 unique bigrams → log2(3) ≈ 1.585
        assert!((e - 1.585).abs() < 0.01, "Expected ~1.585, got {}", e);
    }

    #[test]
    fn test_phase_entropy_repeated() {
        let events = vec![
            TransitionEvent { from: "s0".into(), to: "s1".into(), trigger_tool: "e".into(), timestamp: 0 },
            TransitionEvent { from: "s0".into(), to: "s1".into(), trigger_tool: "e".into(), timestamp: 1 },
            TransitionEvent { from: "s0".into(), to: "s1".into(), trigger_tool: "e".into(), timestamp: 2 },
        ];
        let e = compute_phase_entropy(&events);
        // 1 unique bigram → 0.0
        assert!((e - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_transition_entropy_equals_phase_for_simple() {
        let events = vec![
            TransitionEvent { from: "s0".into(), to: "s1".into(), trigger_tool: "e".into(), timestamp: 0 },
            TransitionEvent { from: "s1".into(), to: "s2".into(), trigger_tool: "e".into(), timestamp: 1 },
        ];
        // phase and transition entropy are computed identically on edge distribution
        // for this case (no temporal collapse needed since all edges unique)
        let pe = compute_phase_entropy(&events);
        let te = compute_transition_entropy(&events);
        assert!((pe - te).abs() < 0.01);
    }

    #[test]
    fn test_distribution_vecs() {
        let graph = make_graph(4, 24, 4);
        let tools = vec![tc("read", "1")];
        let snap = FeatureSnapshot::from_graph_and_tools(&graph, &tools);
        let td = snap.tool_distribution_vec();
        let sd = snap.state_distribution_vec();
        assert_eq!(td.len(), 5);
        assert_eq!(sd.len(), 5);
    }
}
