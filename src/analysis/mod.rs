//! 混合分析架构模块
//!
//! L1: AST/Pattern 确定性解析 → L2: 规则引擎 → L3: LLM 增强
//!
//! 对应 PRD P0 的三层漏斗架构。

pub mod types;
pub mod preprocessor;
pub mod l1_ast;
pub mod l2_rules;
pub mod l3_llm_bridge;
pub mod prompts;
