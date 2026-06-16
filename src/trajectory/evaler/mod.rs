//! v2 State-centric Eval。
//!
//! 三层指标: state / transition / graph。

pub mod state_eval;
pub mod transition_eval;
pub mod graph_eval;

use serde::{Deserialize, Serialize};

use crate::trajectory::types::BehaviorStateGraph;

/// Eval 结果。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StateEvalResult {
    pub trace_id: String,
    pub state_metrics: state_eval::StateMetrics,
    pub transition_metrics: transition_eval::TransitionMetrics,
    pub graph_metrics: graph_eval::GraphMetrics,
    pub verdict: EvalVerdict,
    pub hits: Vec<EvalHit>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum EvalVerdict {
    Pass,
    Watch,
    Degraded,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvalHit {
    pub rule_id: String,
    pub severity: String,
    pub description: String,
}

/// 完整评估：Trace → StateGraph → 三层指标 → 判定。
pub fn evaluate(graph: &BehaviorStateGraph) -> StateEvalResult {
    let sm = state_eval::compute(graph);
    let tm = transition_eval::compute(graph);
    let gm = graph_eval::compute(graph);

    let mut hits: Vec<EvalHit> = Vec::new();

    // State-level rules
    if sm.phase_entropy > 1.5 {
        hits.push(EvalHit {
            rule_id: "SE001".into(),
            severity: "high".into(),
            description: format!("Phase entropy too high ({:.2}), behavior pattern unstable", sm.phase_entropy),
        });
    }
    if sm.loop_ratio > 0.5 {
        hits.push(EvalHit {
            rule_id: "SE002".into(),
            severity: "high".into(),
            description: format!("Loop ratio too high ({:.2}), possible infinite loop", sm.loop_ratio),
        });
    }

    // Transition-level rules
    if tm.oscillation_count > 3 {
        hits.push(EvalHit {
            rule_id: "TE001".into(),
            severity: "medium".into(),
            description: format!("{} oscillation cycles detected, workflow unstable", tm.oscillation_count),
        });
    }
    if tm.reversal_ratio > 0.6 {
        hits.push(EvalHit {
            rule_id: "TE002".into(),
            severity: "high".into(),
            description: format!("High reversal ratio ({:.2}), excessive back-and-forth", tm.reversal_ratio),
        });
    }

    // Graph-level rules
    if gm.state_count < 1 {
        hits.push(EvalHit {
            rule_id: "GE001".into(),
            severity: "high".into(),
            description: "Empty state graph, behavior missing".into(),
        });
    }

    let verdict = if hits.iter().any(|h| h.severity == "high") {
        EvalVerdict::Degraded
    } else if hits.len() >= 2 {
        EvalVerdict::Watch
    } else {
        EvalVerdict::Pass
    };

    StateEvalResult {
        trace_id: graph.trace_id.clone(),
        state_metrics: sm,
        transition_metrics: tm,
        graph_metrics: gm,
        verdict,
        hits,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::types::{BehaviorState, StateFeatures, TransitionEvent};
    use std::collections::HashMap;

    fn make_state(id: &str, phase: &str) -> BehaviorState {
        let mut pd = HashMap::new();
        pd.insert(phase.to_string(), 1.0);
        BehaviorState {
            id: id.into(), phase_dist: pd, primary_phase: phase.into(),
            features: StateFeatures::default(), stability_score: 0.8, tool_range: (0, 1),
        }
    }

    fn make_graph(states: Vec<BehaviorState>) -> BehaviorStateGraph {
        let events: Vec<_> = states.windows(2).enumerate().map(|(i, w)| TransitionEvent {
            from: w[0].id.clone(), to: w[1].id.clone(),
            trigger_tool: "read".into(), timestamp: i as u64,
        }).collect();
        let edges: Vec<_> = events.iter().map(|e| {
            crate::trajectory::types::TransitionEdge {
                from: e.from.clone(), to: e.to.clone(), count: 1, avg_cost: 1.0,
            }
        }).collect();
        BehaviorStateGraph {
            trace_id: "t1".into(), session_id: "s1".into(),
            states, event_log: events, edges, total_tools: 4,
        }
    }

    #[test]
    fn test_evaluate_simple_pass() {
        let states = vec![
            make_state("s0", "locate"),
            make_state("s1", "modify"),
            make_state("s2", "commit"),
        ];
        let graph = make_graph(states);
        let result = evaluate(&graph);
        assert_eq!(result.verdict, EvalVerdict::Pass);
    }

    #[test]
    fn test_evaluate_empty_graph() {
        let graph = BehaviorStateGraph {
            trace_id: "t".into(), session_id: "s".into(),
            states: vec![], event_log: vec![], edges: vec![], total_tools: 0,
        };
        let result = evaluate(&graph);
        assert_eq!(result.verdict, EvalVerdict::Degraded);
    }

    #[test]
    fn test_evaluate_oscillation() {
        // Oscillation: s0→s1→s0→s1→s0→s1→s2 (same state IDs)
        let s0 = make_state("s0", "modify");
        let s1 = make_state("s1", "verify");
        let s2 = make_state("s2", "commit");
        let states = vec![
            s0.clone(), s1.clone(), s0.clone(), s1.clone(), s0.clone(), s1.clone(), s2.clone(),
        ];
        let graph = make_graph(states);
        let result = evaluate(&graph);
        assert!(result.hits.iter().any(|h| h.rule_id == "TE001"),
            "Should detect oscillation");
    }
}
