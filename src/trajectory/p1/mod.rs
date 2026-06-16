//! P1: Statistical Trajectory Vector (STV)
//!
//! 把行为轨迹压缩为可计算的数值向量，用于聚类、异常检测、趋势分析。
//!
//! Architecture principle:
//!   P0 uses features for DECISIONS (merge/split, threshold-based)
//!   P1 uses SAME features for MEASUREMENT (projection, no threshold, no decision)
//!
//! Shared feature space (StateFeatures) → P1 projection → TrajectoryVector。

pub mod feature_extractor;
pub mod sequence_metrics;
pub mod timeseries;
pub mod vector;
pub mod cluster;
pub mod registry;

pub use vector::TrajectoryVector;
pub use vector::build_vector;
pub use vector::compute_state_stability;
pub use cluster::{ClusterResult, Clusterer};
pub use feature_extractor::FeatureSnapshot;
pub use sequence_metrics::SequenceMetrics;
pub use timeseries::TimeSeries;
pub use registry::{FeatureRegistry, SparseVector};
