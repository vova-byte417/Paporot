//! P2 Coupling Builder: 从 P1 vectors + co-change evidence 构建 CouplingGraph。
//!
//! D9: correlation_score = cochange × (1 + λ × similarity), λ ∈ [0.2, 0.4]

use std::collections::HashMap;

use crate::trajectory::p1::vector::TrajectoryVector;
use crate::trajectory::p2::cochange::CochangeEvidence;
use crate::trajectory::p2::similarity::{cosine_sim, compute_grouped_contributions, FeatureContribution};

pub type CapabilityId = String;

/// P2 coupling edge（D9, D12）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CouplingEdge {
    pub from_capability: CapabilityId,
    pub to_capability: CapabilityId,
    /// Primary: 3-layer log-saturated co-change evidence (D11)
    pub cochange_score: f32,
    /// Secondary: cosine(P1_vec_A, P1_vec_B)
    pub similarity_score: f32,
    /// Derived: cochange × (1 + λ × similarity) (D9)
    pub correlation_score: f32,
    /// 4 semantic group attribution (D12)
    pub feature_contribution: FeatureContribution,
}

/// P2 coupling graph。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CouplingGraph {
    pub capabilities: Vec<CapabilityId>,
    pub edges: Vec<CouplingEdge>,
    pub version: u64,
}

/// Builder for CouplingGraph.
pub struct CouplingBuilder {
    /// λ: similarity modulation factor (D9)
    pub lambda: f32,
}

impl Default for CouplingBuilder {
    fn default() -> Self {
        CouplingBuilder { lambda: 0.3 }
    }
}

impl CouplingBuilder {
    pub fn new(lambda: f32) -> Self {
        let lambda = lambda.clamp(0.0, 0.5);
        CouplingBuilder { lambda }
    }

    /// Build coupling edges for a set of capabilities.
    ///
    /// `vectors`: map from capability_id → TrajectoryVector
    /// `cochange_fn`: function (cap_a, cap_b) → CochangeEvidence
    pub fn build_edges<F>(
        &self,
        vectors: &HashMap<CapabilityId, TrajectoryVector>,
        cochange_fn: &F,
    ) -> Vec<CouplingEdge>
    where
        F: Fn(&str, &str) -> CochangeEvidence,
    {
        let caps: Vec<&String> = vectors.keys().collect();
        let mut edges = Vec::new();

        for i in 0..caps.len() {
            let cap_a = caps[i];
            let vec_a = &vectors[cap_a];

            for j in (i + 1)..caps.len() {
                let cap_b = caps[j];
                let vec_b = &vectors[cap_b];

                let evidence = cochange_fn(cap_a, cap_b);
                let similarity = cosine_sim(vec_a, vec_b);

                // D9: correlation = cochange × (1 + λ × similarity)
                let correlation = evidence.fused_score * (1.0 + self.lambda * similarity);

                // D12: feature contribution
                let feature_contribution = compute_grouped_contributions(vec_a, vec_b);

                edges.push(CouplingEdge {
                    from_capability: cap_a.clone(),
                    to_capability: cap_b.clone(),
                    cochange_score: evidence.fused_score,
                    similarity_score: similarity,
                    correlation_score: correlation,
                    feature_contribution,
                });
            }
        }

        edges
    }

    /// Build a complete CouplingGraph (unsorted edges).
    pub fn build<F>(
        &self,
        vectors: &HashMap<CapabilityId, TrajectoryVector>,
        cochange_fn: &F,
    ) -> CouplingGraph
    where
        F: Fn(&str, &str) -> CochangeEvidence,
    {
        let capabilities: Vec<String> = vectors.keys().cloned().collect();
        let edges = self.build_edges(vectors, cochange_fn);

        CouplingGraph {
            capabilities,
            edges,
            version: 1,
        }
    }

    /// Aggregate multiple TrajectoryVectors per capability into a single representative vector.
    /// Uses element-wise mean of scalar vectors.
    pub fn aggregate_vectors(vectors: &[TrajectoryVector]) -> TrajectoryVector {
        if vectors.is_empty() {
            return TrajectoryVector::default();
        }
        if vectors.len() == 1 {
            return vectors[0].clone();
        }

        let n = vectors.len() as f32;
        let mut mean_scalar = vec![0.0_f32; TrajectoryVector::SCALAR_DIM];

        for v in vectors {
            let scalars = v.to_scalar_vec();
            for (i, &s) in scalars.iter().enumerate() {
                mean_scalar[i] += s / n;
            }
        }

        TrajectoryVector {
            tool_entropy: mean_scalar[0],
            phase_entropy: mean_scalar[1],
            transition_entropy: mean_scalar[2],
            loop_ratio: mean_scalar[3],
            backtrack_ratio: mean_scalar[4],
            burst_ratio: mean_scalar[5],
            state_stability_score: mean_scalar[6],
            ..TrajectoryVector::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::p1::vector::TrajectoryVector;
    use std::collections::HashMap;

    fn make_vector(
        te: f32, pe: f32, tre: f32, lr: f32, br: f32, bur: f32, ss: f32,
    ) -> TrajectoryVector {
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

    fn make_evidence(cooccur: bool) -> CochangeEvidence {
        if cooccur {
            CochangeEvidence {
                commit_score: 0.5,
                file_score: 0.3,
                session_score: 0.2,
                fused_score: (1.0_f32 + 1.0 * 0.5 + 1.5 * 0.3 + 0.5 * 0.2).ln(),
            }
        } else {
            CochangeEvidence::default()
        }
    }

    #[test]
    fn test_build_empty() {
        let builder = CouplingBuilder::default();
        let vectors: HashMap<String, TrajectoryVector> = HashMap::new();
        let cochange_fn = |_: &str, _: &str| CochangeEvidence::default();
        let edges = builder.build_edges(&vectors, &cochange_fn);
        assert!(edges.is_empty());
    }

    #[test]
    fn test_build_two_caps() {
        let builder = CouplingBuilder::default();
        let mut vectors = HashMap::new();
        vectors.insert("cap_a".into(), make_vector(0.3, 0.2, 0.1, 0.1, 0.05, 0.02, 0.8));
        vectors.insert("cap_b".into(), make_vector(0.35, 0.25, 0.15, 0.12, 0.06, 0.03, 0.85));

        let evidence_map: HashMap<(String, String), CochangeEvidence> = HashMap::from([
            (("cap_a".into(), "cap_b".into()), make_evidence(true)),
        ]);

        let cochange_fn = |a: &str, b: &str| {
            let key = (a.to_string(), b.to_string());
            let key_rev = (b.to_string(), a.to_string());
            evidence_map.get(&key)
                .or_else(|| evidence_map.get(&key_rev))
                .cloned()
                .unwrap_or_default()
        };

        let edges = builder.build_edges(&vectors, &cochange_fn);
        assert_eq!(edges.len(), 1);
        let edge = &edges[0];
        assert!(edge.correlation_score > 0.0);
        assert!(edge.similarity_score > 0.0);
        assert!(edge.cochange_score > 0.0);

        // Check feature contribution sums to 1
        let fc = &edge.feature_contribution;
        let total = fc.entropy + fc.structural + fc.temporal + fc.density;
        assert!((total - 1.0).abs() < 0.01, "Sum {}", total);
    }

    #[test]
    fn test_build_no_cochange() {
        let builder = CouplingBuilder::default();
        let mut vectors = HashMap::new();
        vectors.insert("cap_a".into(), make_vector(0.3, 0.2, 0.1, 0.1, 0.05, 0.02, 0.8));
        vectors.insert("cap_b".into(), make_vector(0.9, 0.8, 0.7, 0.5, 0.4, 0.3, 0.2));

        let cochange_fn = |_: &str, _: &str| CochangeEvidence::default();
        let edges = builder.build_edges(&vectors, &cochange_fn);
        // With no co-change evidence, correlation should be 0
        assert_eq!(edges[0].correlation_score, 0.0);
        assert!(edges[0].similarity_score > 0.0); // similarity still computed
    }

    #[test]
    fn test_aggregate_vectors() {
        let v1 = make_vector(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let v2 = make_vector(1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0);
        let agg = CouplingBuilder::aggregate_vectors(&[v1, v2]);
        assert!((agg.tool_entropy - 0.5).abs() < 0.01);
        assert!((agg.loop_ratio - 0.5).abs() < 0.01);
    }
}
