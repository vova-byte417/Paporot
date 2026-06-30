//! Paporot v0.4.0 核心数据类型
//!
//! 对齐 Anthropic Agent 评估框架定义：
//! EvalResult = Task + Transcript + Outcome + CodeChange
//!
//! 与 v0.1.0 BehaviorSnapshot 的区别：
//! - "行为" = Agent 完成任务的表现，不是代码契约变更
//! - 新增 TaskSpec / GraderResult / OutcomeVerdict / ToolPattern
//! - CodeChange 是辅助说明，不等同于"行为"

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── EvalResult（顶层评估对象） ────────────────────────────────────

/// 一次 Agent Task 执行的完整评估记录
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvalResult {
    /// 唯一标识符: eval_{timestamp}_{uuid}
    pub eval_id: String,

    /// 关联的 Task
    pub task: TaskSpec,

    /// 第几次 Trial（同一 Task 多次执行）
    pub trial_index: u32,

    // ── Transcript：Agent 执行面 ──
    /// Agent 执行轨迹（如果捕获了）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<TranscriptSummary>,

    /// Tool 调用模式摘要
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_pattern: Option<ToolPattern>,

    // ── Outcome：结果面 ──
    /// 最终裁定
    pub outcome: OutcomeVerdict,

    /// 各 Grader 结果
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grader_results: Vec<GraderResult>,

    // ── CodeChange：产出面 ──
    /// 代码变更摘要
    pub code_change: CodeChangeSummary,

    /// 时间戳 ISO-8601
    pub created_at: String,

    /// 关联的 GitEvent ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_event_id: Option<String>,

    /// 关联的 Session ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// 用户标签
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// Transcript 摘要（避免在 EvalResult 中嵌套完整 BehaviorTrace）
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TranscriptSummary {
    pub trace_id: String,
    pub session_id: String,
    pub total_tool_calls: usize,
    pub total_tokens: u64,
    pub duration_ms: u64,
}

// ─── TaskSpec ──────────────────────────────────────────────────────

/// 任务规格
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TaskSpec {
    /// 唯一标识符
    pub id: String,

    /// 任务描述
    pub description: String,

    /// 成功标准
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub success_criteria: Vec<String>,

    /// 任务类别
    pub category: TaskCategory,

    /// 相关模块（文件路径前缀）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<String>,

    /// 创建方式
    pub source: TaskSource,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TaskCategory {
    BugFix,
    Feature,
    Refactor,
    Test,
    Doc,
    Other(String),
}

impl std::fmt::Display for TaskCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BugFix => write!(f, "BugFix"),
            Self::Feature => write!(f, "Feature"),
            Self::Refactor => write!(f, "Refactor"),
            Self::Test => write!(f, "Test"),
            Self::Doc => write!(f, "Doc"),
            Self::Other(s) => write!(f, "Other({})", s),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TaskSource {
    /// 自动从 git commit 创建
    Auto { commit_sha: String },
    /// 用户手动创建
    Manual { created_by: String },
    /// 从已有 Task 拆分/合并
    Derived { parent_ids: Vec<String> },
}

// ─── OutcomeVerdict ────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum OutcomeVerdict {
    Pass,
    Fail {
        failing_graders: Vec<String>,
        summary: String,
    },
    Partial {
        passed: u32,
        total: u32,
        failures: Vec<String>,
    },
    NotEvaluated {
        reason: String,
    },
}

impl OutcomeVerdict {
    /// 是否通过
    pub fn is_pass(&self) -> bool {
        matches!(self, Self::Pass)
    }

    /// 是否失败
    pub fn is_fail(&self) -> bool {
        matches!(self, Self::Fail { .. })
    }

    /// 人类可读标签
    pub fn label(&self) -> &str {
        match self {
            Self::Pass => "PASS",
            Self::Fail { .. } => "FAIL",
            Self::Partial { .. } => "PARTIAL",
            Self::NotEvaluated { .. } => "N/A",
        }
    }
}

// ─── GraderResult ──────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GraderResult {
    /// Grader 类型
    pub grader_type: GraderType,

    /// Grader 名称
    pub name: String,

    /// 是否通过
    pub passed: bool,

    /// 详细结果（类型取决于 Grader）
    pub details: serde_json::Value,

    /// 执行耗时 ms
    pub duration_ms: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GraderType {
    /// 确定性测试（运行 test suite）
    DeterministicTest,
    /// 静态分析（lint/type check/format）
    StaticAnalysis,
    /// 构建检查
    BuildCheck,
    /// LLM Rubric 评分
    LlmRubric,
    /// 安全扫描
    SecurityScan,
}

impl std::fmt::Display for GraderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeterministicTest => write!(f, "test"),
            Self::StaticAnalysis => write!(f, "lint"),
            Self::BuildCheck => write!(f, "build"),
            Self::LlmRubric => write!(f, "llm_rubric"),
            Self::SecurityScan => write!(f, "security"),
        }
    }
}

// ─── ToolPattern（Agent 行为摘要） ─────────────────────────────────

/// Tool 调用模式摘要（从 BehaviorTrace 提取）
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ToolPattern {
    /// Tool 调用总数
    pub total_tool_calls: usize,

    /// Tool 类型分布: tool_name → count
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tool_distribution: HashMap<String, usize>,

    /// 编辑类 tool 占比
    pub edit_ratio: f32,

    /// 读取类 tool 占比
    pub read_ratio: f32,

    /// 执行类 tool 占比（run/test）
    pub exec_ratio: f32,

    /// 总 token 消耗
    pub total_tokens: u64,

    /// 总耗时 ms
    pub duration_ms: u64,

    /// 失败/重试次数
    pub error_retry_count: u32,

    /// 循环检测：重复 tool 调用的轮数
    pub loop_rounds: u32,

    /// P0 状态数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_count: Option<usize>,

    /// P1 向量（如果有）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory_vector: Option<serde_json::Value>,
}

// ─── CodeChangeSummary ─────────────────────────────────────────────

/// 代码变更摘要（从 diff + AST 提取，纯机械）
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CodeChangeSummary {
    /// 变更文件列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_changed: Vec<String>,

    /// 新增行数
    pub additions: u32,

    /// 删除行数
    pub deletions: u32,

    /// 新增符号
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols_added: Vec<SymbolChange>,

    /// 删除符号
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols_removed: Vec<SymbolChange>,

    /// 修改符号
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols_modified: Vec<SymbolChange>,

    /// 涉及的模块（文件路径前缀去重）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<String>,

    /// L1 置信度
    pub confidence: f32,

    /// diff 长度（字节）
    pub diff_length: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SymbolChange {
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub line_start: u32,
    pub line_end: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Const,
    Type,
    Module,
    Class,
    Method,
    Interface,
    ArrowFunc,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Function => write!(f, "fn"),
            Self::Struct => write!(f, "struct"),
            Self::Enum => write!(f, "enum"),
            Self::Trait => write!(f, "trait"),
            Self::Impl => write!(f, "impl"),
            Self::Const => write!(f, "const"),
            Self::Type => write!(f, "type"),
            Self::Module => write!(f, "module"),
            Self::Class => write!(f, "class"),
            Self::Method => write!(f, "method"),
            Self::Interface => write!(f, "interface"),
            Self::ArrowFunc => write!(f, "=>"),
        }
    }
}

// ─── EvalCompare（对比输出） ───────────────────────────────────────

/// 两个 EvalResult 的对比报告
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvalCompare {
    pub task_id: String,
    pub from: EvalResult,
    pub to: EvalResult,
    pub trend: EvalTrend,
    pub metrics: Vec<MetricChange>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum EvalTrend {
    Improved,
    Degraded,
    Stable,
}

impl std::fmt::Display for EvalTrend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Improved => write!(f, "改进"),
            Self::Degraded => write!(f, "退化"),
            Self::Stable => write!(f, "持平"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MetricChange {
    pub name: String,
    pub label: String,
    pub from_value: f64,
    pub to_value: f64,
    pub change_pct: f64,
    pub direction: MetricDirection,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MetricDirection {
    Up,
    Down,
    Flat,
}

// ─── EvalTrendHistory ─────────────────────────────────────────────

/// 某个 Task 的多 Trial 趋势
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvalTrendHistory {
    pub task_id: String,
    pub task_description: String,
    pub trials: Vec<EvalTrendPoint>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvalTrendPoint {
    pub eval_id: String,
    pub trial_index: u32,
    pub outcome: OutcomeVerdict,
    pub total_tool_calls: Option<usize>,
    pub total_tokens: Option<u64>,
    pub duration_ms: Option<u64>,
    pub created_at: String,
}

// ─── EvalRegression ───────────────────────────────────────────────

/// 批量回归检测结果
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvalRegression {
    pub checked_tasks: u32,
    pub regressions: Vec<EvalRegressionItem>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvalRegressionItem {
    pub task_id: String,
    pub from_eval: String,
    pub to_eval: String,
    pub from_outcome: String,
    pub to_outcome: String,
    pub severity: RegressionSeverity,
    pub description: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum RegressionSeverity {
    Critical,
    High,
    Medium,
    Low,
}

impl std::fmt::Display for RegressionSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Critical => write!(f, "CRITICAL"),
            Self::High => write!(f, "HIGH"),
            Self::Medium => write!(f, "MEDIUM"),
            Self::Low => write!(f, "LOW"),
        }
    }
}

// ─── EvalContext（Grader 执行上下文） ──────────────────────────────

/// Grader 执行所需的上下文
#[derive(Debug, Clone)]
pub struct EvalContext {
    pub project_root: std::path::PathBuf,
    pub paporot_dir: std::path::PathBuf,
    pub cache_dir: std::path::PathBuf,
    pub commit_sha: Option<String>,
    pub diff_content: String,
}
