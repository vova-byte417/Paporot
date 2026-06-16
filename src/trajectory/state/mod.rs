pub mod features;
pub mod segmentation;
pub mod window;
pub mod merge;
pub mod transition;
pub mod builder;

pub use builder::build_state_graph;
pub use segmentation::RuleSegmenter;
pub use merge::AdjacentMerger;
