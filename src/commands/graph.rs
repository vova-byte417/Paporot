//! `Paporot graph` — 能力依赖图查询
//!
//! 支持子命令:
//! - `graph show`    展示依赖图
//! - `graph impact`  影响分析
//! - `graph evolution`  演化追溯
//! - `graph cycles`  循环依赖检测
//! - `graph module`  按模块查询

use anyhow::Result;
use crate::agent::Agent;
use crate::graph::GraphStorage;

/// 执行 graph show 命令
pub fn show(agent: &Agent, _version: Option<&str>, capability_id: Option<&str>, depth: usize) -> Result<()> {
    println!("Paporot Dependency Graph");
    println!("========================\n");

    let graph = agent.graph_storage.load()?;

    if graph.edges.is_empty() {
        println!("  No dependency edges found.");
        if graph.nodes.is_empty() {
            println!("  No capability nodes found. Run 'Paporot snapshot create' first.");
        }
        return Ok(());
    }

    println!("Nodes: {}", graph.nodes.len());
    println!("Edges: {}\n", graph.edges.len());

    if let Some(cap_id) = capability_id {
        // 展示特定能力的依赖
        let all_deps: Vec<_> = graph.edges.iter()
            .filter(|e| e.from_capability_id == cap_id || e.to_capability_id == cap_id)
            .take(depth * 10) // 限制显示数量
            .collect();

        if all_deps.is_empty() {
            println!("  No dependencies found for '{}'", cap_id);
        } else {
            println!("Dependencies for '{}':", cap_id);
            for edge in &all_deps {
                let arrow = if edge.from_capability_id == cap_id { "→" } else { "←" };
                let target = if edge.from_capability_id == cap_id {
                    &edge.to_capability_id
                } else {
                    &edge.from_capability_id
                };
                println!("  {} {} {} ({:?})",
                    cap_id, arrow, target, edge.relation);
            }
        }
    } else {
        // 全局视图
        println!("Global dependency edges:");
        for edge in &graph.edges {
            println!("  {} → {} ({:?})",
                edge.from_capability_id,
                edge.to_capability_id,
                edge.relation);
        }
    }

    println!("\nEvolution chains:");
    for (cap_id, versions) in &graph.evolution_chains {
        if !versions.is_empty() {
            println!("  {}: {}", cap_id, versions.join(" → "));
        }
    }

    Ok(())
}

/// 执行 graph impact 命令
pub fn impact(agent: &Agent, capability_id: &str) -> Result<()> {
    println!("Paporot Impact Analysis");
    println!("=======================\n");
    println!("Target: {}\n", capability_id);

    let graph = agent.graph_storage.load()?;
    let impacted = GraphStorage::impact_analysis(&graph, capability_id);

    if impacted.is_empty() {
        println!("  No downstream dependencies found.");
    } else {
        println!("  {} downstream impact(s):", impacted.len());
        for edge in impacted {
            println!("    {} → {} ({:?}, confidence: {:.2})",
                edge.from_capability_id,
                edge.to_capability_id,
                edge.relation,
                edge.confidence);
        }
    }

    Ok(())
}

/// 执行 graph evolution 命令
pub fn evolution(agent: &Agent, capability_id: &str) -> Result<()> {
    println!("Paporot Evolution Trace");
    println!("=======================\n");
    println!("Capability: {}\n", capability_id);

    let graph = agent.graph_storage.load()?;
    let trace = GraphStorage::evolution_trace(&graph, capability_id);

    if trace.is_empty() {
        println!("  No evolution history found.");
    } else {
        println!("  History ({} versions):", trace.len());
        for (i, version) in trace.iter().enumerate() {
            println!("    {}. {}", i + 1, version);
        }
    }

    Ok(())
}

/// 执行 graph cycles 命令
pub fn cycles(agent: &Agent) -> Result<()> {
    println!("Paporot Cycle Detection");
    println!("======================\n");

    let graph = agent.graph_storage.load()?;
    let cycles = GraphStorage::detect_cycles(&graph);

    if cycles.is_empty() {
        println!("  ✓ No circular dependencies detected.");
    } else {
        println!("  ✗ {} circular dependency/ies found:", cycles.len());
        for (i, cycle) in cycles.iter().enumerate() {
            println!("    {}. {}", i + 1, cycle.join(" → "));
        }
    }

    Ok(())
}

/// 执行 graph module 命令
pub fn module(agent: &Agent, module_name: &str) -> Result<()> {
    println!("Paporot Module Query");
    println!("===================\n");
    println!("Module: {}\n", module_name);

    let graph = agent.graph_storage.load()?;
    let module_nodes: Vec<_> = graph.nodes.iter()
        .filter(|(_, meta)| {
            meta.module.as_deref().map_or(false, |m| m == module_name)
        })
        .collect();

    if module_nodes.is_empty() {
        println!("  No capabilities found in module '{}'", module_name);
    } else {
        println!("  {} capabilities:", module_nodes.len());
        for (_id, meta) in &module_nodes {
            println!("    - {} [{:?}] (snapshot: {})",
                meta.name, meta.status, meta.latest_snapshot);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::graph::{DependencyEdge, DependencyGraph, NodeMeta};
    use crate::types::{CapabilityCategory, CapabilityStatus, DependencyRelation};

    fn make_test_graph() -> DependencyGraph {
        let mut nodes = HashMap::new();
        nodes.insert("cap_auth_001".into(), NodeMeta {
            name: "JWT Login".into(),
            module: Some("auth".into()),
            tags: vec!["security".into()],
            categories: vec![CapabilityCategory::Security],
            status: CapabilityStatus::New,
            latest_snapshot: "v1".into(),
        });
        nodes.insert("cap_pay_001".into(), NodeMeta {
            name: "Process Payment".into(),
            module: Some("payment".into()),
            tags: vec![],
            categories: vec![CapabilityCategory::Functional],
            status: CapabilityStatus::New,
            latest_snapshot: "v1".into(),
        });

        let edges = vec![
            DependencyEdge {
                from_capability_id: "cap_pay_001".into(),
                from_snapshot: Some("v1".into()),
                to_capability_id: "cap_auth_001".into(),
                to_snapshot: Some("v1".into()),
                relation: DependencyRelation::Calls,
                confidence: 1.0,
            },
        ];

        let mut evolution_chains = HashMap::new();
        evolution_chains.insert("cap_auth_001".into(), vec!["v1".into(), "v2".into()]);

        DependencyGraph { edges, nodes, evolution_chains }
    }

    #[test]
    fn test_impact_analysis_finds_downstream() {
        let graph = make_test_graph();
        let impacted = GraphStorage::impact_analysis(&graph, "cap_auth_001");
        assert_eq!(impacted.len(), 1);
        assert_eq!(impacted[0].from_capability_id, "cap_pay_001");
    }

    #[test]
    fn test_impact_analysis_no_dependents() {
        let graph = make_test_graph();
        let impacted = GraphStorage::impact_analysis(&graph, "cap_pay_001");
        assert_eq!(impacted.len(), 0);
    }

    #[test]
    fn test_evolution_trace() {
        let graph = make_test_graph();
        let trace = GraphStorage::evolution_trace(&graph, "cap_auth_001");
        assert_eq!(trace, vec!["v1", "v2"]);
    }

    #[test]
    fn test_evolution_trace_missing() {
        let graph = make_test_graph();
        let trace = GraphStorage::evolution_trace(&graph, "nonexistent");
        assert!(trace.is_empty());
    }

    #[test]
    fn test_module_query_finds_auth() {
        let graph = make_test_graph();
        let nodes: Vec<_> = graph.nodes.iter()
            .filter(|(_, m)| m.module.as_deref() == Some("auth"))
            .collect();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].1.name, "JWT Login");
    }

    #[test]
    fn test_module_query_no_match() {
        let graph = make_test_graph();
        let nodes: Vec<_> = graph.nodes.iter()
            .filter(|(_, m)| m.module.as_deref() == Some("nonexistent"))
            .collect();
        assert!(nodes.is_empty());
    }
}
