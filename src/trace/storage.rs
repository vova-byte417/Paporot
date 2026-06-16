//! Trace 持久化存储层。
//!
//! 双写架构:
//!   - JSONL 文件: 权威数据源（单行 JSON，追加写入）
//!   - SQLite 索引: 加速查询（通过 byte_offset 回源 JSONL 读详情）

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::trace::error::TraceError;
use crate::trace::types::{BehaviorTrace, ImportResult, TraceFilter, TraceSummary, TraceSource};

/// Trace 存储管理器。
#[derive(Clone)]
pub struct TraceStorage {
    dir: PathBuf,
    db_path: PathBuf,
}

impl TraceStorage {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let base = base_dir.into();
        Self {
            dir: base.join("traces"),
            db_path: base.join("trace_index.db"),
        }
    }

    // ─── 生命周期 ─────────────────────────────────────────────

    /// 初始化存储目录和 SQLite 数据库。幂等操作。
    pub fn init(&self) -> Result<(), TraceError> {
        fs::create_dir_all(&self.dir).map_err(|e| TraceError::Io {
            message: format!("Failed to create trace dir {}: {}", self.dir.display(), e),
        })?;
        self.init_db()?;
        self.ensure_gitignore()?;
        Ok(())
    }

    fn init_db(&self) -> Result<(), TraceError> {
        let conn = self.open_db()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS traces (
                id              TEXT PRIMARY KEY,
                session_id      TEXT NOT NULL,
                tool_names      TEXT NOT NULL DEFAULT '[]',
                prompt_preview  TEXT DEFAULT '',
                started_at      TEXT NOT NULL,
                finished_at     TEXT NOT NULL,
                duration_ms     INTEGER NOT NULL DEFAULT 0,
                input_tokens    INTEGER NOT NULL DEFAULT 0,
                output_tokens   INTEGER NOT NULL DEFAULT 0,
                source_type     TEXT NOT NULL,
                adapter_name    TEXT DEFAULT NULL,
                file_path       TEXT NOT NULL,
                byte_offset     INTEGER NOT NULL,
                capability_ids  TEXT DEFAULT '[]',
                tags            TEXT DEFAULT '[]',
                deleted         INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_traces_session ON traces(session_id);
            CREATE INDEX IF NOT EXISTS idx_traces_started ON traces(started_at);
            CREATE INDEX IF NOT EXISTS idx_traces_deleted ON traces(deleted);",
        )
        .map_err(|e| TraceError::Database {
            message: format!("Failed to initialize SQLite schema: {}", e),
        })?;
        Ok(())
    }

    fn ensure_gitignore(&self) -> Result<(), TraceError> {
        let gitignore_path = self
            .dir
            .parent()
            .unwrap_or(Path::new("."))
            .join(".gitignore");
        let entries = ["traces/", "trace_index.db"];

        let mut existing = if gitignore_path.exists() {
            fs::read_to_string(&gitignore_path).unwrap_or_default()
        } else {
            String::new()
        };

        let mut changed = false;
        for entry in &entries {
            if !existing.lines().any(|l| l.trim() == *entry) {
                existing.push_str(entry);
                existing.push('\n');
                changed = true;
            }
        }

        if changed {
            fs::write(&gitignore_path, existing).map_err(|e| TraceError::Io {
                message: format!("Failed to write .gitignore: {}", e),
            })?;
        }
        Ok(())
    }

    fn open_db(&self) -> Result<Connection, TraceError> {
        Connection::open(&self.db_path).map_err(|e| TraceError::Database {
            message: format!(
                "Failed to open SQLite database {}: {}",
                self.db_path.display(),
                e
            ),
        })
    }

    // ─── 写入 ─────────────────────────────────────────────────

    /// 保存单条 BehaviorTrace。JSONL 追加 + SQLite 索引同步写入。
    pub fn save(&self, trace: &BehaviorTrace) -> Result<PathBuf, TraceError> {
        self.init()?;
        let mut trace = trace.clone();

        if trace.id.is_empty() {
            trace.id = self.next_id()?;
        }

        let jsonl_path = self.current_jsonl_file()?;
        let json_line = serde_json::to_string(&trace).map_err(|e| TraceError::Serialize {
            message: format!("Failed to serialize trace {}: {}", trace.id, e),
        })?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&jsonl_path)
            .map_err(|e| TraceError::Io {
                message: format!("Failed to open {}: {}", jsonl_path.display(), e),
            })?;

        let byte_offset = file.metadata().map(|m| m.len()).unwrap_or(0);
        writeln!(file, "{}", json_line).map_err(|e| TraceError::Io {
            message: format!("Failed to write to {}: {}", jsonl_path.display(), e),
        })?;

        self.insert_index(&trace, &jsonl_path, byte_offset)?;

        Ok(jsonl_path)
    }

    /// 批量保存多条 BehaviorTrace。
    pub fn save_batch(&self, traces: Vec<BehaviorTrace>) -> Result<ImportResult, TraceError> {
        let mut imported = Vec::new();
        let mut skipped = 0usize;
        let mut skip_reasons = Vec::new();

        for trace in &traces {
            match self.save(trace) {
                Ok(_) => {
                    imported.push(self.trace_to_summary(trace));
                }
                Err(e) => {
                    skipped += 1;
                    skip_reasons.push(format!("{}: {}", trace.id, e));
                }
            }
        }

        Ok(ImportResult {
            source_path: String::new(),
            adapter: String::new(),
            auto_detected: false,
            imported,
            skipped_count: skipped,
            skip_reasons,
        })
    }

    fn insert_index(
        &self,
        trace: &BehaviorTrace,
        file_path: &Path,
        byte_offset: u64,
    ) -> Result<(), TraceError> {
        let conn = self.open_db()?;

        let tool_names: HashSet<&str> = trace
            .tool_calls
            .iter()
            .map(|tc| tc.tool_name.as_str())
            .collect();
        let tool_names_json = serde_json::to_string(&tool_names.iter().collect::<Vec<_>>())
            .unwrap_or_else(|_| "[]".to_string());

        let prompt_preview = if trace.prompt.len() > 200 {
            format!("{}...", &trace.prompt[..200])
        } else {
            trace.prompt.clone()
        };

        let duration_ms = self.calc_duration_ms(&trace.started_at, &trace.finished_at);

        let source_type = match &trace.source {
            TraceSource::Imported { .. } => "imported",
            TraceSource::Captured { .. } => "captured",
        };

        let adapter_name = match &trace.source {
            TraceSource::Imported { adapter, .. } => Some(adapter.as_str()),
            _ => None,
        };

        let capability_ids_json =
            serde_json::to_string(&trace.capability_ids).unwrap_or_else(|_| "[]".to_string());
        let tags_json =
            serde_json::to_string(&trace.tags).unwrap_or_else(|_| "[]".to_string());

        conn.execute(
            "INSERT OR REPLACE INTO traces
                (id, session_id, tool_names, prompt_preview, started_at, finished_at,
                 duration_ms, input_tokens, output_tokens, source_type, adapter_name,
                 file_path, byte_offset, capability_ids, tags, deleted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            rusqlite::params![
                trace.id,
                trace.session_id,
                tool_names_json,
                prompt_preview,
                trace.started_at,
                trace.finished_at,
                duration_ms as i64,
                trace.token_usage.input_tokens as i64,
                trace.token_usage.output_tokens as i64,
                source_type,
                adapter_name,
                file_path.to_string_lossy().to_string(),
                byte_offset as i64,
                capability_ids_json,
                tags_json,
                if trace.deleted { 1 } else { 0 },
            ],
        )
        .map_err(|e| TraceError::Database {
            message: format!("Failed to insert trace index for {}: {}", trace.id, e),
        })?;

        Ok(())
    }

    // ─── 读取 ─────────────────────────────────────────────────

    /// 按 ID 加载单条 trace 完整内容。
    pub fn load(&self, id: &str) -> Result<BehaviorTrace, TraceError> {
        let conn = self.open_db()?;

        let (file_path_str, byte_offset): (String, i64) = conn
            .query_row(
                "SELECT file_path, byte_offset FROM traces WHERE id = ?1 AND deleted = 0",
                rusqlite::params![id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(|_| TraceError::NotFound {
                message: format!("Trace {} not found", id),
            })?;

        let content =
            fs::read_to_string(&file_path_str).map_err(|e| TraceError::Io {
                message: format!("Failed to read {}: {}", file_path_str, e),
            })?;

        let target = &content[byte_offset as usize..];
        let line = target.lines().next().ok_or_else(|| TraceError::NotFound {
            message: format!(
                "Trace {} data not found at offset {} in {}",
                id, byte_offset, file_path_str
            ),
        })?;

        serde_json::from_str(line).map_err(|e| TraceError::Serialize {
            message: format!("Failed to parse trace {}: {}", id, e),
        })
    }

    /// 按 session_id 加载该 session 的所有 trace。
    pub fn load_by_session(&self, session_id: &str) -> Result<Vec<BehaviorTrace>, TraceError> {
        let conn = self.open_db()?;

        let mut stmt = conn
            .prepare(
                "SELECT file_path, byte_offset
                 FROM traces
                 WHERE session_id = ?1 AND deleted = 0
                 ORDER BY started_at ASC",
            )
            .map_err(|e| TraceError::Database {
                message: format!("Failed to query traces by session: {}", e),
            })?;

        let rows: Vec<(String, i64)> = stmt
            .query_map(rusqlite::params![session_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|e| TraceError::Database {
                message: format!("Failed to read trace rows: {}", e),
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut traces = Vec::new();
        for (file_path_str, byte_offset) in &rows {
            let content = fs::read_to_string(file_path_str).map_err(|e| TraceError::Io {
                message: format!("Failed to read {}: {}", file_path_str, e),
            })?;
            let target = &content[*byte_offset as usize..];
            if let Some(line) = target.lines().next() {
                if let Ok(trace) = serde_json::from_str::<BehaviorTrace>(line) {
                    traces.push(trace);
                }
            }
        }
        Ok(traces)
    }

    /// 按过滤条件列出 trace 摘要。
    pub fn list(&self, filter: &TraceFilter) -> Result<Vec<TraceSummary>, TraceError> {
        let conn = self.open_db()?;

        let mut sql = String::from(
            "SELECT id, session_id, tool_names, prompt_preview, started_at, finished_at,
                    duration_ms, input_tokens, output_tokens, source_type, adapter_name,
                    capability_ids, tags, deleted
             FROM traces WHERE 1=1",
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if !filter.include_deleted {
            sql.push_str(" AND deleted = 0");
        }
        if let Some(ref sid) = filter.session_id {
            sql.push_str(" AND session_id = ?1");
            params.push(Box::new(sid.clone()));
        }
        if let Some(ref tool) = filter.tool_name {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND tool_names LIKE ?{}", idx));
            params.push(Box::new(format!("%{}%", tool)));
        }
        if let Some(ref tag) = filter.tag {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND tags LIKE ?{}", idx));
            params.push(Box::new(format!("%{}%", tag)));
        }
        if let Some(ref cap_id) = filter.capability_id {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND capability_ids LIKE ?{}", idx));
            params.push(Box::new(format!("%{}%", cap_id)));
        }
        if let Some(ref from) = filter.from_date {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND started_at >= ?{}", idx));
            params.push(Box::new(from.clone()));
        }
        if let Some(ref to) = filter.to_date {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND started_at <= ?{}", idx));
            params.push(Box::new(format!("{}T23:59:59Z", to)));
        }
        if let Some(ref st) = filter.source_type {
            let idx = params.len() + 1;
            sql.push_str(&format!(" AND source_type = ?{}", idx));
            params.push(Box::new(st.clone()));
        }

        sql.push_str(" ORDER BY started_at DESC");

        if filter.limit > 0 {
            sql.push_str(&format!(" LIMIT {}", filter.limit));
        }
        if filter.offset > 0 {
            sql.push_str(&format!(" OFFSET {}", filter.offset));
        }

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| TraceError::Database {
            message: format!("Failed to prepare trace list query: {}", e),
        })?;

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                let tool_names_json: String = row.get(2)?;
                let tool_names: Vec<String> =
                    serde_json::from_str(&tool_names_json).unwrap_or_default();
                let caps_json: String = row.get(11)?;
                let caps: Vec<String> =
                    serde_json::from_str(&caps_json).unwrap_or_default();
                let tags_json: String = row.get(12)?;
                let tags: Vec<String> =
                    serde_json::from_str(&tags_json).unwrap_or_default();
                let input: i64 = row.get(7)?;
                let output: i64 = row.get(8)?;
                let dur: i64 = row.get(6)?;
                let deleted_flag: i32 = row.get(13)?;

                Ok(TraceSummary {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    tool_names,
                    prompt_preview: row.get::<_, String>(3).unwrap_or_default(),
                    tool_call_count: 0,
                    total_tokens: (input + output) as u64,
                    started_at: row.get(4)?,
                    finished_at: row.get(5)?,
                    duration_ms: dur as u64,
                    source_type: row.get(9)?,
                    adapter_name: row.get::<_, Option<String>>(10).unwrap_or(None),
                    capability_count: caps.len(),
                    tags,
                    deleted: deleted_flag != 0,
                })
            })
            .map_err(|e| TraceError::Database {
                message: format!("Failed to query trace list: {}", e),
            })?;

        let summaries: Result<Vec<_>, _> = rows.collect();
        summaries.map_err(|e| TraceError::Database {
            message: format!("Failed to collect trace summaries: {}", e),
        })
    }

    // ─── 删除 ─────────────────────────────────────────────────

    /// Soft delete 一条 trace。
    pub fn delete(&self, id: &str) -> Result<(), TraceError> {
        let conn = self.open_db()?;
        let affected = conn
            .execute(
                "UPDATE traces SET deleted = 1 WHERE id = ?1",
                rusqlite::params![id],
            )
            .map_err(|e| TraceError::Database {
                message: format!("Failed to delete trace {}: {}", id, e),
            })?;

        if affected == 0 {
            return Err(TraceError::NotFound {
                message: format!("Trace {} not found for deletion", id),
            });
        }
        Ok(())
    }

    // ─── 实用方法 ─────────────────────────────────────────────

    /// 生成下一个 trace ID。
    pub fn next_id(&self) -> Result<String, TraceError> {
        let conn = self.open_db()?;
        let date = chrono_now_date();
        let date_no_dash = date.replace('-', "");
        let pattern = format!("trace_{}%", date_no_dash);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE id LIKE ?1",
                rusqlite::params![pattern],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(format!("trace_{}_{:03}", date_no_dash, count + 1))
    }

    /// 获取当前活动的 JSONL 文件路径。
    fn current_jsonl_file(&self) -> Result<PathBuf, TraceError> {
        let date = chrono_now_date();
        let mut seq = 1u32;

        loop {
            let path = self.dir.join(format!("{}-{:03}.jsonl", date, seq));
            if !path.exists() {
                return Ok(path);
            }
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if size < 100 * 1024 * 1024 {
                return Ok(path);
            }
            seq += 1;
            if seq > 999 {
                return Err(TraceError::Io {
                    message: format!("Too many JSONL files for date {}", date),
                });
            }
        }
    }

    fn calc_duration_ms(&self, started_at: &str, finished_at: &str) -> u64 {
        use chrono::DateTime;
        let start = DateTime::parse_from_rfc3339(started_at);
        let end = DateTime::parse_from_rfc3339(finished_at);
        match (start, end) {
            (Ok(s), Ok(e)) => {
                let dur = e - s;
                dur.num_milliseconds().max(0) as u64
            }
            _ => 0,
        }
    }

    fn trace_to_summary(&self, trace: &BehaviorTrace) -> TraceSummary {
        let tool_names: Vec<String> = trace
            .tool_calls
            .iter()
            .map(|tc| tc.tool_name.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let prompt_preview = if trace.prompt.len() > 200 {
            format!("{}...", &trace.prompt[..200])
        } else {
            trace.prompt.clone()
        };

        let source_type = match &trace.source {
            TraceSource::Imported { .. } => "imported".to_string(),
            TraceSource::Captured { .. } => "captured".to_string(),
        };

        let adapter_name = match &trace.source {
            TraceSource::Imported { adapter, .. } => Some(adapter.clone()),
            _ => None,
        };

        TraceSummary {
            id: trace.id.clone(),
            session_id: trace.session_id.clone(),
            prompt_preview,
            tool_names,
            tool_call_count: trace.tool_calls.len(),
            total_tokens: trace.token_usage.input_tokens + trace.token_usage.output_tokens,
            started_at: trace.started_at.clone(),
            finished_at: trace.finished_at.clone(),
            duration_ms: self.calc_duration_ms(&trace.started_at, &trace.finished_at),
            source_type,
            adapter_name,
            capability_count: trace.capability_ids.len(),
            tags: trace.tags.clone(),
            deleted: trace.deleted,
        }
    }
}

fn chrono_now_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

// ─── 测试 ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_storage() -> (TraceStorage, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().join(".Paporot");
        let storage = TraceStorage::new(&base);
        storage.init().unwrap();
        (storage, tmp)
    }

    fn make_trace(id: &str, session: &str) -> BehaviorTrace {
        BehaviorTrace {
            id: id.to_string(),
            session_id: session.to_string(),
            prompt: "Test prompt".into(),
            tool_calls: vec![ToolCall {
                id: format!("call_{}_001", id),
                tool_name: "grep".into(),
                args: serde_json::json!({"pattern": "test"}),
                timestamp: "2026-06-12T14:00:00Z".into(),
                duration_ms: 100,
                result_id: Some(format!("obs_{}_001", id)),
            }],
            observations: vec![Observation {
                id: format!("obs_{}_001", id),
                tool_call_id: format!("call_{}_001", id),
                content: "result".into(),
                truncated: false,
                truncated_at_bytes: None,
            }],
            final_output: "done".into(),
            token_usage: Default::default(),
            started_at: "2026-06-12T14:00:00Z".into(),
            finished_at: "2026-06-12T14:01:00Z".into(),
            source: TraceSource::Captured {
                agent_name: "test-agent".into(),
            },
            tags: Vec::new(),
            capability_ids: Vec::new(),
            deleted: false,
        }
    }

    #[test]
    fn test_save_and_load() {
        let (storage, _tmp) = create_test_storage();
        let trace = make_trace("", "sess-001");
        let path = storage.save(&trace).unwrap();
        assert!(path.exists());

        // id should have been auto-assigned
        let saved = storage.load(&trace.id).unwrap_or_else(|_| {
            // If id was auto-assigned, load by listing
            let all = storage.list(&TraceFilter::default()).unwrap();
            assert_eq!(all.len(), 1);
            storage.load(&all[0].id).unwrap()
        });
        assert_eq!(saved.session_id, "sess-001");
        assert_eq!(saved.prompt, "Test prompt");
    }

    #[test]
    fn test_load_nonexistent() {
        let (storage, _tmp) = create_test_storage();
        let result = storage.load("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_empty() {
        let (storage, _tmp) = create_test_storage();
        let results = storage.list(&TraceFilter::default()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_list_with_filter() {
        let (storage, _tmp) = create_test_storage();

        let mut t1 = make_trace("trace_test_001", "sess-a");
        t1.tags = vec!["security".into()];
        storage.save(&t1).unwrap();

        let mut t2 = make_trace("trace_test_002", "sess-b");
        t2.tags = vec!["performance".into()];
        storage.save(&t2).unwrap();

        // All
        let all = storage.list(&TraceFilter::default()).unwrap();
        assert_eq!(all.len(), 2);

        // Filter by session
        let by_session = storage
            .list(&TraceFilter {
                session_id: Some("sess-a".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(by_session.len(), 1);

        // Filter by tag
        let by_tag = storage
            .list(&TraceFilter {
                tag: Some("security".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(by_tag.len(), 1);
    }

    #[test]
    fn test_soft_delete() {
        let (storage, _tmp) = create_test_storage();
        let trace = make_trace("trace_del_001", "sess-a");
        storage.save(&trace).unwrap();

        storage.delete("trace_del_001").unwrap();

        // Default: not visible
        let all = storage.list(&TraceFilter::default()).unwrap();
        assert!(all.is_empty());

        // With include_deleted: visible
        let all = storage
            .list(&TraceFilter {
                include_deleted: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].deleted);
    }

    #[test]
    fn test_delete_nonexistent() {
        let (storage, _tmp) = create_test_storage();
        let result = storage.delete("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_next_id_format() {
        let (storage, _tmp) = create_test_storage();
        let id = storage.next_id().unwrap();
        assert!(id.starts_with("trace_"));
        assert!(id.ends_with("_001"));
    }

    #[test]
    fn test_save_batch() {
        let (storage, _tmp) = create_test_storage();
        let traces = vec![make_trace("", "sess-a"), make_trace("", "sess-b")];
        let result = storage.save_batch(traces).unwrap();
        assert_eq!(result.imported.len(), 2);
        assert_eq!(result.skipped_count, 0);
    }

    #[test]
    fn test_load_by_session() {
        let (storage, _tmp) = create_test_storage();
        storage.save(&make_trace("trace_sess_001", "sess-x")).unwrap();
        storage.save(&make_trace("trace_sess_002", "sess-x")).unwrap();
        storage.save(&make_trace("trace_sess_003", "sess-y")).unwrap();

        let results = storage.load_by_session("sess-x").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_date_filter() {
        let (storage, _tmp) = create_test_storage();
        storage.save(&make_trace("trace_date_001", "sess-a")).unwrap();

        // Filter with wide range
        let results = storage
            .list(&TraceFilter {
                from_date: Some("2020-01-01".into()),
                to_date: Some("2030-01-01".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(!results.is_empty());
    }

    use crate::trace::types::{Observation, ToolCall};
}
