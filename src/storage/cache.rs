//! .Paporot/cache/ 结构化数据读写
//!
//! 缓存目录是 Native 宿主（CodeExporter / Graders）与 WASM Skill 之间的数据交换层。
//! 宿主写入，Skill 只读。
//!
//! 目录结构：
//! .Paporot/cache/
//!   code_change.json      — 最新 commit 的 CodeChangeSummary
//!   test_results.json     — 确定性测试 Grader 结果
//!   lint_results.json     — 静态分析 Grader 结果
//!   build_results.json    — 构建检查结果
//!   eval_context.json     — 当前评估上下文
//!   skill_output/         — Skill 输出目录（Skill 写入）

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::eval::types::*;

// ─── CacheManager ─────────────────────────────────────────────────

pub struct CacheManager {
    cache_dir: PathBuf,
}

impl CacheManager {
    /// 创建缓存管理器
    pub fn new(paporot_dir: &Path) -> Self {
        Self {
            cache_dir: paporot_dir.join("cache"),
        }
    }

    /// 确保缓存目录存在
    pub fn init(&self) -> Result<()> {
        std::fs::create_dir_all(&self.cache_dir)
            .context("Failed to create cache directory")?;
        std::fs::create_dir_all(self.cache_dir.join("skill_output"))
            .context("Failed to create skill_output directory")?;
        Ok(())
    }

    // ─── 写入（宿主侧） ──────────────────────────────────────────

    /// 写入 CodeChangeSummary
    pub fn write_code_change(&self, summary: &CodeChangeSummary) -> Result<()> {
        self.init()?;
        let json = serde_json::to_string_pretty(summary)?;
        std::fs::write(self.cache_dir.join("code_change.json"), json)
            .context("Failed to write code_change.json")?;
        Ok(())
    }

    /// 写入测试结果
    pub fn write_test_results(&self, result: &GraderResult) -> Result<()> {
        self.init()?;
        let json = serde_json::to_string_pretty(result)?;
        std::fs::write(self.cache_dir.join("test_results.json"), json)
            .context("Failed to write test_results.json")?;
        Ok(())
    }

    /// 写入 Lint 结果
    pub fn write_lint_results(&self, result: &GraderResult) -> Result<()> {
        self.init()?;
        let json = serde_json::to_string_pretty(result)?;
        std::fs::write(self.cache_dir.join("lint_results.json"), json)
            .context("Failed to write lint_results.json")?;
        Ok(())
    }

    /// 写入构建结果
    pub fn write_build_results(&self, result: &GraderResult) -> Result<()> {
        self.init()?;
        let json = serde_json::to_string_pretty(result)?;
        std::fs::write(self.cache_dir.join("build_results.json"), json)
            .context("Failed to write build_results.json")?;
        Ok(())
    }

    /// 写入评估上下文
    pub fn write_eval_context(&self, context: &EvalContext) -> Result<()> {
        self.init()?;
        let json = serde_json::json!({
            "project_root": context.project_root.to_string_lossy(),
            "paporot_dir": context.paporot_dir.to_string_lossy(),
            "cache_dir": context.cache_dir.to_string_lossy(),
            "commit_sha": context.commit_sha,
            "diff_length": context.diff_content.len(),
        });
        std::fs::write(
            self.cache_dir.join("eval_context.json"),
            serde_json::to_string_pretty(&json)?,
        ).context("Failed to write eval_context.json")?;
        Ok(())
    }

    /// 写入任意 JSON 数据到缓存（通用接口）
    pub fn write_json(&self, key: &str, data: &serde_json::Value) -> Result<()> {
        self.init()?;
        let json = serde_json::to_string_pretty(data)?;
        std::fs::write(self.cache_dir.join(format!("{}.json", key)), json)
            .context(format!("Failed to write {}.json", key))?;
        Ok(())
    }

    // ─── 读取（Skill 侧读取，也用于宿主） ────────────────────────

    /// 读取 CodeChangeSummary
    pub fn read_code_change(&self) -> Result<Option<CodeChangeSummary>> {
        self.read_json_file::<CodeChangeSummary>("code_change.json")
    }

    /// 读取测试结果
    pub fn read_test_results(&self) -> Result<Option<GraderResult>> {
        self.read_json_file::<GraderResult>("test_results.json")
    }

    /// 读取 Lint 结果
    pub fn read_lint_results(&self) -> Result<Option<GraderResult>> {
        self.read_json_file::<GraderResult>("lint_results.json")
    }

    /// 读取构建结果
    pub fn read_build_results(&self) -> Result<Option<GraderResult>> {
        self.read_json_file::<GraderResult>("build_results.json")
    }

    /// 读取任意 JSON 文件（通用接口）
    pub fn read_json(&self, key: &str) -> Result<Option<serde_json::Value>> {
        self.read_json_file::<serde_json::Value>(&format!("{}.json", key))
    }

    // ─── 清理 ─────────────────────────────────────────────────────

    /// 清除所有缓存文件
    pub fn clear(&self) -> Result<()> {
        if self.cache_dir.exists() {
            std::fs::remove_dir_all(&self.cache_dir)
                .context("Failed to clear cache")?;
        }
        self.init()?;
        Ok(())
    }

    /// 获取缓存目录路径（供 Skill Host 使用）
    pub fn dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    // ─── 内部 ─────────────────────────────────────────────────────

    fn read_json_file<T: serde::de::DeserializeOwned>(&self, filename: &str) -> Result<Option<T>> {
        let path = self.cache_dir.join(filename);
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let value: T = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(Some(value))
    }
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cache() -> CacheManager {
        let dir = std::env::temp_dir().join(format!("paporot_cache_test_{}", uuid::Uuid::new_v4()));
        CacheManager::new(&dir)
    }

    #[test]
    fn test_write_and_read_code_change() {
        let cache = temp_cache();
        let summary = CodeChangeSummary {
            files_changed: vec!["src/main.rs".into()],
            additions: 10,
            deletions: 3,
            ..Default::default()
        };

        cache.write_code_change(&summary).unwrap();
        let loaded = cache.read_code_change().unwrap().unwrap();
        assert_eq!(loaded.additions, 10);
        assert_eq!(loaded.files_changed, vec!["src/main.rs"]);
    }

    #[test]
    fn test_missing_file_returns_none() {
        let cache = temp_cache();
        let result = cache.read_code_change().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_write_and_read_test_results() {
        let cache = temp_cache();
        let result = GraderResult {
            grader_type: GraderType::DeterministicTest,
            name: "cargo test".into(),
            passed: true,
            details: serde_json::json!({"total": 42, "passed": 42}),
            duration_ms: 3200,
        };

        cache.write_test_results(&result).unwrap();
        let loaded = cache.read_test_results().unwrap().unwrap();
        assert!(loaded.passed);
        assert_eq!(loaded.name, "cargo test");
    }

    #[test]
    fn test_clear() {
        let cache = temp_cache();
        cache.write_code_change(&CodeChangeSummary::default()).unwrap();
        assert!(cache.dir().join("code_change.json").exists());

        cache.clear().unwrap();
        assert!(!cache.dir().join("code_change.json").exists());
    }
}
