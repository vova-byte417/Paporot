//! Layer 1 State-level evaluation。
//!
//! 指标: phase_entropy, loop_ratio, tool_diversity。

use serde::{Deserialize, Serialize};

use crate::trajectory::types::BehaviorStateGraph;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct StateMetrics {
    /// Phase distribution 的信息熵 −Σ p·log(p)。
    pub phase_entropy: f32,
    /// 连续相同 primary_phase 的占比。
    pub loop_ratio: f32,
    /// 状态数。
    pub state_count: usize,
}

pub fn compute(graph: &BehaviorStateGraph) -> StateMetrics {
    let state_count = graph.states.len();

    if state_count == 0 {
        return StateMetrics::default();
    }

    // phase_entropy: average entropy across all states
    let mut total_entropy = 0.0_f32;
    for state in &graph.states {
        let mut entropy = 0.0_f32;
        for (_phase, &prob) in &state.phase_dist {
            if prob > 0.0 {
                entropy -= prob * prob.log2();
            }
        }
        total_entropy += entropy;
    }
    let phase_entropy = total_entropy / state_count as f32;

    // loop_ratio: consecutive states with same primary_phase
    let mut loops = 0;
    for i in 1..state_count {
        if graph.states[i].primary_phase == graph.states[i - 1].primary_phase {
            loops += 1;
        }
    }
    let loop_ratio = if state_count > 1 {
        loops as f32 / (state_count - 1) as f32
    } else {
        0.0
    };

    StateMetrics { phase_entropy, loop_ratio, state_count }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::types::{BehaviorState, BehaviorStateGraph, StateFeatures};
    use std::collections::HashMap;

    fn make_state(id: &str, phase: &str, entropy_val: f32) -> BehaviorState {
        let mut pd = HashMap::new();
        if entropy_val > 0.0 {
            pd.insert(phase.to_string(), 0.7);
            pd.insert("other".into(), 0.3);
        } else {
            pd.insert(phase.to_string(), 1.0);
        }
        BehaviorState {
            id: id.into(), phase_dist: pd, primary_phase: phase.into(),
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
        assert_eq!(m.state_count, 0);
        assert_eq!(m.phase_entropy, 0.0);
    }

    #[test]
    fn test_single_state() {
        let states = vec![make_state("s0", "locate", 0.0)];
        let graph = BehaviorStateGraph {
            trace_id: "t".into(), session_id: "s".into(),
            states, event_log: vec![], edges: vec![], total_tools: 1,
        };
        let m = compute(&graph);
        assert_eq!(m.state_count, 1);
        assert_eq!(m.loop_ratio, 0.0);
    }

    #[test]
    fn test_loop_detection() {
        let states = vec![
            make_state("s0", "modify", 0.0),
            make_state("s1", "modify", 0.0), // loop
            make_state("s2", "verify", 0.0),
        ];
        let graph = BehaviorStateGraph {
            trace_id: "t".into(), session_id: "s".into(),
            states, event_log: vec![], edges: vec![], total_tools: 3,
        };
        let m = compute(&graph);
        assert!((m.loop_ratio - 0.5).abs() < 0.01);
    }
}
