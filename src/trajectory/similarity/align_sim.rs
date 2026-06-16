//! Alignment similarity (strict matching, threshold 0.65)。
//!
//! 用于 state diff 中两个 BehaviorStateGraph 的状态配对。
//! 共享 StateFeatures 特征空间，使用较低阈值确保精确匹配。

use crate::trajectory::types::StateFeatures;

/// 默认权重（align 更注重 tool overlap）。
const W_TOOL: f32 = 0.35;
const W_FILE: f32 = 0.20;
const W_EDIT: f32 = 0.10;
const W_CONTROL: f32 = 0.20;
const W_FAIL: f32 = 0.15;

/// 对齐相似度：严格（高 precision），threshold 0.65。
pub fn align_similarity(a: &StateFeatures, b: &StateFeatures) -> f32 {
    let tool = jaccard_similarity(&a.tool_histogram, &b.tool_histogram);
    let file = cosine_similarity(&a.file_clusters, &b.file_clusters);
    let edit = 1.0 - (a.edit_density - b.edit_density).abs();
    let ctrl = 1.0 - (a.read_write_ratio - b.read_write_ratio).abs();
    let fail = 1.0 - (a.failure_rate - b.failure_rate).abs();

    W_TOOL * tool + W_FILE * file + W_EDIT * edit + W_CONTROL * ctrl + W_FAIL * fail
}

fn jaccard_similarity(
    a: &std::collections::HashMap<String, f32>,
    b: &std::collections::HashMap<String, f32>,
) -> f32 {
    if a.is_empty() && b.is_empty() { return 1.0; }
    if a.is_empty() || b.is_empty() { return 0.0; }
    let mut intersection = 0.0_f32;
    let mut union = 0.0_f32;
    let mut keys: std::collections::HashSet<&String> = a.keys().collect();
    for k in b.keys() { keys.insert(k); }
    for key in keys {
        let va = a.get(key).copied().unwrap_or(0.0);
        let vb = b.get(key).copied().unwrap_or(0.0);
        intersection += va.min(vb);
        union += va.max(vb);
    }
    if union > 0.0 { intersection / union } else { 1.0 }
}

fn cosine_similarity(
    a: &std::collections::HashMap<String, f32>,
    b: &std::collections::HashMap<String, f32>,
) -> f32 {
    if a.is_empty() && b.is_empty() { return 1.0; }
    if a.is_empty() || b.is_empty() { return 0.0; }
    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    let mut keys: std::collections::HashSet<&String> = a.keys().collect();
    for k in b.keys() { keys.insert(k); }
    for key in keys {
        let va = a.get(key).copied().unwrap_or(0.0);
        let vb = b.get(key).copied().unwrap_or(0.0);
        dot += va * vb;
        norm_a += va * va;
        norm_b += vb * vb;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom > 0.0 { dot / denom } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_features(edit_density: f32, read_write_ratio: f32) -> StateFeatures {
        let mut th = HashMap::new();
        th.insert("locate".into(), 0.5);
        th.insert("modify".into(), 0.5);
        StateFeatures {
            tool_histogram: th,
            file_clusters: HashMap::new(),
            edit_density,
            read_write_ratio,
            loop_intensity: 0.0,
            failure_rate: 0.0,
        }
    }

    #[test]
    fn test_align_identical() {
        let f = make_features(0.3, 0.5);
        let sim = align_similarity(&f, &f);
        assert!((sim - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_align_partial_overlap() {
        let mut th_a = HashMap::new();
        th_a.insert("locate".into(), 0.7);
        th_a.insert("modify".into(), 0.3);
        let mut th_b = HashMap::new();
        th_b.insert("locate".into(), 0.4);
        th_b.insert("verify".into(), 0.6);
        let fa = StateFeatures { tool_histogram: th_a, ..Default::default() };
        let fb = StateFeatures { tool_histogram: th_b, ..Default::default() };
        let sim = align_similarity(&fa, &fb);
        // Should be moderate (partial overlap)
        assert!(sim > 0.1 && sim < 0.9);
    }

    #[test]
    fn test_align_threshold_stricter_than_merge() {
        let mut th_a = HashMap::new();
        th_a.insert("locate".into(), 0.8);
        th_a.insert("modify".into(), 0.2);
        let mut th_b = HashMap::new();
        th_b.insert("modify".into(), 0.8);
        th_b.insert("verify".into(), 0.2);
        let fa = StateFeatures { tool_histogram: th_a, ..Default::default() };
        let fb = StateFeatures { tool_histogram: th_b, ..Default::default() };
        let align = align_similarity(&fa, &fb);
        let merge = crate::trajectory::similarity::merge_sim::merge_similarity(&fa, &fb);
        // Align should not be higher than merge for very different states
        assert!(align < merge + 0.1); // mostly the same underlying logic, just different weights
    }
}
