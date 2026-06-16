//! Merge similarity (loose clustering, threshold 0.85)。
//!
//! 用于 Layer 3 相邻 StateCandidate 合并决策。
//! 共享 StateFeatures 特征空间，使用高阈值以避免过度拆分。

use crate::trajectory::types::StateFeatures;

/// 默认权重。
const W_TOOL: f32 = 0.30;
const W_FILE: f32 = 0.15;
const W_EDIT: f32 = 0.15;
const W_CONTROL: f32 = 0.25;
const W_FAIL: f32 = 0.15;

/// 合并相似度：宽松（高 recall），threshold 0.85。
/// 加权多分量相似度。
pub fn merge_similarity(a: &StateFeatures, b: &StateFeatures) -> f32 {
    let tool = jaccard_similarity(&a.tool_histogram, &b.tool_histogram);
    let file = cosine_similarity(&a.file_clusters, &b.file_clusters);
    let edit = 1.0 - (a.edit_density - b.edit_density).abs();
    let ctrl = 1.0 - (a.read_write_ratio - b.read_write_ratio).abs(); // simplified control flow proxy
    let fail = 1.0 - (a.failure_rate - b.failure_rate).abs();

    W_TOOL * tool + W_FILE * file + W_EDIT * edit + W_CONTROL * ctrl + W_FAIL * fail
}

/// Jaccard similarity between two hashmaps.
fn jaccard_similarity(
    a: &std::collections::HashMap<String, f32>,
    b: &std::collections::HashMap<String, f32>,
) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let mut intersection = 0.0_f32;
    let mut union = 0.0_f32;

    let mut keys: std::collections::HashSet<&String> = a.keys().collect();
    for k in b.keys() {
        keys.insert(k);
    }

    for key in keys {
        let va = a.get(key).copied().unwrap_or(0.0);
        let vb = b.get(key).copied().unwrap_or(0.0);
        intersection += va.min(vb);
        union += va.max(vb);
    }

    if union > 0.0 { intersection / union } else { 1.0 }
}

/// Cosine similarity between two feature vectors.
fn cosine_similarity(
    a: &std::collections::HashMap<String, f32>,
    b: &std::collections::HashMap<String, f32>,
) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;

    let mut keys: std::collections::HashSet<&String> = a.keys().collect();
    for k in b.keys() {
        keys.insert(k);
    }

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
    fn test_merge_identical() {
        let f = make_features(0.3, 0.5);
        let sim = merge_similarity(&f, &f);
        assert!((sim - 1.0).abs() < 0.01, "identical features should be 1.0, got {}", sim);
    }

    #[test]
    fn test_merge_different_phases() {
        let mut th_a = HashMap::new();
        th_a.insert("locate".into(), 1.0);
        let mut th_b = HashMap::new();
        th_b.insert("verify".into(), 1.0);
        let fa = StateFeatures { tool_histogram: th_a, ..Default::default() };
        let fb = StateFeatures { tool_histogram: th_b, ..Default::default() };
        let sim = merge_similarity(&fa, &fb);
        // Different phases with other matching defaults → 0.7 (file/ctrl/fail match)
        assert!(sim > 0.6 && sim < 0.8, "expected ~0.7, got {}", sim);
    }

    #[test]
    fn test_jaccard_identical() {
        let mut a = HashMap::new();
        a.insert("x".into(), 0.5);
        a.insert("y".into(), 0.5);
        assert!((jaccard_similarity(&a, &a) - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_jaccard_disjoint() {
        let mut a = HashMap::new();
        a.insert("x".into(), 1.0);
        let mut b = HashMap::new();
        b.insert("y".into(), 1.0);
        assert!((jaccard_similarity(&a, &b) - 0.0).abs() < 0.01);
    }
}
