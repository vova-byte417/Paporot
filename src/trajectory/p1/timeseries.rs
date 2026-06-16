//! P1 Time Series Builder: 行为轨迹的时序聚合。
//!
//! D3: edit_intensity_curve = 完整 Vec<f32>, P2 用一阶导数统计量。

use crate::trajectory::types::BehaviorStateGraph;

/// 时间序列摘要。
#[derive(Debug, Clone)]
pub struct TimeSeries {
    /// 时间窗口大小 (ms)
    pub window_ms: u64,
    /// 每个窗口的状态转移计数
    pub transition_counts: Vec<usize>,
    /// 编辑强度曲线: per-window mean edit_density (D3)
    pub edit_intensity_curve: Vec<f32>,
    /// tool usage 扁平化向量 (用于 vector 层)
    pub tool_usage_flat: Vec<f32>,
}

impl TimeSeries {
    /// 从 BehaviorStateGraph 构建时间序列。
    /// 将 states 按时间顺序分窗，计算每窗口的编辑强度和转移密度。
    pub fn from_graph(graph: &BehaviorStateGraph, window_ms: u64) -> Self {
        let state_count = graph.states.len();
        let event_count = graph.event_log.len();

        if state_count == 0 {
            return TimeSeries {
                window_ms,
                transition_counts: Vec::new(),
                edit_intensity_curve: Vec::new(),
                tool_usage_flat: Vec::new(),
            };
        }

        // Use the event log timestamps to determine time range
        let (t_min, t_max) = if event_count > 0 {
            let min_ts = graph.event_log.iter().map(|e| e.timestamp).min().unwrap_or(0);
            let max_ts = graph.event_log.iter().map(|e| e.timestamp).max().unwrap_or(0);
            (min_ts, max_ts)
        } else {
            // No events → single window covering all states
            (0, state_count as u64 * 100)
        };

        let range = t_max.saturating_sub(t_min);
        let effective_window = if range == 0 || window_ms == 0 {
            state_count as u64  // fallback: 1 window per state
        } else {
            window_ms
        };

        let num_windows = if range == 0 {
            state_count.max(1)
        } else {
            ((range / effective_window) as usize + 1).max(1)
        };

        let mut edit_curve = Vec::with_capacity(num_windows);
        let mut trans_counts = Vec::with_capacity(num_windows);
        let mut tool_flat: Vec<f32> = Vec::new();

        for wi in 0..num_windows {
            let w_start = t_min + wi as u64 * effective_window;
            let w_end = w_start + effective_window;

            // Count transitions in this window
            let trans_in_window = graph
                .event_log
                .iter()
                .filter(|e| e.timestamp >= w_start && e.timestamp < w_end)
                .count();
            trans_counts.push(trans_in_window);

            // Approximate edit density from states whose tool_range falls in window
            let total_tools = graph.total_tools.max(1) as f32;
            let state_start = (wi as f32 / num_windows as f32 * total_tools) as usize;
            let state_end = ((wi + 1) as f32 / num_windows as f32 * total_tools) as usize;

            let mut edits: Vec<f32> = Vec::new();
            for state in &graph.states {
                let mid = (state.tool_range.0 + state.tool_range.1) / 2;
                if mid >= state_start && mid < state_end {
                    edits.push(state.features.edit_density);
                }
            }
            let edit_mean = if edits.is_empty() {
                0.0
            } else {
                edits.iter().sum::<f32>() / edits.len() as f32
            };
            edit_curve.push(edit_mean);

            // Collect tool usage for this window (5 categories)
            let categories = ["locate", "modify", "verify", "commit", "other"];
            let mut usage = vec![0.0_f32; 5];
            let mut count = 0;
            for state in &graph.states {
                let mid = (state.tool_range.0 + state.tool_range.1) / 2;
                if mid >= state_start && mid < state_end {
                    for (ci, cat) in categories.iter().enumerate() {
                        usage[ci] += state.features.tool_histogram.get(*cat).copied().unwrap_or(0.0);
                    }
                    count += 1;
                }
            }
            if count > 0 {
                for v in &mut usage {
                    *v /= count as f32;
                }
            }
            tool_flat.extend_from_slice(&usage);
        }

        TimeSeries {
            window_ms: effective_window,
            transition_counts: trans_counts,
            edit_intensity_curve: edit_curve,
            tool_usage_flat: tool_flat,
        }
    }

    /// D3: 编辑强度曲线的一阶导数统计（用于 P2 correlation）。
    pub fn edit_intensity_derivative_stats(&self) -> (f32, f32, f32) {
        if self.edit_intensity_curve.len() < 2 {
            return (0.0, 0.0, 0.0);
        }

        let derivatives: Vec<f32> = self
            .edit_intensity_curve
            .windows(2)
            .map(|w| w[1] - w[0])
            .collect();

        let n = derivatives.len() as f32;
        let mean = derivatives.iter().sum::<f32>() / n;
        let min = derivatives.iter().cloned().fold(f32::MAX, f32::min);
        let max = derivatives.iter().cloned().fold(f32::MIN, f32::max);
        (mean, min, max)
    }

    /// 编辑强度曲线摘要统计。
    pub fn edit_intensity_stats(&self) -> (f32, f32, f32) {
        if self.edit_intensity_curve.is_empty() {
            return (0.0, 0.0, 0.0);
        }
        let n = self.edit_intensity_curve.len() as f32;
        let mean = self.edit_intensity_curve.iter().sum::<f32>() / n;
        let min = self
            .edit_intensity_curve
            .iter()
            .cloned()
            .fold(f32::MAX, f32::min);
        let max = self
            .edit_intensity_curve
            .iter()
            .cloned()
            .fold(f32::MIN, f32::max);
        (mean, min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::types::{BehaviorState, StateFeatures, TransitionEvent};
    use std::collections::HashMap;

    fn make_test_graph() -> BehaviorStateGraph {
        let mut tool_hist = HashMap::new();
        tool_hist.insert("locate".into(), 0.5);
        tool_hist.insert("modify".into(), 0.5);

        let state = BehaviorState {
            id: "s0".into(),
            phase_dist: HashMap::from([("locate".into(), 1.0)]),
            primary_phase: "locate".into(),
            features: StateFeatures {
                tool_histogram: tool_hist,
                edit_density: 0.3,
                ..Default::default()
            },
            stability_score: 0.9,
            tool_range: (0, 5),
        };

        BehaviorStateGraph {
            trace_id: "t1".into(),
            session_id: "s1".into(),
            states: vec![state],
            event_log: vec![
                TransitionEvent {
                    from: "s0".into(),
                    to: "s1".into(),
                    trigger_tool: "edit".into(),
                    timestamp: 0,
                },
                TransitionEvent {
                    from: "s1".into(),
                    to: "s2".into(),
                    trigger_tool: "edit".into(),
                    timestamp: 5000,
                },
            ],
            edges: Vec::new(),
            total_tools: 10,
        }
    }

    #[test]
    fn test_timeseries_empty_events() {
        let mut graph = make_test_graph();
        graph.event_log = vec![];
        let ts = TimeSeries::from_graph(&graph, 1000);
        assert!(!ts.edit_intensity_curve.is_empty());
    }

    #[test]
    fn test_timeseries_single_window() {
        let mut graph = make_test_graph();
        for ev in &mut graph.event_log {
            ev.timestamp = 1000;
        }
        let ts = TimeSeries::from_graph(&graph, 1000);
        assert!(!ts.edit_intensity_curve.is_empty());
    }

    #[test]
    fn test_edit_intensity_stats_empty() {
        let ts = TimeSeries {
            window_ms: 1000,
            transition_counts: vec![],
            edit_intensity_curve: vec![],
            tool_usage_flat: vec![],
        };
        let (mean, min, max) = ts.edit_intensity_stats();
        assert_eq!(mean, 0.0);
        assert_eq!(min, 0.0);
        assert_eq!(max, 0.0);
    }

    #[test]
    fn test_derivative_stats() {
        let ts = TimeSeries {
            window_ms: 1000,
            transition_counts: vec![1, 2, 3],
            edit_intensity_curve: vec![0.1, 0.3, 0.6],
            tool_usage_flat: vec![],
        };
        let (mean, min, max) = ts.edit_intensity_derivative_stats();
        // derivatives: [0.2, 0.3] → mean=0.25, min=0.2, max=0.3
        assert!((mean - 0.25).abs() < 0.01, "mean {}", mean);
        assert!((min - 0.2).abs() < 0.01, "min {}", min);
        assert!((max - 0.3).abs() < 0.01, "max {}", max);
    }
}
