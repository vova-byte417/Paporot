//! Trace ↔ Snapshot 三级自动匹配算法
//!
//! 对应 PRD §3.6：将 Agent 行为轨迹与代码版本 Snapshot 自动关联。
//!
//! ## 匹配策略（三级回退）
//!
//! 1. **Git commit hash 精确匹配** — 置信度 1.0
//! 2. **文件重叠度 Jaccard 相似度** — 置信度 [0, 1]
//! 3. **时间窗口最近匹配** — 置信度按时间距离衰减
//!
//! ## 匹配结果持久化
//!
//! 写入 `.Paporot/snapshots/trace_map.json`：
//! ```json
//! {
//!   "snapshot_version": [
//!     { "trace_id": "trace_001", "confidence": 1.0, "match_level": "commit" }
//!   ]
//! }
//! ```

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ─── Match Types ─────────────────────────────────────────────────

/// 单条匹配记录
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TraceSnapshotMatch {
    pub trace_id: String,
    pub confidence: f32,
    pub match_level: MatchLevel,
}

/// 匹配等级
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MatchLevel {
    /// 1st: commit hash 精确匹配
    Commit,
    /// 2nd: 文件重叠度 Jaccard
    FileOverlap,
    /// 3rd: 时间窗口最近
    TimeWindow,
}

/// trace_map.json 的顶层结构
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TraceSnapshotMap {
    /// snapshot_version_id → Vec<TraceSnapshotMatch>
    pub mappings: HashMap<String, Vec<TraceSnapshotMatch>>,
}

impl TraceSnapshotMap {
    pub fn load_or_new(path: &PathBuf) -> Self {
        if let Ok(json) = std::fs::read_to_string(path) {
            serde_json::from_str(&json).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, &json)?;
        Ok(())
    }

    pub fn get_traces_for_snapshot(&self, version_id: &str) -> Vec<&TraceSnapshotMatch> {
        self.mappings.get(version_id).map(|v| v.iter().collect()).unwrap_or_default()
    }
}

// ─── Matcher ─────────────────────────────────────────────────────

pub struct TraceMatcher;

impl TraceMatcher {
    /// L1: commit hash 匹配
    pub fn match_by_commit(
        trace_commit: Option<&str>,
        snapshot_commit: Option<&str>,
    ) -> Option<TraceSnapshotMatch> {
        match (trace_commit, snapshot_commit) {
            (Some(tc), Some(sc)) if tc == sc => {
                Some(TraceSnapshotMatch {
                    trace_id: String::new(), // caller sets
                    confidence: 1.0,
                    match_level: MatchLevel::Commit,
                })
            }
            _ => None,
        }
    }

    /// L2: 文件重叠度 Jaccard 相似度
    ///
    /// Jaccard = |A ∩ B| / |A ∪ B|
    pub fn match_by_file_overlap(
        trace_files: &[String],
        snapshot_files: &[String],
    ) -> Option<f32> {
        if trace_files.is_empty() || snapshot_files.is_empty() {
            return None;
        }

        let trace_set: std::collections::HashSet<_> = trace_files.iter().collect();
        let snap_set: std::collections::HashSet<_> = snapshot_files.iter().collect();

        let intersection = trace_set.intersection(&snap_set).count();
        let union = trace_set.union(&snap_set).count();

        let jaccard = intersection as f32 / union as f32;

        if jaccard > 0.0 {
            Some(jaccard)
        } else {
            None
        }
    }

    /// L3: 时间窗口匹配
    ///
    /// 返回按时间距离的置信度（最远 24h → 0.0，最接近 0s → 1.0）
    pub fn match_by_time_window(
        trace_time: &str,
        snapshot_time: &str,
    ) -> Option<f32> {
        let t = chrono::DateTime::parse_from_rfc3339(trace_time).ok()?;
        let s = chrono::DateTime::parse_from_rfc3339(snapshot_time).ok()?;

        let delta = (t - s).num_seconds().abs();
        let max_window = 86400i64; // 24h
        let confidence = 1.0 - (delta as f32 / max_window as f32).min(1.0);

        if confidence > 0.0 {
            Some(confidence)
        } else {
            None
        }
    }

    /// 三级匹配：依次尝试 L1 → L2 → L3
    pub fn match_trace_to_snapshot(
        trace_id: &str,
        trace_commit: Option<&str>,
        trace_time: &str,
        trace_files: &[String],
        snapshot_version_id: &str,
        snapshot_commit: Option<&str>,
        snapshot_time: &str,
        snapshot_files: &[String],
    ) -> Option<TraceSnapshotMatch> {
        // L1: commit hash
        if let Some(mut m) = Self::match_by_commit(trace_commit, snapshot_commit) {
            m.trace_id = trace_id.to_string();
            return Some(m);
        }

        // L2: file overlap
        if let Some(confidence) = Self::match_by_file_overlap(trace_files, snapshot_files) {
            return Some(TraceSnapshotMatch {
                trace_id: trace_id.to_string(),
                confidence,
                match_level: MatchLevel::FileOverlap,
            });
        }

        // L3: time window
        if let Some(confidence) = Self::match_by_time_window(trace_time, snapshot_time) {
            return Some(TraceSnapshotMatch {
                trace_id: trace_id.to_string(),
                confidence,
                match_level: MatchLevel::TimeWindow,
            });
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commit_match_exact() {
        let result = TraceMatcher::match_by_commit(Some("abc123"), Some("abc123"));
        assert!(result.is_some());
        assert_eq!(result.unwrap().match_level, MatchLevel::Commit);
    }

    #[test]
    fn test_commit_match_mismatch() {
        let result = TraceMatcher::match_by_commit(Some("abc123"), Some("def456"));
        assert!(result.is_none());
    }

    #[test]
    fn test_commit_match_none() {
        let result = TraceMatcher::match_by_commit(None, Some("abc123"));
        assert!(result.is_none());
    }

    #[test]
    fn test_file_overlap_full() {
        let trace = vec!["a.rs".into(), "b.rs".into()];
        let snap = vec!["a.rs".into(), "b.rs".into()];
        let score = TraceMatcher::match_by_file_overlap(&trace, &snap).unwrap();
        assert!((score - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_file_overlap_partial() {
        let trace = vec!["a.rs".into(), "b.rs".into(), "c.rs".into()];
        let snap = vec!["a.rs".into(), "d.rs".into()];
        let score = TraceMatcher::match_by_file_overlap(&trace, &snap).unwrap();
        assert!((score - 0.25).abs() < 0.01); // |{a}| / |{a,b,c,d}| = 1/4
    }

    #[test]
    fn test_file_overlap_none() {
        let trace = vec!["a.rs".into()];
        let snap = vec!["b.rs".into()];
        let score = TraceMatcher::match_by_file_overlap(&trace, &snap);
        assert!(score.is_none());
    }

    #[test]
    fn test_time_window_close() {
        let score = TraceMatcher::match_by_time_window(
            "2026-06-24T10:00:00Z",
            "2026-06-24T10:05:00Z",
        ).unwrap();
        assert!(score > 0.99); // 5 min diff = near 1.0
    }

    #[test]
    fn test_time_window_far() {
        let score = TraceMatcher::match_by_time_window(
            "2026-06-24T10:00:00Z",
            "2026-06-25T10:00:00Z",
        );
        // 24h = confidence drops to 0, so None
        assert!(score.is_none());
    }

    #[test]
    fn test_full_pipeline_commit() {
        let result = TraceMatcher::match_trace_to_snapshot(
            "t1", Some("abc"), "2026-06-24T10:00:00Z", &[],
            "v1", Some("abc"), "2026-06-24T10:00:00Z", &[],
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().match_level, MatchLevel::Commit);
    }

    #[test]
    fn test_full_pipeline_file_overlap() {
        let result = TraceMatcher::match_trace_to_snapshot(
            "t2", Some("abc"), "2026-06-24T10:00:00Z", &["src/main.rs".into()],
            "v2", Some("def"), "2026-06-24T10:00:00Z", &["src/main.rs".into(), "src/lib.rs".into()],
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().match_level, MatchLevel::FileOverlap);
    }

    #[test]
    fn test_trace_map_persistence() {
        let dir = std::env::temp_dir().join("Paporot_test_trace_map");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("trace_map.json");
        let mut map = TraceSnapshotMap::default();
        map.mappings.insert("v1".into(), vec![TraceSnapshotMatch {
            trace_id: "t1".into(),
            confidence: 1.0,
            match_level: MatchLevel::Commit,
        }]);
        map.save(&path).unwrap();

        let loaded = TraceSnapshotMap::load_or_new(&path);
        let matches = loaded.get_traces_for_snapshot("v1");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].trace_id, "t1");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
