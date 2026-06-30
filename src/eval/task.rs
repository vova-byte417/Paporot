//! TaskManager —— Task 自动创建与管理
//!
//! 职责：
//! - 从 git commit 自动创建 Task
//! - 手动创建/命名 Task
//! - 查询 Task 历史
//! - 管理 Task 元数据

use anyhow::{Context, Result};
use std::path::Path;
use uuid::Uuid;

use super::types::*;
use crate::storage::timeline::TimelineStore;
use crate::storage::cache::CacheManager;

// ─── TaskManager ───────────────────────────────────────────────────

pub struct TaskManager {
    store: TimelineStore,
    _cache: CacheManager,
}

impl TaskManager {
    /// 从 .Paporot 目录初始化
    pub fn new(paporot_dir: &Path) -> Result<Self> {
        let store = TimelineStore::open(paporot_dir)?;
        let cache = CacheManager::new(paporot_dir);
        Ok(Self { store, _cache: cache })
    }

    // ─── 自动从 Git Commit 创建 Task ──────────────────────────────

    /// 从当前 HEAD commit 自动创建 Task
    pub fn auto_create(&self, commit_sha: Option<&str>) -> Result<TaskSpec> {
        let commit = if let Some(sha) = commit_sha {
            get_commit_info(sha)?
        } else {
            get_commit_info("HEAD")?
        };

        let task_id = format!("task_{}", truncate_sha(&commit.sha, 8));
        let category = infer_category(&commit.message);
        let modules = Vec::new(); // CodeExporter 会在后续填充

        let task = TaskSpec {
            id: task_id,
            description: first_line(&commit.message),
            success_criteria: vec![],
            category,
            modules,
            source: TaskSource::Auto {
                commit_sha: commit.sha.clone(),
            },
        };

        self.store.save_task(&task)?;

        // 记录 GitEvent
        self.store.save_git_event(
            &format!("git_{}", Uuid::new_v4()),
            &commit.sha,
            &commit.message,
            &commit.author,
            &commit.timestamp,
            &commit.diff,
            Some(&task.id),
        )?;

        Ok(task)
    }

    // ─── 手动创建 Task ─────────────────────────────────────────────

    /// 手动创建 Task
    pub fn create(
        &self,
        description: &str,
        category: TaskCategory,
        modules: Vec<String>,
        success_criteria: Vec<String>,
    ) -> Result<TaskSpec> {
        let task_id = format!("task_{}", Uuid::new_v4().to_string().split('-').next().unwrap_or("manual"));

        let task = TaskSpec {
            id: task_id,
            description: description.to_string(),
            success_criteria,
            category,
            modules,
            source: TaskSource::Manual {
                created_by: "user".into(),
            },
        };

        self.store.save_task(&task)?;
        Ok(task)
    }

    // ─── 查询 ──────────────────────────────────────────────────────

    /// 加载 Task
    pub fn load(&self, task_id: &str) -> Result<Option<TaskSpec>> {
        self.store.load_task(task_id)
    }

    /// 列出所有 Task
    pub fn list(&self) -> Result<Vec<TaskSpec>> {
        self.store.list_tasks()
    }

    /// 获取 TimelineStore 引用（供其他模块使用）
    pub fn store(&self) -> &TimelineStore {
        &self.store
    }
}

// ─── Git Helpers ───────────────────────────────────────────────────

struct CommitInfo {
    sha: String,
    message: String,
    author: String,
    timestamp: String,
    diff: String,
}

fn get_commit_info(refspec: &str) -> Result<CommitInfo> {
    let sha = run_git(&["rev-parse", refspec])?;
    let message = run_git(&["log", "-1", "--format=%s", refspec])
        .unwrap_or_else(|_| "No commit message".into());
    let author = run_git(&["log", "-1", "--format=%an", refspec])
        .unwrap_or_else(|_| "unknown".into());
    let timestamp = run_git(&["log", "-1", "--format=%aI", refspec])
        .unwrap_or_else(|_| chrono::Utc::now().to_rfc3339());
    let diff = if refspec == "HEAD" {
        run_git(&["diff", "HEAD~1..HEAD"])
            .unwrap_or_else(|_| String::new())
    } else {
        run_git(&["diff", &format!("{}~1..{}", refspec, refspec)])
            .unwrap_or_else(|_| String::new())
    };

    Ok(CommitInfo {
        sha, message, author, timestamp, diff,
    })
}

fn run_git(args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .output()
        .context("Failed to execute git — are you in a git repo?")?;

    if !output.status.success() {
        anyhow::bail!("git command failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn first_line(msg: &str) -> String {
    msg.lines().next().unwrap_or("No description").to_string()
}

fn truncate_sha(sha: &str, len: usize) -> String {
    sha.chars().take(len).collect()
}

fn infer_category(msg: &str) -> TaskCategory {
    let lower = msg.to_lowercase();
    if lower.contains("fix") || lower.contains("bug") || lower.contains("hotfix") {
        TaskCategory::BugFix
    } else if lower.contains("refactor") || lower.contains("clean") {
        TaskCategory::Refactor
    } else if lower.contains("test") || lower.contains("spec") {
        TaskCategory::Test
    } else if lower.contains("doc") || lower.contains("readme") {
        TaskCategory::Doc
    } else if lower.contains("feat") || lower.contains("add") || lower.contains("feature") {
        TaskCategory::Feature
    } else {
        TaskCategory::Other("general".into())
    }
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_category_bugfix() {
        assert!(matches!(infer_category("fix: auth bypass"), TaskCategory::BugFix));
        assert!(matches!(infer_category("hotfix: critical bug"), TaskCategory::BugFix));
    }

    #[test]
    fn test_infer_category_feature() {
        assert!(matches!(infer_category("feat: add login"), TaskCategory::Feature));
        assert!(matches!(infer_category("add: user registration"), TaskCategory::Feature));
    }

    #[test]
    fn test_infer_category_refactor() {
        assert!(matches!(infer_category("refactor: clean auth"), TaskCategory::Refactor));
    }

    #[test]
    fn test_infer_category_test() {
        assert!(matches!(infer_category("test: add spec"), TaskCategory::Test));
    }

    #[test]
    fn test_infer_category_doc() {
        assert!(matches!(infer_category("docs: update readme"), TaskCategory::Doc));
    }

    #[test]
    fn test_infer_category_other() {
        assert!(matches!(infer_category("wip"), TaskCategory::Other(_)));
    }

    #[test]
    fn test_first_line() {
        assert_eq!(first_line("fix: auth\n\nDetails here"), "fix: auth");
        assert_eq!(first_line(""), "No description");
    }

    #[test]
    fn test_truncate_sha() {
        assert_eq!(truncate_sha("abc123def456", 8), "abc123de");
    }
}
