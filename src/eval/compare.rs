//! Eval 对比逻辑
//!
//! 对比同一 Task 的两个 Trial，生成 EvalCompare 报告。
//! 核心问题：Agent 这次比上次好还是坏？

use anyhow::Result;

use super::types::*;
use crate::storage::timeline::TimelineStore;

// ─── compare ────────────────────────────────────────────────────────

/// 对比同一 Task 的两个 Trial
pub fn compare(
    store: &TimelineStore,
    task_id: &str,
    from_eval_id: Option<&str>,
    to_eval_id: Option<&str>,
) -> Result<EvalCompare> {
    // 确定要对比的两个 eval
    let (from, to) = if let (Some(f), Some(t)) = (from_eval_id, to_eval_id) {
        (store.load_eval(f)?, store.load_eval(t)?)
    } else {
        let trials = store.list_trials(task_id)?;
        match trials.len() {
            0 => anyhow::bail!("No trials found for task '{}'", task_id),
            1 => anyhow::bail!("Only 1 trial found for '{}'. Need at least 2 for comparison.", task_id),
            n => {
                let from = trials.get(n - 2).unwrap().clone();
                let to = trials.get(n - 1).unwrap().clone();
                (from, to)
            }
        }
    };

    // 计算趋势
    let trend = compute_trend(&from, &to);

    // 计算指标变化
    let metrics = compute_metrics(&from, &to);

    Ok(EvalCompare {
        task_id: task_id.to_string(),
        from,
        to,
        trend,
        metrics,
    })
}

/// 计算总体趋势
fn compute_trend(from: &EvalResult, to: &EvalResult) -> EvalTrend {
    // 从 Fail 变 Pass → Improved
    // 从 Pass 变 Fail → Degraded
    // 其他 → Stable
    match (&from.outcome, &to.outcome) {
        (OutcomeVerdict::Pass, OutcomeVerdict::Pass) => EvalTrend::Stable,
        (_, OutcomeVerdict::Pass) => EvalTrend::Improved,
        (OutcomeVerdict::Pass, _) => EvalTrend::Degraded,
        _ => EvalTrend::Stable,
    }
}

/// 计算结构化指标变化
fn compute_metrics(from: &EvalResult, to: &EvalResult) -> Vec<MetricChange> {
    let mut metrics = Vec::new();

    // 一级指标（Anthropic tracked_metrics）
    if let (Some(tp_from), Some(tp_to)) = (&from.tool_pattern, &to.tool_pattern) {
        // Tool 调用数
        metrics.push(metric_change(
            "n_toolcalls", "工具调用数",
            tp_from.total_tool_calls as f64,
            tp_to.total_tool_calls as f64,
        ));

        // Token 消耗
        metrics.push(metric_change(
            "n_tokens", "Token 消耗",
            tp_from.total_tokens as f64,
            tp_to.total_tokens as f64,
        ));

        // 执行耗时
        metrics.push(metric_change(
            "duration_ms", "执行耗时(ms)",
            tp_from.duration_ms as f64,
            tp_to.duration_ms as f64,
        ));
    }

    // 深度诊断指标（P1 轨迹向量）
    if let (Some(tp_from), Some(tp_to)) = (&from.tool_pattern, &to.tool_pattern) {
        if let (Some(v_from), Some(v_to)) = (&tp_from.trajectory_vector, &tp_to.trajectory_vector) {
            let vec_dimensions = [
                ("tool_entropy", "工具混乱度", true),
                ("phase_entropy", "阶段混乱度", true),
                ("loop_ratio", "循环比例", true),
                ("backtrack_ratio", "回溯比例", true),
                ("state_stability_score", "行为稳定性", false),
            ];

            for (key, label, lower_is_better) in &vec_dimensions {
                let from_val = v_from.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0);
                let to_val = v_to.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0);
                let mut m = metric_change(key, label, from_val, to_val);
                // 调整方向：如果 lower_is_better，则下降是改善
                if *lower_is_better {
                    m.direction = match m.direction {
                        MetricDirection::Up => MetricDirection::Down,
                        MetricDirection::Down => MetricDirection::Up,
                        MetricDirection::Flat => MetricDirection::Flat,
                    };
                }
                metrics.push(m);
            }
        }
    }

    // 代码变更规模
    metrics.push(metric_change(
        "additions", "新增行数",
        from.code_change.additions as f64,
        to.code_change.additions as f64,
    ));

    metrics.push(metric_change(
        "deletions", "删除行数",
        from.code_change.deletions as f64,
        to.code_change.deletions as f64,
    ));

    metrics
}

fn metric_change(key: &str, label: &str, from: f64, to: f64) -> MetricChange {
    let change_pct = if from == 0.0 {
        if to == 0.0 { 0.0 } else { 100.0 }
    } else {
        ((to - from) / from) * 100.0
    };

    let direction = if change_pct > 5.0 {
        MetricDirection::Up
    } else if change_pct < -5.0 {
        MetricDirection::Down
    } else {
        MetricDirection::Flat
    };

    MetricChange {
        name: key.to_string(),
        label: label.to_string(),
        from_value: from,
        to_value: to,
        change_pct,
        direction,
    }
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_change_up() {
        let m = metric_change("test", "测试", 10.0, 15.0);
        assert_eq!(m.change_pct, 50.0);
        assert!(matches!(m.direction, MetricDirection::Up));
    }

    #[test]
    fn test_metric_change_down() {
        let m = metric_change("test", "测试", 10.0, 5.0);
        assert_eq!(m.change_pct, -50.0);
        assert!(matches!(m.direction, MetricDirection::Down));
    }

    #[test]
    fn test_metric_change_flat() {
        let m = metric_change("test", "测试", 10.0, 10.1);
        assert!(matches!(m.direction, MetricDirection::Flat));
    }
}
