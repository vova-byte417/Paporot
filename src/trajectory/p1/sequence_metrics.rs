//! P1 Sequence Metrics: 序列级行为度量。
//!
//! D6: loop_ratio (state-level cycles), backtrack_ratio (temporal regression),
//! burst_ratio (tool-level density). Oscillation absorbed into loop_ratio.

use std::collections::HashMap;

use crate::trace::types::ToolCall;
use crate::trajectory::types::{BehaviorStateGraph, TransitionEvent};

/// 序列度量结果。
#[derive(Debug, Clone, Default)]
pub struct SequenceMetrics {
    /// 回环比率: state-level cycle transitions / total transitions (D6)
    pub loop_ratio: f32,
    /// 回退比率: k-step temporal regression / total transitions (D6)
    pub backtrack_ratio: f32,
    /// 突发比率: max consecutive same tool category / total (D6)
    pub burst_ratio: f32,
    /// 检测到的 loop 详情
    pub loops_detected: Vec<LoopInfo>,
}

#[derive(Debug, Clone)]
pub struct LoopInfo {
    /// 循环起始状态
    pub start_state: String,
    /// 循环长度（状态数）
    pub length: usize,
    /// 循环重复次数
    pub repetitions: u32,
}

impl SequenceMetrics {
    /// 从 BehaviorStateGraph 计算所有序列度量。
    /// `tools` 用于 tool-level burst detection。
    pub fn from_graph(graph: &BehaviorStateGraph, tools: &[ToolCall]) -> Self {
        let event_log = &graph.event_log;

        // ── loop_ratio: state-level structural cycles (includes oscillation) ── (D6)
        let loops_detected = Self::detect_cycles(event_log, 2, 5);
        let loop_count = loops_detected.iter().map(|l| l.repetitions as usize).sum::<usize>();
        let loop_ratio = if !event_log.is_empty() {
            loop_count as f32 / event_log.len() as f32
        } else {
            0.0
        };

        // ── backtrack_ratio: k-step temporal regression (k=1) ── (D6)
        let backtrack_ratio = Self::compute_backtrack_ratio(event_log);

        // ── burst_ratio: tool-level consecutive same-category density ── (D6)
        let burst_ratio = Self::compute_burst_ratio(tools);

        SequenceMetrics {
            loop_ratio,
            backtrack_ratio,
            burst_ratio,
            loops_detected,
        }
    }

    /// D6: state-level structural cycles via DFS.
    /// Includes oscillations as 2-state loops.
    fn detect_cycles(
        events: &[TransitionEvent],
        min_len: usize,
        max_len: usize,
    ) -> Vec<LoopInfo> {
        // Build adjacency list
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for ev in events {
            adj.entry(&ev.from).or_default().push(&ev.to);
        }

        let mut loops: Vec<LoopInfo> = Vec::new();
        let mut visited_at: HashMap<&str, usize> = HashMap::new();
        let mut path: Vec<&str> = Vec::new();

        for start in adj.keys() {
            if visited_at.contains_key(start) {
                continue;
            }
            Self::dfs_cycles(
                start,
                &adj,
                min_len,
                max_len,
                &mut visited_at,
                &mut path,
                &mut loops,
                0,
            );
        }

        // Deduplicate loops (same set of states, different start point)
        Self::dedup_loops(&mut loops);
        loops
    }

    fn dfs_cycles<'a>(
        node: &'a str,
        adj: &HashMap<&str, Vec<&'a str>>,
        min_len: usize,
        max_len: usize,
        visited: &mut HashMap<&'a str, usize>,
        path: &mut Vec<&'a str>,
        loops: &mut Vec<LoopInfo>,
        depth: usize,
    ) {
        if let Some(&pos) = visited.get(node) {
            let cycle_len = depth - pos;
            if cycle_len >= min_len && cycle_len <= max_len {
                let loop_states: Vec<String> =
                    path[pos..depth].iter().map(|s| s.to_string()).collect();
                if !loop_states.is_empty() {
                    let start_state = loop_states[0].clone();
                    // Check if this is a repeat of an existing loop
                    if let Some(existing) = loops.iter_mut().find(|l| {
                        l.start_state == start_state && l.length == cycle_len
                    }) {
                        existing.repetitions += 1;
                    } else {
                        loops.push(LoopInfo {
                            start_state,
                            length: cycle_len,
                            repetitions: 1,
                        });
                    }
                }
            }
            return;
        }

        if depth > max_len {
            return;
        }

        visited.insert(node, depth);
        path.push(node);

        if let Some(neighbors) = adj.get(node) {
            for &next in neighbors {
                Self::dfs_cycles(next, adj, min_len, max_len, visited, path, loops, depth + 1);
            }
        }

        path.pop();
        visited.remove(node);
    }

    fn dedup_loops(loops: &mut Vec<LoopInfo>) {
        // Sort by start_state + length, merge duplicates
        let mut merged: Vec<LoopInfo> = Vec::new();
        loops.sort_by(|a, b| {
            a.start_state
                .cmp(&b.start_state)
                .then(a.length.cmp(&b.length))
        });

        for l in loops.drain(..) {
            if let Some(last) = merged.last_mut() {
                if last.start_state == l.start_state && last.length == l.length {
                    last.repetitions += l.repetitions;
                    continue;
                }
            }
            merged.push(l);
        }
        *loops = merged;
    }

    /// D6: k-step temporal regression ratio (k=1).
    fn compute_backtrack_ratio(events: &[TransitionEvent]) -> f32 {
        if events.len() < 2 {
            return 0.0;
        }

        // Build state visit sequence from transitions
        let mut seq: Vec<&str> = Vec::new();
        for ev in events {
            if seq.is_empty() || seq.last() != Some(&&ev.from[..]) {
                seq.push(&ev.from);
            }
        }
        if let Some(last) = events.last() {
            if seq.last() != Some(&&last.to[..]) {
                seq.push(&last.to);
            }
        }

        let mut backtracks = 0;
        for i in 1..seq.len() {
            // Check if current state appeared in previous positions (up to k=3 back)
            let lookback = 3.min(i);
            for j in 1..=lookback {
                if i >= j && seq[i] == seq[i - j] {
                    backtracks += 1;
                    break; // count once per position
                }
            }
        }

        if seq.len() > 1 {
            backtracks as f32 / (seq.len() - 1) as f32
        } else {
            0.0
        }
    }

    /// D6: burst ratio — max consecutive same-category tools.
    fn compute_burst_ratio(tools: &[ToolCall]) -> f32 {
        if tools.is_empty() {
            return 0.0;
        }

        let mut max_run: usize = 0;
        let mut current_run: usize = 0;
        let mut prev_cat: Option<String> = None;

        for tc in tools {
            let cat = tool_category(&tc.tool_name);
            if Some(cat.clone()) == prev_cat {
                current_run += 1;
            } else {
                max_run = max_run.max(current_run);
                current_run = 1;
                prev_cat = Some(cat);
            }
        }
        max_run = max_run.max(current_run);

        // burst = max consecutive / total, normalized: bursts typically ≥ 3
        if max_run >= 3 {
            (max_run as f32 - 2.0) / tools.len() as f32
        } else {
            0.0
        }
    }
}

fn tool_category(name: &str) -> String {
    match name {
        "read" | "grep" | "glob" | "search_codebase" | "web_search" | "web_fetch"
        | "ls" | "list" => "locate".into(),
        "write" | "edit" | "search_replace" | "delete_file" | "bash" | "run_command" => {
            "modify".into()
        }
        "test" | "cargo" | "check" | "lint" | "clippy" | "build" | "compile" => "verify".into(),
        "commit" | "git" | "push" | "pull_request" => "commit".into(),
        _ => "other".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::types::TransitionEvent;

    fn ev(from: &str, to: &str, ts: u64) -> TransitionEvent {
        TransitionEvent {
            from: from.into(),
            to: to.into(),
            trigger_tool: "edit".into(),
            timestamp: ts,
        }
    }

    fn tc(name: &str) -> ToolCall {
        ToolCall {
            id: "x".into(),
            tool_name: name.into(),
            args: serde_json::json!({}),
            timestamp: "now".into(),
            duration_ms: 100,
            result_id: None,
        }
    }

    #[test]
    fn test_backtrack_no_backtrack() {
        let events = vec![ev("s0", "s1", 100), ev("s1", "s2", 200), ev("s2", "s3", 300)];
        let ratio = SequenceMetrics::compute_backtrack_ratio(&events);
        assert!(ratio < 0.01);
    }

    #[test]
    fn test_backtrack_with_backtrack() {
        let events = vec![
            ev("s0", "s1", 100),
            ev("s1", "s0", 200),
            ev("s0", "s2", 300),
        ];
        let ratio = SequenceMetrics::compute_backtrack_ratio(&events);
        assert!(ratio > 0.0, "Expected backtrack > 0, got {}", ratio);
    }

    #[test]
    fn test_cycle_detection_2_state() {
        let events = vec![
            ev("s0", "s1", 100),
            ev("s1", "s0", 200),
            ev("s0", "s1", 300),
        ];
        let loops = SequenceMetrics::detect_cycles(&events, 2, 5);
        assert!(!loops.is_empty(), "Expected at least one loop detected");
        // s0→s1→s0 should be a 2-state cycle with 2 occurrences
        let total_reps: u32 = loops.iter().map(|l| l.repetitions).sum();
        assert!(total_reps >= 1);
    }

    #[test]
    fn test_cycle_detection_3_state() {
        let events = vec![
            ev("s0", "s1", 100),
            ev("s1", "s2", 200),
            ev("s2", "s0", 300),
        ];
        let loops = SequenceMetrics::detect_cycles(&events, 2, 5);
        assert!(!loops.is_empty());
        assert!(loops.iter().any(|l| l.length == 3));
    }

    #[test]
    fn test_burst_no_burst() {
        let tools = vec![tc("read"), tc("edit"), tc("test"), tc("commit")];
        let ratio = SequenceMetrics::compute_burst_ratio(&tools);
        assert_eq!(ratio, 0.0, "No consecutive same tool, burst should be 0");
    }

    #[test]
    fn test_burst_detected() {
        let tools = vec![
            tc("read"), tc("read"), tc("read"), tc("read"), // 4 consecutive read
            tc("edit"),
        ];
        let ratio = SequenceMetrics::compute_burst_ratio(&tools);
        assert!(ratio > 0.0, "Expected burst > 0 for 4 consecutive same tools");
    }

    #[test]
    fn test_empty_events() {
        let metrics = SequenceMetrics::from_graph(
            &BehaviorStateGraph {
                trace_id: "t".into(),
                session_id: "s".into(),
                states: vec![],
                event_log: vec![],
                edges: vec![],
                total_tools: 0,
            },
            &[],
        );
        assert_eq!(metrics.loop_ratio, 0.0);
        assert_eq!(metrics.backtrack_ratio, 0.0);
        assert_eq!(metrics.burst_ratio, 0.0);
    }

    #[test]
    fn test_loop_ratio_includes_oscillation() {
        // s0→s1→s0→s1→s0 (oscillation = 2-state loop)
        let events = vec![
            ev("s0", "s1", 100),
            ev("s1", "s0", 200),
            ev("s0", "s1", 300),
            ev("s1", "s0", 400),
        ];
        let loops = SequenceMetrics::detect_cycles(&events, 2, 5);
        assert!(!loops.is_empty(), "Oscillation should be detected as loop");
    }
}
