//! Capability 聚类引擎
//!
//! 将符号级的变更数据聚合为模块级 Capability 视图，供 Dashboard 力导向图使用。
//! Capability 为查询视图，不持久化。

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// 一个 Capability 聚类节点
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CapabilityCluster {
    /// 模块名
    pub module: String,
    /// 关联的符号列表
    pub symbols: Vec<SymbolRef>,
    /// 关联的 Task 数量
    pub task_count: usize,
    /// 最近活跃日期
    pub last_active: Option<String>,
    /// 聚类类型
    pub cluster_type: ClusterType,
}

/// 符号引用
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SymbolRef {
    pub name: String,
    pub kind: String,
    pub file_path: String,
}

/// 聚类类型
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ClusterType {
    /// 本次变更新增/修改的模块
    Changed,
    /// 历史活跃模块
    Active,
    /// 低活跃模块
    Cold,
}

/// 模块间的耦合关系
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CouplingLink {
    pub source_module: String,
    pub target_module: String,
    /// 耦合强度 (0.0 - 1.0)
    pub strength: f64,
    /// 关联类型
    pub link_type: LinkType,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum LinkType {
    /// 依赖关系
    Dependency,
    /// 共同变更 (co-change)
    CoChange,
    /// 两者皆有
    Both,
}

/// 聚类结果
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClusterResult {
    /// Capability 节点
    pub nodes: Vec<CapabilityCluster>,
    /// 耦合连线
    pub links: Vec<CouplingLink>,
    /// 本次变更涉及模块
    pub changed_modules: Vec<String>,
    /// 统计摘要
    pub summary: ClusterSummary,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClusterSummary {
    pub total_modules: usize,
    pub changed_count: usize,
    pub active_count: usize,
    pub cold_count: usize,
    pub avg_coupling_strength: f64,
}

/// Capability 聚类引擎
pub struct CapabilityClusterEngine;

impl CapabilityClusterEngine {
    /// 从代码变更摘要构建 Capability 聚类
    ///
    /// 按文件路径前缀提取模块名（取第一段目录名），
    /// 将符号按模块分组，计算 Task 关联数和耦合强度。
    pub fn build_from_change(
        files_changed: &[String],
        symbols_added: &[(String, String, String)], // (name, kind, file_path)
        symbols_removed: &[(String, String, String)],
        task_count_per_module: &HashMap<String, usize>,
    ) -> ClusterResult {
        // 按模块分组
        let mut module_symbols: HashMap<String, Vec<SymbolRef>> = HashMap::new();
        let mut changed_modules = Vec::new();

        for file in files_changed {
            let module = extract_module(file);
            if !module_symbols.contains_key(&module) {
                module_symbols.insert(module.clone(), Vec::new());
            }
            if !changed_modules.contains(&module) {
                changed_modules.push(module.clone());
            }
        }

        for (name, kind, file_path) in symbols_added {
            let module = extract_module(file_path);
            module_symbols.entry(module.clone()).or_default().push(SymbolRef {
                name: name.clone(),
                kind: kind.clone(),
                file_path: file_path.clone(),
            });
            if !changed_modules.contains(&module) {
                changed_modules.push(module);
            }
        }

        for (name, kind, file_path) in symbols_removed {
            let module = extract_module(file_path);
            module_symbols.entry(module.clone()).or_default().push(SymbolRef {
                name: name.clone(),
                kind: kind.clone(),
                file_path: file_path.clone(),
            });
        }

        // 构建节点
        let mut nodes: Vec<CapabilityCluster> = module_symbols
            .into_iter()
            .map(|(module, symbols)| {
                let task_count = task_count_per_module.get(&module).copied().unwrap_or(0);
                let cluster_type = if changed_modules.contains(&module) {
                    ClusterType::Changed
                } else if task_count > 0 {
                    ClusterType::Active
                } else {
                    ClusterType::Cold
                };
                CapabilityCluster {
                    module,
                    symbols,
                    task_count,
                    last_active: None,
                    cluster_type,
                }
            })
            .collect();

        // 按字母排序
        nodes.sort_by(|a, b| a.module.cmp(&b.module));

        // 构建连线：模块间如果共享文件路径前缀，建立弱耦合
        let mut links = Vec::new();
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                // 检查是否有共享的符号文件路径
                let a_sym_paths: Vec<&str> = nodes[i].symbols.iter().map(|s| s.file_path.as_str()).collect();
                let b_sym_paths: Vec<&str> = nodes[j].symbols.iter().map(|s| s.file_path.as_str()).collect();

                let mut shared = false;
                for ap in &a_sym_paths {
                    for bp in &b_sym_paths {
                        if ap == bp {
                            shared = true;
                            break;
                        }
                    }
                    if shared {
                        break;
                    }
                }

                if shared {
                    links.push(CouplingLink {
                        source_module: nodes[i].module.clone(),
                        target_module: nodes[j].module.clone(),
                        strength: 0.5,
                        link_type: LinkType::CoChange,
                    });
                }
            }
        }

        let changed_count = nodes.iter().filter(|n| matches!(n.cluster_type, ClusterType::Changed)).count();
        let active_count = nodes.iter().filter(|n| matches!(n.cluster_type, ClusterType::Active)).count();
        let cold_count = nodes.iter().filter(|n| matches!(n.cluster_type, ClusterType::Cold)).count();

        let avg_coupling = if links.is_empty() {
            0.0
        } else {
            links.iter().map(|l| l.strength).sum::<f64>() / links.len() as f64
        };

        ClusterResult {
            nodes,
            links,
            changed_modules,
            summary: ClusterSummary {
                total_modules: changed_count + active_count + cold_count,
                changed_count,
                active_count,
                cold_count,
                avg_coupling_strength: avg_coupling,
            },
        }
    }
}

/// 从文件路径提取模块名
fn extract_module(file_path: &str) -> String {
    // src/auth/handler.rs → auth
    // crates/core/src/lib.rs → crates
    // tests/integration/test_auth.rs → tests
    let clean = file_path.trim_start_matches("./").trim_start_matches('/');
    let parts: Vec<&str> = clean.split('/').collect();

    if parts.is_empty() {
        return "root".to_string();
    }

    // 跳过常见的根前缀
    let start = match parts[0] {
        "src" | "lib" | "bin" | "tests" => {
            if parts.len() > 1 {
                1
            } else {
                0
            }
        }
        _ => 0,
    };

    if start < parts.len() {
        parts[start].to_string()
    } else {
        parts[0].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_module() {
        assert_eq!(extract_module("src/auth/handler.rs"), "auth");
        assert_eq!(extract_module("src/models/user.rs"), "models");
        assert_eq!(extract_module("src/lib.rs"), "lib.rs"); // root file
        assert_eq!(extract_module("crates/core/src/lib.rs"), "crates");
        assert_eq!(extract_module("tests/integration/test_auth.rs"), "integration");
    }

    #[test]
    fn test_build_empty() {
        let result = CapabilityClusterEngine::build_from_change(
            &[],
            &[],
            &[],
            &HashMap::new(),
        );
        assert!(result.nodes.is_empty());
        assert!(result.links.is_empty());
    }

    #[test]
    fn test_build_with_symbols() {
        let files = vec!["src/auth/handler.rs".to_string()];
        let added = vec![(
            "validate_pwd".to_string(),
            "function".to_string(),
            "src/auth/handler.rs".to_string(),
        )];
        let result = CapabilityClusterEngine::build_from_change(
            &files, &added, &[], &HashMap::new(),
        );
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].module, "auth");
        assert_eq!(result.summary.changed_count, 1);
    }
}
