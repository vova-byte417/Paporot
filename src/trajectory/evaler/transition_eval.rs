//! Layer 2 Transition-level evaluation。
//!
//! 指标: oscillation_count, transition_entropy, reversal_ratio。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::trajectory::types::BehaviorStateGraph;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TransitionMetrics {
    /// A→B→A 来回次数。
    pub oscillation_count: usize,
    /// 转移信息熵。
    pub transition_entropy: f32,
    /// 存在反向边（A→B 且 B→A）的比例。
    pub reversal_ratio: f32,
    /// 总转移数（state count - 1）。
    pub total_transitions: usize,
}

pub fn compute(graph: &BehaviorStateGraph) -> TransitionMetrics {
    if graph.states.len() < 2 {
        return TransitionMetrics::default();
    }

    let total = graph.states.len() - 1;

    // oscillation_count: A→B→A 模式
    let mut oscillations = 0;
    for i in 2..graph.states.len() {
        if graph.states[i].id == graph.states[i - 2].id
            && graph.states[i].id != graph.states[i - 1].id
        {
            oscillations += 1;
        }
    }

    // transition_entropy & reversal_ratio
    let mut edge_counts: HashMap<(String, String), usize> = HashMap::new();
    let mut forward_edges: HashMap<(String, String), bool> = HashMap::new();
    for i in 0..total {
        let from = &graph.states[i].id;
        let to = &graph.states[i + 1].id;
        let fwd = (from.clone(), to.clone());
        let rev = (to.clone(), from.clone());
        *edge_counts.entry(fwd.clone()).or_insert(0) += 1;
        forward_edges.entry(fwd.clone()).or_insert(false);
        if forward_edges.contains_key(&rev) {
            *forward_edges.get_mut(&fwd).unwrap() = true;
            *forward_edges.get_mut(&rev).unwrap() = true;
        }
    }

    let total_f = total as f32;
    let mut entropy = 0.0;
    for &count in edge_counts.values() {
        let p = count as f32 / total_f;
        if p > 0.0 {
            entropy -= p * p.log2();
        }
    }

    let reversal_count = forward_edges.values().filter(|&&v| v).count();
    let reversal_ratio = if forward_edges.is_empty() {
        0.0
    } else {
        reversal_count as f32 / forward_edges.len() as f32
    };

    TransitionMetrics {
        oscillation_count: oscillations,
        transition_entropy: entropy,
        reversal_ratio,
        total_transitions: total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::types::{BehaviorState, BehaviorStateGraph, StateFeatures};
    use std::collections::HashMap;

    fn make_state(id: &str) -> BehaviorState {
        let mut pd = HashMap::new();
        pd.insert("other".into(), 1.0);
        BehaviorState {
            id: id.into(), phase_dist: pd, primary_phase: "other".into(),
            features: StateFeatures::default(), stability_score: 0.8, tool_range: (0, 1),
        }
    }

    #[test]
    fn test_no_oscillation() {
        let states = vec![
            make_state("s0"), make_state("s1"), make_state("s2"), make_state("s3"),
        ];
        let graph = BehaviorStateGraph {
            trace_id: "t".into(), session_id: "s".into(),
            states, event_log: vec![], edges: vec![], total_tools: 4,
        };
        let m = compute(&graph);
        assert_eq!(m.oscillation_count, 0);
        assert_eq!(m.total_transitions, 3);
    }

    #[test]
    fn test_with_oscillation() {
        let states = vec![
            make_state("s0"), make_state("s1"), make_state("s0"), make_state("s1"), make_state("s2"),
        ];
        let graph = BehaviorStateGraph {
            trace_id: "t".into(), session_id: "s".into(),
            states, event_log: vec![], edges: vec![], total_tools: 5,
        };
        let m = compute(&graph);
        assert!(m.oscillation_count >= 1);
    }

    #[test]
    fn test_reversal_ratio() {
        let states = vec![
            make_state("s0"), make_state("s1"), make_state("s0"), make_state("s2"),
        ];
        let graph = BehaviorStateGraph {
            trace_id: "t".into(), session_id: "s".into(),
            states, event_log: vec![], edges: vec![], total_tools: 4,
        };
        let m = compute(&graph);
        assert!(m.reversal_ratio > 0.0);
    }
}
