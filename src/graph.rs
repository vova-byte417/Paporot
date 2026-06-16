//! 能力依赖图存储
//!
//! 对应 PRD P2 §2.2。维护独立于快照的能力依赖关系索引。
//!
//! ## 架构
//!
//! ```text
//! .Paporot/
//!   snapshots/    ← BehaviorSnapshot JSON 文件（现有）
//!   graph/
//!     graph.json  ← 依赖图索引（新增）
//! ```
//!
//! ## 数据结构
//!
//! `DependencyGraph` 包含：
//! - `edges`: 所有依赖边列表
//! - `nodes`: 按 CapabilityRef 索引的节点元数据
//! - `evolution_chains`: capability_id → 跨版本历史

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use crate::types::*;

/// 依赖图索引
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DependencyGraph {
    /// 依赖边
    pub edges: Vec<DependencyEdge>,
    /// 节点元数据
    #[serde(default)]
    pub nodes: HashMap<String, NodeMeta>,
    /// 演化链: capability_id → 历史快照版本列表
    #[serde(default)]
    pub evolution_chains: HashMap<String, Vec<String>>,
}

/// 依赖边
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DependencyEdge {
    pub from_capability_id: String,
    pub from_snapshot: Option<String>,
    pub to_capability_id: String,
    pub to_snapshot: Option<String>,
    pub relation: DependencyRelation,
    pub confidence: f32,
}

/// 节点元数据
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeMeta {
    pub name: String,
    pub module: Option<String>,
    pub tags: Vec<String>,
    pub categories: Vec<CapabilityCategory>,
    pub status: CapabilityStatus,
    pub latest_snapshot: String,
}

/// 依赖图存储管理器
pub struct GraphStorage {
    dir: PathBuf,
}

impl GraphStorage {
    /// 创建存储实例
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let mut dir = base_dir.into();
        dir.push("graph");
        Self { dir }
    }

    /// 确保存储目录存在
    pub fn init(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("Failed to create graph dir: {}", self.dir.display()))?;
        Ok(())
    }

    /// 加载依赖图，不存在则返回空图
    pub fn load(&self) -> Result<DependencyGraph> {
        let path = self.graph_path();
        if !path.exists() {
            return Ok(DependencyGraph {
                edges: vec![],
                nodes: HashMap::new(),
                evolution_chains: HashMap::new(),
            });
        }

        let json = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read graph: {}", path.display()))?;
        serde_json::from_str(&json)
            .with_context(|| format!("Failed to parse graph: {}", path.display()))
    }

    /// 保存依赖图
    pub fn save(&self, graph: &DependencyGraph) -> Result<()> {
        self.init()?;
        let path = self.graph_path();
        let json = serde_json::to_string_pretty(graph)
            .context("Failed to serialize graph")?;
        std::fs::write(&path, json)
            .with_context(|| format!("Failed to write graph: {}", path.display()))
    }

    /// 从快照更新依赖图索引
    ///
    /// 在每次 `snapshot create` 后调用，增量更新图的节点、边和演化链。
    pub fn update_from_snapshot(
        &self,
        graph: &mut DependencyGraph,
        snapshot: &BehaviorSnapshot,
    ) -> Result<()> {
        // 1. 更新节点
        for cap in &snapshot.capabilities {
            let meta = NodeMeta {
                name: cap.name.clone(),
                module: cap.module.clone(),
                tags: cap.tags.clone(),
                categories: cap.categories.clone(),
                status: cap.status.clone(),
                latest_snapshot: snapshot.version_id.clone(),
            };

            // 记录能力 id（可能跨快照重复）
            let key = graph_key(&cap.id);
            graph.nodes.insert(key, meta);

            // 2. 更新演化链
            let chain = graph
                .evolution_chains
                .entry(cap.id.clone())
                .or_default();
            if chain.last() != Some(&snapshot.version_id) {
                chain.push(snapshot.version_id.clone());
            }

            // 3. 更新依赖边
            for dep in &cap.depends_on {
                let edge = DependencyEdge {
                    from_capability_id: cap.id.clone(),
                    from_snapshot: Some(snapshot.version_id.clone()),
                    to_capability_id: dep.target.capability_id.clone(),
                    to_snapshot: dep.target.snapshot_version.clone(),
                    relation: dep.relation.clone(),
                    confidence: dep.confidence,
                };

                // 避免重复边
                let already_exists = graph.edges.iter().any(|e| {
                    e.from_capability_id == edge.from_capability_id
                        && e.to_capability_id == edge.to_capability_id
                        && e.from_snapshot == edge.from_snapshot
                });

                if !already_exists {
                    graph.edges.push(edge);
                }
            }
        }

        Ok(())
    }

    /// 查询某个能力的直接影响范围（下游）
    pub fn impact_analysis<'a>(graph: &'a DependencyGraph, capability_id: &str) -> Vec<&'a DependencyEdge> {
        graph
            .edges
            .iter()
            .filter(|e| e.to_capability_id == capability_id)
            .collect()
    }

    /// 查询某个能力的演化历史
    pub fn evolution_trace(graph: &DependencyGraph, capability_id: &str) -> Vec<String> {
        graph
            .evolution_chains
            .get(capability_id)
            .cloned()
            .unwrap_or_default()
    }

    /// 检测循环依赖
    pub fn detect_cycles(graph: &DependencyGraph) -> Vec<Vec<String>> {
        // 简单 DFS 循环检测
        let mut cycles = Vec::new();
        let mut visited = HashMap::new();

        for edge in &graph.edges {
            if !visited.contains_key(&edge.from_capability_id) {
                let mut path = Vec::new();
                Self::dfs(
                    graph,
                    &edge.from_capability_id,
                    &mut visited,
                    &mut path,
                    &mut cycles,
                );
            }
        }

        cycles
    }

    fn dfs(
        graph: &DependencyGraph,
        current: &str,
        visited: &mut HashMap<String, u8>, // 0=unvisited, 1=visiting, 2=done
        path: &mut Vec<String>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        *visited.entry(current.to_string()).or_insert(0) = 1;
        path.push(current.to_string());

        // 找当前节点的出边
        let neighbors: Vec<_> = graph
            .edges
            .iter()
            .filter(|e| e.from_capability_id == current)
            .map(|e| e.to_capability_id.clone())
            .collect();

        for neighbor in neighbors {
            match visited.get(&neighbor) {
                Some(&1) => {
                    // 找到回路
                    if let Some(pos) = path.iter().position(|n| n == &neighbor) {
                        let cycle: Vec<_> = path[pos..].to_vec();
                        if !cycles.contains(&cycle) {
                            cycles.push(cycle);
                        }
                    }
                }
                Some(&2) => continue,
                _ => {
                    Self::dfs(graph, &neighbor, visited, path, cycles);
                }
            }
        }

        path.pop();
        *visited.get_mut(current).unwrap() = 2;
    }

    fn graph_path(&self) -> PathBuf {
        self.dir.join("graph.json")
    }
}

/// 生成图的唯一键
fn graph_key(capability_id: &str) -> String {
    capability_id.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_save_and_load() {
        let dir = std::env::temp_dir().join("Paporot_test_graph");
        let _ = std::fs::remove_dir_all(&dir);

        let storage = GraphStorage::new(&dir);
        storage.init().unwrap();

        let mut graph = DependencyGraph {
            edges: vec![DependencyEdge {
                from_capability_id: "cap_001".into(),
                from_snapshot: Some("v1".into()),
                to_capability_id: "cap_002".into(),
                to_snapshot: Some("v1".into()),
                relation: DependencyRelation::Calls,
                confidence: 0.9,
            }],
            nodes: HashMap::new(),
            evolution_chains: HashMap::new(),
        };

        graph.nodes.insert(
            "cap_001".into(),
            NodeMeta {
                name: "Test Cap".into(),
                module: Some("auth".into()),
                tags: vec![],
                categories: vec![],
                status: CapabilityStatus::New,
                latest_snapshot: "v1".into(),
            },
        );

        storage.save(&graph).unwrap();
        let loaded = storage.load().unwrap();
        assert_eq!(loaded.edges.len(), 1);
        assert_eq!(loaded.nodes.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cycle_detection() {
        let edges = vec![
            DependencyEdge {
                from_capability_id: "A".into(),
                from_snapshot: None,
                to_capability_id: "B".into(),
                to_snapshot: None,
                relation: DependencyRelation::Calls,
                confidence: 1.0,
            },
            DependencyEdge {
                from_capability_id: "B".into(),
                from_snapshot: None,
                to_capability_id: "C".into(),
                to_snapshot: None,
                relation: DependencyRelation::Calls,
                confidence: 1.0,
            },
            DependencyEdge {
                from_capability_id: "C".into(),
                from_snapshot: None,
                to_capability_id: "A".into(),
                to_snapshot: None,
                relation: DependencyRelation::Calls,
                confidence: 1.0,
            },
        ];

        let graph = DependencyGraph {
            edges,
            nodes: HashMap::new(),
            evolution_chains: HashMap::new(),
        };

        let cycles = GraphStorage::detect_cycles(&graph);
        assert!(!cycles.is_empty(), "应检测到循环依赖");
    }

    #[test]
    fn test_no_cycle() {
        let edges = vec![
            DependencyEdge {
                from_capability_id: "A".into(),
                from_snapshot: None,
                to_capability_id: "B".into(),
                to_snapshot: None,
                relation: DependencyRelation::Calls,
                confidence: 1.0,
            },
            DependencyEdge {
                from_capability_id: "B".into(),
                from_snapshot: None,
                to_capability_id: "C".into(),
                to_snapshot: None,
                relation: DependencyRelation::Calls,
                confidence: 1.0,
            },
        ];

        let graph = DependencyGraph {
            edges,
            nodes: HashMap::new(),
            evolution_chains: HashMap::new(),
        };

        let cycles = GraphStorage::detect_cycles(&graph);
        assert!(cycles.is_empty(), "不应有循环依赖");
    }
}
