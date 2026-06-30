//! Paporot v0.4.0 CLI 接口定义
//!
//! v0.4.0 重构：
//! - 新增 eval / task 命令
//! - 删除 snapshot / diff / coverage / regression / risk / review / graph / feedback / testmap
//! - 保留 trace / trajectory / trajectory-vector / state / coupling / skill / analyze

use clap::{Parser, Subcommand};

/// Paporot —— AI Coding Agent Behavior Evaluation Platform
#[derive(Parser, Debug)]
#[command(name = "paporot", version, about, long_about = None)]
pub struct Cli {
    /// 配置文件路径（可选）
    #[arg(short, long, global = true, default_value = ".Paporot/config.toml")]
    pub config: String,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 评估：自动评分 + 对比 + 趋势分析
    Eval {
        #[command(subcommand)]
        action: EvalAction,
    },

    /// Task 管理：创建 / 查看 / 列出
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },

    /// 执行轨迹管理
    Trace {
        #[command(subcommand)]
        action: TraceAction,
    },

    /// 轨迹差异对比与管理
    Trajectory {
        #[command(subcommand)]
        action: TrajectoryAction,
    },

    /// 行为状态机构建与对比
    State {
        #[command(subcommand)]
        action: StateAction,
    },

    /// P1: 轨迹向量构建与分析
    TrajectoryVector {
        #[command(subcommand)]
        action: TrajectoryVectorAction,
    },

    /// P2: 行为耦合图构建与分析
    Coupling {
        #[command(subcommand)]
        action: CouplingAction,
    },

    /// Skill 管理
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },

    /// 运行完整 Skill 分析管线（DAG 编排）
    Analyze {
        /// 额外输入（JSON 格式的 key=value 对）
        #[arg(short, long)]
        input: Option<String>,

        /// PRD 文件路径（注入为覆盖率参考）
        #[arg(short, long)]
        prd: Option<String>,

        /// LLM API Key（可选，优先级高于配置文件）
        #[arg(long)]
        api_key: Option<String>,

        /// 跳过 LLM Rubric（纯确定性评分）
        #[arg(long)]
        no_llm: bool,

        /// 跳过 Graders（仅 Skill 分析）
        #[arg(long)]
        no_graders: bool,
    },

    /// 初始化 .Paporot/
    Init {
        /// LLM API Key（写入配置文件）
        #[arg(long)]
        api_key: Option<String>,
    },

    /// 显示版本信息
    Version,

    /// 显示当前项目状态摘要
    Status,
}

// ─── EvalAction ────────────────────────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum EvalAction {
    /// 自动评估最新 commit
    Auto {
        /// 指定 commit，默认 HEAD
        #[arg(long)]
        commit: Option<String>,

        /// 指定 Graders: test,lint,build,llm-rubric
        #[arg(long, value_delimiter = ',')]
        graders: Vec<String>,

        /// 跳过 LLM Rubric Grader
        #[arg(long)]
        no_llm: bool,
    },

    /// 运行指定 Task 的评估
    Run {
        /// Task ID
        #[arg(long)]
        task: String,
    },

    /// 对比同一 Task 的两个 Trial
    Compare {
        /// Task ID
        #[arg(long)]
        task: String,

        /// 基准 eval ID（默认倒数第二）
        #[arg(long)]
        from: Option<String>,

        /// 目标 eval ID（默认最新）
        #[arg(long)]
        to: Option<String>,
    },

    /// 查看某 Task 的趋势
    Trend {
        /// Task ID
        #[arg(long)]
        task: String,
    },

    /// 批量回归检测（所有 Task 的最新 Trial vs 基线）
    Regression,
}

// ─── TaskAction ────────────────────────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum TaskAction {
    /// 创建新 Task
    New {
        /// 任务描述
        description: String,

        /// 成功标准（可多次指定）
        #[arg(long = "criteria", value_delimiter = ',')]
        success_criteria: Vec<String>,

        /// 任务类别
        #[arg(long, default_value = "Other")]
        category: String,

        /// 相关模块（可多次指定）
        #[arg(long = "module", value_delimiter = ',')]
        modules: Vec<String>,
    },

    /// 列出所有 Task
    List,

    /// 查看 Task 详情
    Show {
        /// Task ID
        task_id: String,
    },
}

// ─── Retained subcommands (unchanged) ──────────────────────────────

#[derive(Subcommand, Debug)]
pub enum TrajectoryVectorAction {
    Build {
        #[arg(long)] trace: String,
        #[arg(short, long)] output: Option<String>,
    },
    Diff {
        #[arg(long)] v1: String,
        #[arg(long)] v2: String,
    },
    Cluster {
        #[arg(long, num_args = 1..)] traces: Vec<String>,
    },
    Anomaly {
        #[arg(long, num_args = 1..)] traces: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum CouplingAction {
    Build {
        #[arg(long, num_args = 1.., value_delimiter = ' ')] pairs: Vec<String>,
        #[arg(short, long)] output: Option<String>,
    },
    Analyze {
        #[arg(long)] cap: String,
        #[arg(long, num_args = 1.., value_delimiter = ' ')] pairs: Vec<String>,
    },
    Export {
        #[arg(long, num_args = 1.., value_delimiter = ' ')] pairs: Vec<String>,
        #[arg(long, default_value = "mermaid")] format: String,
    },
    Impact {
        #[arg(long)] cap: String,
        #[arg(long, num_args = 1.., value_delimiter = ' ')] pairs: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum TraceAction {
    Import { file: String, #[arg(short, long)] adapter: Option<String> },
    List {
        #[arg(short, long)] session: Option<String>,
        #[arg(short = 'T', long)] tool: Option<String>,
        #[arg(long)] tag: Option<String>,
        #[arg(long)] capability: Option<String>,
        #[arg(long)] from: Option<String>,
        #[arg(long)] to: Option<String>,
        #[arg(long, default_value = "100")] limit: usize,
        #[arg(long, default_value = "0")] offset: usize,
    },
    Show { trace_id: String, #[arg(short, long, default_value = "full")] format: String },
    Delete { trace_id: String },
    Link { trace_id: String, #[arg(long)] cap: String },
    Unlink { trace_id: String, #[arg(long)] cap: String },
    Redact { trace_id: String },
    Adapter { #[command(subcommand)] action: AdapterAction },
}

#[derive(Subcommand, Debug)]
pub enum AdapterAction {
    List,
}

#[derive(Subcommand, Debug)]
pub enum TrajectoryAction {
    Diff {
        #[arg(long)] capability: Option<String>,
        #[arg(long)] trace_a: Option<String>,
        #[arg(long)] trace_b: Option<String>,
        #[arg(long, default_value = "html")] format: String,
        #[arg(long)] output: Option<String>,
    },
    List,
    Show { diff_id: String },
}

#[derive(Subcommand, Debug)]
pub enum StateAction {
    Build { #[arg(long)] trace: String },
    Show { trace_id: String, #[arg(long, default_value = "terminal")] format: String },
    Diff {
        #[arg(long)] capability: Option<String>,
        #[arg(long)] trace_a: Option<String>,
        #[arg(long)] trace_b: Option<String>,
        #[arg(long, default_value = "terminal")] format: String,
    },
    Eval { #[arg(long)] trace: String },
}

#[derive(Subcommand, Debug)]
pub enum SkillAction {
    List,
}
