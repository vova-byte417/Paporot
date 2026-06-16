//! P1 Feature Registry: 全局版本化特征坐标系统。
//!
//! D5: append-only, versioned, sparse feature space.
//! D8: 逻辑重投影（view mapping），历史 vector 不变。

use std::collections::HashMap;

/// 稀疏向量（D5）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SparseVector {
    pub indices: Vec<u32>,
    pub values: Vec<f32>,
    #[serde(default)]
    pub registry_version: u64,
}

impl SparseVector {
    pub fn new(indices: Vec<u32>, values: Vec<f32>, registry_version: u64) -> Self {
        assert_eq!(indices.len(), values.len(), "indices and values must have same length");
        SparseVector {
            indices,
            values,
            registry_version,
        }
    }

    pub fn empty() -> Self {
        SparseVector {
            indices: vec![],
            values: vec![],
            registry_version: 0,
        }
    }

    /// 转换为 dense Vec<f32>（按最大 index padding）。
    pub fn to_dense(&self, dim: usize) -> Vec<f32> {
        let mut dense = vec![0.0_f32; dim];
        for (i, &idx) in self.indices.iter().enumerate() {
            let idx = idx as usize;
            if idx < dim {
                dense[idx] = self.values[i];
            }
        }
        dense
    }

    /// 从 dense Vec<f32> 构建 sparse（去除非零）。
    pub fn from_dense(dense: &[f32], registry_version: u64) -> Self {
        let mut indices = Vec::new();
        let mut values = Vec::new();
        for (i, &v) in dense.iter().enumerate() {
            if v.abs() > 1e-8 {
                indices.push(i as u32);
                values.push(v);
            }
        }
        SparseVector {
            indices,
            values,
            registry_version,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn len(&self) -> usize {
        self.indices.len()
    }
}

/// 全局 Feature Registry（版本化，append-only）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FeatureRegistry {
    pub version: u64,
    /// tool category → dimension index
    pub tool_mapping: HashMap<String, u32>,
    /// phase label → dimension index
    pub phase_mapping: HashMap<String, u32>,
    /// parent registry version (for reprojection chain)
    pub parent_version: Option<u64>,
    /// 历史映射：old_version → (tool_remap, phase_remap)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remap_history: Vec<RemapEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemapEntry {
    pub from_version: u64,
    pub to_version: u64,
    /// tool index remap: old_index → new_index (None = unknown/new)
    pub tool_index_map: HashMap<u32, u32>,
    /// phase index remap
    pub phase_index_map: HashMap<u32, u32>,
}

impl FeatureRegistry {
    /// 创建初始 registry（与 P0 tool_category 对齐）。
    pub fn initial() -> Self {
        let mut tool_mapping = HashMap::new();
        let mut phase_mapping = HashMap::new();

        let tools = vec!["locate", "modify", "verify", "commit", "other"];
        let phases = vec!["locate", "modify", "verify", "commit", "other"];

        for (i, t) in tools.iter().enumerate() {
            tool_mapping.insert(t.to_string(), i as u32);
        }
        for (i, p) in phases.iter().enumerate() {
            phase_mapping.insert(p.to_string(), i as u32);
        }

        FeatureRegistry {
            version: 1,
            tool_mapping,
            phase_mapping,
            parent_version: None,
            remap_history: Vec::new(),
        }
    }

    /// 添加新的 tool category（append-only）。
    /// 返回新 dimension index。
    pub fn register_tool(&mut self, category: &str) -> u32 {
        if let Some(&idx) = self.tool_mapping.get(category) {
            return idx;
        }
        let idx = self.tool_mapping.len() as u32;
        self.tool_mapping.insert(category.to_string(), idx);
        idx
    }

    /// 添加新的 phase label。
    pub fn register_phase(&mut self, label: &str) -> u32 {
        if let Some(&idx) = self.phase_mapping.get(label) {
            return idx;
        }
        let idx = self.phase_mapping.len() as u32;
        self.phase_mapping.insert(label.to_string(), idx);
        idx
    }

    /// 升级 registry：生成新版本并记录 remap。
    pub fn upgrade(&self) -> Self {
        let new_version = self.version + 1;
        FeatureRegistry {
            version: new_version,
            tool_mapping: self.tool_mapping.clone(),
            phase_mapping: self.phase_mapping.clone(),
            parent_version: Some(self.version),
            remap_history: self.remap_history.clone(),
        }
    }

    /// D8: 逻辑重投影 — 将旧版本 vector 投影到当前 registry 空间。
    pub fn reproject(&self, vec: &SparseVector) -> SparseVector {
        if vec.registry_version == self.version {
            return vec.clone();
        }

        // Find the remap chain from source version to current
        let mut current_indices: Vec<u32> = vec.indices.clone();
        let mut current_values: Vec<f32> = vec.values.clone();
        let target_version = vec.registry_version;

        // Walk remap history from target_version to current
        let mut v = target_version;
        while v < self.version {
            if let Some(entry) = self.remap_history.iter().find(|e| e.from_version == v) {
                // Apply tool index remap
                let new_dim = self.tool_mapping.len().max(self.phase_mapping.len()) as u32;
                let mut new_indices = Vec::with_capacity(current_indices.len());
                let mut new_values = Vec::with_capacity(current_values.len());

                for (i, &idx) in current_indices.iter().enumerate() {
                    // Try tool remap first, then phase
                    if let Some(&new_idx) = entry.tool_index_map.get(&idx) {
                        new_indices.push(new_idx);
                        new_values.push(current_values[i]);
                    } else if let Some(&new_idx) = entry.phase_index_map.get(&idx) {
                        new_indices.push(new_idx);
                        new_values.push(current_values[i]);
                    } else if idx < new_dim {
                        // direct mapping (index unchanged)
                        new_indices.push(idx);
                        new_values.push(current_values[i]);
                    }
                    // else: unknown mapping → drop silently
                }

                current_indices = new_indices;
                current_values = new_values;
                v = entry.to_version;
            } else {
                break; // no remap entry, stop
            }
        }

        SparseVector {
            indices: current_indices,
            values: current_values,
            registry_version: self.version,
        }
    }

    /// 获取 tool dim 数量。
    pub fn tool_dim(&self) -> usize {
        self.tool_mapping.len()
    }

    /// 获取 phase dim 数量。
    pub fn phase_dim(&self) -> usize {
        self.phase_mapping.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_initial() {
        let reg = FeatureRegistry::initial();
        assert_eq!(reg.version, 1);
        assert_eq!(reg.tool_mapping.len(), 5);
        assert_eq!(reg.phase_mapping.len(), 5);
        assert_eq!(reg.tool_mapping.get("locate"), Some(&0));
        assert_eq!(reg.tool_mapping.get("modify"), Some(&1));
        assert_eq!(reg.tool_mapping.get("other"), Some(&4));
    }

    #[test]
    fn test_register_new_tool() {
        let mut reg = FeatureRegistry::initial();
        let idx = reg.register_tool("semantic_search");
        assert_eq!(idx, 5);
        assert_eq!(reg.tool_mapping.len(), 6);
    }

    #[test]
    fn test_register_existing_tool() {
        let mut reg = FeatureRegistry::initial();
        let idx = reg.register_tool("locate");
        assert_eq!(idx, 0); // returns existing index
        assert_eq!(reg.tool_mapping.len(), 5); // no duplication
    }

    #[test]
    fn test_sparse_vector_to_dense() {
        let sv = SparseVector::new(vec![0, 2, 4], vec![0.1, 0.3, 0.5], 1);
        let dense = sv.to_dense(5);
        assert_eq!(dense, vec![0.1, 0.0, 0.3, 0.0, 0.5]);
    }

    #[test]
    fn test_sparse_vector_from_dense() {
        let dense = vec![0.1, 0.0, 0.3, 0.0, 0.5];
        let sv = SparseVector::from_dense(&dense, 1);
        assert_eq!(sv.indices, vec![0, 2, 4]);
        assert_eq!(sv.values, vec![0.1, 0.3, 0.5]);
    }

    #[test]
    fn test_sparse_vector_empty() {
        let sv = SparseVector::empty();
        assert!(sv.is_empty());
        assert_eq!(sv.to_dense(5), vec![0.0; 5]);
    }

    #[test]
    fn test_reproject_same_version() {
        let reg = FeatureRegistry::initial();
        let sv = SparseVector::new(vec![0, 1], vec![0.5, 0.5], 1);
        let projected = reg.reproject(&sv);
        assert_eq!(projected.indices, vec![0, 1]);
        assert_eq!(projected.values, vec![0.5, 0.5]);
        assert_eq!(projected.registry_version, 1);
    }

    #[test]
    fn test_reproject_direct_mapping() {
        // v1 → v2: indices unchanged (no new categories added yet)
        let mut reg_v1 = FeatureRegistry::initial();
        reg_v1.register_tool("semantic_search"); // now has 6 tools
        let reg_v2 = reg_v1.upgrade();

        let sv = SparseVector::new(vec![0, 1, 2], vec![0.3, 0.4, 0.3], 1);
        let projected = reg_v2.reproject(&sv);
        // No remap entry, so indices stay the same
        assert_eq!(projected.registry_version, 2);
    }

    #[test]
    fn test_reproject_with_remap() {
        let mut reg_v1 = FeatureRegistry::initial();
        reg_v1.register_tool("new_tool_6"); // index 5

        let mut reg_v2 = reg_v1.upgrade();
        // Create a remap entry: v1→v2 where index 5 → 5 (direct)
        reg_v2.remap_history.push(RemapEntry {
            from_version: 1,
            to_version: 2,
            tool_index_map: HashMap::new(),
            phase_index_map: HashMap::new(),
        });

        let sv = SparseVector::new(vec![0, 1, 5], vec![0.2, 0.5, 0.3], 1);
        let projected = reg_v2.reproject(&sv);
        assert_eq!(projected.registry_version, 2);
        // index 5 should be preserved (direct mapping when key not in remap)
        assert!(projected.indices.contains(&5));
    }
}
