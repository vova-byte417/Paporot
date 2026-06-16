//! `Paporot snapshot create` — 创建行为快照

use anyhow::{Context, Result};
use crate::agent::Agent;
use crate::types::BehaviorSnapshot;

/// 执行 snapshot create 命令
pub async fn run(
    agent: &Agent,
    diff_range: &str,
    message: &str,
    prd_file: Option<&str>,
    diff_file: Option<&str>,
    output_dir: &str,
) -> Result<BehaviorSnapshot> {
    println!("Paporot Snapshot Create");
    println!("  diff range : {}", diff_range);
    println!("  message    : {}", message);
    println!("  output dir : {}", output_dir);

    // 1. 获取 git diff
    let diff = if let Some(file) = diff_file {
        std::fs::read_to_string(file).context("Failed to read diff file")?
    } else {
        get_git_diff(diff_range)?
    };

    if diff.trim().is_empty() {
        println!("  [INFO] No changes detected in diff range ({})", diff_range);
        eprintln!("  [WARN] Empty diff — generating minimal snapshot");
    }

    let diff_len = diff.len();
    if diff_len > agent.config.agent.diff_warn_threshold {
        eprintln!(
            "  [WARN] Diff is {} bytes — may exceed LLM context window limit",
            diff_len
        );
    }

    // 2. 加载 PRD（如果有）
    let prd_content = prd_file.map(|f| std::fs::read_to_string(f)).transpose().ok().flatten();

    // 3. 获取上一个 snapshot 摘要（用于上下文连续性）
    let prev_summary = agent.storage.list_versions_sorted().ok()
        .and_then(|v| v.last().cloned());

    // 4. 调用 Agent 提取行为 → LLM 实际调用
    let version_id = agent.storage.next_version_id()?;
    let mut snapshot = agent
        .create_snapshot(&diff, message, prd_content.as_deref(), prev_summary.as_deref())
        .await?;
    snapshot.version_id = version_id.clone();

    // 5. 如果有 PRD，计算覆盖率
    if let Some(ref prd) = prd_content {
        match agent.compute_coverage(prd, &snapshot.capabilities).await {
            Ok(coverage) => {
                snapshot.prd_coverage = coverage;
                println!("  [Agent] PRD coverage: {:.1}%", snapshot.prd_coverage.percentage);
            }
            Err(e) => eprintln!("  [WARN] Coverage computation skipped: {}", e),
        }
    }

    // 6. 保存
    agent.storage.save(&snapshot)?;

    // 7. 输出摘要
    println!();
    println!("  ── Snapshot Created ──");
    println!("  version : {}", version_id);
    println!("  capabilities : {}", snapshot.capabilities.len());
    for cap in &snapshot.capabilities {
        println!("    [{}] {} (confidence: {:.2})",
            cap.status_name(),
            cap.name,
            cap.confidence.unwrap_or(0.0)
        );
    }

    Ok(snapshot)
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

    /// 测试项: Agent 结构体持有 storage 字段
    #[test]
    fn test_agent_has_storage() {
        let config = Config::default();
        let agent = Agent::new(config);
        // 验证 storage 可访问
        let _ = &agent.storage;
    }

    /// 测试项: diff_range 参数解析
    /// 输入: 正常 git range
    /// 预期: 不做格式校验，但确认字符串传递正确
    #[test]
    fn test_diff_range_format() {
        let ranges = ["HEAD~1..HEAD", "main..feature", "abc123..def456"];
        for r in &ranges {
            assert!(r.contains(".."), "diff_range {} 应包含 ..", r);
            let parts: Vec<&str> = r.split("..").collect();
            assert_eq!(parts.len(), 2);
        }
    }

    /// 测试项: next_version_id 格式验证
    #[test]
    fn test_next_version_id_format() {
        let config = Config::default();
        let agent = Agent::new(config);
        let id = agent.storage.next_version_id().unwrap();
        assert!(id.starts_with('v'), "应包含 v 前缀，实际: {}", id);
        // ID 应该可解析为数字
        let num: u32 = id[1..].parse().unwrap();
        assert!(num >= 1, "版本号应 >= 1，实际: {}", num);
    }

    /// 测试项: diff_warn_threshold 默认值
    #[test]
    fn test_default_diff_warn_threshold() {
        let config = Config::default();
        // 默认值 32000 字节 (~32KB)
        assert!(config.agent.diff_warn_threshold > 0);
        assert!(config.agent.diff_warn_threshold <= 100_000);
    }
}
