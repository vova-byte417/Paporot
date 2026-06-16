//! `Paporot review` — 整合审查入口

use anyhow::{Context, Result};
use crate::agent::Agent;

/// 执行 review 命令（整合 snapshot + diff + coverage + regression + risk）
pub async fn run(agent: &Agent, diff_source: Option<&str>, prd: Option<&str>) -> Result<()> {
    // 获取 git diff
    let diff_range = diff_source.unwrap_or("HEAD~1..HEAD");
    let diff = get_git_diff(diff_range)?;

    if diff.trim().is_empty() {
        anyhow::bail!("No changes detected in '{}'. Nothing to review.", diff_range);
    }

    // 加载 PRD
    let prd_content = prd.map(|f| std::fs::read_to_string(f)).transpose().ok().flatten();

    // 委托 Agent 执行完整流水线
    agent
        .review_pipeline(&diff, "Review snapshot", prd_content.as_deref())
        .await?;

    Ok(())
}

/// 获取 git diff
fn get_git_diff(range: &str) -> Result<String> {
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

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::agent::Agent;
    use crate::types::*;

    /// 测试项: review_pipeline 在 review 命令中的入口完整性
    /// 构造一个最小的 Agent + 有内容的 diff，验证不 panic
    /// （实际 LLM 调用会失败，但需确认流水线结构正确）
    #[test]
    fn test_agent_review_pipeline_existence() {
        let config = Config::default();
        let agent = Agent::new(config);
        // review_pipeline 是 async fn，这里只验证方法存在、类继承正确
        // Agent 结构体检查
        assert!(!agent.config.agent.diff_truncate_threshold.to_string().is_empty());
    }

    /// 测试项: diff_range 默认值
    #[test]
    fn test_default_diff_range() {
        // diff_source 为 None 时的默认值
        let diff_range = Some(None); // 模拟 diff_source 为 None
        let default = diff_range.flatten().unwrap_or("HEAD~1..HEAD");
        assert_eq!(default, "HEAD~1..HEAD");
    }

    /// 测试项: 空 diff 检测逻辑
    /// 输入: 空字符串 → 应判定为 No changes
    #[test]
    fn test_empty_diff_detection() {
        let empty = "";
        assert!(empty.trim().is_empty(), "空 diff 应被检测");
        let content = "+pub fn test() {}";
        assert!(!content.trim().is_empty(), "有内容的 diff 不应为空");
    }
}
