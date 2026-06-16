//! `Paporot coverage` — PRD 覆盖率分析

use anyhow::Result;
use crate::agent::Agent;
use crate::types::BehaviorSnapshot;

/// 执行 coverage 命令
pub async fn run(agent: &Agent, prd: Option<&str>, version: Option<&str>) -> Result<()> {
    println!("Paporot PRD Coverage");

    // 1. 加载 PRD
    let prd_content = if let Some(path) = prd {
        match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                anyhow::bail!("Failed to read PRD file '{}': {}", path, e);
            }
        }
    } else {
        anyhow::bail!("No PRD file specified. Use --prd <path>.");
    };

    println!("  prd     : {} ({} bytes)", prd.unwrap_or("-"), prd_content.len());

    // 2. 加载 snapshot
    let snapshot: BehaviorSnapshot = if let Some(ref v) = version {
        agent.storage.load_by_version(v)?
    } else {
        agent.storage.load_latest()?
    };
    println!("  version : {}", snapshot.version_id);

    // 3. 调用 Agent 计算覆盖率
    let coverage = agent.compute_coverage(&prd_content, &snapshot.capabilities).await?;

    // 4. 输出
    println!();
    println!("  ── PRD Coverage Result ──");
    println!("  percentage : {:.1}%", coverage.percentage);
    println!("  covered    : {}/{}",
        coverage.covered_items.unwrap_or(0),
        coverage.total_items
    );

    if !coverage.details.is_empty() {
        println!();
        println!("  Per-item breakdown:");
        for detail in &coverage.details {
            let icon = match detail.status {
                crate::types::CoverageStatus::Pass => "✓",
                crate::types::CoverageStatus::Partial => "~",
                crate::types::CoverageStatus::Fail => "✗",
                crate::types::CoverageStatus::NotDetected => "?",
            };
            println!("    {} [{}] {}", icon, detail.prd_id, detail.requirement);
            if !detail.mapped_capabilities.is_empty() {
                println!("       → mapped to: {}", detail.mapped_capabilities.join(", "));
            }
        }
    }

    Ok(())
}

/// CoverageStatus 对应的图标
pub fn coverage_icon(status: &crate::types::CoverageStatus) -> &'static str {
    use crate::types::CoverageStatus;
    match status {
        CoverageStatus::Pass => "✓",
        CoverageStatus::Partial => "~",
        CoverageStatus::Fail => "✗",
        CoverageStatus::NotDetected => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试项: coverage_icon 四种状态映射
    /// 输入: 4 种 CoverageStatus
    /// 预期: 对应图标正确
    #[test]
    fn test_coverage_icon_mapping() {
        assert_eq!(coverage_icon(&crate::types::CoverageStatus::Pass), "✓");
        assert_eq!(coverage_icon(&crate::types::CoverageStatus::Partial), "~");
        assert_eq!(coverage_icon(&crate::types::CoverageStatus::Fail), "✗");
        assert_eq!(coverage_icon(&crate::types::CoverageStatus::NotDetected), "?");
    }
}
