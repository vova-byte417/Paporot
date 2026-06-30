//! Eval 趋势分析
//!
//! 查询某个 Task 的多 Trial 历史，生成 EvalTrendHistory。

use anyhow::Result;

use super::types::*;
use crate::storage::timeline::TimelineStore;

/// 获取某个 Task 的完整趋势历史
pub fn trend_history(store: &TimelineStore, task_id: &str) -> Result<EvalTrendHistory> {
    let trials = store.list_trials(task_id)?;
    let task = store.load_task(task_id)?
        .unwrap_or_else(|| TaskSpec {
            id: task_id.to_string(),
            description: "Unknown task".into(),
            success_criteria: vec![],
            category: TaskCategory::Other("unknown".into()),
            modules: vec![],
            source: TaskSource::Manual { created_by: "unknown".into() },
        });

    let points: Vec<EvalTrendPoint> = trials
        .iter()
        .map(|eval| EvalTrendPoint {
            eval_id: eval.eval_id.clone(),
            trial_index: eval.trial_index,
            outcome: eval.outcome.clone(),
            total_tool_calls: eval.tool_pattern.as_ref().map(|tp| tp.total_tool_calls),
            total_tokens: eval.tool_pattern.as_ref().map(|tp| tp.total_tokens),
            duration_ms: eval.tool_pattern.as_ref().map(|tp| tp.duration_ms),
            created_at: eval.created_at.clone(),
        })
        .collect();

    Ok(EvalTrendHistory {
        task_id: task_id.to_string(),
        task_description: task.description,
        trials: points,
    })
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> TimelineStore {
        let dir = std::env::temp_dir().join(format!("paporot_test_{}", uuid::Uuid::new_v4()));
        TimelineStore::open(&dir).unwrap()
    }

    #[test]
    fn test_trend_history_empty() {
        let store = temp_store();
        // 不保存任何数据，查询一个不存在的 task
        let result = trend_history(&store, "nonexistent");
        assert!(result.is_ok());
        let history = result.unwrap();
        assert_eq!(history.task_id, "nonexistent");
        assert!(history.trials.is_empty());
    }
}
