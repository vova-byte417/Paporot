//! TrajectoryDiff 兼容投影层。
//!
//! 将 BehaviorStateGraph 单向转换为 TrajectoryDiff。
//! StateGraph → Diff (不可逆)。

use crate::trajectory::types::*;
use crate::trajectory::types as v1;

/// 从 BehaviorStateGraph 降级生成 TrajectoryDiff(v1 兼容)。
pub fn state_graph_to_diff(
    graph_a: &BehaviorStateGraph,
    graph_b: &BehaviorStateGraph,
    capability_id: Option<String>,
) -> v1::TrajectoryDiff {
    let tool_count_a = graph_a.total_tools;
    let tool_count_b = graph_b.total_tools;

    // Convert states to SegmentDiff
    let mut segments: Vec<v1::SegmentDiff> = Vec::new();
    let mut seg_added = 0;
    let mut seg_deleted = 0;
    let mut seg_modified = 0;
    let mut seg_unchanged = 0;
    let mut tool_added = 0;
    let mut tool_deleted = 0;
    let mut tool_unchanged = 0;

    // Only state_a states → align to state_b
    let max_states = graph_a.states.len().max(graph_b.states.len());

    for i in 0..max_states {
        let state_a = graph_a.states.get(i);
        let state_b = graph_b.states.get(i);

        match (state_a, state_b) {
            (Some(sa), Some(sb)) => {
                let kind = if sa.primary_phase == sb.primary_phase {
                    seg_unchanged += 1;
                    v1::SegmentKind::Unchanged
                } else {
                    seg_modified += 1;
                    v1::SegmentKind::Modified
                };

                let tools_a = (sa.tool_range.1 - sa.tool_range.0).min(tool_count_a);
                let tools_b = (sb.tool_range.1 - sb.tool_range.0).min(tool_count_b);
                let n = tools_a.max(tools_b);

                let mut tool_diffs = Vec::new();
                for j in 0..n {
                    if j < tools_a && j < tools_b {
                        tool_unchanged += 1;
                        tool_diffs.push(v1::ToolDiff {
                            tool_name: "".into(),
                            kind: v1::ToolDiffKind::Unchanged,
                            index_a: Some(sa.tool_range.0 + j),
                            index_b: Some(sb.tool_range.0 + j),
                            args_diff: None,
                            duration_ms: 0,
                        });
                    } else if j >= tools_a {
                        tool_added += 1;
                        tool_diffs.push(v1::ToolDiff {
                            tool_name: "".into(),
                            kind: v1::ToolDiffKind::Added,
                            index_a: None,
                            index_b: Some(sb.tool_range.0 + j),
                            args_diff: None,
                            duration_ms: 0,
                        });
                    } else {
                        tool_deleted += 1;
                        tool_diffs.push(v1::ToolDiff {
                            tool_name: "".into(),
                            kind: v1::ToolDiffKind::Deleted,
                            index_a: Some(sa.tool_range.0 + j),
                            index_b: None,
                            args_diff: None,
                            duration_ms: 0,
                        });
                    }
                }

                segments.push(v1::SegmentDiff {
                    label: format!("{}/{}", sa.primary_phase, sb.primary_phase),
                    kind,
                    tool_diffs,
                    index_a: Some(i),
                    index_b: Some(i),
                });
            }
            (Some(_sa), None) => {
                seg_deleted += 1;
                segments.push(v1::SegmentDiff {
                    label: "deleted".into(),
                    kind: v1::SegmentKind::Deleted,
                    tool_diffs: vec![],
                    index_a: Some(i),
                    index_b: None,
                });
            }
            (None, Some(_sb)) => {
                seg_added += 1;
                segments.push(v1::SegmentDiff {
                    label: "added".into(),
                    kind: v1::SegmentKind::Added,
                    tool_diffs: vec![],
                    index_a: None,
                    index_b: Some(i),
                });
            }
            (None, None) => unreachable!(),
        }
    }

    let token_delta = 0i64; // not tracked in state graph

    v1::TrajectoryDiff {
        capability_id,
        version_a: v1::TrajectoryVersion {
            trace_id: graph_a.trace_id.clone(),
            session_id: graph_a.session_id.clone(),
            tool_count: tool_count_a,
            duration_ms: 0,
            total_tokens: 0,
            started_at: "".into(),
        },
        version_b: v1::TrajectoryVersion {
            trace_id: graph_b.trace_id.clone(),
            session_id: graph_b.session_id.clone(),
            tool_count: tool_count_b,
            duration_ms: 0,
            total_tokens: 0,
            started_at: "".into(),
        },
        segments,
        summary: v1::DiffSummary {
            segments_added: seg_added,
            segments_deleted: seg_deleted,
            segments_modified: seg_modified,
            segments_unchanged: seg_unchanged,
            tool_calls_added: tool_added,
            tool_calls_deleted: tool_deleted,
            tool_calls_modified: 0,
            tool_calls_unchanged: tool_unchanged,
            token_delta,
            duration_delta_ms: 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::types::{BehaviorState, StateFeatures, TransitionEvent, TransitionEdge};
    use std::collections::HashMap;

    fn make_graph(id: &str, state_phases: &[&str]) -> BehaviorStateGraph {
        let states: Vec<_> = state_phases.iter().enumerate().map(|(i, &phase)| {
            let mut pd = HashMap::new();
            pd.insert(phase.to_string(), 1.0);
            BehaviorState {
                id: format!("s{}", i),
                phase_dist: pd,
                primary_phase: phase.into(),
                features: StateFeatures::default(),
                stability_score: 0.8,
                tool_range: (i, i + 1),
            }
        }).collect();

        let event_log: Vec<TransitionEvent> = states.windows(2).enumerate().map(|(i, w)| TransitionEvent {
            from: w[0].id.clone(), to: w[1].id.clone(),
            trigger_tool: "read".into(), timestamp: i as u64,
        }).collect();

        let edges = event_log.iter().map(|e| TransitionEdge {
            from: e.from.clone(), to: e.to.clone(), count: 1, avg_cost: 1.0,
        }).collect();

        BehaviorStateGraph {
            trace_id: id.into(), session_id: format!("sess_{}", id),
            states, event_log, edges,
            total_tools: state_phases.len(),
        }
    }

    #[test]
    fn test_identical_graphs() {
        let ga = make_graph("ta", &["locate", "modify", "commit"]);
        let gb = make_graph("tb", &["locate", "modify", "commit"]);
        let diff = state_graph_to_diff(&ga, &gb, None);

        assert_eq!(diff.version_a.tool_count, 3);
        assert_eq!(diff.version_b.tool_count, 3);
        assert!(diff.summary.segments_unchanged > 0);
        assert_eq!(diff.summary.segments_added, 0);
        assert_eq!(diff.summary.segments_deleted, 0);
    }

    #[test]
    fn test_graph_with_changes() {
        let ga = make_graph("ta", &["locate", "modify", "commit"]);
        let gb = make_graph("tb", &["locate", "verify", "modify", "commit"]);
        let diff = state_graph_to_diff(&ga, &gb, Some("cap_001".into()));

        assert_eq!(diff.capability_id, Some("cap_001".into()));
        assert_eq!(diff.version_a.tool_count, 3);
        assert_eq!(diff.version_b.tool_count, 4);
    }
}
