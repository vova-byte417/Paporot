//! Paporot CLI 接口定义

use clap::{Parser, Subcommand};

/// Paporot —— AI 生成软件的行为版本控制与审计系统
#[derive(Parser, Debug)]
#[command(name = "Paporot", version, about, long_about = None)]
pub struct Cli {
    /// 配置文件路径（可选）
    #[arg(short, long, global = true, default_value = ".Paporot/config.toml")]
    pub config: String,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 创建行为快照
    Snapshot {
        #[command(subcommand)]
        action: SnapshotAction,
    },

    /// 行为差异对比
    Diff {
        /// 基准版本 ID（默认上一版本）
        #[arg(short, long)]
        from: Option<String>,

        /// 目标版本 ID（默认当前）
        #[arg(short, long)]
        to: Option<String>,

        /// 输出格式
        #[arg(short, long, default_value = "markdown")]
        format: String,
    },

    /// PRD 覆盖率分析
    Coverage {
        /// PRD 文件路径
        #[arg(short, long)]
        prd: Option<String>,

        /// 目标 snapshot 版本 ID
        #[arg(short, long)]
        version: Option<String>,
    },

    /// 回归检测
    Regression {
        /// 基准版本 ID
        #[arg(short, long)]
        from: Option<String>,

        /// 目标版本 ID
        #[arg(short, long)]
        to: Option<String>,
    },

    /// 风险评估
    Risk {
        /// 目标版本 ID
        #[arg(short, long)]
        version: Option<String>,
    },

    /// 整合审查入口（snapshot + diff + coverage + regression + risk）
    Review {
        /// Git diff 来源（默认从 HEAD~1..HEAD 获取）
        #[arg(short, long)]
        diff_source: Option<String>,

        /// PRD 文件路径
        #[arg(short, long)]
        prd: Option<String>,
    },

    /// 查看版本信息
    Version,

    /// 查看当前状态
    Status,

    /// 依赖图查询与分析
    Graph {
        #[command(subcommand)]
        action: GraphAction,
    },

    /// 人机验证 feedback 回路
    Feedback {
        #[command(subcommand)]
        action: FeedbackAction,
    },

    /// 行为-测试闭环映射
    Testmap {
        #[command(subcommand)]
        action: TestmapAction,
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
}

#[derive(Subcommand, Debug)]
pub enum TrajectoryVectorAction {
    /// 从 trace 构建 TrajectoryVector
    Build {
        /// Trace ID
        #[arg(long)]
        trace: String,
        /// 输出文件（JSON）
        #[arg(short, long)]
        output: Option<String>,
    },
    /// 对比两个 TrajectoryVector
    Diff {
        /// Vector A 的文件路径
        #[arg(long)]
        v1: String,
        /// Vector B 的文件路径
        #[arg(long)]
        v2: String,
    },
    /// 聚类分析
    Cluster {
        /// Trace ID 列表
        #[arg(long, num_args = 1..)]
        traces: Vec<String>,
    },
    /// 异常检测
    Anomaly {
        /// Trace ID 列表
        #[arg(long, num_args = 1..)]
        traces: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum CouplingAction {
    /// 构建耦合图
    Build {
        /// trace_id=capability_id 对，格式: trace1:cap1 trace2:cap2 ...
        #[arg(long, num_args = 1.., value_delimiter = ' ')]
        pairs: Vec<String>,
        /// 输出文件（JSON）
        #[arg(short, long)]
        output: Option<String>,
    },
    /// 分析特定 capability 的耦合
    Analyze {
        /// 目标 capability
        #[arg(long)]
        cap: String,
        /// trace_id=capability_id 对
        #[arg(long, num_args = 1.., value_delimiter = ' ')]
        pairs: Vec<String>,
    },
    /// 导出耦合图
    Export {
        /// trace_id=capability_id 对
        #[arg(long, num_args = 1.., value_delimiter = ' ')]
        pairs: Vec<String>,
        /// 输出格式: json | mermaid
        #[arg(long, default_value = "mermaid")]
        format: String,
    },
    /// 影响分析
    Impact {
        /// 目标 capability
        #[arg(long)]
        cap: String,
        /// trace_id=capability_id 对
        #[arg(long, num_args = 1.., value_delimiter = ' ')]
        pairs: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum GraphAction {
    /// 展示依赖图
    Show {
        /// 目标快照版本 ID（可选）
        #[arg(short, long)]
        version: Option<String>,

        /// 指定能力 ID 查看其依赖
        #[arg(short, long)]
        capability: Option<String>,

        /// 查询深度（0=仅直接依赖，默认 2）
        #[arg(short, long, default_value = "2")]
        depth: usize,
    },

    /// 影响分析：修改某能力会影响哪些下游
    Impact {
        /// 目标能力 ID
        #[arg(short, long)]
        capability: String,
    },

    /// 演化追溯：查看能力跨版本历史
    Evolution {
        /// 能力 ID
        #[arg(short, long)]
        capability: String,
    },

    /// 循环依赖检测
    Cycles,

    /// 按模块查询能力
    Module {
        /// 模块名
        #[arg(short, long)]
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum FeedbackAction {
    /// 确认某个能力正确
    Approve {
        /// 能力 ID
        #[arg(short, long)]
        capability: String,
        /// 快照版本
        #[arg(short, long)]
        version: String,
        /// 审查者标识
        #[arg(short, long, default_value = "human")]
        reviewer: String,
        /// 审查备注
        #[arg(short, long)]
        comment: Option<String>,
    },

    /// 标记能力为误报
    Reject {
        #[arg(short, long)]
        capability: String,
        #[arg(short, long)]
        version: String,
        #[arg(short, long, default_value = "human")]
        reviewer: String,
        /// 拒绝原因
        #[arg(short, long)]
        reason: Option<String>,
    },

    /// 修正能力描述
    Correct {
        #[arg(short, long)]
        capability: String,
        #[arg(short, long)]
        version: String,
        #[arg(short, long, default_value = "human")]
        reviewer: String,
        /// 修正后的名称
        #[arg(long)]
        name: String,
        /// 修正后的描述
        #[arg(long)]
        desc: String,
        /// 修正备注
        #[arg(short, long)]
        comment: Option<String>,
    },

    /// 标记为待定
    Flag {
        #[arg(short, long)]
        capability: String,
        #[arg(short, long)]
        version: String,
        #[arg(short, long, default_value = "human")]
        reviewer: String,
        /// 标记原因
        #[arg(short, long)]
        note: Option<String>,
    },

    /// 查看审查记录
    Show {
        /// 查看特定能力的审查历史
        #[arg(short, long)]
        capability: Option<String>,
    },

    /// 查看审查统计
    Stats,
}

#[derive(Subcommand, Debug)]
pub enum TestmapAction {
    /// 从 diff 自动扫描测试映射
    Scan {
        /// 快照版本
        #[arg(short, long)]
        version: String,
        /// diff 来源（默认 HEAD~1..HEAD）
        #[arg(short, long)]
        diff: Option<String>,
    },

    /// 手动添加测试映射
    Add {
        /// 能力 ID
        #[arg(short, long)]
        capability: String,
        /// 测试文件路径
        #[arg(long)]
        test_file: String,
        /// 测试函数名
        #[arg(long)]
        test_name: String,
        /// 测试状态 (pass/fail/missing/unknown)
        #[arg(short, long, default_value = "unknown")]
        status: String,
        /// 测试框架
        #[arg(short, long)]
        framework: Option<String>,
        /// 映射来源 (manual/name/file)
        #[arg(long, default_value = "manual")]
        source: String,
    },

    /// 查看测试映射
    Show {
        /// 查看特定能力的映射
        #[arg(short, long)]
        capability: Option<String>,
    },

    /// 查看测试覆盖统计
    Stats,

    /// 验证某个能力的测试文件
    Verify {
        #[arg(short, long)]
        capability: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum SnapshotAction {
    /// 从 Git diff 创建新的 behavior snapshot
    Create {
        /// Git ref 或 commit 范围（默认 HEAD~1..HEAD）
        #[arg(short, long, default_value = "HEAD~1..HEAD")]
        diff_range: String,

        /// 版本消息
        #[arg(short, long, default_value = "Snapshot")]
        message: String,

        /// PRD 文件路径（用于覆盖率计算）
        #[arg(short, long)]
        prd: Option<String>,

        /// 直接提供 diff 文件路径（而非从 git 获取）
        #[arg(short, long)]
        diff_file: Option<String>,

        /// 输出目录
        #[arg(short, long, default_value = ".Paporot/snapshots")]
        output_dir: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum TraceAction {
    /// 从外部文件导入 trace
    Import {
        file: String,
        #[arg(short, long)]
        adapter: Option<String>,
    },
    /// 列出 trace 摘要
    List {
        #[arg(short, long)]
        session: Option<String>,
        #[arg(short = 'T', long)]
        tool: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        capability: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long, default_value = "100")]
        limit: usize,
        #[arg(long, default_value = "0")]
        offset: usize,
    },
    /// 查看单条 trace 详情
    Show {
        trace_id: String,
        #[arg(short, long, default_value = "full")]
        format: String,
    },
    /// 删除 trace（soft delete）
    Delete {
        trace_id: String,
    },
    /// 关联 trace 与 capability
    Link {
        trace_id: String,
        #[arg(long)]
        cap: String,
    },
    /// 取消关联
    Unlink {
        trace_id: String,
        #[arg(long)]
        cap: String,
    },
    /// 对 trace 内容脱敏
    Redact {
        trace_id: String,
    },
    /// 适配器管理
    Adapter {
        #[command(subcommand)]
        action: AdapterAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum AdapterAction {
    /// 列出所有可用适配器
    List,
}

#[derive(Subcommand, Debug)]
pub enum TrajectoryAction {
    /// 对比两条执行轨迹
    Diff {
        /// Capability ID（自动关联模式）
        #[arg(long)]
        capability: Option<String>,

        /// 版本 A 的 trace ID（手动模式）
        #[arg(long)]
        trace_a: Option<String>,

        /// 版本 B 的 trace ID（手动模式）
        #[arg(long)]
        trace_b: Option<String>,

        /// 输出格式: json | mermaid | html
        #[arg(long, default_value = "html")]
        format: String,

        /// 输出文件路径
        #[arg(long)]
        output: Option<String>,
    },

    /// 列出缓存的 trajectory diff
    List,

    /// 查看某个 trajectory diff 详情
    Show {
        diff_id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum StateAction {
    /// 从 trace 构建 BehaviorStateGraph
    Build {
        /// Trace ID
        #[arg(long)]
        trace: String,
    },

    /// 查看 BehaviorStateGraph
    Show {
        /// Trace ID
        trace_id: String,

        /// 输出格式: json | mermaid | terminal
        #[arg(long, default_value = "terminal")]
        format: String,
    },

    /// 对比两条 trace 的 BehaviorStateGraph
    Diff {
        /// Capability ID（自动关联）
        #[arg(long)]
        capability: Option<String>,

        /// Trace A ID
        #[arg(long)]
        trace_a: Option<String>,

        /// Trace B ID
        #[arg(long)]
        trace_b: Option<String>,

        /// 输出格式
        #[arg(long, default_value = "terminal")]
        format: String,
    },

    /// 评估 BehaviorStateGraph
    Eval {
        /// Trace ID (single trace eval)
        #[arg(long)]
        trace: String,
    },
}
