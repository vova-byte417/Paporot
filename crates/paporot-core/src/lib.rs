//! Paporot Core — WASM 沙盒内执行
//!
//! 所有分析逻辑在此模块内完成。
//! 通过 3 个 Host Function 与外部交互：
//! - host_read_file  — 读项目文件
//! - host_write_file — 写报告/日志
//! - host_llm_call   — LLM 推理

pub mod host;
pub mod pipeline;
pub mod suppressor;

// Phase 0: 共享类型
pub mod types;

// Phase 1: 预处理器（L1/L2）
pub mod analysis;
pub mod evidence;
// pub mod report; // 待 Skill 输出格式确定后启用

// Phase 2: Snapshot 引擎
pub mod snapshot_store;
pub mod snapshot_analyzer;
