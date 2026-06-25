//! 混合分析架构（L1 → L2 → L3 三层漏斗）
//!
//! L1: AST/Pattern 确定性解析
//! L2: 规则引擎
//! L3: LLM 增强（通过 host_llm_call）
//!
//! 所有模块为确定性算法，仅在 L3 调用 LLM。

pub mod preprocessor;
pub mod l1_ast;
pub mod l2_rules;
pub mod l3_llm_bridge;
pub mod preprocessor_bridge;
