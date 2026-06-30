//! EvalRunner —— 评估编排器
//!
//! 编排 Grader 执行序列：
//! 1. CodeExporter 提取代码变更
//! 2. 运行确定性 Graders（test/lint/build）
//! 3. 计算 OutcomeVerdict
//! 4. 生成 EvalResult 并保存到 Timeline

use anyhow::{Context, Result};
use std::path::Path;

use super::exporter::CodeExporter;
use super::grader::{BuildCheckGrader, DeterministicTestGrader, Grader, ProjectLanguage, StaticAnalysisGrader};
use super::task::TaskManager;
use super::types::*;
use crate::storage::timeline::TimelineStore;

// ─── EvalRunner ────────────────────────────────────────────────────

pub struct EvalRunner {
    task_manager: TaskManager,
    store: TimelineStore,
    project_root: std::path::PathBuf,
}

impl EvalRunner {
    /// 从项目根目录初始化
    pub fn new(project_root: &Path) -> Result<Self> {
        let paporot_dir = project_root.join(".Paporot");
        let task_manager = TaskManager::new(&paporot_dir)?;
        let store = TimelineStore::open(&paporot_dir)?;

        Ok(Self {
            task_manager,
            store,
            project_root: project_root.to_path_buf(),
        })
    }

    /// 获取 TaskManager 引用
    pub fn task(&self) -> &TaskManager {
        &self.task_manager
    }

    /// 获取 TimelineStore 引用
    pub fn store(&self) -> &TimelineStore {
        &self.store
    }

    // ─── eval auto ──────────────────────────────────────────────────

    /// 自动评估最新 commit
    pub async fn eval_auto(&self, commit_sha: Option<&str>) -> Result<EvalResult> {
        let paporot_dir = self.project_root.join(".Paporot");

        // 1. 自动创建 Task
        println!("  [Eval] Auto-creating task from git commit...");
        let task = self.task_manager.auto_create(commit_sha)?;
        println!("  [Eval] Task: {} — {}", task.id, task.description);

        // 2. 提取代码变更
        println!("  [Eval] Exporting code changes...");
        let exporter = CodeExporter::new(&paporot_dir);
        let git_diff = if let Some(sha) = commit_sha {
            get_git_diff_range(&format!("{}~1..{}", sha, sha))?
        } else {
            get_git_diff_range("HEAD~1..HEAD")?
        };

        let code_change = exporter.export(&git_diff)?;
        println!(
            "  [Eval] {} files, +{}/-{} lines, {} symbols",
            code_change.files_changed.len(),
            code_change.additions,
            code_change.deletions,
            code_change.symbols_added.len() + code_change.symbols_removed.len()
        );

        // 3. 检测项目语言
        let lang = ProjectLanguage::detect(&self.project_root);
        println!("  [Eval] Project language: {:?}", lang);

        // 4. 运行确定性 Graders
        println!("  [Eval] Running graders...");
        let context = EvalContext {
            project_root: self.project_root.clone(),
            paporot_dir: paporot_dir.clone(),
            cache_dir: paporot_dir.join("cache"),
            commit_sha: commit_sha.map(String::from),
            diff_content: git_diff,
        };

        let mut grader_results = Vec::new();
        let mut grader_names_failed = Vec::new();
        let mut passed_count = 0u32;
        let mut total_count = 0u32;

        // Build Check — 仅编译型语言
        if lang.is_compiled() {
            print!("    - Build check: ");
            let grader = BuildCheckGrader::for_language(&lang, &self.project_root);
            match grader.run(&context) {
                Ok(result) => {
                    total_count += 1;
                    let label = if result.passed { "PASS" } else { "FAIL" };
                    println!("{}", label);
                    if result.passed { passed_count += 1; } else { grader_names_failed.push("build".into()); }
                    grader_results.push(result);
                }
                Err(e) => {
                    println!("ERROR ({})", e);
                    grader_names_failed.push("build".into());
                    total_count += 1;
                }
            }
        } else {
            println!("    - Build check: SKIP (interpreted language: {:?})", lang);
        }

        // Static Analysis
        {
            print!("    - Static analysis: ");
            let grader = StaticAnalysisGrader::for_language(&lang, &self.project_root);
            match grader.run(&context) {
                Ok(result) => {
                    total_count += 1;
                    let label = if result.passed { "PASS" } else { "FAIL" };
                    println!("{}", label);
                    if result.passed { passed_count += 1; } else { grader_names_failed.push("lint".into()); }
                    grader_results.push(result);
                }
                Err(e) => {
                    println!("ERROR ({})", e);
                    grader_names_failed.push("lint".into());
                    total_count += 1;
                }
            }
        }

        // Tests
        {
            print!("    - Tests: ");
            let grader = DeterministicTestGrader::for_language(&lang, &self.project_root);
            match grader.run(&context) {
                Ok(result) => {
                    total_count += 1;
                    let label = if result.passed { "PASS" } else { "FAIL" };
                    println!("{}", label);
                    if result.passed { passed_count += 1; } else { grader_names_failed.push("test".into()); }
                    grader_results.push(result);
                }
                Err(e) => {
                    println!("SKIP ({})", e);
                }
            }
        }

        // 5. 计算 Outcome
        let outcome = if total_count == 0 {
            OutcomeVerdict::NotEvaluated {
                reason: "No graders ran successfully".into(),
            }
        } else if grader_names_failed.is_empty() {
            OutcomeVerdict::Pass
        } else if passed_count > 0 {
            OutcomeVerdict::Partial {
                passed: passed_count,
                total: total_count,
                failures: grader_names_failed.clone(),
            }
        } else {
            OutcomeVerdict::Fail {
                failing_graders: grader_names_failed.clone(),
                summary: format!("{} / {} graders failed", grader_names_failed.len(), total_count),
            }
        };

        // 6. 组装 EvalResult
        let eval_id = format!("eval_{}", uuid::Uuid::new_v4());
        let eval = EvalResult {
            eval_id,
            task,
            trial_index: 1,
            transcript: None,
            tool_pattern: None,
            outcome,
            grader_results,
            code_change,
            created_at: chrono::Utc::now().to_rfc3339(),
            git_event_id: None,
            session_id: None,
            tags: vec![],
        };

        // 7. 保存
        self.store.save_eval(&eval)?;
        println!("  [Eval] EvalResult {} saved", eval.eval_id);
        println!("  [Eval] Outcome: {}", eval.outcome.label());

        Ok(eval)
    }

    /// 返回项目根目录
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }
}

// ─── Helpers ───────────────────────────────────────────────────────

fn get_git_diff_range(range: &str) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["diff", range])
        .output()
        .context("Failed to execute git diff — are you in a git repo?")?;

    if !output.status.success() {
        anyhow::bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runner_creation() {
        let runner = EvalRunner::new(Path::new(".")).unwrap();
        assert!(runner.project_root().exists());
    }
}
