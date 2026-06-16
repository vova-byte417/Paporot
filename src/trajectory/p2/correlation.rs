//! P2 Correlation Engine: feature correlation matrix + cross-trajectory similarity。
//!
//! 输出 coupling strength scoring + cross-feature correlation 分析。

use crate::trajectory::p1::vector::TrajectoryVector;
use crate::trajectory::p2::coupling_builder::CouplingEdge;

/// Correlation analysis engine.
pub struct CorrelationEngine;

impl CorrelationEngine {
    /// Compute feature correlation matrix across a set of TrajectoryVectors.
    /// Returns (n_features × n_features) correlation matrix.
    pub fn feature_correlation_matrix(vectors: &[TrajectoryVector]) -> Vec<Vec<f32>> {
        let n = vectors.len();
        if n < 2 {
            return vec![];
        }

        let dim = TrajectoryVector::SCALAR_DIM;
        let mut matrix = vec![vec![0.0_f32; dim]; dim];

        for fi in 0..dim {
            for fj in fi..dim {
                let mut x: Vec<f32> = Vec::with_capacity(n);
                let mut y: Vec<f32> = Vec::with_capacity(n);

                for v in vectors {
                    let scalars = v.to_scalar_vec();
                    x.push(scalars[fi]);
                    y.push(scalars[fj]);
                }

                let corr = pearson_correlation(&x, &y);
                matrix[fi][fj] = corr;
                matrix[fj][fi] = corr;
            }
        }

        matrix
    }

    /// Cross-trajectory similarity: compute all-pairs cosine similarity.
    pub fn cross_similarity_matrix(vectors: &[TrajectoryVector]) -> Vec<Vec<f32>> {
        let n = vectors.len();
        let mut matrix = vec![vec![1.0_f32; n]; n];

        for i in 0..n {
            for j in (i + 1)..n {
                let va = vectors[i].to_scalar_vec();
                let vb = vectors[j].to_scalar_vec();
                let sim = crate::trajectory::p1::vector::cosine_similarity(&va, &vb);
                matrix[i][j] = sim;
                matrix[j][i] = sim;
            }
        }
        matrix
    }

    /// Coupling strength scoring: compute per-capability coupling summary.
    pub fn coupling_strength(
        edges: &[CouplingEdge],
        capability: &str,
    ) -> CouplingStrength {
        let connected: Vec<&CouplingEdge> = edges
            .iter()
            .filter(|e| e.from_capability == capability || e.to_capability == capability)
            .collect();

        let edge_count = connected.len();
        if edge_count == 0 {
            return CouplingStrength::default();
        }

        let total_corr: f32 = connected.iter().map(|e| e.correlation_score).sum();
        let max_corr = connected
            .iter()
            .map(|e| e.correlation_score)
            .fold(0.0_f32, f32::max);
        let avg_corr = total_corr / edge_count as f32;

        // Compute standard deviation
        let variance: f32 = connected
            .iter()
            .map(|e| {
                let diff = e.correlation_score - avg_corr;
                diff * diff
            })
            .sum::<f32>()
            / edge_count as f32;
        let std_dev = variance.sqrt();

        CouplingStrength {
            capability: capability.to_string(),
            edge_count,
            total_coupling: total_corr,
            max_coupling: max_corr,
            avg_coupling: avg_corr,
            std_dev,
        }
    }

    /// Impact analysis: which capabilities are most affected by a change to `capability`?
    pub fn impact(
        edges: &[CouplingEdge],
        capability: &str,
        top_n: usize,
    ) -> Vec<ImpactEntry> {
        let mut impacts: Vec<ImpactEntry> = edges
            .iter()
            .filter_map(|e| {
                if e.from_capability == capability {
                    Some(ImpactEntry {
                        target: e.to_capability.clone(),
                        correlation: e.correlation_score,
                        similarity: e.similarity_score,
                        direction: ImpactDirection::Outgoing,
                    })
                } else if e.to_capability == capability {
                    Some(ImpactEntry {
                        target: e.from_capability.clone(),
                        correlation: e.correlation_score,
                        similarity: e.similarity_score,
                        direction: ImpactDirection::Incoming,
                    })
                } else {
                    None
                }
            })
            .collect();

        impacts.sort_by(|a, b| b.correlation.partial_cmp(&a.correlation).unwrap_or(std::cmp::Ordering::Equal));
        impacts.truncate(top_n);
        impacts
    }
}

/// Per-capability coupling summary.
#[derive(Debug, Clone, Default)]
pub struct CouplingStrength {
    pub capability: String,
    pub edge_count: usize,
    pub total_coupling: f32,
    pub max_coupling: f32,
    pub avg_coupling: f32,
    pub std_dev: f32,
}

/// Impact analysis result.
#[derive(Debug, Clone)]
pub struct ImpactEntry {
    pub target: String,
    pub correlation: f32,
    pub similarity: f32,
    pub direction: ImpactDirection,
}

#[derive(Debug, Clone)]
pub enum ImpactDirection {
    Outgoing,
    Incoming,
}

/// Pearson correlation coefficient.
fn pearson_correlation(x: &[f32], y: &[f32]) -> f32 {
    if x.len() != y.len() || x.is_empty() {
        return 0.0;
    }

    let n = x.len() as f32;
    let mean_x = x.iter().sum::<f32>() / n;
    let mean_y = y.iter().sum::<f32>() / n;

    let mut cov = 0.0;
    let mut var_x = 0.0;
    let mut var_y = 0.0;

    for i in 0..x.len() {
        let dx = x[i] - mean_x;
        let dy = y[i] - mean_y;
        cov += dx * dy;
        var_x += dx * dx;
        var_y += dy * dy;
    }

    let denom = (var_x * var_y).sqrt();
    if denom > 0.0 {
        (cov / denom).clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::p2::similarity::FeatureContribution;

    fn make_edge(from: &str, to: &str, corr: f32, sim: f32) -> CouplingEdge {
        CouplingEdge {
            from_capability: from.into(),
            to_capability: to.into(),
            cochange_score: corr / (1.0 + 0.3 * sim), // reverse formula
            similarity_score: sim,
            correlation_score: corr,
            feature_contribution: FeatureContribution::default(),
        }
    }

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
    fn test_pearson_perfect_positive() {
        let x = vec![1.0, 2.0, 3.0];
        let y = vec![2.0, 4.0, 6.0];
        assert!((pearson_correlation(&x, &y) - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_pearson_perfect_negative() {
        let x = vec![1.0, 2.0, 3.0];
        let y = vec![3.0, 2.0, 1.0];
        assert!((pearson_correlation(&x, &y) + 1.0).abs() < 0.01);
    }

    #[test]
    fn test_pearson_no_correlation() {
        let x = vec![1.0, 1.0, 1.0];
        let y = vec![1.0, 2.0, 3.0];
        assert!((pearson_correlation(&x, &y) - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_feature_correlation_matrix() {
        let vectors = vec![
            make_v(0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.5),
            make_v(0.2, 0.2, 0.2, 0.2, 0.2, 0.2, 0.6),
            make_v(0.3, 0.3, 0.3, 0.3, 0.3, 0.3, 0.7),
        ];
        let matrix = CorrelationEngine::feature_correlation_matrix(&vectors);
        assert_eq!(matrix.len(), 7);
        assert_eq!(matrix[0].len(), 7);
        // All features scale together → near-perfect correlation
        for i in 0..7 {
            for j in 0..7 {
                assert!((matrix[i][j] - 1.0).abs() < 0.01,
                    "Expected corr ~1.0 for [{},{}], got {}", i, j, matrix[i][j]);
            }
        }
    }

    #[test]
    fn test_cross_similarity_matrix() {
        let vectors = vec![
            make_v(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0),
            make_v(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0),
        ];
        let matrix = CorrelationEngine::cross_similarity_matrix(&vectors);
        assert!((matrix[0][1] - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_coupling_strength_single() {
        let edges = vec![
            make_edge("a", "b", 0.7, 0.5),
            make_edge("a", "c", 0.3, 0.2),
        ];
        let strength = CorrelationEngine::coupling_strength(&edges, "a");
        assert_eq!(strength.edge_count, 2);
        assert!(strength.max_coupling > 0.0);
    }

    #[test]
    fn test_coupling_strength_none() {
        let edges = vec![make_edge("a", "b", 0.7, 0.5)];
        let strength = CorrelationEngine::coupling_strength(&edges, "c");
        assert_eq!(strength.edge_count, 0);
        assert_eq!(strength.total_coupling, 0.0);
    }

    #[test]
    fn test_impact_top_n() {
        let edges = vec![
            make_edge("a", "b", 0.7, 0.5),
            make_edge("a", "c", 0.3, 0.2),
            make_edge("a", "d", 0.5, 0.4),
        ];
        let impacts = CorrelationEngine::impact(&edges, "a", 2);
        assert_eq!(impacts.len(), 2);
        assert_eq!(impacts[0].target, "b"); // highest correlation
    }
}
