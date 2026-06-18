//! Paporot Skill System
//!
//! 三层架构的最上层：Skill Runtime + WASM Host + Registry + Compat
//!
//! ```text
//! Skill Runtime
//!   ├── Registry      — 扫描 .paporot/skills/*/skill.toml
//!   ├── DAG Engine    — 基于依赖声明自动编排执行
//!   ├── WASM Host     — wasmtime 加载执行 skill.wasm
//!   ├── Schema Compat — Core 升级后保持旧 Skill 兼容
//!   └── LLM Bridge    — 统一 LLM 调用管理
//! ```

pub mod error_log;
pub mod registry;
pub mod runtime;
pub mod schema_compat;
pub mod types;

pub use error_log::ErrorLogger;
pub use registry::SkillRegistry;
pub use runtime::SkillRuntime;
pub use types::*;
