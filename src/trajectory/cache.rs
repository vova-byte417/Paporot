//! Trajectory Diff 缓存层：SQLite 存储 Derived Data。
//!
//! 与 trace_index.db 分离，独立管理 trajectory_cache.db。

use rusqlite::Connection;
use std::path::PathBuf;
use std::fs;

use super::error::TrajectoryError;
use super::types::TrajectoryDiff;

/// 缓存管理：SQLite + JSON 导出。
#[derive(Clone)]
pub struct TrajectoryCache {
    db_path: PathBuf,
    export_dir: PathBuf,
}

impl TrajectoryCache {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let base = base_dir.into();
        Self {
            db_path: base.join("trajectory_cache.db"),
            export_dir: base.join("trajectory"),
        }
    }

    /// 初始化缓存目录和数据库。幂等。
    pub fn init(&self) -> Result<(), TrajectoryError> {
        fs::create_dir_all(&self.export_dir).map_err(|e| {
            TrajectoryError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to create trajectory export dir: {}", e),
            ))
        })?;

        let conn = self.open_db()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS trajectory_diffs (
                id                  TEXT PRIMARY KEY,
                trace_id_a          TEXT NOT NULL,
                trace_id_b          TEXT NOT NULL,
                capability_id       TEXT,
                diff_json           TEXT NOT NULL,
                analysis_json       TEXT NOT NULL,
                mermaid_text        TEXT,
                classifier_name     TEXT NOT NULL,
                classifier_version  TEXT NOT NULL,
                computed_at         TEXT NOT NULL,
                score_tool_churn    REAL NOT NULL DEFAULT 0,
                score_phase_reorder REAL NOT NULL DEFAULT 0,
                score_capability_shift REAL NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_diffs_trace_a ON trajectory_diffs(trace_id_a);
            CREATE INDEX IF NOT EXISTS idx_diffs_trace_b ON trajectory_diffs(trace_id_b);
            CREATE INDEX IF NOT EXISTS idx_diffs_capability ON trajectory_diffs(capability_id);",
        )
        .map_err(|e| TrajectoryError::CacheError(format!(
            "Failed to init trajectory_cache.db: {}", e
        )))?;

        Ok(())
    }

    fn open_db(&self) -> Result<Connection, TrajectoryError> {
        Connection::open(&self.db_path).map_err(|e| {
            TrajectoryError::CacheError(format!("Failed to open trajectory_cache.db: {}", e))
        })
    }

    /// 存储 diff、analysis 和 mermaid 到缓存。
    pub fn store(
        &self,
        id: &str,
        trace_id_a: &str,
        trace_id_b: &str,
        capability_id: Option<&str>,
        diff: &TrajectoryDiff,
        analysis_json: &str,
        mermaid: &str,
        classifier_name: &str,
        classifier_version: &str,
        tool_churn: f32,
        phase_reorder: f32,
        capability_shift: f32,
    ) -> Result<(), TrajectoryError> {
        let conn = self.open_db()?;
        let diff_json =
            serde_json::to_string(diff).map_err(|e| TrajectoryError::Json(e))?;
        let computed_at = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT OR REPLACE INTO trajectory_diffs
             (id, trace_id_a, trace_id_b, capability_id, diff_json, analysis_json,
              mermaid_text, classifier_name, classifier_version, computed_at,
              score_tool_churn, score_phase_reorder, score_capability_shift)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                id, trace_id_a, trace_id_b, capability_id,
                diff_json, analysis_json, mermaid,
                classifier_name, classifier_version, computed_at,
                tool_churn, phase_reorder, capability_shift,
            ],
        )
        .map_err(|e| TrajectoryError::CacheError(format!("Failed to store diff: {}", e)))?;

        // Also export to JSON file for dashboard consumption
        let export_path = self.export_dir.join(format!("{}.json", id));
        let dashboard_json = serde_json::json!({
            "id": id,
            "capability_id": capability_id,
            "trace_id_a": trace_id_a,
            "trace_id_b": trace_id_b,
            "diff": diff,
            "analysis": serde_json::from_str::<serde_json::Value>(analysis_json).unwrap_or_default(),
            "mermaid": mermaid,
            "computed_at": computed_at,
        });
        let json_str = serde_json::to_string_pretty(&dashboard_json)
            .map_err(|e| TrajectoryError::Json(e))?;
        fs::write(&export_path, json_str)
            .map_err(|e| TrajectoryError::Io(e))?;

        Ok(())
    }

    /// 按 trace 对查询缓存（命中返回 diff JSON 字符串）。
    pub fn lookup(
        &self,
        trace_id_a: &str,
        trace_id_b: &str,
    ) -> Result<Option<String>, TrajectoryError> {
        let conn = self.open_db()?;
        let mut stmt = conn
            .prepare(
                "SELECT diff_json FROM trajectory_diffs
                 WHERE trace_id_a = ?1 AND trace_id_b = ?2
                 ORDER BY computed_at DESC LIMIT 1",
            )
            .map_err(|e| TrajectoryError::CacheError(format!("Failed to prepare lookup: {}", e)))?;

        let result = stmt
            .query_row(rusqlite::params![trace_id_a, trace_id_b], |row| {
                row.get::<_, String>(0)
            })
            .optional()
            .map_err(|e| TrajectoryError::CacheError(format!("Lookup failed: {}", e)))?;

        result.map_or(
            Ok(None),
            |json| Ok(Some(json)),
        )
    }
}

// Helper: convert rusqlite error result to Option
trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
