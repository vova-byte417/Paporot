//! Trajectory Diff 模块。
//!
//! 消费 trace::BehaviorTrace，产出 BehaviorStateGraph → TrajectoryDiff → TrajectoryAnalysis → Eval。

pub mod align;
pub mod analysis;
pub mod cache;
pub mod classifier;
pub mod error;
pub mod evaler;
pub mod hash;
pub mod p1;
pub mod p2;
pub mod projection;
pub mod report;
pub mod similarity;
pub mod state;
pub mod types;

pub use classifier::PhaseClassifier;
pub use classifier::RuleBasedClassifier;
pub use align::engine::AlignmentEngine;
pub use analysis::TrajectoryAnalysis;
pub use state::builder::build_state_graph;
pub use state::segmentation::RuleSegmenter;
pub use evaler::evaluate as evaluate_state;
