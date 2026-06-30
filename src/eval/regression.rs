//! Eval 批量回归检测
//!
//! 对比所有 Task 的最新 Trial vs 基线 Trial，检测是否存在退化。

use anyhow::Result;

use super::types::*;
use crate::storage::timeline::TimelineStore;

/// 批量回归检测
pub fn regression(store: &TimelineStore) -> Result<EvalRegression> {
    let tasks = store.list_tasks()?;
    let mut regressions = Vec::new();

    for task in &tasks {
        let trials = store.list_trials(&task.id)?;

        if trials.len() < 2 {
            continue;
        }

        // 最新 vs 倒数第二
        let prev = &trials[trials.len() - 2];
        let latest = &trials[trials.len() - 1];

        // 检查退化
        let severity = detect_regression_severity(prev, latest);

        if severity != RegressionSeverity::Low {
            regressions.push(EvalRegressionItem {
                task_id: task.id.clone(),
                from_eval: prev.eval_id.clone(),
                to_eval: latest.eval_id.clone(),
                from_outcome: prev.outcome.label().to_string(),
                to_outcome: latest.outcome.label().to_string(),
                severity,
                description: format!(
                    "Task '{}' outcome changed from {} to {}",
                    task.description,
                    prev.outcome.label(),
                    latest.outcome.label()
                ),
            });
        }
    }

    Ok(EvalRegression {
        checked_tasks: tasks.len() as u32,
        regressions,
    })
}

/// 检测回归严重程度
fn detect_regression_severity(prev: &EvalResult, latest: &EvalResult) -> RegressionSeverity {
    match (&prev.outcome, &latest.outcome) {
        // Pass → Fail = Critical
        (OutcomeVerdict::Pass, OutcomeVerdict::Fail { .. }) => RegressionSeverity::Critical,

        // Pass → Partial = High
        (OutcomeVerdict::Pass, OutcomeVerdict::Partial { .. }) => RegressionSeverity::High,

        // Partial → Fail = High
        (OutcomeVerdict::Partial { .. }, OutcomeVerdict::Fail { .. }) => RegressionSeverity::High,

        // 改进：High → Low（标记但不报警）
        (OutcomeVerdict::Fail { .. }, OutcomeVerdict::Pass) => RegressionSeverity::Low,
        (OutcomeVerdict::Fail { .. }, OutcomeVerdict::Partial { .. }) => RegressionSeverity::Medium,
        (OutcomeVerdict::Partial { .. }, OutcomeVerdict::Pass) => RegressionSeverity::Low,

        // Stable
        _ => RegressionSeverity::Low,
    }
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_eval(outcome: OutcomeVerdict) -> EvalResult {
        EvalResult {
            eval_id: "test_eval".into(),
            task: TaskSpec {
                id: "test_task".into(),
                description: "test".into(),
                success_criteria: vec![],
                category: TaskCategory::Other("test".into()),
                modules: vec![],
                source: TaskSource::Manual { created_by: "test".into() },
            },
            trial_index: 1,
            transcript: None,
            tool_pattern: None,
            outcome,
            grader_results: vec![],
            code_change: CodeChangeSummary::default(),
            created_at: "2026-06-29T10:00:00Z".into(),
            git_event_id: None,
            session_id: None,
            tags: vec![],
        }
    }

    #[test]
    fn test_pass_to_fail_is_critical() {
        let prev = make_eval(OutcomeVerdict::Pass);
        let latest = make_eval(OutcomeVerdict::Fail {
            failing_graders: vec!["test".into()],
            summary: "failed".into(),
        });
        assert!(matches!(
            detect_regression_severity(&prev, &latest),
            RegressionSeverity::Critical
        ));
    }

    #[test]
    fn test_fail_to_pass_is_low() {
        let prev = make_eval(OutcomeVerdict::Fail {
            failing_graders: vec!["test".into()],
            summary: "failed".into(),
        });
        let latest = make_eval(OutcomeVerdict::Pass);
        assert!(matches!(
            detect_regression_severity(&prev, &latest),
            RegressionSeverity::Low
        ));
    }

    #[test]
    fn test_same_pass_is_low() {
        let prev = make_eval(OutcomeVerdict::Pass);
        let latest = make_eval(OutcomeVerdict::Pass);
        assert!(matches!(
            detect_regression_severity(&prev, &latest),
            RegressionSeverity::Low
        ));
    }
}
