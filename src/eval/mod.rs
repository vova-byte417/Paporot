//! Paporot v0.4.0 评估引擎
//!
//! 核心职责：
//! - 定义 EvalResult / TaskSpec / GraderResult 等核心数据类型
//! - Task 自动创建与管理
//! - Grader 评分框架
//! - 评估编排与对比
//!
//! 设计原则：宿主做机械，Skill 做判断。

pub mod types;
pub mod task;
pub mod exporter;
pub mod grader;
pub mod runner;
pub mod compare;
pub mod trend;
pub mod regression;

pub use types::*;
pub use task::TaskManager;
pub use exporter::CodeExporter;
pub use grader::{Grader, DeterministicTestGrader, StaticAnalysisGrader, BuildCheckGrader};
pub use runner::EvalRunner;
pub use compare::compare as compare_evals;
pub use trend::trend_history;
pub use regression::regression as check_regression;
