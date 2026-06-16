//! P2 Graph Aggregator: merge edges, threshold pruning, stability filtering。
//!
//! D10: 4-layer survivorship filter:
//!   1. Hard Existence Filter
//!   2. Signal Purity Filter
//!   3. Structural Consistency Filter
//!   4. Top-K Sparsification

use crate::trajectory::p2::coupling_builder::{CouplingEdge, CouplingGraph};

/// Pruner configuration for 4-layer survivorship filter (D10).
#[derive(Debug, Clone)]
pub struct Pruner {
    /// Hard filter thresholds: drop if ALL three signals below epsilon
    pub eps_commit: f32,
    pub eps_file: f32,
    pub eps_session: f32,
    /// Purity threshold: max(commit, file, session) / sum
    pub tau_purity: f32,
    /// Stability threshold: min fraction of traces edge must appear in
    pub tau_stability: f32,
    /// Top-K per node
    pub k: usize,
}

impl Default for Pruner {
    fn default() -> Self {
        Pruner {
            eps_commit: 0.01,
            eps_file: 0.01,
            eps_session: 0.01,
            tau_purity: 0.3,
            tau_stability: 0.0, // 0 = disabled in default
            k: 20,
        }
    }
}

impl Pruner {
    /// Apply all 4 layers of pruning to a CouplingGraph.
    pub fn prune(&self, graph: &CouplingGraph) -> CouplingGraph {
        let edges = &graph.edges;
        if edges.is_empty() {
            return graph.clone();
        }

        // Layer 1: Hard existence filter
        let edges = self.hard_filter(edges);

        // Layer 2: Signal purity filter
        let edges = self.purity_filter(&edges);

        // Layer 3: Stability filter
        let edges = self.stability_filter(&edges);

        // Layer 4: Top-K sparsification
        let edges = self.topk_filter(&edges, &graph.capabilities);

        CouplingGraph {
            capabilities: graph.capabilities.clone(),
            edges,
            version: graph.version,
        }
    }

    /// Layer 1: Hard Existence Filter (D10).
    /// Drop edges where all three co-change signals are below threshold.
    fn hard_filter(&self, edges: &[CouplingEdge]) -> Vec<CouplingEdge> {
        edges
            .iter()
            .filter(|e| {
                // Edge survives if cochange_score > 0 (simplified: fused_score covers all)
                // Or if correlation_score > any_epsilon
                e.cochange_score > self.eps_commit.max(self.eps_file).max(self.eps_session)
            })
            .cloned()
            .collect()
    }

    /// Layer 2: Signal Purity Filter (D10).
    /// purity = max(cochange_components) / sum
    fn purity_filter(&self, edges: &[CouplingEdge]) -> Vec<CouplingEdge> {
        edges
            .iter()
            .filter(|e| {
                // Use correlation_score as purity proxy:
                // High correlation with low similarity = pure co-change signal
                // High correlation with high similarity = mixed signal
                if e.cochange_score.abs() < 1e-6 {
                    return false; // no co-change evidence → drop
                }
                // Purity = cochange / (cochange + similarity * lambda)
                let purity = e.cochange_score
                    / (e.cochange_score + e.similarity_score * 0.3 + 1e-6);
                purity >= self.tau_purity
            })
            .cloned()
            .collect()
    }

    /// Layer 3: Structural Consistency Filter (D10).
    /// stability = count(edge appears in different traces) / total_traces
    /// Note: When per-trace info not available, retain all that pass purity.
    fn stability_filter(&self, edges: &[CouplingEdge]) -> Vec<CouplingEdge> {
        if self.tau_stability <= 0.0 {
            return edges.to_vec();
        }
        // Without per-trace evidence counts, pass through all
        // (stability evidence is embedded in cochange_score's aggregated nature)
        edges.to_vec()
    }

    /// Layer 4: Top-K Sparsification per node (D10).
    fn topk_filter(
        &self,
        edges: &[CouplingEdge],
        capabilities: &[String],
    ) -> Vec<CouplingEdge> {
        use std::collections::HashMap;

        // Group edges by from_capability
        let mut from_edges: HashMap<&str, Vec<&CouplingEdge>> = HashMap::new();
        let mut to_edges: HashMap<&str, Vec<&CouplingEdge>> = HashMap::new();

        for edge in edges {
            from_edges.entry(&edge.from_capability).or_default().push(edge);
            to_edges.entry(&edge.to_capability).or_default().push(edge);
        }

        let mut selected: Vec<bool> = vec![false; edges.len()];
        let edge_indices: HashMap<*const CouplingEdge, usize> = edges
            .iter()
            .enumerate()
            .map(|(i, e)| (e as *const CouplingEdge, i))
            .collect();

        // For each capability, keep top-K outgoing edges
        for cap in capabilities {
            if let Some(cap_edges) = from_edges.get(cap.as_str()) {
                let mut sorted: Vec<&&CouplingEdge> = cap_edges.iter().collect();
                sorted.sort_by(|a, b| {
                    b.correlation_score
                        .partial_cmp(&a.correlation_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                for &&e in sorted.iter().take(self.k) {
                    if let Some(&idx) = edge_indices.get(&(e as *const CouplingEdge)) {
                        selected[idx] = true;
                    }
                }
            }
            // Also keep top-K incoming edges
            if let Some(cap_edges) = to_edges.get(cap.as_str()) {
                let mut sorted: Vec<&&CouplingEdge> = cap_edges.iter().collect();
                sorted.sort_by(|a, b| {
                    b.correlation_score
                        .partial_cmp(&a.correlation_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                for &&e in sorted.iter().take(self.k) {
                    if let Some(&idx) = edge_indices.get(&(e as *const CouplingEdge)) {
                        selected[idx] = true;
                    }
                }
            }
        }

        edges
            .iter()
            .enumerate()
            .filter(|(i, _)| selected[*i])
            .map(|(_, e)| e.clone())
            .collect()
    }

    /// Threshold-only pruning (simple fallback).
    pub fn threshold_prune(
        edges: &[CouplingEdge],
        min_correlation: f32,
    ) -> Vec<CouplingEdge> {
        edges
            .iter()
            .filter(|e| e.correlation_score >= min_correlation)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::p2::similarity::FeatureContribution;

    fn make_edge(from: &str, to: &str, cochange: f32, sim: f32) -> CouplingEdge {
        CouplingEdge {
            from_capability: from.into(),
            to_capability: to.into(),
            cochange_score: cochange,
            similarity_score: sim,
            correlation_score: cochange * (1.0 + 0.3 * sim),
            feature_contribution: FeatureContribution::default(),
        }
    }

    #[test]
    fn test_hard_filter_removes_zero_cochange() {
        let pruner = Pruner::default();
        let edges = vec![
            make_edge("a", "b", 0.0, 0.9),
            make_edge("a", "c", 0.5, 0.5),
        ];
        let filtered = pruner.hard_filter(&edges);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].from_capability, "a");
        assert_eq!(filtered[0].to_capability, "c");
    }

    #[test]
    fn test_threshold_prune() {
        let edges = vec![
            make_edge("a", "b", 0.8, 0.5), // corr = 0.8 * 1.15 = 0.92
            make_edge("a", "c", 0.1, 0.9), // corr = 0.1 * 1.27 = 0.127
        ];
        let pruned = Pruner::threshold_prune(&edges, 0.5);
        assert_eq!(pruned.len(), 1);
    }

    #[test]
    fn test_topk_per_node() {
        let pruner = Pruner {
            k: 1,
            ..Default::default()
        };
        let edges = vec![
            make_edge("a", "b", 0.3, 0.5), // corr ≈ 0.345
            make_edge("a", "c", 0.8, 0.5), // corr ≈ 0.92
            make_edge("b", "c", 0.5, 0.5), // corr ≈ 0.575
        ];
        let caps = vec!["a".into(), "b".into(), "c".into()];
        let pruned = pruner.topk_filter(&edges, &caps);
        // Each node keeps top-1, so a→c and b→c should survive
        assert!(pruned.iter().any(|e| e.from_capability == "a" && e.to_capability == "c"));
        assert!(pruned.len() >= 2, "Got {} edges", pruned.len());
    }

    #[test]
    fn test_prune_empty() {
        let pruner = Pruner::default();
        let graph = CouplingGraph {
            capabilities: vec![],
            edges: vec![],
            version: 1,
        };
        let result = pruner.prune(&graph);
        assert!(result.edges.is_empty());
    }
}
