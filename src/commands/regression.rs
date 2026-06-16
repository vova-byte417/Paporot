//! `Paporot regression` — 回归检测

use anyhow::Result;
use crate::agent::Agent;

/// 执行 regression 命令
pub async fn run(agent: &Agent, from: Option<&str>, to: Option<&str>) -> Result<()> {
    println!("Paporot Regression Detection");

    // 加载两个 snapshot
    let prev = if let Some(ref v) = from {
        agent.storage.load_by_version(v)?
    } else {
        let versions = agent.storage.list_versions_sorted()?;
        if versions.len() < 2 {
            anyhow::bail!("Need at least 2 snapshots. Found {}. Run 'Paporot snapshot create' first.", versions.len());
        }
        agent.storage.load_by_version(&versions[versions.len() - 2])?
    };

    let curr = if let Some(ref v) = to {
        agent.storage.load_by_version(v)?
    } else {
        agent.storage.load_latest()?
    };

    println!("  from : {}", prev.version_id);
    println!("  to   : {}", curr.version_id);

    // 调用 Agent 检测回归
    let regression = agent.detect_regressions(&prev, &curr).await?;

    // 输出
    println!();
    println!("  ── Regression Result ──");
    println!("  status : {:?}", regression.status);

    if !regression.detected_regressions.is_empty() {
        println!("  regressions detected: {}", regression.detected_regressions.len());
        for r in &regression.detected_regressions {
            println!("    [{:?}] {} : {} → {}", r.severity, r.workflow, r.previous_status, r.current_status);
            println!("      {}", r.description);
        }
    } else {
        println!("  No regressions detected.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::agent::Agent;
    use crate::types::*;

    /// 创建带隔离存储目录的 Config
    fn isolated_config(suffix: &str) -> Config {
        let dir = std::env::temp_dir()
            .join(format!("Paporot_reg_{}_{}", std::process::id(), suffix));
        let _ = std::fs::create_dir_all(&dir);
        let mut config = Config::default();
        config.storage.snapshots_dir = dir.to_string_lossy().into();
        config
    }

    /// 测试项: 少于 2 个 snapshot 时的回退逻辑
    #[test]
    fn test_insufficient_snapshots_for_regression() {
        let config = isolated_config("insuf");
        let agent = Agent::new(config);
        let versions = agent.storage.list_versions_sorted().unwrap();
        assert!(!versions.iter().any(|v| v == "garbage"), "应无残留数据");
        let need_more = versions.len() < 2;
        assert!(need_more, "少于 2 个 snapshot 时应报错");
    }

    /// 测试项: 两个 snapshot 可正确选出 prev 和 curr
    #[test]
    fn test_two_snapshots_select_prev_and_curr() {
        let config = isolated_config("two_snap");
        let agent = Agent::new(config);

        let make = |v: &str| BehaviorSnapshot {
            schema_version: 3,
            version_id: v.into(),
            git_commit: None, git_ref: None,
            timestamp: "t".into(), message: String::new(),
            capabilities: vec![],
            prd_coverage: PrdCoverage { percentage: 0.0, total_items: 0, covered_items: None, details: vec![] },
            regression: None, risk: None, metadata: None,
        };

        agent.storage.save(&make("v1")).unwrap();
        agent.storage.save(&make("v2")).unwrap();

        let versions = agent.storage.list_versions_sorted().unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[versions.len() - 2], "v1");
        assert_eq!(versions[versions.len() - 1], "v2");
    }
}
