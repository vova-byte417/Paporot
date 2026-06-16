//! Layer 2: 窗口化 StateCandidate 构造。
//!
//! 在每个 RawSegment 内做 sliding window，生成 StateCandidate。
//! window size: 3–8, stride: 1–3。不跨 segment 边界。

use crate::trace::types::ToolCall;
use crate::trajectory::types::{RawSegment, StateCandidate};

use super::features::extract_features;

/// 默认 window 大小范围。
const WINDOW_MIN: usize = 3;
const WINDOW_MAX: usize = 8;

/// Window builder：在每个 segment 内生成 StateCandidate。
pub struct WindowBuilder {
    pub window_min: usize,
    pub window_max: usize,
    pub stride: usize,
}

impl Default for WindowBuilder {
    fn default() -> Self {
        Self {
            window_min: WINDOW_MIN,
            window_max: WINDOW_MAX,
            stride: 2, // default stride
        }
    }
}

impl WindowBuilder {
    pub fn new(window_min: usize, window_max: usize, stride: usize) -> Self {
        Self { window_min, window_max, stride }
    }

    /// 从 RawSegment 和完整的 tool calls 生成 StateCandidate 列表。
    pub fn build_candidates(
        &self,
        segment: &RawSegment,
        all_tools: &[ToolCall],
        phase_dist_fn: &dyn Fn(&ToolCall) -> std::collections::HashMap<String, f32>,
    ) -> Vec<StateCandidate> {
        if segment.tool_indices.is_empty() {
            return Vec::new();
        }

        let indices = &segment.tool_indices;
        let n = indices.len();
        let window_size = self.window_max.min(n);

        if window_size < self.window_min {
            // 段太短，直接作为一个 candidate
            let tools: Vec<_> = indices.iter().map(|&i| &all_tools[i]).collect::<Vec<_>>();
            let tools_owned: Vec<_> = tools.iter().map(|&t| t.clone()).collect();
            let features = extract_features(&tools_owned, all_tools);
            let phase_dist = compute_phase_dist(&tools, phase_dist_fn);
            return vec![StateCandidate {
                segment_idx: 0,
                tool_indices: indices.clone(),
                features,
                phase_dist,
            }];
        }

        let mut candidates: Vec<StateCandidate> = Vec::new();
        let mut start = 0;

        while start + self.window_min <= n {
            let end = (start + window_size).min(n);
            let window_indices: Vec<usize> = indices[start..end].to_vec();
            let tools: Vec<_> = window_indices.iter().map(|&i| &all_tools[i]).collect::<Vec<_>>();
            let tools_owned: Vec<_> = tools.iter().map(|&t| t.clone()).collect();
            let features = extract_features(&tools_owned, all_tools);
            let phase_dist = compute_phase_dist(&tools, phase_dist_fn);

            candidates.push(StateCandidate {
                segment_idx: candidates.len(),
                tool_indices: window_indices,
                features,
                phase_dist,
            });

            start += self.stride;
            if start >= n {
                break;
            }
        }

        candidates
    }
}

/// 从 tool calls 计算 phase distribution。
fn compute_phase_dist(
    tools: &[&ToolCall],
    phase_fn: &dyn Fn(&ToolCall) -> std::collections::HashMap<String, f32>,
) -> std::collections::HashMap<String, f32> {
    let n = tools.len() as f32;
    if n == 0.0 {
        return std::collections::HashMap::new();
    }

    // Aggregate all individual tool distributions
    let mut merged: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
    for tool in tools {
        let dist = phase_fn(tool);
        for (phase, prob) in dist {
            *merged.entry(phase).or_insert(0.0) += prob;
        }
    }

    // Normalize
    for v in merged.values_mut() {
        *v /= n;
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::types::{BehaviorTrace, TokenUsage, ToolCall, TraceSource};
    use crate::trajectory::types::BoundaryReason;

    fn tc(name: &str, id: &str) -> ToolCall {
        ToolCall { id: id.into(), tool_name: name.into(), args: serde_json::json!({}), timestamp: "now".into(), duration_ms: 100, result_id: None }
    }

    fn make_trace(tools: Vec<ToolCall>) -> BehaviorTrace {
        BehaviorTrace {
            id: "t".into(), session_id: "s".into(), prompt: "x".into(),
            tool_calls: tools, observations: vec![],
            final_output: "ok".into(), token_usage: TokenUsage::default(),
            started_at: "now".into(), finished_at: "now".into(),
            source: TraceSource::Captured { agent_name: "test".into() },
            tags: vec![], capability_ids: vec![], deleted: false,
        }
    }

    fn default_phase_fn(t: &ToolCall) -> std::collections::HashMap<String, f32> {
        let phase = match t.tool_name.as_str() {
            "read" | "grep" | "ls" => "locate",
            "edit" | "write" | "delete" => "modify",
            "test" | "check" | "lint" => "verify",
            "commit" | "push" => "commit",
            _ => "other",
        };
        let mut m = std::collections::HashMap::new();
        m.insert(phase.to_string(), 1.0);
        m
    }

    #[test]
    fn test_window_within_boundary() {
        let trace = make_trace(vec![
            tc("read", "c1"), tc("read", "c2"), tc("grep", "c3"),
            tc("edit", "c4"), // boundary
            tc("edit", "c5"), tc("write", "c6"), tc("delete", "c7"),
        ]);
        let seg = RawSegment {
            tool_indices: vec![3, 4, 5, 6],  // only modify tools
            boundary_reason: BoundaryReason::ToolTypeChange,
        };

        let wb = WindowBuilder::new(3, 8, 2);
        let candidates = wb.build_candidates(&seg, &trace.tool_calls, &default_phase_fn);
        assert!(!candidates.is_empty());
        // All tool indices should be within [3, 6]
        for c in &candidates {
            for &idx in &c.tool_indices {
                assert!(idx >= 3 && idx <= 6,
                    "tool index {} outside segment boundary", idx);
            }
        }
    }

    #[test]
    fn test_small_segment_one_candidate() {
        let trace = make_trace(vec![
            tc("read", "c1"), tc("edit", "c2"), // boundary
        ]);
        let seg = RawSegment {
            tool_indices: vec![1],
            boundary_reason: BoundaryReason::ToolTypeChange,
        };
        let wb = WindowBuilder::default();
        let candidates = wb.build_candidates(&seg, &trace.tool_calls, &default_phase_fn);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].tool_indices, vec![1]);
    }

    #[test]
    fn test_stride_behavior() {
        let tools: Vec<_> = (0..10).map(|i| tc("edit", &format!("c{}", i))).collect();
        let trace = make_trace(tools);
        let seg = RawSegment {
            tool_indices: (0..10).collect(),
            boundary_reason: BoundaryReason::Start,
        };
        let wb = WindowBuilder::new(3, 5, 3);
        let candidates = wb.build_candidates(&seg, &trace.tool_calls, &default_phase_fn);
        // With stride 3 and 10 items: windows start at 0, 3, 6 → 3 candidates
        assert!(candidates.len() >= 2);
    }

    #[test]
    fn test_phase_distribution() {
        let t1 = tc("read", "c1");
        let t2 = tc("edit", "c2");
        let t3 = tc("read", "c3");
        let t4 = tc("test", "c4");
        let tools = vec![&t1, &t2, &t3, &t4];
        let dist = compute_phase_dist(&tools, &default_phase_fn);
        assert_eq!(dist.get("locate"), Some(&0.5));
        assert_eq!(dist.get("modify"), Some(&0.25));
        assert_eq!(dist.get("verify"), Some(&0.25));
    }
}
