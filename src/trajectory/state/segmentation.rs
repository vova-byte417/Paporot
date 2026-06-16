//! Layer 1: 硬边界切割 (irreversible structural segmentation)。
//!
//! 触发规则:
//! - tool type change (tool category switch)
//! - failure → fix loop (verify category → modify category)
//! - idle gap > threshold (10000ms)
//! - file scope jump (path prefix change)

use crate::trace::types::BehaviorTrace;
use crate::trajectory::types::{BoundaryReason, RawSegment};

/// 默认空闲间隔阈值（ms）。
const IDLE_GAP_THRESHOLD_MS: u64 = 10000;

/// 把 tool_name 归入语义类别。
fn tool_category(name: &str) -> &str {
    match name {
        "read" | "grep" | "glob" | "search_codebase" | "web_search" | "web_fetch"
        | "ls" | "list" => "locate",
        "write" | "edit" | "search_replace" | "delete_file" | "bash" | "run_command"
        => "modify",
        "test" | "cargo" | "check" | "lint" | "clippy" | "build" | "compile"
        => "verify",
        "commit" | "git" | "push" | "pull_request" => "commit",
        _ => "other",
    }
}

fn extract_path(args: &serde_json::Value) -> Option<String> {
    args.get("path")
        .or_else(|| args.get("file"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// 提取路径的 scope 前缀（目录级别）。
fn file_scope(path: &str) -> &str {
    if let Some(slash) = path.rfind('/') {
        &path[..slash]
    } else if let Some(bslash) = path.rfind('\\') {
        &path[..bslash]
    } else {
        ""
    }
}

/// Layer 1 硬切段器。
pub struct RuleSegmenter;

impl RuleSegmenter {
    pub fn new() -> Self {
        Self
    }

    /// 将 trace 切分为不可穿透的 RawSegment 序列。
    pub fn cut(&self, trace: &BehaviorTrace) -> Vec<RawSegment> {
        if trace.tool_calls.is_empty() {
            return Vec::new();
        }

        let mut segments: Vec<RawSegment> = Vec::new();
        let mut current_indices: Vec<usize> = Vec::new();
        current_indices.push(0);

        for i in 1..trace.tool_calls.len() {
            let prev = &trace.tool_calls[i - 1];
            let curr = &trace.tool_calls[i];
            let reason = self.check_boundary(prev, curr);

            if let Some(reason) = reason {
                segments.push(RawSegment {
                    tool_indices: std::mem::take(&mut current_indices),
                    boundary_reason: reason,
                });
                // 使用上一次的 reason 作为当前段的起始原因
                // 新段起始索引
            }
            current_indices.push(i);
        }

        // 最后一段
        if !current_indices.is_empty() {
            segments.push(RawSegment {
                tool_indices: current_indices,
                boundary_reason: BoundaryReason::Start,
            });
        }

        segments
    }

    /// 检查两个相邻 tool call 之间是否有硬边界。
    fn check_boundary(
        &self,
        prev: &crate::trace::types::ToolCall,
        curr: &crate::trace::types::ToolCall,
    ) -> Option<BoundaryReason> {
        let cat_prev = tool_category(&prev.tool_name);
        let cat_curr = tool_category(&curr.tool_name);

        // Rule 2: failure → fix loop (verify → modify) — check BEFORE type change
        if cat_prev == "verify" && cat_curr == "modify" {
            if prev.tool_name.contains("test")
                || prev.tool_name.contains("check")
                || prev.tool_name.contains("lint")
            {
                return Some(BoundaryReason::FailureLoop);
            }
        }

        // Rule 1: tool type change
        if cat_prev != cat_curr {
            return Some(BoundaryReason::ToolTypeChange);
        }

        // Rule 3: idle gap
        // Parse timestamps; if they differ by > threshold, cut
        if let (Ok(ts_prev), Ok(ts_curr)) = (
            chrono::DateTime::parse_from_rfc3339(&prev.timestamp),
            chrono::DateTime::parse_from_rfc3339(&curr.timestamp),
        ) {
            let gap_ms = (ts_curr - ts_prev).num_milliseconds() as u64;
            if gap_ms > IDLE_GAP_THRESHOLD_MS {
                return Some(BoundaryReason::IdleGap { ms: gap_ms });
            }
        }

        // Rule 4: file scope jump
        if let (Some(p_prev), Some(p_curr)) = (extract_path(&prev.args), extract_path(&curr.args))
        {
            let scope_prev = file_scope(&p_prev);
            let scope_curr = file_scope(&p_curr);
            if !scope_prev.is_empty()
                && !scope_curr.is_empty()
                && scope_prev != scope_curr
            {
                return Some(BoundaryReason::FileScopeJump);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::types::{BehaviorTrace, TokenUsage, ToolCall, TraceSource};

    fn tc(name: &str, args: serde_json::Value, id: &str, ts: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            tool_name: name.into(),
            args,
            timestamp: ts.into(),
            duration_ms: 100,
            result_id: None,
        }
    }

    fn make_trace(tools: Vec<ToolCall>) -> BehaviorTrace {
        BehaviorTrace {
            id: "t".into(),
            session_id: "s".into(),
            prompt: "x".into(),
            tool_calls: tools,
            observations: vec![],
            final_output: "ok".into(),
            token_usage: TokenUsage::default(),
            started_at: "now".into(),
            finished_at: "now".into(),
            source: TraceSource::Captured { agent_name: "test".into() },
            tags: vec![],
            capability_ids: vec![],
            deleted: false,
        }
    }

    #[test]
    fn test_empty_trace() {
        let s = RuleSegmenter::new();
        let trace = make_trace(vec![]);
        assert!(s.cut(&trace).is_empty());
    }

    #[test]
    fn test_single_tool() {
        let s = RuleSegmenter::new();
        let trace = make_trace(vec![tc("read", serde_json::json!({}), "c1", "2026-01-01T00:00:00Z")]);
        let segs = s.cut(&trace);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].tool_indices.len(), 1);
    }

    #[test]
    fn test_tool_type_change() {
        let s = RuleSegmenter::new();
        let trace = make_trace(vec![
            tc("read", serde_json::json!({}), "c1", "2026-01-01T00:00:00Z"),
            tc("edit", serde_json::json!({}), "c2", "2026-01-01T00:00:01Z"),
        ]);
        let segs = s.cut(&trace);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].boundary_reason, BoundaryReason::ToolTypeChange);
    }

    #[test]
    fn test_failure_loop() {
        let s = RuleSegmenter::new();
        let trace = make_trace(vec![
            tc("test", serde_json::json!({}), "c1", "2026-01-01T00:00:00Z"),
            tc("edit", serde_json::json!({"path":"a.rs"}), "c2", "2026-01-01T00:00:01Z"),
        ]);
        let segs = s.cut(&trace);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].boundary_reason, BoundaryReason::FailureLoop);
    }

    #[test]
    fn test_file_scope_jump() {
        let s = RuleSegmenter::new();
        let trace = make_trace(vec![
            tc("read", serde_json::json!({"path":"src/auth.rs"}), "c1", "2026-01-01T00:00:00Z"),
            tc("read", serde_json::json!({"path":"tests/auth_test.rs"}), "c2", "2026-01-01T00:00:01Z"),
        ]);
        let segs = s.cut(&trace);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].boundary_reason, BoundaryReason::FileScopeJump);
    }

    #[test]
    fn test_idle_gap() {
        let s = RuleSegmenter::new();
        let trace = make_trace(vec![
            tc("read", serde_json::json!({}), "c1", "2026-01-01T00:00:00Z"),
            tc("read", serde_json::json!({}), "c2", "2026-01-01T01:00:00Z"), // 1 hour gap
        ]);
        let segs = s.cut(&trace);
        assert_eq!(segs.len(), 2);
        assert!(matches!(segs[0].boundary_reason, BoundaryReason::IdleGap { .. }));
    }

    #[test]
    fn test_consecutive_same_tool_no_cut() {
        let s = RuleSegmenter::new();
        let trace = make_trace(vec![
            tc("read", serde_json::json!({"path":"src/a.rs"}), "c1", "2026-01-01T00:00:00Z"),
            tc("read", serde_json::json!({"path":"src/a.rs"}), "c2", "2026-01-01T00:00:01Z"),
            tc("grep", serde_json::json!({"path":"src/a.rs"}), "c3", "2026-01-01T00:00:02Z"),
        ]);
        let segs = s.cut(&trace);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].tool_indices.len(), 3);
    }

    #[test]
    fn test_mixed_trace() {
        let s = RuleSegmenter::new();
        let trace = make_trace(vec![
            tc("read", serde_json::json!({"path":"src/main.rs"}), "c1", "2026-01-01T00:00:00Z"),
            tc("grep", serde_json::json!({}), "c2", "2026-01-01T00:00:01Z"),
            tc("edit", serde_json::json!({"path":"src/main.rs"}), "c3", "2026-01-01T00:00:02Z"),
            tc("test", serde_json::json!({}), "c4", "2026-01-01T00:00:03Z"),
            tc("edit", serde_json::json!({"path":"src/main.rs"}), "c5", "2026-01-01T00:00:04Z"),
            tc("test", serde_json::json!({}), "c6", "2026-01-01T00:00:05Z"),
            tc("edit", serde_json::json!({"path":"src/main.rs"}), "c7", "2026-01-01T00:00:06Z"),
            tc("commit", serde_json::json!({}), "c8", "2026-01-01T00:00:07Z"),
        ]);
        let segs = s.cut(&trace);
        // locate → modify → verify(loop) → modify → verify(loop) → modify → commit
        assert!(segs.len() >= 5, "Expected at least 5 segments, got {}", segs.len());
    }
}
