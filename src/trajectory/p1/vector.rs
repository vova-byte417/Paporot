//! P1 Vector Builder: 组装 TrajectoryVector + normalization pipeline。
//!
//! D7: bounded normalization → log compression → robust scaling。
//! 禁止 whitening（保留语义轴可解释性）。

use crate::trajectory::p1::feature_extractor::FeatureSnapshot;
use crate::trajectory::p1::registry::SparseVector;
use crate::trajectory::p1::sequence_metrics::SequenceMetrics;
use crate::trajectory::p1::timeseries::TimeSeries;

/// P1 核心产物：行为轨迹的数值向量表示。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrajectoryVector {
    pub tool_distribution: SparseVector,
    pub state_distribution: SparseVector,
    pub tool_entropy: f32,
    pub phase_entropy: f32,
    pub transition_entropy: f32,
    pub loop_ratio: f32,
    pub backtrack_ratio: f32,
    pub burst_ratio: f32,
    pub state_stability_score: f32,
    pub edit_intensity_curve: Vec<f32>,
}

impl Default for TrajectoryVector {
    fn default() -> Self {
        TrajectoryVector {
            tool_distribution: SparseVector::empty(),
            state_distribution: SparseVector::empty(),
            tool_entropy: 0.0,
            phase_entropy: 0.0,
            transition_entropy: 0.0,
            loop_ratio: 0.0,
            backtrack_ratio: 0.0,
            burst_ratio: 0.0,
            state_stability_score: 0.0,
            edit_intensity_curve: Vec::new(),
        }
    }
}

impl TrajectoryVector {
    /// 将所有标量字段展平为 dense Vec<f32>（用于 cosine/distance）。
    /// 顺序: [tool_entropy, phase_entropy, transition_entropy, loop_ratio,
    ///         backtrack_ratio, burst_ratio, state_stability_score]
    pub fn to_scalar_vec(&self) -> Vec<f32> {
        vec![
            self.tool_entropy,
            self.phase_entropy,
            self.transition_entropy,
            self.loop_ratio,
            self.backtrack_ratio,
            self.burst_ratio,
            self.state_stability_score,
        ]
    }

    /// 标量向量维度。
    pub const SCALAR_DIM: usize = 7;
}

/// 构建 normalized TrajectoryVector。
///
/// Pipeline (D7):
///   1. feature extraction (already done via FeatureSnapshot + SequenceMetrics + TimeSeries)
///   2. bounded normalization (entropy: /log(N), ratios: [0,1])
///   3. log compression (burst heavy-tail)
///   4. robust scaling (median/IQR)
pub fn build_vector(
    snapshot: &FeatureSnapshot,
    metrics: &SequenceMetrics,
    timeseries: &TimeSeries,
    registry_version: u64,
) -> TrajectoryVector {
    // ── Step 1: Extract raw values ──

    // Build SparseVectors from histograms
    let tool_categories = ["locate", "modify", "verify", "commit", "other"];
    let _tool_indices: Vec<u32> = (0..tool_categories.len() as u32).collect();
    let tool_values: Vec<f32> = tool_categories
        .iter()
        .map(|c| snapshot.tool_histogram.get(*c).copied().unwrap_or(0.0))
        .collect();
    let tool_distribution = SparseVector::from_dense(&tool_values, registry_version);

    let phase_labels = ["locate", "modify", "verify", "commit", "other"];
    let _phase_indices: Vec<u32> = (0..phase_labels.len() as u32).collect();
    let phase_values: Vec<f32> = phase_labels
        .iter()
        .map(|p| snapshot.state_histogram.get(*p).copied().unwrap_or(0.0))
        .collect();
    let state_distribution = SparseVector::from_dense(&phase_values, registry_version);

    // ── Step 2: Bounded normalization ── (D7)

    // Entropy: normalize by log2(max_possible)
    let tool_entropy = bounded_entropy(snapshot.tool_entropy, snapshot.total_tools.max(1) as f32);
    let phase_entropy = bounded_entropy(
        snapshot.phase_entropy,
        snapshot.state_count.max(2) as f32 * snapshot.state_count.max(2) as f32,
    );
    let transition_entropy = bounded_entropy(
        snapshot.transition_entropy,
        snapshot.state_count.max(2) as f32 * snapshot.state_count.max(2) as f32,
    );

    // Ratios: already in [0,1], clamp for safety
    let loop_ratio = snapshot.state_count.max(1) as f32;
    let loop_ratio_norm = if loop_ratio > 0.0 {
        (metrics.loop_ratio / loop_ratio).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let backtrack_ratio = metrics.backtrack_ratio.clamp(0.0, 1.0);

    // ── Step 3: Log compression for burst (heavy-tail) ── (D7)
    let burst_ratio = log_compress(metrics.burst_ratio);

    // ── Step 4: state_stability_score ── (D2: already from cosine, in [0,1])
    // Computed externally via adjacency cosine similarity
    let state_stability_score = snapshot.state_count.max(1) as f32; // placeholder

    // ── Robust scaling: median/IQR normalization across features ── (D7)
    // Apply per-feature scaling to align magnitudes
    let raw_vec = vec![
        tool_entropy,
        phase_entropy,
        transition_entropy,
        loop_ratio_norm,
        backtrack_ratio,
        burst_ratio,
        state_stability_score,
    ];

    let scaled = robust_scale(&raw_vec);

    TrajectoryVector {
        tool_distribution,
        state_distribution,
        tool_entropy: scaled[0],
        phase_entropy: scaled[1],
        transition_entropy: scaled[2],
        loop_ratio: scaled[3],
        backtrack_ratio: scaled[4],
        burst_ratio: scaled[5],
        state_stability_score: scaled[6],
        edit_intensity_curve: timeseries.edit_intensity_curve.clone(),
    }
}

/// 构建 state_stability_score via adjacency cosine (D2)。
/// 注意：这与 feature_extractor 不同——它测量相邻 state 的结构连续性。
pub fn compute_state_stability(
    graph: &crate::trajectory::types::BehaviorStateGraph,
) -> f32 {
    let states = &graph.states;
    if states.len() < 2 {
        return if states.len() == 1 { 1.0 } else { 0.0 };
    }

    // State vector = [tool_histogram features + edit_density + read_write_ratio]
    let mut similarities = Vec::with_capacity(states.len() - 1);

    for i in 1..states.len() {
        let a = &states[i - 1].features;
        let b = &states[i].features;

        // Build vectors from StateFeatures
        let categories = ["locate", "modify", "verify", "commit", "other"];
        let mut va = Vec::with_capacity(7);
        let mut vb = Vec::with_capacity(7);

        for cat in &categories {
            va.push(a.tool_histogram.get(*cat).copied().unwrap_or(0.0));
            vb.push(b.tool_histogram.get(*cat).copied().unwrap_or(0.0));
        }
        va.push(a.edit_density);
        vb.push(b.edit_density);
        va.push(a.read_write_ratio);
        vb.push(b.read_write_ratio);

        let sim = cosine_similarity(&va, &vb);
        similarities.push(sim);
    }

    if similarities.is_empty() {
        0.0
    } else {
        similarities.iter().sum::<f32>() / similarities.len() as f32
    }
}

/// D7: bounded entropy normalization → [0,1].
fn bounded_entropy(raw: f32, max_possible: f32) -> f32 {
    if max_possible <= 0.0 {
        return 0.0;
    }
    let max_entropy = max_possible.log2();
    if max_entropy <= 0.0 {
        return 0.0;
    }
    (raw / max_entropy).clamp(0.0, 1.0)
}

/// D7: log compression for heavy-tail features.
fn log_compress(value: f32) -> f32 {
    if value <= 0.0 {
        return 0.0;
    }
    // log(1 + value) / log(2)  (assume max possible ≈ 1.0 for normalized ratios)
    ((1.0 + value).ln() / 2.0_f32.ln()).clamp(0.0, 1.0)
}

/// D7: robust scaling (median ± IQR).
fn robust_scale(values: &[f32]) -> Vec<f32> {
    let n = values.len();
    if n == 0 {
        return vec![];
    }

    // Compute median
    let mut sorted: Vec<f32> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = if n % 2 == 0 {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    };

    // Compute IQR
    let q1_idx = n / 4;
    let q3_idx = (3 * n) / 4;
    let q1 = sorted[q1_idx.min(n - 1)];
    let q3 = sorted[q3_idx.min(n - 1)];
    let iqr = q3 - q1;

    if iqr.abs() < 1e-8 {
        // No variance → return original (or zeros from median)
        return values.to_vec();
    }

    values
        .iter()
        .map(|v| (v - median) / iqr)
        .collect()
}

/// Cosine similarity between two f32 vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    debug_assert_eq!(a.len(), b.len(), "Vectors must have same length");

    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;

    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom > 0.0 {
        (dot / denom).clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounded_entropy_max() {
        // 4 unique categories → max entropy log2(4) = 2.0
        let raw = 2.0;
        let bounded = bounded_entropy(raw, 4.0);
        assert!((bounded - 1.0).abs() < 0.01, "Expected 1.0, got {}", bounded);
    }

    #[test]
    fn test_bounded_entropy_half() {
        let raw = 1.0; // half of max entropy for 4 categories
        let bounded = bounded_entropy(raw, 4.0);
        assert!((bounded - 0.5).abs() < 0.01, "Expected 0.5, got {}", bounded);
    }

    #[test]
    fn test_bounded_entropy_zero() {
        assert_eq!(bounded_entropy(0.0, 4.0), 0.0);
    }

    #[test]
    fn test_log_compress_zero() {
        assert_eq!(log_compress(0.0), 0.0);
    }

    #[test]
    fn test_log_compress_one() {
        let val = log_compress(1.0);
        // log2(2) = 1.0
        assert!((val - 1.0).abs() < 0.01, "Expected 1.0, got {}", val);
    }

    #[test]
    fn test_robust_scale() {
        let values = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let scaled = robust_scale(&values);
        assert_eq!(scaled.len(), 5);
        // median=2.0, IQR=q3(3.0)-q1(1.0)=2.0
        // scaled = (v-2.0)/2.0 → [-1.0, -0.5, 0.0, 0.5, 1.0]
        assert!((scaled[2] - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_cosine_identical() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_cosine_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine_similarity(&a, &b) - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_build_vector_basic() {
        use crate::trajectory::p1::feature_extractor::FeatureSnapshot;
        use std::collections::HashMap;

        let snap = FeatureSnapshot {
            tool_histogram: HashMap::from([
                ("locate".into(), 0.3),
                ("modify".into(), 0.5),
                ("verify".into(), 0.2),
            ]),
            state_histogram: HashMap::from([
                ("locate".into(), 0.4),
                ("modify".into(), 0.6),
            ]),
            transition_counts: HashMap::new(),
            tool_entropy: 1.5,
            phase_entropy: 1.0,
            transition_entropy: 0.8,
            avg_tools_per_state: 6.0,
            state_count: 4,
            total_tools: 24,
        };

        let metrics = SequenceMetrics {
            loop_ratio: 0.3,
            backtrack_ratio: 0.2,
            burst_ratio: 0.15,
            loops_detected: vec![],
        };

        let ts = TimeSeries {
            window_ms: 1000,
            transition_counts: vec![1, 2],
            edit_intensity_curve: vec![0.1, 0.3],
            tool_usage_flat: vec![0.3, 0.5, 0.2, 0.0, 0.0, 0.4, 0.6, 0.0, 0.0, 0.0],
        };

        let vector = build_vector(&snap, &metrics, &ts, 1);
        assert_eq!(vector.edit_intensity_curve.len(), 2);
        assert!(!vector.tool_distribution.is_empty());
    }
}
