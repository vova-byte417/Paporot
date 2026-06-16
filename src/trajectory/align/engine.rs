//! 对齐引擎：编排 segment → tool 双层对齐。

use super::segment::match_segments;
use super::scorer::AlignmentCosts;
use super::tool::align_tools;
use crate::trace::types::BehaviorTrace;
use crate::trajectory::classifier::PhaseClassifier;
use crate::trajectory::hash::semantic_hashes;
use crate::trajectory::types::{
    DiffSummary, SegmentDiff, SegmentKind, ToolDiff, ToolDiffKind, TrajectoryDiff,
    TrajectoryVersion,
};

/// 双层对齐引擎。
pub struct AlignmentEngine {
    #[allow(dead_code)]
    costs: AlignmentCosts,
}

impl AlignmentEngine {
    pub fn new(costs: AlignmentCosts) -> Self {
        Self { costs }
    }

    /// 计算两条 BehaviorTrace 的 TrajectoryDiff。
    pub fn diff(
        &self,
        classifier: &dyn PhaseClassifier,
        trace_a: &BehaviorTrace,
        trace_b: &BehaviorTrace,
        capability_id: Option<String>,
    ) -> TrajectoryDiff {
        // Step 1: Phase classification
        let segments_a = classifier.classify(trace_a);
        let segments_b = classifier.classify(trace_b);

        // Step 2: Segment-level alignment
        let segment_matches = match_segments(&segments_a, &segments_b);

        // Precompute hashes
        let hashes_a = semantic_hashes(&trace_a.tool_calls);
        let hashes_b = semantic_hashes(&trace_b.tool_calls);

        // Step 3: Tool-level alignment per segment
        let mut segment_diffs: Vec<SegmentDiff> = Vec::new();
        let mut summary = DiffSummary::default();

        for sm in &segment_matches {
            let tool_diffs = match sm.kind {
                SegmentKind::Unchanged | SegmentKind::Modified => {
                    let indices_a = &segments_a[sm.index_a.unwrap()].tool_indices;
                    let indices_b = &segments_b[sm.index_b.unwrap()].tool_indices;

                    let subs_a: Vec<_> = indices_a.iter().map(|ti| trace_a.tool_calls[ti.index].clone()).collect();
                    let subs_b: Vec<_> = indices_b.iter().map(|ti| trace_b.tool_calls[ti.index].clone()).collect();
                    let sub_hashes_a: Vec<_> = indices_a.iter().map(|ti| hashes_a[ti.index]).collect();
                    let sub_hashes_b: Vec<_> = indices_b.iter().map(|ti| hashes_b[ti.index]).collect();

                    let diffs = align_tools(&subs_a, &subs_b, &sub_hashes_a, &sub_hashes_b);

                    // Determine if the segment is unchanged or modified
                    let all_unchanged = diffs.iter().all(|d| d.kind == ToolDiffKind::Unchanged);
                    if all_unchanged && sm.kind == SegmentKind::Unchanged {
                        summary.segments_unchanged += 1;
                    } else {
                        summary.segments_modified += 1;
                    }

                    diffs
                }
                SegmentKind::Added => {
                    // SAFETY: Added segments always have index_b
                    let indices = &segments_b[sm.index_b.unwrap()].tool_indices;
                    summary.segments_added += 1;
                    indices.iter().map(|ti| ToolDiff {
                        tool_name: trace_b.tool_calls[ti.index].tool_name.clone(),
                        kind: ToolDiffKind::Added,
                        index_a: None,
                        index_b: Some(ti.index),
                        args_diff: None,
                        duration_ms: trace_b.tool_calls[ti.index].duration_ms,
                    }).collect()
                }
                SegmentKind::Deleted => {
                    // SAFETY: Deleted segments always have index_a
                    let indices = &segments_a[sm.index_a.unwrap()].tool_indices;
                    summary.segments_deleted += 1;
                    indices.iter().map(|ti| ToolDiff {
                        tool_name: trace_a.tool_calls[ti.index].tool_name.clone(),
                        kind: ToolDiffKind::Deleted,
                        index_a: Some(ti.index),
                        index_b: None,
                        args_diff: None,
                        duration_ms: trace_a.tool_calls[ti.index].duration_ms,
                    }).collect()
                }
            };

            // Count tool-level diffs
            for td in &tool_diffs {
                match td.kind {
                    ToolDiffKind::Unchanged => summary.tool_calls_unchanged += 1,
                    ToolDiffKind::Added => summary.tool_calls_added += 1,
                    ToolDiffKind::Deleted => summary.tool_calls_deleted += 1,
                    ToolDiffKind::ArgsChanged => summary.tool_calls_modified += 1,
                }
            }

            let actual_kind = if sm.kind == SegmentKind::Unchanged
                && tool_diffs.iter().any(|d| d.kind != ToolDiffKind::Unchanged)
            {
                SegmentKind::Modified
            } else {
                sm.kind.clone()
            };

            segment_diffs.push(SegmentDiff {
                label: sm.label.clone(),
                kind: actual_kind,
                tool_diffs,
                index_a: sm.index_a,
                index_b: sm.index_b,
            });
        }

        // Compute summary deltas
        let tokens_a = trace_a.token_usage.input_tokens + trace_a.token_usage.output_tokens;
        let tokens_b = trace_b.token_usage.input_tokens + trace_b.token_usage.output_tokens;
        summary.token_delta = tokens_b as i64 - tokens_a as i64;

        let duration_a = trace_a.tool_calls.iter().map(|t| t.duration_ms).sum::<u64>();
        let duration_b = trace_b.tool_calls.iter().map(|t| t.duration_ms).sum::<u64>();
        summary.duration_delta_ms = duration_b as i64 - duration_a as i64;

        TrajectoryDiff {
            capability_id,
            version_a: TrajectoryVersion {
                trace_id: trace_a.id.clone(),
                session_id: trace_a.session_id.clone(),
                tool_count: trace_a.tool_calls.len(),
                duration_ms: duration_a,
                total_tokens: tokens_a,
                started_at: trace_a.started_at.clone(),
            },
            version_b: TrajectoryVersion {
                trace_id: trace_b.id.clone(),
                session_id: trace_b.session_id.clone(),
                tool_count: trace_b.tool_calls.len(),
                duration_ms: duration_b,
                total_tokens: tokens_b,
                started_at: trace_b.started_at.clone(),
            },
            segments: segment_diffs,
            summary,
        }
    }
}

impl Default for AlignmentEngine {
    fn default() -> Self {
        Self::new(AlignmentCosts::default())
    }
}

// ─── 测试 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::types::{Observation, TokenUsage, ToolCall, TraceSource};
    use crate::trajectory::classifier::RuleBasedClassifier;

    fn make_tool(name: &str, args: serde_json::Value, id: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            tool_name: name.into(),
            args,
            timestamp: "2026-06-12T10:00:00Z".into(),
            duration_ms: 100,
            result_id: None,
        }
    }

    fn make_trace(id: &str, tools: Vec<ToolCall>) -> BehaviorTrace {
        let total = tools.iter().map(|t| t.duration_ms).sum::<u64>();
        BehaviorTrace {
            id: id.into(),
            session_id: format!("sess_{}", id),
            prompt: "do something".into(),
            tool_calls: tools,
            observations: vec![],
            final_output: "done".into(),
            token_usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                ..Default::default()
            },
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
    fn test_engine_full_diff() {
        let engine = AlignmentEngine::default();
        let classifier = RuleBasedClassifier::default();

        let trace_a = make_trace("trace_a", vec![
            make_tool("read", serde_json::json!({"path": "src/main.rs"}), "c1"),
            make_tool("edit", serde_json::json!({"path": "src/main.rs"}), "c2"),
            make_tool("test", serde_json::json!({}), "c3"),
        ]);
        let trace_b = make_trace("trace_b", vec![
            make_tool("read", serde_json::json!({"path": "src/main.rs"}), "d1"),
            make_tool("test", serde_json::json!({}), "d2"),
            make_tool("edit", serde_json::json!({"path": "src/main.rs"}), "d3"),
            make_tool("commit", serde_json::json!({}), "d4"),
        ]);

        let diff = engine.diff(&classifier, &trace_a, &trace_b, Some("cap_001".into()));

        assert_eq!(diff.capability_id, Some("cap_001".into()));
        assert_eq!(diff.version_a.tool_count, 3);
        assert_eq!(diff.version_b.tool_count, 4);
        assert!(!diff.segments.is_empty());
        assert!(diff.summary.tool_calls_added + diff.summary.tool_calls_unchanged
            + diff.summary.tool_calls_deleted + diff.summary.tool_calls_modified > 0);
    }

    #[test]
    fn test_engine_empty_traces() {
        let engine = AlignmentEngine::default();
        let classifier = RuleBasedClassifier::default();

        let trace_a = make_trace("trace_a", vec![]);
        let trace_b = make_trace("trace_b", vec![]);

        let diff = engine.diff(&classifier, &trace_a, &trace_b, None);

        assert!(diff.segments.is_empty());
        assert_eq!(diff.summary.segments_added, 0);
        assert_eq!(diff.summary.tool_calls_unchanged, 0);
    }

    #[test]
    fn test_engine_identical_traces() {
        let engine = AlignmentEngine::default();
        let classifier = RuleBasedClassifier::default();

        let tools = vec![
            make_tool("read", serde_json::json!({"path": "a.rs"}), "c1"),
            make_tool("edit", serde_json::json!({"path": "a.rs"}), "c2"),
        ];
        let trace_a = make_trace("trace_a", tools.clone());
        let trace_b = make_trace("trace_b", tools.clone());

        let diff = engine.diff(&classifier, &trace_a, &trace_b, None);

        assert_eq!(diff.summary.tool_calls_unchanged, 2);
        assert_eq!(diff.summary.tool_calls_added, 0);
        assert_eq!(diff.summary.tool_calls_deleted, 0);
    }
}
