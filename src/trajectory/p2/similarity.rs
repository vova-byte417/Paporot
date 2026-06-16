//! P2 Vector Similarity: cosine, Jaccard, weighted feature distance.
//!
//! D13: 主 attribution = weighted linear projection（wi × Ai × Bi）。
//! D12: feature_contribution 聚合为 4 个语义 group。

use crate::trajectory::p1::vector::TrajectoryVector;

/// Cosine similarity between two TrajectoryVectors (scalar part only).
pub fn cosine_sim(a: &TrajectoryVector, b: &TrajectoryVector) -> f32 {
    let va = a.to_scalar_vec();
    let vb = b.to_scalar_vec();
    crate::trajectory::p1::vector::cosine_similarity(&va, &vb)
}

/// Jaccard similarity on sparse distributions.
pub fn jaccard_sparse(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    if a.is_empty() {
        return 1.0;
    }

    let mut inter = 0.0_f32;
    let mut union = 0.0_f32;
    for i in 0..a.len() {
        inter += a[i].min(b[i]);
        union += a[i].max(b[i]);
    }
    if union > 0.0 {
        inter / union
    } else {
        1.0
    }
}

/// Weighted feature distance via linear projection (D13).
/// attribution_i = wi × VA_i × VB_i
pub fn weighted_projection(a: &[f32], b: &[f32], weights: &[f32]) -> Vec<f32> {
    debug_assert_eq!(a.len(), b.len());
    debug_assert_eq!(a.len(), weights.len());

    let mut contributions = vec![0.0_f32; a.len()];
    let mut total = 0.0_f32;

    for i in 0..a.len() {
        contributions[i] = weights[i] * a[i] * b[i];
        total += contributions[i];
    }

    if total > 0.0 {
        for c in &mut contributions {
            *c /= total;
        }
    }

    contributions
}

/// D12: 4 semantic feature groups.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct FeatureContribution {
    pub entropy: f32,
    pub structural: f32,
    pub temporal: f32,
    pub density: f32,
}

/// D12 + D13: 计算 grouped feature contribution。
///
/// 内部 per-field linear projection，聚合为 4 个 group。
/// P1 scalar layout: [tool_entropy, phase_entropy, transition_entropy, loop_ratio,
///                    backtrack_ratio, burst_ratio, state_stability_score]
pub fn compute_grouped_contributions(a: &TrajectoryVector, b: &TrajectoryVector) -> FeatureContribution {
    let va = a.to_scalar_vec();
    let vb = b.to_scalar_vec();

    // Feature weights (D7 cross-feature scaling weights)
    let weights = vec![
        1.0,  // entropy
        1.0,  // entropy
        1.0,  // entropy
        1.2,  // structural
        1.0,  // temporal
        1.5,  // density
        0.8,  // stability (secondary)
    ];

    let per_field = weighted_projection(&va, &vb, &weights);

    // Group aggregation:
    //   entropy group = tool_entropy(0) + phase_entropy(1) + transition_entropy(2)
    //   structural    = loop_ratio(3)
    //   temporal      = backtrack_ratio(4)
    //   density       = burst_ratio(5)
    //   (stability at index 6 is distributed proportionally)
    let entropy = per_field[0] + per_field[1] + per_field[2];
    let structural = per_field[3];
    let temporal = per_field[4];
    let density = per_field[5];
    // Redistribute stability(6) proportionally into other groups
    let stability = per_field[6];
    let base_sum = entropy + structural + temporal + density;
    let entropy = if base_sum > 0.0 { entropy + stability * entropy / base_sum } else { entropy + stability * 0.25 };
    let structural = if base_sum > 0.0 { structural + stability * structural / base_sum } else { structural + stability * 0.25 };
    let temporal = if base_sum > 0.0 { temporal + stability * temporal / base_sum } else { temporal + stability * 0.25 };
    let density = if base_sum > 0.0 { density + stability * density / base_sum } else { density + stability * 0.25 };

    FeatureContribution {
        entropy,
        structural,
        temporal,
        density,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::p1::vector::TrajectoryVector;

    fn make_v(te: f32, pe: f32, tre: f32, lr: f32, br: f32, bur: f32, ss: f32) -> TrajectoryVector {
        TrajectoryVector {
            tool_entropy: te,
            phase_entropy: pe,
            transition_entropy: tre,
            loop_ratio: lr,
            backtrack_ratio: br,
            burst_ratio: bur,
            state_stability_score: ss,
            ..Default::default()
        }
    }

    #[test]
    fn test_cosine_identical() {
        let v = make_v(0.5, 0.4, 0.3, 0.2, 0.1, 0.05, 0.9);
        assert!((cosine_sim(&v, &v) - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_cosine_different() {
        let a = make_v(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
        let b = make_v(0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0);
        let sim = cosine_sim(&a, &b);
        assert!(sim < 0.5, "Expected low similarity for different vectors");
    }

    #[test]
    fn test_jaccard_identical() {
        let a = vec![0.3, 0.7, 0.0];
        assert!((jaccard_sparse(&a, &a) - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_jaccard_disjoint() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!((jaccard_sparse(&a, &b) - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_weighted_projection() {
        let a = vec![1.0, 0.5, 0.0];
        let b = vec![0.5, 0.5, 1.0];
        let w = vec![1.0, 1.0, 1.0];
        let c = weighted_projection(&a, &b, &w);
        // contrib: [0.5, 0.25, 0.0], total=0.75
        // normalized: [0.667, 0.333, 0.0]
        assert!((c[0] - 0.666).abs() < 0.01, "expected ~0.667");
        assert!((c[1] - 0.333).abs() < 0.01, "expected ~0.333");
    }

    #[test]
    fn test_grouped_contributions() {
        let a = make_v(0.8, 0.7, 0.6, 0.2, 0.3, 0.1, 0.9);
        let b = make_v(0.7, 0.8, 0.5, 0.3, 0.2, 0.15, 0.85);
        let fc = compute_grouped_contributions(&a, &b);
        let total = fc.entropy + fc.structural + fc.temporal + fc.density;
        assert!((total - 1.0).abs() < 0.01, "contributions should sum to 1.0, got {}", total);
        assert!(fc.entropy > 0.0, "entropy contribution should be positive");
    }
}
