//! paporot-skill-sdk — Paporot Skill 开发 SDK

pub mod host;

/// Prelude: re-exports all host functions and serde_json helpers
pub mod prelude {
    pub use crate::host::*;
    pub use serde_json::{json, Value};
}
