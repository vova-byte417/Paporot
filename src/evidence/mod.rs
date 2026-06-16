//! Capability Evidence 模块
//!
//! 为每个 Capability 提供推断决策的透明溯源证据。
//! 包含 L1 AST → L2 Rules → L3 LLM 的完整决策链路。

pub mod types;
pub mod confidence;
pub mod provider;
