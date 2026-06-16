//! Layer 3 Graph-level evaluation。
//!
//! 指标: path_length_drift, structural_entropy, stability_trend。

use serde::{Deserialize, Serialize};

use crate::trajectory::types::BehaviorStateGraph;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct GraphMetrics {
    /// 状态路径长度（state count）。
    pub path_length: usize,
    /// 图结构信息熵。
    pub structural_entropy: f32,
    /// 额外的状态数。
    pub state_count: usize,
}

pub fn compute(graph: &BehaviorStateGraph) -> GraphMetrics {
    let state_count = graph.states.len();
    let path_length = state_count;

    // structural_entropy: entropy of edge distribution
    let structural_entropy = if graph.edges.is_empty() {
        0.0
    } else {
        let total = graph.edges.iter().map(|e| e.count).sum::<u32>() as f32;
        let mut entropy = 0.0_f32;
        for edge in &graph.edges {
            let p = edge.count as f32 / total;
            if p > 0.0 {
                entropy -= p * p.log2();
            }
        }
        entropy
    };

    GraphMetrics { path_length, structural_entropy, state_count }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::types::{BehaviorState, BehaviorStateGraph, StateFeatures, TransitionEdge};
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
    fn test_empty_graph() {
        let graph = BehaviorStateGraph {
            trace_id: "t".into(), session_id: "s".into(),
            states: vec![], event_log: vec![], edges: vec![], total_tools: 0,
        };
        let m = compute(&graph);
        assert_eq!(m.path_length, 0);
        assert_eq!(m.structural_entropy, 0.0);
    }

    #[test]
    fn test_with_edges() {
        let graph = BehaviorStateGraph {
            trace_id: "t".into(), session_id: "s".into(),
            states: vec![make_state("s0"), make_state("s1"), make_state("s2")],
            event_log: vec![],
            edges: vec![
                TransitionEdge { from: "s0".into(), to: "s1".into(), count: 2, avg_cost: 1.0 },
                TransitionEdge { from: "s1".into(), to: "s2".into(), count: 1, avg_cost: 1.0 },
            ],
            total_tools: 3,
        };
        let m = compute(&graph);
        assert_eq!(m.path_length, 3);
        assert!(m.structural_entropy > 0.0);
    }
}
