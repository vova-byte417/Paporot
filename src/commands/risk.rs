//! `Paporot risk` — 风险评估

use anyhow::Result;
use crate::agent::Agent;

/// 执行 risk 命令
pub async fn run(agent: &Agent, version: Option<&str>) -> Result<()> {
    println!("Paporot Risk Assessment");

    // 加载当前 snapshot
    let current = if let Some(ref v) = version {
        agent.storage.load_by_version(v)?
    } else {
        agent.storage.load_latest()?
    };

    println!("  version : {}", current.version_id);

    // 尝试加载上一版本
    let previous = {
        let versions = agent.storage.list_versions_sorted()?;
        versions.iter()
            .rposition(|v| v == &current.version_id)
            .and_then(|pos| pos.checked_sub(1))
            .and_then(|prev_idx| versions.get(prev_idx))
            .and_then(|prev_id| agent.storage.load_by_version(prev_id).ok())
    };

    // 调用 Agent 评估风险
    let risk = agent.assess_risk(&current, previous.as_ref()).await?;

    // 输出
    println!();
    println!("  ── Risk Assessment ──");
    println!("  level : {:?}", risk.level);
    println!("  score : {}/100", risk.score);

    if !risk.factors.is_empty() {
        println!();
        println!("  Risk factors:");
        for factor in &risk.factors {
            println!("    [{:?}] {}: {}", factor.severity, factor.category, factor.description);
        }
    }

    if !risk.mitigations.is_empty() {
        println!();
        println!("  Mitigations:");
        for m in &risk.mitigations {
            println!("    - {}", m);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::agent::Agent;
    use crate::types::*;

    fn isolated_config(suffix: &str) -> Config {
        let dir = std::env::temp_dir()
            .join(format!("Paporot_risk_{}_{}", std::process::id(), suffix));
        let _ = std::fs::create_dir_all(&dir);
        let mut config = Config::default();
        config.storage.snapshots_dir = dir.to_string_lossy().into();
        config
    }

    /// 测试项: list_versions_sorted 空存储返回空列表
    #[test]
    fn test_empty_storage_returns_empty_versions() {
        let config = isolated_config("empty");
        let agent = Agent::new(config);
        let versions = agent.storage.list_versions_sorted().unwrap();
        assert!(versions.is_empty());
    }

    /// 测试项: 单个 snapshot 回退到上一版本返回 None
    #[test]
    fn test_find_previous_version_single() {
        let config = isolated_config("single");
        let agent = Agent::new(config);

        let snap = make_test_snapshot("v1");
        agent.storage.save(&snap).unwrap();

        let versions = agent.storage.list_versions_sorted().unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0], "v1");

        let pos = versions.iter().position(|v| v == &snap.version_id).unwrap();
        assert_eq!(pos, 0);
        let has_prev = pos.checked_sub(1);
        assert!(has_prev.is_none(), "只有一个版本时没有上一个版本");
    }

    fn make_test_snapshot(version: &str) -> BehaviorSnapshot {
        BehaviorSnapshot {
            schema_version: 3,
            version_id: version.into(),
            git_commit: None, git_ref: None,
            timestamp: "t".into(), message: String::new(),
            capabilities: vec![],
            prd_coverage: PrdCoverage { percentage: 0.0, total_items: 0, covered_items: None, details: vec![] },
            regression: None, risk: None, metadata: None,
        }
    }
}
