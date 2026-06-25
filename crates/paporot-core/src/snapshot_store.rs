//! Snapshot 持久化存储 — SnapshotStore
//!
//! 负责 Snapshot 的读写、版本管理、列表查询。纯 I/O 层，不包含分析逻辑。
//!
//! 对应 PRD §3.2：SnapshotStore + SnapshotAnalyzer 拆分。

use anyhow::{Context, Result};
use std::path::PathBuf;
use crate::types::BehaviorSnapshot;

/// Snapshot 存储管理器（纯存储，零分析逻辑）
#[derive(Clone)]
pub struct SnapshotStore {
    dir: PathBuf,
}

impl SnapshotStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn init(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("Failed to create snapshot dir: {}", self.dir.display()))?;
        Ok(())
    }

    pub fn save(&self, snapshot: &BehaviorSnapshot) -> Result<PathBuf> {
        self.init()?;
        let filepath = self.snapshot_path(&snapshot.version_id);
        let json = serde_json::to_string_pretty(snapshot)
            .context("Failed to serialize snapshot")?;
        std::fs::write(&filepath, json)
            .with_context(|| format!("Failed to write snapshot: {}", filepath.display()))?;
        Ok(filepath)
    }

    pub fn load_by_version(&self, version_id: &str) -> Result<BehaviorSnapshot> {
        let filepath = self.snapshot_path(version_id);
        let json = std::fs::read_to_string(&filepath)
            .with_context(|| format!("Snapshot not found: {} (version {})", filepath.display(), version_id))?;
        serde_json::from_str(&json)
            .with_context(|| format!("Failed to parse snapshot: {}", filepath.display()))
    }

    pub fn load_latest(&self) -> Result<BehaviorSnapshot> {
        let versions = self.list_versions_sorted()?;
        if versions.is_empty() {
            anyhow::bail!("No snapshots found in {}. Run analyze first.", self.dir.display());
        }
        let latest = versions.last().unwrap();
        self.load_by_version(latest)
    }

    pub fn list_versions_sorted(&self) -> Result<Vec<String>> {
        if !self.dir.exists() {
            return Ok(vec![]);
        }
        let mut entries: Vec<String> = std::fs::read_dir(&self.dir)
            .with_context(|| format!("Failed to read dir: {}", self.dir.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
            .filter_map(|e| {
                e.path()
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
            })
            .filter(|s| s.starts_with('v'))
            .collect();

        entries.sort_by_key(|s| {
            s.trim_start_matches('v')
                .parse::<u32>()
                .unwrap_or(0)
        });
        Ok(entries)
    }

    pub fn next_version_id(&self) -> Result<String> {
        let versions = self.list_versions_sorted()?;
        let next = versions.len() as u32 + 1;
        Ok(format!("v{}", next))
    }

    fn snapshot_path(&self, version_id: &str) -> PathBuf {
        self.dir.join(format!("{}.json", version_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn make_snapshot(version: &str) -> BehaviorSnapshot {
        BehaviorSnapshot {
            schema_version: 3,
            version_id: version.into(),
            git_commit: Some("abc123".into()),
            git_ref: None,
            timestamp: "2026-06-11T10:00:00Z".into(),
            message: "test".into(),
            capabilities: vec![],
            prd_coverage: PrdCoverage {
                percentage: 0.0,
                total_items: 0,
                covered_items: None,
                details: vec![],
            },
            regression: None,
            risk: None,
            metadata: None,
        }
    }

    #[test]
    fn test_save_and_load() {
        let dir = std::env::temp_dir().join("Paporot_test_store");
        let _ = std::fs::remove_dir_all(&dir);

        let store = SnapshotStore::new(&dir);
        let snap = make_snapshot("v1");
        store.save(&snap).unwrap();

        let loaded = store.load_by_version("v1").unwrap();
        assert_eq!(loaded.version_id, "v1");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_next_version_id() {
        let dir = std::env::temp_dir().join("Paporot_test_store2");
        let _ = std::fs::remove_dir_all(&dir);

        let store = SnapshotStore::new(&dir);
        store.save(&make_snapshot("v1")).unwrap();
        store.save(&make_snapshot("v2")).unwrap();
        assert_eq!(store.next_version_id().unwrap(), "v3");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_list_empty() {
        let dir = std::env::temp_dir().join("Paporot_test_store3");
        let _ = std::fs::remove_dir_all(&dir);

        let store = SnapshotStore::new(&dir);
        let versions = store.list_versions_sorted().unwrap();
        assert!(versions.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_list_sorted() {
        let dir = std::env::temp_dir().join("Paporot_test_store4");
        let _ = std::fs::remove_dir_all(&dir);

        let store = SnapshotStore::new(&dir);
        store.save(&make_snapshot("v10")).unwrap();
        store.save(&make_snapshot("v2")).unwrap();
        store.save(&make_snapshot("v1")).unwrap();

        let versions = store.list_versions_sorted().unwrap();
        assert_eq!(versions, vec!["v1", "v2", "v10"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_latest() {
        let dir = std::env::temp_dir().join("Paporot_test_store5");
        let _ = std::fs::remove_dir_all(&dir);

        let store = SnapshotStore::new(&dir);
        store.save(&make_snapshot("v1")).unwrap();
        store.save(&make_snapshot("v3")).unwrap();
        store.save(&make_snapshot("v2")).unwrap();

        let latest = store.load_latest().unwrap();
        assert_eq!(latest.version_id, "v3");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
