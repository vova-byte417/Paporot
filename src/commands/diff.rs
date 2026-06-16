//! `Paporot diff` — 行为差异对比

use anyhow::Result;
use crate::agent::{self, Agent};
use crate::types::BehaviorDiff;

/// 执行 diff 命令
pub async fn run(agent: &Agent, from: Option<&str>, to: Option<&str>, format: &str) -> Result<BehaviorDiff> {
    println!("Paporot Behavior Diff");

    // 加载 snapshot
    let from_snapshot = if let Some(ref v) = from {
        agent.storage.load_by_version(v)?
    } else {
        // 默认取倒数第二个
        let versions = agent.storage.list_versions_sorted()?;
        if versions.len() < 2 {
            anyhow::bail!("Need at least 2 snapshots for diff. Found: {}. Run 'Paporot snapshot create' first.", versions.len());
        }
        agent.storage.load_by_version(&versions[versions.len() - 2])?
    };

    let to_snapshot = if let Some(ref v) = to {
        agent.storage.load_by_version(v)?
    } else {
        agent.storage.load_latest()?
    };

    println!("  from : {}", from_snapshot.version_id);
    println!("  to   : {}", to_snapshot.version_id);

    // 调用 Agent 计算 diff
    let diff = agent.compute_diff(&from_snapshot, &to_snapshot);

    // 输出
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&diff)?);
        }
        _ => {
            agent::print_markdown_diff(&diff);
        }
    }

    Ok(diff)
}

#[cfg(test)]
mod tests {
    use crate::agent::Agent;
    use crate::config::Config;
    use crate::types::*;

    /// 测试项: agent.compute_diff 返回正确的 diff 元数据
    /// 输入: v1(3 caps) → v2(4 caps)，包含新增+修改+未变化+删除
    /// 预期: from_version="v1", to_version="v2", 各分类计数正确
    #[test]
    fn test_agent_compute_diff_correct_counts() {
        let config = Config::default();
        let agent = Agent::new(config);

        let cap_base = || Capability {
            id: String::new(), name: String::new(), description: String::new(),
            status: CapabilityStatus::New,
            module: None, sub_modules: vec![], confidence: Some(1.0),
            evidence: vec![], tags: vec![], contract: None,
            preconditions: vec![], postconditions: vec![], invariants: vec![],
            categories: vec![], depends_on: vec![], depended_by: vec![],
            evolved_from: None, evidence_trace_ids: vec![], verified_by: None, verified_at: None,
        };

        let make = |id: &str, name: &str, status: CapabilityStatus| {
            let mut c = cap_base();
            c.id = id.into();
            c.name = name.into();
            c.status = status;
            c
        };

        let from_snap = BehaviorSnapshot {
            schema_version: 3,
            version_id: "v1".into(), git_commit: None, git_ref: None,
            timestamp: "t".into(), message: String::new(),
            capabilities: vec![
                make("c1", "A", CapabilityStatus::New),
                make("c2", "B", CapabilityStatus::New),
                make("c3", "C", CapabilityStatus::New),
            ],
            prd_coverage: PrdCoverage { percentage: 0.0, total_items: 0, covered_items: None, details: vec![] },
            regression: None, risk: None, metadata: None,
        };

        let to_snap = BehaviorSnapshot {
            schema_version: 3,
            version_id: "v2".into(), git_commit: None, git_ref: None,
            timestamp: "t".into(), message: String::new(),
            capabilities: vec![
                make("c1", "A", CapabilityStatus::Unchanged),
                make("c2", "B", CapabilityStatus::Modified),
                make("c4", "D", CapabilityStatus::New),
            ],
            prd_coverage: PrdCoverage { percentage: 0.0, total_items: 0, covered_items: None, details: vec![] },
            regression: None, risk: None, metadata: None,
        };

        let diff = agent.compute_diff(&from_snap, &to_snap);
        assert_eq!(diff.from_version, "v1");
        assert_eq!(diff.to_version, "v2");
        assert_eq!(diff.added.len(), 1);    // D 新增
        assert_eq!(diff.modified.len(), 1);  // B 修改
        assert_eq!(diff.deleted.len(), 1);   // C 不再存在
        assert_eq!(diff.unchanged.len(), 1); // A 未变
    }
}
