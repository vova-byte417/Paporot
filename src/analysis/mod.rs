//! 混合分析架构模块
//!
//! L1: AST/Pattern 确定性解析 → L2: 规则引擎 → L3: LLM 增强
//!
//! 对应 PRD P0 的三层漏斗架构。
//!
//! NOTE: l3_llm_bridge 暂时禁用，待 Phase 1 迁移到 paporot-core 后
//! 通过 host_llm_call 重新启用。

pub mod types;
pub mod preprocessor;
pub mod l1_ast;
pub mod l2_rules;
// pub mod l3_llm_bridge; // 待 Phase 1 重新启用
