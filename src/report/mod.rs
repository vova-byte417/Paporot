//! Report Generator —— 将 Skill 执行结果转换为多种报告格式
//!
//! 提供：
//! - JSON 报告（机器可消费）
//! - Markdown 报告（人类可读）
//! - Dashboard HTML 模板数据（供前端渲染）

pub mod dashboard;
pub mod generator;

pub use generator::ReportGenerator;
pub use dashboard::DashboardData;
