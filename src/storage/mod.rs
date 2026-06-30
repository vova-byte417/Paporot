//! Paporot 持久化存储层
//!
//! - timeline.rs: SQLite 事件存储（EvalResult / Task / GitEvent）
//! - cache.rs:    .Paporot/cache/ 结构化数据读写

pub mod cache;
pub mod timeline;
