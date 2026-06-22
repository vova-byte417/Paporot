//! paporot-skill-sdk — Paporot Skill 开发 SDK

pub mod host;

/// Prelude: re-exports all host functions and serde_json helpers
pub mod prelude {
    pub use crate::host::{
        read_input, llm_complete, write_output, write_error,
        cache_put, cache_get, skill_log,
        verify_contract, capture_evidence, save_replay_case, load_replay_cases,
    };
    pub use serde_json::{json, Value};
}
