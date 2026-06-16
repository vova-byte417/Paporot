//! StateGraph Builder: Trace → BehaviorStateGraph 三层编排。

use crate::trace::types::BehaviorTrace;
use crate::trajectory::types::{BehaviorStateGraph, StateCandidate};
use crate::trajectory::classifier::RuleBasedClassifier;

use super::segmentation::RuleSegmenter;
use super::window::WindowBuilder;
use super::merge::AdjacentMerger;
use super::transition::build_graph;

/// 从 BehaviorTrace 构建 BehaviorStateGraph。
/// 三层 pipeline: hard segmentation → window candidates → adjacent merge → transition build。
pub fn build_state_graph(trace: &BehaviorTrace) -> BehaviorStateGraph {
    let segmenter = RuleSegmenter::new();
    let window_builder = WindowBuilder::default();
    let merger = AdjacentMerger::default();
    let classifier = RuleBasedClassifier::default();

    // Phase-labeling function per tool
    let phase_fn = |tc: &crate::trace::types::ToolCall| -> std::collections::HashMap<String, f32> {
        let phase = classifier.find_phase_fallback(&tc.tool_name, "other");
        let mut m = std::collections::HashMap::new();
        m.insert(phase, 1.0);
        m
    };

    // Layer 1: Hard segmentation
    let raw_segments = segmenter.cut(trace);

    // Layer 2: Window-based candidate generation
    let mut all_candidates: Vec<StateCandidate> = Vec::new();
    let phase_dist_fn = &phase_fn;
    for (seg_idx, seg) in raw_segments.iter().enumerate() {
        let mut candidates = window_builder.build_candidates(seg, &trace.tool_calls, phase_dist_fn);
        // Assign segment index
        for c in &mut candidates {
            c.segment_idx = seg_idx;
        }
        all_candidates.append(&mut candidates);
    }

    // Layer 3: Adjacent merge
    let states = merger.merge(&all_candidates);

    // Build graph
    build_graph(
        trace.id.clone(),
        trace.session_id.clone(),
        states,
        trace.tool_calls.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::types::{BehaviorTrace, TokenUsage, ToolCall, TraceSource};

    fn tc(name: &str, args: serde_json::Value, id: &str) -> ToolCall {
        ToolCall { id: id.into(), tool_name: name.into(), args, timestamp: "2026-01-01T00:00:00Z".into(), duration_ms: 100, result_id: None }
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

    #[test]
    fn test_build_empty_trace() {
        let trace = make_trace(vec![]);
        let graph = build_state_graph(&trace);
        assert!(graph.states.is_empty());
        assert!(graph.event_log.is_empty());
        assert_eq!(graph.total_tools, 0);
        assert_eq!(graph.trace_id, "t");
    }

    #[test]
    fn test_build_simple_trace() {
        let trace = make_trace(vec![
            tc("read", serde_json::json!({"path":"src/a.rs"}), "c1"),
            tc("read", serde_json::json!({"path":"src/a.rs"}), "c2"),
            tc("edt", serde_json::json!({"path":"src/a.rs"}), "c3"),
            tc("edt", serde_json::json!({"path":"src/a.rs"}), "c4"),
            tc("test", serde_json::json!({}), "c5"),
            tc("test", serde_json::json!({}), "c6"),
            tc("commit", serde_json::json!({}), "c7"),
        ]);
        let graph = build_state_graph(&trace);
        // Should have at least 2-3 states (locate → modify → verify → commit)
        assert!(graph.states.len() >= 2, "Expected at least 2 states, got {}", graph.states.len());
        assert!(graph.event_log.len() >= 1);
        assert_eq!(graph.total_tools, 7);
    }

    #[test]
    fn test_build_single_tool_trace() {
        let trace = make_trace(vec![
            tc("read", serde_json::json!({"path":"src/main.rs"}), "c1"),
        ]);
        let graph = build_state_graph(&trace);
        assert_eq!(graph.states.len(), 1);
        assert!(graph.event_log.is_empty());
        assert_eq!(graph.total_tools, 1);
    }
}
