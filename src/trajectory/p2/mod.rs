//! P2: Behavior Coupling Graph (BCG)
//!
//! 构建多个 capability / trajectory 之间的行为耦合关系图（correlational，非 causal）。
//!
//! D9: correlation = cochange × (1 + λ × similarity), λ ∈ [0.2, 0.4]
//! D10: 4-layer survivorship filter: hard → purity → stability → top-K
//! D11: cochange = log(1 + w1×commit + w2×file + w3×session)
//! D12: feature_contribution = 4 semantic groups, per-field linear projection
//! D13: main attribution = weighted linear projection

pub mod similarity;
pub mod cochange;
pub mod coupling_builder;
pub mod graph;
pub mod correlation;

pub use similarity::compute_grouped_contributions;
pub use similarity::FeatureContribution;
pub use cochange::CochangeEvidence;
pub use coupling_builder::CouplingBuilder;
pub use coupling_builder::{CouplingEdge, CouplingGraph};
pub use graph::Pruner;
pub use correlation::CorrelationEngine;
