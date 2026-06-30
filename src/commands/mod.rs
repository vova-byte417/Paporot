//! Paporot 命令模块
//!
//! v0.4.0 重构：
//! - native 命令 (eval/task/status/dashboard) 在 src/main.rs 直接 dispatch
//! - WASM 命令 (trace/trajectory/state/trajectory_vector/coupling/skill) 保留在 commands/
//! - 旧命令 (snapshot/diff/coverage 等) 已删除

pub mod trace;
pub mod trajectory;
pub mod state;
pub mod trajectory_vector;
pub mod coupling;
