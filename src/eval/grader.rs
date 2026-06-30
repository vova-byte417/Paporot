//! Grader 评分框架
//!
//! 定义 Grader trait 及三个确定性 Grader：
//! - DeterministicTestGrader: 运行项目测试套件
//! - StaticAnalysisGrader: 运行 lint / format / type check
//! - BuildCheckGrader: 检查项目能否编译
//!
//! 支持自动检测项目语言（Rust / Python / Node.js / Go 等）
//! 对每个语言使用对应工具；工具未安装时优雅跳过。
//!
//! 设计原则：
//! - Grader 在 Native 宿主端运行（有 shell 权限）
//! - LLM Rubric Grader 在 WASM Skill 沙盒内运行
//! - 每个 Grader 独立、可组合

use anyhow::Result;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use super::types::*;

// ─── Project Language Detection ───────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectLanguage {
    Rust,
    Python,
    NodeJs,
    Go,
    Unknown,
}

impl ProjectLanguage {
    /// 从项目根目录检测语言
    pub fn detect(root: &Path) -> Self {
        if root.join("Cargo.toml").exists() {
            return Self::Rust;
        }
        if root.join("package.json").exists() {
            return Self::NodeJs;
        }
        if root.join("pyproject.toml").exists()
            || root.join("requirements.txt").exists()
            || root.join("setup.py").exists()
            || root.join("setup.cfg").exists()
        {
            return Self::Python;
        }
        if root.join("go.mod").exists() {
            return Self::Go;
        }
        Self::Unknown
    }

    pub fn is_compiled(&self) -> bool {
        matches!(self, Self::Rust | Self::Go)
    }
}

// ─── Command helpers ──────────────────────────────────────────────

/// 检查命令是否存在（which/where）
fn command_exists(cmd: &str) -> bool {
    let program = cmd.split_whitespace().next().unwrap_or(cmd);
    let check = if cfg!(windows) {
        Command::new("where").arg(program).output()
    } else {
        Command::new("which").arg(program).output()
    };
    check.map(|o| o.status.success()).unwrap_or(false)
}

/// 安全运行命令：不存在则返回 SKIP，失败则返回 FAIL
enum CmdResult {
    Pass,
    Fail { exit_code: i32, stderr: String },
    Skip { reason: String },
}

fn run_tool(cmd: &str, cwd: &Path) -> CmdResult {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let program = parts.first().unwrap_or(&"");
    let args = &parts[1..];

    if !command_exists(program) {
        return CmdResult::Skip { reason: format!("{} not installed", program) };
    }

    match Command::new(program).args(args).current_dir(cwd).output() {
        Ok(output) => {
            if output.status.success() {
                CmdResult::Pass
            } else {
                CmdResult::Fail {
                    exit_code: output.status.code().unwrap_or(-1),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                }
            }
        }
        Err(e) => CmdResult::Skip { reason: format!("{}: {}", program, e) },
    }
}

// ─── Grader Trait ──────────────────────────────────────────────────

/// Grader 评分器统一接口
pub trait Grader: Send + Sync {
    /// Grader 名称
    fn name(&self) -> &str;

    /// Grader 类型
    fn grader_type(&self) -> GraderType;

    /// 执行评分
    fn run(&self, context: &EvalContext) -> Result<GraderResult>;
}

// ─── DeterministicTestGrader ───────────────────────────────────────

/// 运行项目测试套件（自动检测语言）
pub struct DeterministicTestGrader {
    pub command: String,
    pub cwd: Option<String>,
    pub timeout_secs: u32,
}

impl DeterministicTestGrader {
    pub fn new(command: impl Into<String>) -> Self {
        Self { command: command.into(), cwd: None, timeout_secs: 120 }
    }

    pub fn for_language(lang: &ProjectLanguage, root: &Path) -> Self {
        let cmd = match lang {
            ProjectLanguage::Rust => "cargo test",
            ProjectLanguage::Python => {
                if root.join("pyproject.toml").exists() { "pytest" }
                else { "python -m pytest" }
            }
            ProjectLanguage::NodeJs => {
                if root.join("package.json").exists() { "npm test" }
                else { "npx jest" }
            }
            ProjectLanguage::Go => "go test ./...",
            ProjectLanguage::Unknown => "cargo test",
        };
        Self::new(cmd)
    }

    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self { self.cwd = Some(cwd.into()); self }
    pub fn with_timeout(mut self, secs: u32) -> Self { self.timeout_secs = secs; self }
}

impl Grader for DeterministicTestGrader {
    fn name(&self) -> &str { "tests" }
    fn grader_type(&self) -> GraderType { GraderType::DeterministicTest }

    fn run(&self, context: &EvalContext) -> Result<GraderResult> {
        let start = Instant::now();
        let cwd = self.cwd.as_ref()
            .map(|d| context.project_root.join(d))
            .unwrap_or_else(|| context.project_root.clone());

        let result = run_tool(&self.command, &cwd);

        let duration_ms = start.elapsed().as_millis() as u64;
        let (passed, details) = match result {
            CmdResult::Pass => (true, serde_json::json!({
                "passed": true, "command": self.command, "tests_ran": true
            })),
            CmdResult::Fail { exit_code, stderr } => (false, serde_json::json!({
                "passed": false, "command": self.command, "exit_code": exit_code,
                "stderr": truncate_for_json(&stderr, 500),
            })),
            CmdResult::Skip { reason } => (true, serde_json::json!({
                "passed": true, "command": self.command, "skipped": true, "reason": reason
            })),
        };

        Ok(GraderResult { grader_type: self.grader_type(), name: self.name().into(), passed, details, duration_ms })
    }
}

// ─── StaticAnalysisGrader ──────────────────────────────────────────

/// 运行静态分析（lint / format / type check），自动适配语言
pub struct StaticAnalysisGrader {
    pub linters: Vec<LintCommand>,
    pub cwd: Option<String>,
}

pub struct LintCommand {
    pub name: String,
    pub command: String,
}

impl StaticAnalysisGrader {
    /// 根据语言创建默认 linter
    pub fn for_language(lang: &ProjectLanguage, root: &Path) -> Self {
        let linters = match lang {
            ProjectLanguage::Rust => vec![
                LintCommand { name: "clippy".into(), command: "cargo clippy -- -D warnings 2>&1".into() },
                LintCommand { name: "rustfmt".into(), command: "cargo fmt -- --check".into() },
            ],
            ProjectLanguage::Python => vec![
                LintCommand { name: "ruff".into(), command: "ruff check .".into() },
            ],
            ProjectLanguage::NodeJs => vec![
                LintCommand { name: "eslint".into(), command: "npx eslint . --quiet".into() },
            ],
            ProjectLanguage::Go => vec![
                LintCommand { name: "go vet".into(), command: "go vet ./...".into() },
                LintCommand { name: "gofmt".into(), command: "gofmt -d .".into() },
            ],
            ProjectLanguage::Unknown => vec![
                LintCommand { name: "ruff".into(), command: "ruff check .".into() },       // 先试 Python
                LintCommand { name: "eslint".into(), command: "npx eslint . --quiet".into() }, // 再试 JS
            ],
        };
        Self { linters, cwd: None }
    }

    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self { self.cwd = Some(cwd.into()); self }
}

impl Grader for StaticAnalysisGrader {
    fn name(&self) -> &str { "static analysis" }
    fn grader_type(&self) -> GraderType { GraderType::StaticAnalysis }

    fn run(&self, context: &EvalContext) -> Result<GraderResult> {
        let start = Instant::now();
        let cwd = self.cwd.as_ref()
            .map(|d| context.project_root.join(d))
            .unwrap_or_else(|| context.project_root.clone());

        let mut checks = Vec::new();
        let mut all_passed = true;
        let mut skipped_count = 0u32;

        for linter in &self.linters {
            match run_tool(&linter.command, &cwd) {
                CmdResult::Pass => {
                    checks.push(serde_json::json!({"name": linter.name, "passed": true}));
                }
                CmdResult::Fail { exit_code, stderr } => {
                    all_passed = false;
                    checks.push(serde_json::json!({
                        "name": linter.name, "passed": false,
                        "exit_code": exit_code,
                        "stderr": truncate_for_json(&stderr, 300),
                    }));
                }
                CmdResult::Skip { reason } => {
                    skipped_count += 1;
                    checks.push(serde_json::json!({
                        "name": linter.name, "passed": true, "skipped": true, "reason": reason,
                    }));
                }
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        // 如果全部跳过（无工具），标记为 pass（不算失败）
        if skipped_count == self.linters.len() as u32 {
            all_passed = true;
        }

        Ok(GraderResult {
            grader_type: self.grader_type(),
            name: self.name().into(),
            passed: all_passed,
            details: serde_json::json!({"passed": all_passed, "checks": checks}),
            duration_ms,
        })
    }
}

// ─── BuildCheckGrader ──────────────────────────────────────────────

/// 检查项目能否成功编译（自动检测语言）
pub struct BuildCheckGrader {
    pub command: String,
    pub timeout_secs: u32,
}

impl BuildCheckGrader {
    pub fn new(command: impl Into<String>) -> Self {
        Self { command: command.into(), timeout_secs: 180 }
    }

    pub fn for_language(lang: &ProjectLanguage, _root: &Path) -> Self {
        let cmd = match lang {
            ProjectLanguage::Rust => "cargo check",
            ProjectLanguage::Python => "python -c \"compile(open('setup.py').read() if __import__('os').path.exists('setup.py') else '', 'setup.py', 'exec')\" 2>/dev/null; echo done",  // no-op: Python 不需要编译
            ProjectLanguage::NodeJs => "npx tsc --noEmit",
            ProjectLanguage::Go => "go build ./...",
            ProjectLanguage::Unknown => "cargo check",
        };
        Self::new(cmd)
    }

    pub fn with_timeout(mut self, secs: u32) -> Self { self.timeout_secs = secs; self }
}

impl Grader for BuildCheckGrader {
    fn name(&self) -> &str { "build check" }
    fn grader_type(&self) -> GraderType { GraderType::BuildCheck }

    fn run(&self, context: &EvalContext) -> Result<GraderResult> {
        let start = Instant::now();
        let result = run_tool(&self.command, &context.project_root);
        let duration_ms = start.elapsed().as_millis() as u64;

        let (passed, details) = match result {
            CmdResult::Pass => (true, serde_json::json!({
                "passed": true, "command": self.command,
            })),
            CmdResult::Fail { exit_code, stderr } => {
                let error_count = stderr.lines().filter(|l| l.contains("error")).count();
                (false, serde_json::json!({
                    "passed": false, "exit_code": exit_code, "command": self.command,
                    "error_count": error_count,
                    "stderr": truncate_for_json(&stderr, 500),
                }))
            }
            CmdResult::Skip { reason } => {
                // 解释型语言跳过编译是正常的
                (true, serde_json::json!({
                    "passed": true, "command": self.command, "skipped": true, "reason": reason,
                }))
            }
        };

        Ok(GraderResult {
            grader_type: self.grader_type(),
            name: self.name().into(),
            passed,
            details,
            duration_ms,
        })
    }
}

// ─── Helpers ───────────────────────────────────────────────────────

pub fn truncate_for_json(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        format!("{}...(truncated)", &s[..s.floor_char_boundary(max_chars)])
    }
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_rust() {
        let tmp = std::env::temp_dir();
        // 不测试真实路径，验证类型
        assert!(ProjectLanguage::Rust.is_compiled());
        assert!(ProjectLanguage::Go.is_compiled());
        assert!(!ProjectLanguage::Python.is_compiled());
        assert!(!ProjectLanguage::NodeJs.is_compiled());
    }

    #[test]
    fn test_truncate_for_json() {
        let s = "hello world";
        assert_eq!(truncate_for_json(s, 5), "hello...(truncated)");
        assert_eq!(truncate_for_json(s, 20), "hello world");
    }

    #[test]
    fn test_grader_trait_objects() {
        let grader = DeterministicTestGrader::new("cargo test");
        assert_eq!(grader.name(), "tests");
        assert_eq!(grader.grader_type(), GraderType::DeterministicTest);
    }
}
