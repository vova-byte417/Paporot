//! SQLite Timeline 事件存储
//!
//! 遵循事件溯源设计：只追加事件，不修改历史记录。
//! 存储 EvalResult、Task 定义、GitEvent 三大事件流。
//!
//! Schema:
//! - eval_results: 完整 EvalResult JSON + 索引字段
//! - task_defs:    TaskSpec 定义
//! - git_events:   Git commit 事件

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::eval::types::*;

// ─── TimelineStore ─────────────────────────────────────────────────

pub struct TimelineStore {
    db: Connection,
}

impl TimelineStore {
    /// 打开或创建 SQLite 数据库
    pub fn open(paporot_dir: &std::path::Path) -> Result<Self> {
        std::fs::create_dir_all(paporot_dir).ok();
        let db_path = paporot_dir.join("paporot.db");
        let db = Connection::open(&db_path)
            .with_context(|| format!("Failed to open {}", db_path.display()))?;

        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .context("Failed to set pragmas")?;

        let store = Self { db };
        store.migrate()?;
        Ok(store)
    }

    // ─── 迁移 ──────────────────────────────────────────────────────

    fn migrate(&self) -> Result<()> {
        self.db.execute_batch(
            "CREATE TABLE IF NOT EXISTS eval_results (
                eval_id         TEXT PRIMARY KEY,
                task_id         TEXT NOT NULL,
                trial_index     INTEGER NOT NULL DEFAULT 1,
                outcome         TEXT NOT NULL DEFAULT 'NotEvaluated',
                outcome_json    TEXT NOT NULL,
                tool_pattern_json TEXT,
                code_change_json  TEXT NOT NULL DEFAULT '{}',
                grader_results_json TEXT NOT NULL DEFAULT '[]',
                created_at      TEXT NOT NULL,
                git_event_id    TEXT,
                session_id      TEXT,
                tags_json       TEXT NOT NULL DEFAULT '[]'
            );

            CREATE INDEX IF NOT EXISTS idx_eval_task ON eval_results(task_id);
            CREATE INDEX IF NOT EXISTS idx_eval_created ON eval_results(created_at);
            CREATE INDEX IF NOT EXISTS idx_eval_git ON eval_results(git_event_id);

            CREATE TABLE IF NOT EXISTS task_defs (
                task_id         TEXT PRIMARY KEY,
                description     TEXT NOT NULL,
                success_criteria_json TEXT NOT NULL DEFAULT '[]',
                category        TEXT NOT NULL DEFAULT 'Other',
                modules_json    TEXT NOT NULL DEFAULT '[]',
                source_type     TEXT NOT NULL,
                source_json     TEXT NOT NULL DEFAULT '{}',
                created_at      TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS git_events (
                event_id        TEXT PRIMARY KEY,
                commit_sha      TEXT NOT NULL,
                commit_message  TEXT NOT NULL DEFAULT '',
                author          TEXT NOT NULL DEFAULT '',
                timestamp       TEXT NOT NULL,
                diff_content    TEXT NOT NULL DEFAULT '',
                task_id         TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_git_commit ON git_events(commit_sha);
            CREATE INDEX IF NOT EXISTS idx_git_task ON git_events(task_id);"
        ).context("Failed to create tables")?;

        Ok(())
    }

    // ─── EvalResult CRUD ───────────────────────────────────────────

    /// 保存 EvalResult（插入，不覆盖）
    pub fn save_eval(&self, eval: &EvalResult) -> Result<()> {
        let outcome_json = serde_json::to_string(&eval.outcome)?;
        let tool_pattern_json = eval.tool_pattern.as_ref()
            .map(|tp| serde_json::to_string(tp))
            .transpose()?;
        let code_change_json = serde_json::to_string(&eval.code_change)?;
        let grader_json = serde_json::to_string(&eval.grader_results)?;
        let tags_json = serde_json::to_string(&eval.tags)?;

        self.db.execute(
            "INSERT INTO eval_results (eval_id, task_id, trial_index, outcome, outcome_json,
             tool_pattern_json, code_change_json, grader_results_json, created_at,
             git_event_id, session_id, tags_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                eval.eval_id,
                eval.task.id,
                eval.trial_index,
                eval.outcome.label(),
                outcome_json,
                tool_pattern_json,
                code_change_json,
                grader_json,
                eval.created_at,
                eval.git_event_id,
                eval.session_id,
                tags_json,
            ],
        )?;

        Ok(())
    }

    /// 按 eval_id 加载 EvalResult
    pub fn load_eval(&self, eval_id: &str) -> Result<EvalResult> {
        let mut stmt = self.db.prepare(
            "SELECT eval_id, task_id, trial_index, outcome_json, tool_pattern_json,
                    code_change_json, grader_results_json, created_at,
                    git_event_id, session_id, tags_json
             FROM eval_results WHERE eval_id = ?1"
        )?;

        let result = stmt.query_row(rusqlite::params![eval_id], |row| {
            let task_id: String = row.get(1)?;
            Ok(EvalRow {
                eval_id: row.get(0)?,
                task_id,
                trial_index: row.get(2)?,
                outcome_json: row.get(3)?,
                tool_pattern_json: row.get(4)?,
                code_change_json: row.get(5)?,
                grader_results_json: row.get(6)?,
                created_at: row.get(7)?,
                git_event_id: row.get(8)?,
                session_id: row.get(9)?,
                tags_json: row.get(10)?,
            })
        })?;

        self.row_to_eval(&result)
    }

    /// 按 task_id 列出所有 trials
    pub fn list_trials(&self, task_id: &str) -> Result<Vec<EvalResult>> {
        let mut stmt = self.db.prepare(
            "SELECT eval_id, task_id, trial_index, outcome_json, tool_pattern_json,
                    code_change_json, grader_results_json, created_at,
                    git_event_id, session_id, tags_json
             FROM eval_results WHERE task_id = ?1
             ORDER BY trial_index ASC"
        )?;

        let rows: Vec<EvalRow> = stmt.query_map(rusqlite::params![task_id], |row| {
            Ok(EvalRow {
                eval_id: row.get(0)?,
                task_id: row.get(1)?,
                trial_index: row.get(2)?,
                outcome_json: row.get(3)?,
                tool_pattern_json: row.get(4)?,
                code_change_json: row.get(5)?,
                grader_results_json: row.get(6)?,
                created_at: row.get(7)?,
                git_event_id: row.get(8)?,
                session_id: row.get(9)?,
                tags_json: row.get(10)?,
            })
        })?.filter_map(|r| r.ok()).collect();

        rows.iter()
            .map(|r| self.row_to_eval(r))
            .collect::<Result<Vec<_>>>()
    }

    /// 获取某个 Task 的最新 trial
    pub fn latest_trial(&self, task_id: &str) -> Result<Option<EvalResult>> {
        let mut stmt = self.db.prepare(
            "SELECT eval_id, task_id, trial_index, outcome_json, tool_pattern_json,
                    code_change_json, grader_results_json, created_at,
                    git_event_id, session_id, tags_json
             FROM eval_results WHERE task_id = ?1
             ORDER BY trial_index DESC LIMIT 1"
        )?;

        let result = stmt.query_row(rusqlite::params![task_id], |row| {
            Ok(EvalRow {
                eval_id: row.get(0)?,
                task_id: row.get(1)?,
                trial_index: row.get(2)?,
                outcome_json: row.get(3)?,
                tool_pattern_json: row.get(4)?,
                code_change_json: row.get(5)?,
                grader_results_json: row.get(6)?,
                created_at: row.get(7)?,
                git_event_id: row.get(8)?,
                session_id: row.get(9)?,
                tags_json: row.get(10)?,
            })
        });

        match result {
            Ok(row) => Ok(Some(self.row_to_eval(&row)?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("Query error: {}", e)),
        }
    }

    /// 列出最近的 eval（按时间倒序）
    pub fn list_recent_evals(&self, limit: usize) -> Result<Vec<EvalResult>> {
        let mut stmt = self.db.prepare(
            "SELECT eval_id, task_id, trial_index, outcome_json, tool_pattern_json,
                    code_change_json, grader_results_json, created_at,
                    git_event_id, session_id, tags_json
             FROM eval_results
             ORDER BY created_at DESC LIMIT ?1"
        )?;

        let rows: Vec<EvalRow> = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(EvalRow {
                eval_id: row.get(0)?,
                task_id: row.get(1)?,
                trial_index: row.get(2)?,
                outcome_json: row.get(3)?,
                tool_pattern_json: row.get(4)?,
                code_change_json: row.get(5)?,
                grader_results_json: row.get(6)?,
                created_at: row.get(7)?,
                git_event_id: row.get(8)?,
                session_id: row.get(9)?,
                tags_json: row.get(10)?,
            })
        })?.filter_map(|r| r.ok()).collect();

        rows.iter()
            .map(|r| self.row_to_eval(r))
            .collect::<Result<Vec<_>>>()
    }

    // ─── Task 定义 ─────────────────────────────────────────────────

    /// 保存 Task 定义
    pub fn save_task(&self, task: &TaskSpec) -> Result<()> {
        let criteria_json = serde_json::to_string(&task.success_criteria)?;
        let modules_json = serde_json::to_string(&task.modules)?;
        let (source_type, source_json) = task_source_to_pair(&task.source);

        self.db.execute(
            "INSERT OR REPLACE INTO task_defs (task_id, description, success_criteria_json,
             category, modules_json, source_type, source_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))",
            rusqlite::params![
                task.id,
                task.description,
                criteria_json,
                task.category.to_string(),
                modules_json,
                source_type,
                source_json,
            ],
        )?;

        Ok(())
    }

    /// 加载 Task 定义
    pub fn load_task(&self, task_id: &str) -> Result<Option<TaskSpec>> {
        let mut stmt = self.db.prepare(
            "SELECT task_id, description, success_criteria_json, category,
                    modules_json, source_type, source_json, created_at
             FROM task_defs WHERE task_id = ?1"
        )?;

        let result = stmt.query_row(rusqlite::params![task_id], |row| {
            let criteria_json: String = row.get(2)?;
            let modules_json: String = row.get(4)?;
            let source_type: String = row.get(5)?;
            let source_json: String = row.get(6)?;

            Ok(TaskSpec {
                id: row.get(0)?,
                description: row.get(1)?,
                success_criteria: serde_json::from_str(&criteria_json).unwrap_or_default(),
                category: parse_task_category(&row.get::<_, String>(3)?),
                modules: serde_json::from_str(&modules_json).unwrap_or_default(),
                source: parse_task_source(&source_type, &source_json),
            })
        });

        match result {
            Ok(task) => Ok(Some(task)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("Query error: {}", e)),
        }
    }

    /// 列出所有 Task
    pub fn list_tasks(&self) -> Result<Vec<TaskSpec>> {
        let mut stmt = self.db.prepare(
            "SELECT task_id, description, success_criteria_json, category,
                    modules_json, source_type, source_json, created_at
             FROM task_defs ORDER BY created_at DESC"
        )?;

        let rows: Vec<TaskSpec> = stmt.query_map([], |row| {
            let criteria_json: String = row.get(2)?;
            let modules_json: String = row.get(4)?;
            let source_type: String = row.get(5)?;
            let source_json: String = row.get(6)?;

            Ok(TaskSpec {
                id: row.get(0)?,
                description: row.get(1)?,
                success_criteria: serde_json::from_str(&criteria_json).unwrap_or_default(),
                category: parse_task_category(&row.get::<_, String>(3)?),
                modules: serde_json::from_str(&modules_json).unwrap_or_default(),
                source: parse_task_source(&source_type, &source_json),
            })
        })?.filter_map(|r| r.ok()).collect();

        Ok(rows)
    }

    // ─── GitEvent ──────────────────────────────────────────────────

    /// 保存 Git 事件
    pub fn save_git_event(
        &self,
        event_id: &str,
        commit_sha: &str,
        commit_message: &str,
        author: &str,
        timestamp: &str,
        diff_content: &str,
        task_id: Option<&str>,
    ) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO git_events (event_id, commit_sha, commit_message, author,
             timestamp, diff_content, task_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![event_id, commit_sha, commit_message, author, timestamp, diff_content, task_id],
        )?;
        Ok(())
    }

    /// 按 commit_sha 查找
    pub fn find_git_event_by_commit(&self, commit_sha: &str) -> Result<Option<GitEventRow>> {
        let mut stmt = self.db.prepare(
            "SELECT event_id, commit_sha, commit_message, author, timestamp, diff_content, task_id
             FROM git_events WHERE commit_sha = ?1"
        )?;

        let result = stmt.query_row(rusqlite::params![commit_sha], |row| {
            Ok(GitEventRow {
                event_id: row.get(0)?,
                commit_sha: row.get(1)?,
                commit_message: row.get(2)?,
                author: row.get(3)?,
                timestamp: row.get(4)?,
                diff_content: row.get(5)?,
                task_id: row.get(6)?,
            })
        });

        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("Query error: {}", e)),
        }
    }

    /// 列出最近的 Git 事件
    pub fn list_recent_git_events(&self, limit: usize) -> Result<Vec<GitEventRow>> {
        let mut stmt = self.db.prepare(
            "SELECT event_id, commit_sha, commit_message, author, timestamp, diff_content, task_id
             FROM git_events ORDER BY timestamp DESC LIMIT ?1"
        )?;

        let rows: Vec<GitEventRow> = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(GitEventRow {
                event_id: row.get(0)?,
                commit_sha: row.get(1)?,
                commit_message: row.get(2)?,
                author: row.get(3)?,
                timestamp: row.get(4)?,
                diff_content: row.get(5)?,
                task_id: row.get(6)?,
            })
        })?.filter_map(|r| r.ok()).collect();

        Ok(rows)
    }

    // ─── Helpers ───────────────────────────────────────────────────

    fn row_to_eval(&self, row: &EvalRow) -> Result<EvalResult> {
        let task = self.load_task(&row.task_id)?
            .unwrap_or_else(|| TaskSpec {
                id: row.task_id.clone(),
                description: "Unknown task".into(),
                success_criteria: vec![],
                category: TaskCategory::Other("unknown".into()),
                modules: vec![],
                source: TaskSource::Manual { created_by: "unknown".into() },
            });

        let outcome: OutcomeVerdict = serde_json::from_str(&row.outcome_json)
            .unwrap_or(OutcomeVerdict::NotEvaluated { reason: "parse error".into() });

        let tool_pattern: Option<ToolPattern> = row.tool_pattern_json.as_ref()
            .and_then(|s| serde_json::from_str(s).ok());

        let code_change: CodeChangeSummary = serde_json::from_str(&row.code_change_json)
            .unwrap_or_default();

        let grader_results: Vec<GraderResult> = serde_json::from_str(&row.grader_results_json)
            .unwrap_or_default();

        let tags: Vec<String> = serde_json::from_str(&row.tags_json).unwrap_or_default();

        Ok(EvalResult {
            eval_id: row.eval_id.clone(),
            task,
            trial_index: row.trial_index,
            transcript: None,
            tool_pattern,
            outcome,
            grader_results,
            code_change,
            created_at: row.created_at.clone(),
            git_event_id: row.git_event_id.clone(),
            session_id: row.session_id.clone(),
            tags,
        })
    }
}

// ─── Internal row types ───────────────────────────────────────────

struct EvalRow {
    eval_id: String,
    task_id: String,
    trial_index: u32,
    outcome_json: String,
    tool_pattern_json: Option<String>,
    code_change_json: String,
    grader_results_json: String,
    created_at: String,
    git_event_id: Option<String>,
    session_id: Option<String>,
    tags_json: String,
}

pub struct GitEventRow {
    pub event_id: String,
    pub commit_sha: String,
    pub commit_message: String,
    pub author: String,
    pub timestamp: String,
    pub diff_content: String,
    pub task_id: Option<String>,
}

// ─── Helper functions ─────────────────────────────────────────────

fn task_source_to_pair(source: &TaskSource) -> (String, String) {
    match source {
        TaskSource::Auto { commit_sha } => {
            ("auto".into(), serde_json::json!({"commit_sha": commit_sha}).to_string())
        }
        TaskSource::Manual { created_by } => {
            ("manual".into(), serde_json::json!({"created_by": created_by}).to_string())
        }
        TaskSource::Derived { parent_ids } => {
            ("derived".into(), serde_json::json!({"parent_ids": parent_ids}).to_string())
        }
    }
}

fn parse_task_category(s: &str) -> TaskCategory {
    match s {
        "BugFix" => TaskCategory::BugFix,
        "Feature" => TaskCategory::Feature,
        "Refactor" => TaskCategory::Refactor,
        "Test" => TaskCategory::Test,
        "Doc" => TaskCategory::Doc,
        other => {
            if let Some(inner) = other.strip_prefix("Other(").and_then(|s| s.strip_suffix(")")) {
                TaskCategory::Other(inner.to_string())
            } else {
                TaskCategory::Other(other.to_string())
            }
        }
    }
}

fn parse_task_source(source_type: &str, source_json: &str) -> TaskSource {
    match source_type {
        "auto" => {
            let v: serde_json::Value = serde_json::from_str(source_json).unwrap_or_default();
            TaskSource::Auto {
                commit_sha: v.get("commit_sha").and_then(|s| s.as_str()).unwrap_or("").into(),
            }
        }
        "manual" => {
            let v: serde_json::Value = serde_json::from_str(source_json).unwrap_or_default();
            TaskSource::Manual {
                created_by: v.get("created_by").and_then(|s| s.as_str()).unwrap_or("").into(),
            }
        }
        "derived" => {
            let v: serde_json::Value = serde_json::from_str(source_json).unwrap_or_default();
            TaskSource::Derived {
                parent_ids: v.get("parent_ids")
                    .and_then(|a| a.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
            }
        }
        _ => TaskSource::Manual { created_by: "unknown".into() },
    }
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::types::*;

    fn temp_store() -> TimelineStore {
        let dir = std::env::temp_dir().join(format!("paporot_test_{}", uuid::Uuid::new_v4()));
        TimelineStore::open(&dir).unwrap()
    }

    fn sample_task() -> TaskSpec {
        TaskSpec {
            id: "test-fix-auth".into(),
            description: "修复认证绕过漏洞".into(),
            success_criteria: vec!["test_empty_pw_rejected 通过".into()],
            category: TaskCategory::BugFix,
            modules: vec!["src/auth".into()],
            source: TaskSource::Auto { commit_sha: "abc123".into() },
        }
    }

    fn sample_eval(task: &TaskSpec) -> EvalResult {
        EvalResult {
            eval_id: "eval_001".into(),
            task: task.clone(),
            trial_index: 1,
            transcript: None,
            tool_pattern: None,
            outcome: OutcomeVerdict::Pass,
            grader_results: vec![],
            code_change: CodeChangeSummary::default(),
            created_at: "2026-06-29T10:00:00Z".into(),
            git_event_id: Some("git_001".into()),
            session_id: None,
            tags: vec![],
        }
    }

    #[test]
    fn test_save_and_load_eval() {
        let store = temp_store();
        let task = sample_task();
        store.save_task(&task).unwrap();
        let eval = sample_eval(&task);
        store.save_eval(&eval).unwrap();

        let loaded = store.load_eval("eval_001").unwrap();
        assert_eq!(loaded.eval_id, "eval_001");
        assert!(loaded.outcome.is_pass());
        assert_eq!(loaded.task.id, "test-fix-auth");
    }

    #[test]
    fn test_list_trials() {
        let store = temp_store();
        let task = sample_task();
        store.save_task(&task).unwrap();

        for i in 1..=3 {
            let mut eval = sample_eval(&task);
            eval.eval_id = format!("eval_{:03}", i);
            eval.trial_index = i as u32;
            store.save_eval(&eval).unwrap();
        }

        let trials = store.list_trials("test-fix-auth").unwrap();
        assert_eq!(trials.len(), 3);
        assert_eq!(trials[0].trial_index, 1);
        assert_eq!(trials[2].trial_index, 3);
    }

    #[test]
    fn test_latest_trial() {
        let store = temp_store();
        let task = sample_task();
        store.save_task(&task).unwrap();

        for i in 1..=2 {
            let mut eval = sample_eval(&task);
            eval.eval_id = format!("eval_{:03}", i);
            eval.trial_index = i;
            store.save_eval(&eval).unwrap();
        }

        let latest = store.latest_trial("test-fix-auth").unwrap().unwrap();
        assert_eq!(latest.trial_index, 2);
    }

    #[test]
    fn test_list_tasks() {
        let store = temp_store();
        store.save_task(&sample_task()).unwrap();

        let mut task2 = sample_task();
        task2.id = "add-feature".into();
        task2.description = "添加新功能".into();
        task2.category = TaskCategory::Feature;
        store.save_task(&task2).unwrap();

        let tasks = store.list_tasks().unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn test_git_event() {
        let store = temp_store();
        store.save_git_event(
            "git_001", "abc123", "fix auth", "dev", "2026-06-29T10:00:00Z", "diff content", None,
        ).unwrap();

        let event = store.find_git_event_by_commit("abc123").unwrap().unwrap();
        assert_eq!(event.commit_sha, "abc123");
        assert_eq!(event.commit_message, "fix auth");

        let not_found = store.find_git_event_by_commit("nonexistent").unwrap();
        assert!(not_found.is_none());
    }
}
