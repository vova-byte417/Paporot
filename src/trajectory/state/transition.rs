//! Transition 模型：Event Log (Layer A) + Aggregated Graph (Layer B)。

use std::collections::HashMap;

use crate::trajectory::types::{BehaviorState, BehaviorStateGraph, TransitionEdge, TransitionEvent};

/// 从 BehaviorState 列表生成 TransitionEvent 序列。
pub fn build_event_log(states: &[BehaviorState]) -> Vec<TransitionEvent> {
    let mut events = Vec::new();
    if states.len() < 2 {
        return events;
    }

    for i in 0..states.len() - 1 {
        let from = &states[i];
        let to = &states[i + 1];

        // trigger_tool: 使用 to 状态中第一个工具的 primary tool category
        let trigger_tool = if to.phase_dist.contains_key("locate") {
            "read".into()
        } else if to.phase_dist.contains_key("modify") {
            "edit".into()
        } else if to.phase_dist.contains_key("verify") {
            "test".into()
        } else if to.phase_dist.contains_key("commit") {
            "commit".into()
        } else {
            "unknown".into()
        };

        events.push(TransitionEvent {
            from: from.id.clone(),
            to: to.id.clone(),
            trigger_tool,
            timestamp: i as u64,
        });
    }

    events
}

/// 从 TransitionEvent 序列聚合生成 TransitionEdge 列表。
pub fn aggregate_edges(events: &[TransitionEvent]) -> Vec<TransitionEdge> {
    let mut edge_map: HashMap<(String, String), (u32, f32, u32)> = HashMap::new();

    for (_, event) in events.iter().enumerate() {
        let key = (event.from.clone(), event.to.clone());
        let entry = edge_map.entry(key).or_insert((0, 0.0, 0));
        entry.0 += 1; // count
        entry.2 += 1; // temp: used for avg
        // cost is approximated by indegree (transition frequency)
    }

    let mut edges: Vec<TransitionEdge> = Vec::new();
    for ((from, to), (count, _, _)) in edge_map {
        let avg_cost = count as f32; // simplified cost
        edges.push(TransitionEdge { from, to, count, avg_cost });
    }

    edges
}

/// 构建完整的 BehaviorStateGraph。
pub fn build_graph(
    trace_id: String,
    session_id: String,
    states: Vec<BehaviorState>,
    total_tools: usize,
) -> BehaviorStateGraph {
    let event_log = build_event_log(&states);
    let edges = aggregate_edges(&event_log);

    BehaviorStateGraph {
        trace_id,
        session_id,
        states,
        event_log,
        edges,
        total_tools,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::types::{StateFeatures, BehaviorState};
    use std::collections::HashMap;

    fn make_state(id: &str, phase: &str) -> BehaviorState {
        let mut pd = HashMap::new();
        pd.insert(phase.to_string(), 1.0);
        BehaviorState {
            id: id.into(),
            phase_dist: pd,
            primary_phase: phase.into(),
            features: StateFeatures::default(),
            stability_score: 0.8,
            tool_range: (0, 1),
        }
    }

    #[test]
    fn test_empty_event_log() {
        let events = build_event_log(&[]);
        assert!(events.is_empty());
    }

    #[test]
    fn test_single_state_no_events() {
        let states = vec![make_state("s0", "locate")];
        let events = build_event_log(&states);
        assert!(events.is_empty());
    }

    #[test]
    fn test_event_log_sequence() {
        let states = vec![
            make_state("s0", "locate"),
            make_state("s1", "modify"),
            make_state("s2", "verify"),
            make_state("s3", "commit"),
        ];
        let events = build_event_log(&states);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].from, "s0");
        assert_eq!(events[0].to, "s1");
        assert_eq!(events[2].from, "s2");
        assert_eq!(events[2].to, "s3");
    }

    #[test]
    fn test_aggregate_edges() {
        let events = vec![
            TransitionEvent { from: "s0".into(), to: "s1".into(), trigger_tool: "read".into(), timestamp: 0 },
            TransitionEvent { from: "s1".into(), to: "s2".into(), trigger_tool: "test".into(), timestamp: 1 },
            TransitionEvent { from: "s0".into(), to: "s1".into(), trigger_tool: "read".into(), timestamp: 2 },
        ];
        let edges = aggregate_edges(&events);
        assert_eq!(edges.len(), 2);
        let s01 = edges.iter().find(|e| e.from == "s0").unwrap();
        assert_eq!(s01.count, 2);
    }

    #[test]
    fn test_build_graph() {
        let states = vec![
            make_state("s0", "locate"),
            make_state("s1", "modify"),
        ];
        let graph = build_graph("t1".into(), "s1".into(), states, 5);
        assert_eq!(graph.trace_id, "t1");
        assert_eq!(graph.states.len(), 2);
        assert_eq!(graph.event_log.len(), 1);
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.total_tools, 5);
    }
}
