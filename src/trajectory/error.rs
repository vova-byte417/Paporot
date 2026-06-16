//! Trajectory Diff 错误类型。

use std::io;

#[derive(Debug, thiserror::Error)]
pub enum TrajectoryError {
    #[error("Trace not found: {0}")]
    TraceNotFound(String),

    #[error("Capability not found: {0}")]
    CapabilityNotFound(String),

    #[error("No traces linked to capability: {0}")]
    NoTracesForCapability(String),

    #[error("Not enough traces for diff (need 2, got {0})")]
    InsufficientTraces(usize),

    #[error("Tool sequence too long for edit-distance alignment: {0} tools")]
    ToolSequenceTooLong(usize),

    #[error("Cache error: {0}")]
    CacheError(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}
