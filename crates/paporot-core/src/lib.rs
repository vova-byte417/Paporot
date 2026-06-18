//! Paporot Core — WASM 沙盒内执行
//!
//! 所有分析逻辑在此模块内完成。
//! 通过 3 个 Host Function 与外部交互：
//! - host_read_file  — 读项目文件
//! - host_write_file — 写报告/日志
//! - host_llm_call   — LLM 推理

pub mod host;
pub mod pipeline;
