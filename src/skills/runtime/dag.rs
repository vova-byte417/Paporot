//! DAG 编排引擎
//!
//! 基于 Skill 的 `dependencies.uses_outputs_from` 声明构建有向无环图，
//! 拓扑排序后并行调度执行。

use std::collections::{HashMap, HashSet, VecDeque};
use super::super::types::*;

// ─── DAG 构建 ───────────────────────────────────────────────────────

/// 从已安装 Skill 列表构建 DAG 节点
pub fn build_dag(skills: &[InstalledSkill]) -> Result<DagGraph, DagError> {
    let mut nodes: HashMap<String, DagNode> = HashMap::new();
    let mut node_names: HashSet<String> = HashSet::new();

    // 第一遍：收集所有 Skill 名
    for skill in skills {
        let name = &skill.manifest.skill.name;
        node_names.insert(name.clone());
    }

    // 第二遍：构建节点，校验依赖
    for skill in skills {
        let name = &skill.manifest.skill.name;
        let deps = &skill.manifest.dependencies.uses_outputs_from;

        // 校验所有依赖的 Skill 都存在
        for dep in deps {
            if !node_names.contains(dep) {
                return Err(DagError::MissingDependency {
                    skill: name.clone(),
                    missing: dep.clone(),
                });
            }
        }

        nodes.insert(
            name.clone(),
            DagNode {
                name: name.clone(),
                manifest: skill.manifest.clone(),
                wasm_path: skill.wasm_path.clone(),
                deps: deps.clone(),
            },
        );
    }

    Ok(DagGraph { nodes })
}

// ─── 拓扑排序 ───────────────────────────────────────────────────────

/// 拓扑排序，返回执行层级（同层可并行）
pub fn topological_layers(graph: &DagGraph) -> Result<Vec<Vec<String>>, DagError> {
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    let mut children: HashMap<String, Vec<String>> = HashMap::new();

    // 初始化
    for name in graph.nodes.keys() {
        in_degree.insert(name.clone(), 0);
        children.insert(name.clone(), Vec::new());
    }

    // 构建入度和子节点
    for node in graph.nodes.values() {
        for dep in &node.deps {
            *in_degree.get_mut(&node.name).unwrap() += 1;
            children.get_mut(dep).unwrap().push(node.name.clone());
        }
    }

    // Kahn 算法
    let mut queue: VecDeque<String> = VecDeque::new();
    for (name, deg) in &in_degree {
        if *deg == 0 {
            queue.push_back(name.clone());
        }
    }

    let mut layers: Vec<Vec<String>> = Vec::new();
    let mut processed = 0usize;
    let total = graph.nodes.len();

    while !queue.is_empty() {
        let layer_size = queue.len();
        let mut layer = Vec::new();

        for _ in 0..layer_size {
            let node = queue.pop_front().unwrap();
            layer.push(node.clone());

            if let Some(deps) = children.get(&node) {
                for child in deps {
                    let deg = in_degree.get_mut(child).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(child.clone());
                    }
                }
            }
        }

        layers.push(layer);
    }

    // 检查环
    for layer in &layers {
        processed += layer.len();
    }

    if processed < total {
        // 找出未处理的节点（环的一部分）
        let processed_set: HashSet<&String> = layers
            .iter()
            .flat_map(|l| l.iter())
            .collect();

        let in_cycle: Vec<String> = in_degree
            .keys()
            .filter(|k| !processed_set.contains(k))
            .cloned()
            .collect();

        return Err(DagError::CyclicDependency {
            nodes: in_cycle,
        });
    }

    Ok(layers)
}

// ─── 环检测 ─────────────────────────────────────────────────────────

/// 检测依赖图中是否存在环
pub fn detect_cycles(graph: &DagGraph) -> Vec<Vec<String>> {
    let mut cycles = Vec::new();
    let all_nodes: Vec<String> = graph.nodes.keys().cloned().collect();

    for node_name in &all_nodes {
        let mut visited = HashSet::new();
        let mut path = Vec::new();

        if let Some(cycle) = dfs_find_cycle(graph, node_name, &mut visited, &mut path) {
            cycles.push(cycle);
        }
    }

    cycles
}

fn dfs_find_cycle(
    graph: &DagGraph,
    current: &str,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> Option<Vec<String>> {
    if path.contains(&current.to_string()) {
        // 找到环，提取环路径
        let start_idx = path.iter().position(|x| x == current).unwrap();
        let mut cycle: Vec<String> = path[start_idx..].to_vec();
        cycle.push(current.to_string());
        return Some(cycle);
    }

    if visited.contains(current) {
        return None;
    }

    visited.insert(current.to_string());
    path.push(current.to_string());

    if let Some(node) = graph.nodes.get(current) {
        for dep in &node.deps {
            if let Some(cycle) = dfs_find_cycle(graph, dep, visited, path) {
                return Some(cycle);
            }
        }
    }

    path.pop();
    None
}

// ─── 类型 ───────────────────────────────────────────────────────────

/// DAG 图
#[derive(Debug, Clone)]
pub struct DagGraph {
    pub nodes: HashMap<String, DagNode>,
}

/// DAG 节点
#[derive(Debug, Clone)]
pub struct DagNode {
    pub name: String,
    pub manifest: SkillManifest,
    pub wasm_path: std::path::PathBuf,
    pub deps: Vec<String>,
}

// ─── 错误 ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum DagError {
    MissingDependency { skill: String, missing: String },
    CyclicDependency { nodes: Vec<String> },
    NoSuchSkill { name: String },
}

impl std::fmt::Display for DagError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DagError::MissingDependency { skill, missing } => {
                write!(
                    f,
                    "Skill '{}' depends on '{}', but it is not installed",
                    skill, missing
                )
            }
            DagError::CyclicDependency { nodes } => {
                write!(f, "Cyclic dependency detected among: {}", nodes.join(" → "))
            }
            DagError::NoSuchSkill { name } => {
                write!(f, "Skill '{}' not found in registry", name)
            }
        }
    }
}

impl std::error::Error for DagError {}

// ─── 测试 ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_skill(name: &str, deps: Vec<String>) -> InstalledSkill {
        use super::super::super::types::{
            SkillManifest, SkillMeta, SkillInputs, SkillOutputs, SkillDeps, QualityChecks,
        };

        InstalledSkill {
            manifest: SkillManifest {
                skill: SkillMeta {
                    name: name.to_string(),
                    version: "0.1.0".into(),
                    requires_paporot: ">=0.2.0".into(),
                    description: "test".into(),
                    timeout_secs: 30,
                },
                inputs: SkillInputs {
                    required: vec!["repo_tree".into()],
                    optional: vec![],
                    schema_version: HashMap::new(),
                },
                outputs: SkillOutputs {
                    schema: format!("{}_output", name),
                    format: None,
                },
                llm_calls: None,
                dependencies: SkillDeps {
                    uses_outputs_from: deps,
                },
                quality: QualityChecks::default(),
            },
            dir: std::path::PathBuf::new(),
            wasm_path: std::path::PathBuf::new(),
        }
    }

    #[test]
    fn test_build_dag_no_deps() {
        let skills = vec![
            make_skill("A", vec![]),
            make_skill("B", vec![]),
        ];
        let graph = build_dag(&skills).unwrap();
        assert_eq!(graph.nodes.len(), 2);
    }

    #[test]
    fn test_build_dag_with_deps() {
        let skills = vec![
            make_skill("A", vec![]),
            make_skill("B", vec!["A".into()]),
            make_skill("C", vec!["A".into(), "B".into()]),
        ];
        let graph = build_dag(&skills).unwrap();
        let layers = topological_layers(&graph).unwrap();
        assert_eq!(layers.len(), 3);
        assert_eq!(layers[0], vec!["A"]);
        assert_eq!(layers[1], vec!["B"]);
        assert_eq!(layers[2], vec!["C"]);
    }

    #[test]
    fn test_missing_dependency() {
        let skills = vec![
            make_skill("A", vec!["NONEXISTENT".into()]),
        ];
        let result = build_dag(&skills);
        assert!(result.is_err());
    }

    #[test]
    fn test_cycle_detection_simple() {
        let skills = vec![
            make_skill("A", vec!["B".into()]),
            make_skill("B", vec!["A".into()]),
        ];
        let graph = build_dag(&skills).unwrap();
        let result = topological_layers(&graph);
        assert!(result.is_err());
        if let Err(DagError::CyclicDependency { nodes }) = result {
            assert!(nodes.contains(&"A".to_string()));
            assert!(nodes.contains(&"B".to_string()));
        }
    }

    #[test]
    fn test_topological_layers_parallel() {
        // A和B无依赖，应该在同一层
        // C依赖A，D依赖B，C和D应该在第二层
        let skills = vec![
            make_skill("A", vec![]),
            make_skill("B", vec![]),
            make_skill("C", vec!["A".into()]),
            make_skill("D", vec!["B".into()]),
        ];
        let graph = build_dag(&skills).unwrap();
        let layers = topological_layers(&graph).unwrap();
        assert_eq!(layers.len(), 2);
        // 第一层应为A和B（顺序不保证）
        assert_eq!(layers[0].len(), 2);
        assert!(layers[0].contains(&"A".to_string()));
        assert!(layers[0].contains(&"B".to_string()));
        assert_eq!(layers[1].len(), 2);
    }

    #[test]
    fn test_linear_chain() {
        let skills = vec![
            make_skill("ru", vec![]),
            make_skill("md", vec!["ru".into()]),
            make_skill("da", vec!["md".into()]),
            make_skill("fa", vec!["da".into()]),
            make_skill("bd", vec!["fa".into()]),
            make_skill("gen", vec!["bd".into()]),
        ];
        let graph = build_dag(&skills).unwrap();
        let layers = topological_layers(&graph).unwrap();
        // 线性链：每层一个
        assert_eq!(layers.len(), 6);
        assert_eq!(layers[0], vec!["ru"]);
        assert_eq!(layers[5], vec!["gen"]);
    }
}
