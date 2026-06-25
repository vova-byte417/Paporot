//! 分析层类型定义 — 重新导出自 paporot-analysis-types
//!
//! 所有 L1 AST / L2 Rules / L3 LLM / Evidence 相关类型
//! 统一在 paporot-analysis-types crate 中定义，
//! 并被 paporot-core (wasm32-wasip1) 和 native binary (x86_64) 共享。

pub use paporot_analysis_types::*;
