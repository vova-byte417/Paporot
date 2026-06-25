//! Execution Trace 模块
//!
//! 负责记录、存储和查询 Agent 执行轨迹。
//! 这是 Paporot 从 Capability Version Control 迈向 Behavior Version Control 的第一步。

pub mod adapter;
pub mod adapter_registry;
pub mod adapters;
pub mod error;
pub mod storage;
pub mod trace_snapshot_map;
pub mod types;
pub mod wrapper;
