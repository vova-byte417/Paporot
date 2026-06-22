# PRD: Trajectory Diff 模块（接口级）

> Paporot 从 Capability Version Control 迈向 Behavior Version Control 的第二步

---

## 目录

1. [背景与动机](#1-背景与动机)
2. [核心概念](#2-核心概念)
3. [设计决策记录](#3-设计决策记录)
4. [模块架构](#4-模块架构)
5. [数据类型定义](#5-数据类型定义)
6. [双层对齐算法](#6-双层对齐算法)
7. [CLI 子命令](#7-cli-子命令)
8. [HTML 报告](#8-html-报告)
9. [Capability 关联模式](#9-capability-关联模式)
10. [错误类型](#10-错误类型)
11. [测试策略](#11-测试策略)

---

## 1. 背景与动机

### 1.1 问题

Execution Trace 记录了单次 Agent 执行的完整轨迹，但无法回答：

> 同一个任务，Agent 执行了两次，轨迹有什么不同？

举例：

```
Capability: "Bug Fix"
  Version A (trace_001):             Version B (trace_003):
    read_file()                        read_file()
    edit_file()                        grep("pattern: POST /users")
    commit()                           read_file()
                                       write("tests/test_api.rs")
                                       cargo test
                                       edit_file()
                                       cargo test
                                       cargo clippy
                                       commit()
```

两条轨迹都完成了"修 Bug"，但 v2 的行为多了搜索、测试、lint 步骤——是规范了，还是过度工程了？

### 1.2 与 Execution Trace 的关系

Execution Trace 是本模块的**唯一数据基础**。Trajectory Diff 消费 BehaviorTrace，不做任何外部 IO。

---

## 2. 核心概念

| 概念 | 定义 |
|------|------|
| **TrajectoryDiff** | 两条 BehaviorTrace 的差异对比结果 |
| **Segment** | tool 调用序列中按语义类型切分的阶段（定位/修改/验证） |
| **SegmentKind** | 段的变化类型：Added / Deleted / Modified / Unchanged |
| **ToolDiff** | 单个 tool 调用的变化：Unchanged / Added / Deleted / ArgsChanged |
| **SemanticHash** | tool 名称 + 全量 args 的序列化 hash，用于判定两次 tool 调用是否"相同" |
| **PhaseRule** | tool 名称 → 语义阶段类别的映射规则 |

---

## 3. 设计决策记录

| # | 决策 | 备选方案 | 选择理由 |
|---|------|---------|---------|
| D1 | 双层对齐：规则分段 + 段内编辑距离 | 纯编辑距离 / 纯语义分段 | 兼得高层语义和细粒度差异 |
| D2 | 输入来源：Capability 自动关联为主，手动 trace_id 为降级 | 仅手动 / prompt 相似度匹配 | 与 Paporot Capability 体系天然衔接；prompt 相似度匹配误匹配率高 |
| D3 | 输出格式：Mermaid 时序图 + JSON | 终端文本 diff / TUI | 时序图泳道对比最直观；JSON 给 Behavior Eval 消费 |
| D4 | 分段算法：基于规则的 tool 类别映射 | LLM 分段 / 时间间隔分段 | 确定性、零延迟、可配置；LLM 分段引入不可控的外部依赖 |
| D5 | Tool 级对比：全量 args semantic hash | 仅比名称 / 仅比关键 args | 最严格的对齐标准，hash 不同即算不同 |
| D6 | 报告：HTML 页面，纵向滚动布局 | 左右分栏 / Tab 切换 | 摘要→时序图→对比表 自然阅读流 |

---

## 4. 模块架构

### 4.1 目录结构

```
src/
  trajectory/                          ← 新增一级模块
    mod.rs
    types.rs                           ← TrajectoryDiff / Segment / ToolDiff / PhaseRule
    align.rs                           ← 双层对齐算法核心
    phases.rs                          ← rule-based 分段规则
    hash.rs                            ← semantic hash 计算
    report.rs                          ← Mermaid 生成 + JSON 序列化
  commands/
    trajectory.rs                      ← CLI 子命令 diff
  cli.rs                               ← + Commands::Trajectory
  lib.rs                               ← + pub mod trajectory;
```

### 4.2 依赖关系

```
commands/trajectory.rs  →  trajectory/align.rs
                                ├── trajectory/types.rs (零内部依赖)
                                ├── trajectory/phases.rs (零内部依赖)
                                ├── trajectory/hash.rs   (零内部依赖)
                                └── trajectory/report.rs
                                      └── trajectory/types.rs
                                      └── HTML 模板内嵌
```

- `trajectory/` 模块消费 `trace/types.rs` 中的 `BehaviorTrace`，不修改 trace 数据
- 不依赖 `analysis/`、`agent.rs`、`config.rs`

---

## 5. 数据类型定义

### 5.1 文件: `src/trajectory/types.rs`

```rust
use serde::{Deserialize, Serialize};
use crate::trace::types::BehaviorTrace;

// ─── TrajectoryDiff ───────────────────────────────────────────────

/// 两条 BehaviorTrace 的完整差异对比结果。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrajectoryDiff {
    /// 对比的 Capability ID（Capability 自动关联模式时有值）
    pub capability_id: Option<String>,

    /// 版本 A 的 trace 信息
    pub version_a: TrajectoryVersion,
    /// 版本 B 的 trace 信息
    pub version_b: TrajectoryVersion,

    /// 段级差异
    pub segments: Vec<SegmentDiff>,

    /// 整体摘要
    pub summary: DiffSummary,
}

/// 单条轨迹的版本信息（不含完整 tool_calls/observations）。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrajectoryVersion {
    pub trace_id: String,
    pub session_id: String,
    pub tool_count: usize,
    pub duration_ms: u64,
    pub total_tokens: u64,
    pub started_at: String,
}

// ─── SegmentDiff ──────────────────────────────────────────────────

/// 一个语义段的差异。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SegmentDiff {
    /// 段标签，如 "定位问题"、"实施修改"、"验证"
    pub label: String,
    /// 段的变化类型
    pub kind: SegmentKind,
    /// 段内 tool 级差异
    pub tool_diffs: Vec<ToolDiff>,
    /// 段在版本 A 中的起始 call 索引（为空表示不存在于 A）
    pub index_a: Option<usize>,
    /// 段在版本 B 中的起始 call 索引（为空表示不存在于 B）
    pub index_b: Option<usize>,
}

/// 段的变化类型。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum SegmentKind {
    /// 两个版本都有，内容未变
    Unchanged,
    /// 两个版本都有，内容有变化
    Modified,
    /// 仅版本 B 有
    Added,
    /// 仅版本 A 有
    Deleted,
}

// ─── ToolDiff ─────────────────────────────────────────────────────

/// 单个 tool 调用的差异。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolDiff {
    /// tool 名称
    pub tool_name: String,
    /// 变化类型
    pub kind: ToolDiffKind,
    /// 版本 A 中的索引（不存在时为 None）
    pub index_a: Option<usize>,
    /// 版本 B 中的索引（不存在时为 None）
    pub index_b: Option<usize>,
    /// args 变化详情（仅 ArgsChanged 时有值）
    pub args_diff: Option<ArgsDiff>,
    /// 调用耗时（取 B 的耗时，若仅 A 有则取 A）
    pub duration_ms: u64,
}

/// Tool 级变化类型。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ToolDiffKind {
    Unchanged,
    Added,
    Deleted,
    ArgsChanged,
}

/// args 变化详情。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ArgsDiff {
    /// 版本 A 的 args
    pub args_a: serde_json::Value,
    /// 版本 B 的 args
    pub args_b: serde_json::Value,
}

// ─── DiffSummary ──────────────────────────────────────────────────

/// 差异摘要统计。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DiffSummary {
    pub segments_added: usize,
    pub segments_deleted: usize,
    pub segments_modified: usize,
    pub segments_unchanged: usize,
    pub tool_calls_added: usize,
    pub tool_calls_deleted: usize,
    pub tool_calls_modified: usize,
    pub tool_calls_unchanged: usize,
    pub token_delta: i64,
    pub duration_delta_ms: i64,
}

// ─── PhaseRule ────────────────────────────────────────────────────

/// 一条 tool → 语义阶段 的映射规则。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PhaseRule {
    /// 阶段名称，如 "定位问题" / "实施修改" / "验证" / "提交"
    pub phase: String,
    /// 匹配的 tool 名称列表
    pub tool_names: Vec<String>,
}

// ─── DiffInput ────────────────────────────────────────────────────

/// Trajectory Diff 的输入模式。
#[derive(Debug, Clone)]
pub enum DiffInput {
    /// 通过 Capability 自动关联
    ByCapability {
        capability_id: String,
    },
    /// 手动指定两条 trace_id
    Manual {
        trace_id_a: String,
        trace_id_b: String,
    },
}
```

---

## 6. 双层对齐算法

### 6.1 算法流程

```
输入: Version A的 tool_calls, Version B的 tool_calls, PhaseRules[]

Step 1: 语义分段（Phasing）
  对 A 和 B 的 tool_calls 分别按 PhaseRules 进行分段
  规则: 当 tool_name 映射到新 Phase 时切分新段

Step 2: 段级对齐
  用编辑距离（Needleman-Wunsch）对齐 A.segments 和 B.segments
  匹配条件: 段 label 相同
  输出: SegmentKind::{Unchanged, Added, Deleted, Modified}

Step 3: Tool 级对齐（段内）
  对每个 Unchanged/Modified 段，再对其 tool_calls 做编辑距离对齐
  匹配条件: SemanticHash(tool) 相同

Step 4: 组装 SegmentDiff 和 DiffSummary

输出: TrajectoryDiff
```

### 6.2 默认 Phase Rules

```rust
/// 内置的 tool → 语义阶段映射。
pub fn default_phase_rules() -> Vec<PhaseRule> {
    vec![
        PhaseRule {
            phase: "定位问题".into(),
            tool_names: vec![
                "read", "grep", "glob", "search_codebase",
                "web_search", "web_fetch", "ls", "list",
            ].into_iter().map(String::from).collect(),
        },
        PhaseRule {
            phase: "实施修改".into(),
            tool_names: vec![
                "write", "edit", "search_replace", "delete_file",
                "bash", "run_command",
            ].into_iter().map(String::from).collect(),
        },
        PhaseRule {
            phase: "验证".into(),
            tool_names: vec![
                "test", "cargo", "check", "lint", "clippy",
                "build", "compile",
            ].into_iter().map(String::from).collect(),
        },
        PhaseRule {
            phase: "提交".into(),
            tool_names: vec![
                "commit", "git", "push", "pull_request",
            ].into_iter().map(String::from).collect(),
        },
    ]
}
```

用户可通过配置文件覆盖或追加自定义规则。

### 6.3 Semantic Hash

```
fn semantic_hash(tc: &ToolCall) -> u64 {
    let mut hasher = DefaultHasher::new();
    tc.tool_name.hash(&mut hasher);
    // 全量 args 序列化后 hash
    serde_json::to_string(&tc.args).unwrap_or_default().hash(&mut hasher);
    hasher.finish()
}
```

---

## 7. CLI 子命令

### 7.1 `paporot trajectory diff`

```rust
/// Trajectory Diff 命令。
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

        /// HTML 报告输出路径（默认 .Paporot/reports/diff_{timestamp}.html）
        #[arg(long)]
        output: Option<String>,

        /// 分段规则配置文件路径
        #[arg(long)]
        phases_config: Option<String>,
    },
}
```

### 7.2 使用示例

```bash
# 通过 Capability 自动关联，生成 HTML 报告
paporot trajectory diff --capability cap_bug_fix_001

# 手动指定两条 trace，输出 Mermaid 代码
paporot trajectory diff --trace-a trace_20260612_001 --trace-b trace_20260612_003 --format mermaid

# 输出 JSON（给 Behavior Eval 消费）
paporot trajectory diff --capability cap_bug_fix_001 --format json
```

---

## 8. HTML 报告

### 8.1 布局

纵向滚动单页面，内嵌 Mermaid.js CDN：

```
┌──────────────────────────────────────────────┐
│  Trajectory Diff: Bug Fix                    │
│  ┌─────────────┐  ┌─────────────────────┐    │
│  │ v1: 12 calls │→ │ v2: 15 calls        │    │
│  │ 45s, 320 tok │  │ 62s, 450 tok        │    │
│  └─────────────┘  └─────────────────────┘    │
│  +1 阶段 +5 调用 -0 M-1   token +130        │
│──────────────────────────────────────────────│
│  ┌──────────────────────────────────────────┐│
│  │         Mermaid 时序图                    ││
│  │  (v1 泳道 vs v2 泳道，diff 高亮)         ││
│  └──────────────────────────────────────────┘│
│──────────────────────────────────────────────│
│  阶段对比表                                   │
│  ┌──────────┬────────────┬─────────────┐    │
│  │ 阶段     │ v1 (12个)  │ v2 (15个)   │    │
│  ├──────────┼────────────┼─────────────┤    │
│  │ 定位问题 │ read ×2    │ read ×2     │    │
│  │          │            │ grep ×1 (+1)│    │
│  ├──────────┼────────────┼─────────────┤    │
│  │ 实施修改 │ edit ×1    │ edit ×1     │    │
│  │          │ commit ×1  │ write ×1(+1)│    │
│  │          │            │ test ×1 (+1)│    │
│  │          │            │ commit ×1   │    │
│  ├──────────┼────────────┼─────────────┤    │
│  │ 验证     │ —          │ clippy(+1)  │    │
│  └──────────┴────────────┴─────────────┘    │
│──────────────────────────────────────────────│
│  逐 Tool 详细对比 (可折叠)                    │
│  ▼ edit_file (ArgsChanged)                   │
│    args_a: {"file_path": "src/auth.rs", ...} │
│    args_b: {"file_path": "src/auth.rs", ...} │
│  ▼ grep (Added)                              │
│    pattern: "POST /users", path: "src/"      │
│  ▼ write (Added)                             │
│    file_path: "tests/test_api.rs"            │
│  ...                                         │
└──────────────────────────────────────────────┘
```

### 8.2 Mermaid 时序图

```
sequenceDiagram
    participant V1 as v1 (trace_001)
    participant V2 as v2 (trace_003)

    box 定位问题
    V1->>V1: read_file(src/api.rs)
    V1->>V1: read_file(src/db.rs)
    V2->>V2: read_file(src/api.rs)
    V2->>V2: grep(POST /users)
    V2->>V2: read_file(src/db.rs)
    end

    box 实施修改
    V1->>V1: edit_file(src/api.rs)
    V1->>V1: commit(fix bug)
    V2->>V2: write(tests/test_api.rs)
    V2->>V2: cargo test
    V2->>V2: edit_file(src/api.rs)
    V2->>V2: cargo test
    V2->>V2: commit(fix bug)
    end

    box 验证
    V2->>V2: cargo clippy
    end

    Note over V1,V2: v2 新增"验证"阶段
```

---

## 9. Capability 关联模式

### 9.1 查找逻辑

```rust
/// 通过 Capability ID 查找对应的两条 trace。
fn find_traces_for_diff(
    storage: &TraceStorage,
    cap_id: &str,
) -> Result<(BehaviorTrace, BehaviorTrace)> {
    // 1. 从 Capability 的 evidence_trace_ids 或
    //    trace 索引的 capability_ids 中查找关联的 trace
    // 2. 按 started_at 排序，取最早和最新的两条
    // 3. 如果少于 2 条，返回错误提示用户先执行 trace link
    // 4. 如果多于 2 条，默认对比最早 vs 最新，支持 --trace-a/--trace-b 手动覆盖
}
```

### 9.2 降级：手动模式

```bash
# 不走 Capability，直接对比两条 trace
paporot trajectory diff --trace-a trace_20260612_001 --trace-b trace_20260612_003
```

---

## 10. 错误类型

```rust
#[derive(Debug)]
pub enum TrajectoryDiffError {
    /// Capability 未关联足够的 trace（少于 2 条）
    InsufficientTraces {
        capability_id: String,
        count: usize,
    },
    /// trace 未找到
    TraceNotFound { trace_id: String },
    /// 分段规则解析错误
    PhaseConfigError(String),
    /// Trace 数据不可比（tool_calls 均为空等）
    NotComparable(String),
}
```

---

## 11. 测试策略

### 单元测试

| 模块 | 关键用例 |
|------|---------|
| `hash.rs` | 相同 args → 相同 hash；不同 args → 不同 hash |
| `phases.rs` | 单 tool 分段正确；tool 跨越阶段时切分；未知 tool 归入默认段 |
| `align.rs` | 完全相同的序列 → 全 Unchanged；纯新增；纯删除；交叉修改 |
| `report.rs` | Mermaid 代码格式；JSON 序列化往返 |
| `types.rs` | serde 往返 |

### 集成测试

- 创建两条不同但关联的 BehaviorTrace，通过 Capability ID 关联，执行 diff，验证输出
- 手动模式 diff 验证
- 不足 2 条 trace 时的错误处理
- 空 tool_calls 的退化场景

### Fixture

- `tests/fixtures/diff_trace_a.json`: 精简版 trace (4 tool calls)
- `tests/fixtures/diff_trace_b.json`: 变化版 trace (6 tool calls, 与 A 有重叠)
