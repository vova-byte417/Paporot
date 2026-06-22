# PRD: Execution Trace 模块（接口级）

> Paporot 从 Capability Version Control 迈向 Behavior Version Control 的第一步

---

## 目录

1. [背景与动机](#1-背景与动机)
2. [核心概念](#2-核心概念)
3. [设计决策记录](#3-设计决策记录)
4. [模块架构](#4-模块架构)
5. [数据类型定义](#5-数据类型定义)
6. [适配器设计](#6-适配器设计)
7. [存储层设计](#7-存储层设计)
8. [CLI 子命令](#8-cli-子命令)
9. [Capability 弱关联](#9-capability-弱关联)
10. [错误类型](#10-错误类型)
11. [非功能性需求](#11-非功能性需求)
12. [实现计划](#12-实现计划)
13. [测试策略](#13-测试策略)
14. [明确不做的事](#14-明确不做的事)

---

## 1. 背景与动机

### 1.1 问题

Paporot 目前是一个 **Capability Version Control（能力版本控制）** 系统：

```
Code → L1 AST → L2 Rules → L3 LLM → Capability
```

它回答的问题是："这次 git diff 引入了哪些新的功能单元？"

但 Anthropic 所定义的 **Behavior Version Control（行为版本控制）** 回答的是另一个问题：

> **Agent 的行为为什么变了？**

举例：

```
Agent 收到："帮我修复这个 bug"

Version A:                 Version B:
  read_file()                read_file()
  edit_file()                generate_tests()
  commit()                   run_tests()
                             edit_file()
                             run_tests()
                             commit()

git diff ≈ 0（代码改动一样）
Capability 不变（都是 "Bug Fix"）
但 Agent 行为截然不同
```

Paporot 当前无法感知这种变化。

### 1.2 解决方案路径

需要新增 4 个模块，按依赖顺序排列：

```
1. Execution Trace (本 PRD)  ← 基础：记录 Agent 做了什么
2. Trajectory Diff           ← 对比：两次执行的轨迹差异
3. Capability Evidence        ← 证据：为什么系统认为这是某个 Capability
4. Behavior Eval             ← 评测：Capability 的行为是否退化
```

Execution Trace 是其余 3 个模块的数据基础。

---

## 2. 核心概念

| 概念 | 定义 | 类比 |
|------|------|------|
| **BehaviorTrace** | 单次 Agent 执行的完整轨迹（prompt → tool_calls → observations → output） | 一次 HTTP request 的完整 log |
| **ToolCall** | Agent 调用的单个工具（grep / read / write / bash 等） | 函数调用栈帧 |
| **Observation** | Tool 调用后系统返回的结果 | 函数返回值 |
| **TokenUsage** | 该次执行消耗的 token 统计 | 资源账单 |
| **TraceSource** | 轨迹数据来源（被动导入 vs 主动捕获） | 数据管道来源 |
| **Adapter** | 将外部 Agent 平台的原生 trace 格式转为 Paporot 标准格式的转换器 | ETL connector |
| **TraceSummary** | 轻量级摘要（不含 tool_calls/observations 详情体） | 数据库视图行 |
| **TraceFilter** | 列表查询的过滤条件 | SQL WHERE 子句 |

---

## 3. 设计决策记录

| # | 决策 | 备选方案 | 选择理由 |
|---|------|---------|---------|
| D1 | `src/trace/` 一级模块（方案 B） | A: 藏于 commands 下；C: 泛型 Event Log | 避免 types.rs 膨胀；Trajectory Diff 无需重构；适配器测试隔离 |
| D2 | 双模式数据来源 | 纯导入 / 纯捕获 | 最大灵活性：已有 Agent 日志可直接消费，未来可 wrapping 实时捕获 |
| D3 | 细粒度全量记录 | 粗粒度摘要 / 中粒度截断 | 缺少完整 args/result 则 Trajectory Diff 和 Behavior Eval 无法做深 |
| D4 | 通用 JSON 格式 + 适配器 | 绑定特定平台格式 | 不耦合 Agent 平台，通过适配器对接任何 Agent |
| D5 | 与 Capability 弱关联 | 平行独立 / 强关联 | Capability.evidence 可引用 Trace ID，但不强制，保持两者独立生命周期 |
| D6 | 人读 + 机读并重 | 纯人读 / 纯机读 | trace 需同时支持人类审计和 Trajectory Diff 引擎 |
| D7 | 首个适配器：DeepSeek | Claude Code / OpenAI / 通用 | 用户当前技术栈优先级 |
| D8 | 存储：JSONL + SQLite | 纯 JSONL / 纯 SQLite / Parquet | 追加写入性能 + 索引查询能力 |
| D9 | 删除为 soft delete | 物理删除 | 保留审计轨迹，支持 undo |
| D10 | 不做自动脱敏 | 导入时自动 strip | 脱敏逻辑依赖业务上下文，手动控制更安全 |
| D11 | `TraceStorage::save` 中 JSONL 与 SQLite 同步写入 | 仅写 JSONL 事后重建索引 | 查询一致性，导入时直接可用 |

---

## 4. 模块架构

### 4.1 目录结构

```
src/
  trace/                         ← 新增一级模块（与 analysis/ 平级）
    mod.rs                       ← pub mod types; pub mod storage; pub mod adapter; pub mod adapters; pub mod error;
    types.rs                     ← BehaviorTrace / ToolCall / Observation / TokenUsage / TraceSummary / TraceFilter / ...
    storage.rs                   ← TraceStorage: JSONL 文件读写 + SQLite 索引
    adapter.rs                   ← TraceAdapter trait + 注册/自动检测
    error.rs                     ← TraceError 枚举
    adapters/
      mod.rs
      deepseek.rs                ← DeepSeekAdapter
      deepseek_types.rs          ← DeepSeek 原生格式的反序列化类型（内部可见）
  commands/
    trace.rs                     ← CLI 子命令：import / list / show / delete / link / redact
  types.rs                       ← Capability 新增 evidence_trace_ids 字段
  lib.rs                         ← + pub mod trace;
  cli.rs                         ← + Commands::Trace 子命令
```

### 4.2 依赖关系

```
cli.rs  →  commands/trace.rs  →  trace/storage.rs + trace/adapter.rs + trace/error.rs
                                     ↓                        ↓
                                trace/types.rs (纯数据)    trace/adapters/deepseek.rs
                                     ↑
                              types.rs (Capability.evidence_trace_ids)
```

- `trace/types.rs` 零内部依赖
- `trace/error.rs` 零内部依赖
- `trace/storage.rs` 依赖 `trace/types.rs` + `trace/error.rs` + `rusqlite`
- `trace/adapter.rs` 依赖 `trace/types.rs` + `trace/error.rs`
- `trace/adapters/deepseek.rs` 依赖 `trace/adapter.rs` + `trace/types.rs` + `trace/error.rs`
- `commands/trace.rs` 不依赖 `analysis/`，不依赖 `agent.rs`

### 4.3 存储布局

```
.Paporot/
  config.toml                   ← 现有
  snapshots/                    ← 现有
  graph/                        ← 现有
  feedback/                     ← 现有
  testmap/                      ← 现有
  traces/                       ← 新增
    2026-06-12-001.jsonl
    2026-06-12-002.jsonl
  trace_index.db                ← 新增 SQLite 索引
  .gitignore                    ← 新增: traces/ + trace_index.db
```

---

## 5. 数据类型定义

### 5.1 文件: `src/trace/types.rs`

```rust
//! Execution Trace 核心数据类型
//!
//! 零内部依赖，仅依赖 serde。

use serde::{Deserialize, Serialize};

// ─── BehaviorTrace ─────────────────────────────────────────────────

/// 单次 Agent 执行的完整轨迹。
///
/// 这是 Paporot Trace 模块的核心数据单元。
/// 一条 BehaviorTrace 对应一次完整的 Agent turn：
/// 从收到 prompt 到输出 final_output 的全部中间过程。
///
/// # JSONL 存储
///
/// 序列化为单行 JSON 追加写入 `{date}-{seq}.jsonl` 文件。
///
/// # 大小约束
///
/// 单条序列化后不超过 100 MB。超出时 `tool_calls[].args`
/// 和 `observations[].content` 中的字符串字段会被截断。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BehaviorTrace {
    /// 唯一标识符。
    ///
    /// 格式: `trace_{YYYYMMDD}_{NNN}`
    /// 示例: `trace_20260612_001`
    pub id: String,

    /// 关联的 Agent session ID。
    ///
    /// 同一个 session 可能产生多条 BehaviorTrace（多个 turn）。
    /// 来源:
    ///   - Imported: 从外部 trace 继承
    ///   - Captured:  wrapper 模式中用户传入，默认 "session_<timestamp>"
    pub session_id: String,

    /// 原始用户输入 / 系统 prompt。
    ///
    /// 这是触发 Agent 执行的原始文本。
    /// 截断策略: 如果总大小超限，prompt 保留前 8000 字符 + "...[truncated]"。
    pub prompt: String,

    /// 按时间排序的 tool 调用序列。
    ///
    /// tool_calls[i].timestamp <= tool_calls[i+1].timestamp。
    /// 可以为空（Agent 没有调用任何 tool，直接输出文本）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,

    /// tool 调用对应的观察结果。
    ///
    /// 与 tool_calls 通过 ID 关联，非嵌套结构。
    /// 允许 tool_calls.len() != observations.len()（调用可能无返回，或多次重试）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observations: Vec<Observation>,

    /// Agent 最终输出文本。
    ///
    /// 截断策略: 如果总大小超限，保留后 8000 字符 + "[truncated]..."。
    pub final_output: String,

    /// 累计 token 消耗。
    ///
    /// 对于 Imported 来源: 从外部 trace 提取，可能为全零（无法获取时）。
    /// 对于 Captured 来源:  wrapper 逐次累加。
    pub token_usage: TokenUsage,

    /// 执行开始时间。
    ///
    /// ISO-8601 格式，精度到毫秒: "2026-06-12T14:30:00.123Z"
    pub started_at: String,

    /// 执行结束时间。
    ///
    /// ISO-8601 格式，精度到毫秒: "2026-06-12T14:32:15.456Z"
    /// 与 started_at 的差值 = 总执行耗时。
    pub finished_at: String,

    /// 数据来源标记。
    pub source: TraceSource,

    /// 用户自定义标签（可选）。
    ///
    /// 用于分类/过滤，如 ["security-audit", "production", "p0"]。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    /// 关联的 Capability ID 列表（弱关联，双向可选）。
    ///
    /// 可由用户通过 `paporot trace link` 手动关联，
    /// 或未来由 Behavior Eval 模块自动填充。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capability_ids: Vec<String>,

    /// 删除标记。
    ///
    /// 序列化到 JSONL 中用于 soft delete。
    /// true 时 list/show 命令默认不返回该 trace。
    #[serde(default)]
    pub deleted: bool,
}

// ─── ToolCall ──────────────────────────────────────────────────────

/// 单次 tool 调用记录。
///
/// 代表 Agent 在一次执行过程中调用的某一个工具。
/// 常见的 tool_name: "read", "write", "grep", "bash", "glob", "edit", "web_search" 等。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolCall {
    /// 唯一标识符。
    ///
    /// 格式: "call_{trace_id}_{NNN}"
    /// 示例: "call_trace_20260612_001_001"
    pub id: String,

    /// tool 名称。
    ///
    /// 不限制值域，由 Agent 平台定义。
    /// 已知常见值: "read", "write", "edit", "grep", "glob", "bash",
    /// "web_search", "web_fetch", "task", "ask", "search_codebase"。
    pub tool_name: String,

    /// 完整参数。
    ///
    /// JSON value，不预设 schema。
    /// 示例:
    ///   {"pattern": "login", "path": "src/"}  (grep)
    ///   {"file_path": "src/auth.rs", "limit": 100}  (read)
    /// 截断策略:
    ///   - 字符串值 > 10000 字符 → 截断并附加 "[truncated:N bytes]"
    ///   - 对象/数组深度 > 10 → 截断并附加 "[truncated:max depth]"
    pub args: serde_json::Value,

    /// 调用时间戳。
    ///
    /// ISO-8601 格式: "2026-06-12T14:30:05.500Z"
    /// tool_calls 数组按此字段升序排列。
    pub timestamp: String,

    /// 调用耗时（毫秒）。
    ///
    /// 从发起调用到收到 observation 的 wall-clock 时间。
    /// 如果无法获取（某些外部 trace 格式），默认为 0。
    #[serde(default)]
    pub duration_ms: u64,

    /// 关联的 Observation ID。
    ///
    /// None 表示该 tool call 没有产生 observation（执行失败/被取消/流式未完成）。
    /// 一个 ToolCall 可能关联到多个 Observation（重试场景下通过 result_id 数组关联，
    /// 但当前版本简化为一对一映射）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_id: Option<String>,
}

// ─── Observation ───────────────────────────────────────────────────

/// Tool 调用返回的观察结果。
///
/// 这是 Agent "看到" 的工具输出，会作为下一步推理的上下文。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Observation {
    /// 唯一标识符。
    ///
    /// 格式: "obs_{trace_id}_{NNN}"
    /// 示例: "obs_trace_20260612_001_001"
    pub id: String,

    /// 关联的 ToolCall ID。
    ///
    /// 必须存在于同一 BehaviorTrace.tool_calls 中。
    pub tool_call_id: String,

    /// 完整结果内容。
    ///
    /// 截断策略:
    ///   - 总大小 > 1MB → truncated=true, 前 100KB + "...[truncated at N bytes]"
    ///   - 否则 truncated=false
    pub content: String,

    /// 结果是否被截断。
    #[serde(default)]
    pub truncated: bool,

    /// 截断时的原始字节位置。
    ///
    /// truncated=false 时为 None。
    /// truncated=true 时指向内容被切断的字节偏移。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated_at_bytes: Option<u64>,
}

// ─── TokenUsage ────────────────────────────────────────────────────

/// Token 消耗统计。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TokenUsage {
    /// 输入 token 数（prompt + context）。
    pub input_tokens: u64,

    /// 输出 token 数（generated response）。
    pub output_tokens: u64,

    /// 缓存读取的 token 数（如 Anthropic prompt caching，跳过计费的部分）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,

    /// 缓存写入 token 数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
}

// ─── TraceSource ───────────────────────────────────────────────────

/// 轨迹数据来源。
///
/// 使用 serde tag 序列化，JSON 中通过 "type" 字段区分:
///   {"type": "imported", "adapter": "deepseek", "file_path": "..."}
///   {"type": "captured", "agent_name": "claude-code"}
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceSource {
    /// 被动导入：从外部 Agent 日志文件解析。
    Imported {
        /// 适配器名称，如 "deepseek" / "claude" / "openai"
        adapter: String,
        /// 适配器版本号（从 adapter.version() 获取）
        adapter_version: String,
        /// 原始文件路径
        file_path: String,
    },
    /// Paporot wrapper 实时捕获。
    Captured {
        /// 被捕获的 Agent 名称，用户自定义
        agent_name: String,
    },
}

// ─── TraceSummary ──────────────────────────────────────────────────

/// 轻量级摘要。
///
/// 用于 list 命令，不含 tool_calls/observations 详情体。
/// 所有字段从 SQLite 索引读取，无需打开 JSONL 文件。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TraceSummary {
    /// Trace ID
    pub id: String,
    /// Session ID
    pub session_id: String,
    /// Prompt 前 200 字符
    pub prompt_preview: String,
    /// Agent 使用的所有 tool 名称（去重）
    pub tool_names: Vec<String>,
    /// Tool 调用总次数
    pub tool_call_count: usize,
    /// 总 token 消耗（input + output）
    pub total_tokens: u64,
    /// 执行开始时间
    pub started_at: String,
    /// 执行结束时间
    pub finished_at: String,
    /// 总耗时（毫秒）
    pub duration_ms: u64,
    /// 数据来源类型: "imported" | "captured"
    pub source_type: String,
    /// 适配器名称（仅 imported 时有值）
    pub adapter_name: Option<String>,
    /// 关联 Capability 数
    pub capability_count: usize,
    /// 标签列表
    pub tags: Vec<String>,
    /// 是否已删除
    pub deleted: bool,
}

// ─── TraceFilter ───────────────────────────────────────────────────

/// 列表查询的过滤条件。
///
/// 所有字段可选，不提供 = 不限制。
/// 多个条件之间为 AND 关系。
#[derive(Debug, Clone, Default)]
pub struct TraceFilter {
    /// 按 session ID 过滤
    pub session_id: Option<String>,
    /// 按 tool 名称过滤（精确匹配）
    pub tool_name: Option<String>,
    /// 按标签过滤（精确匹配）
    pub tag: Option<String>,
    /// 按 Capability ID 过滤
    pub capability_id: Option<String>,
    /// 起始日期（含）, ISO-8601 日期: "2026-06-01"
    pub from_date: Option<String>,
    /// 结束日期（含）, ISO-8601 日期: "2026-06-12"
    pub to_date: Option<String>,
    /// 按来源类型过滤: "imported" | "captured"
    pub source_type: Option<String>,
    /// 是否包含已删除的 trace（默认 false）
    pub include_deleted: bool,
    /// 返回条数上限（默认 100）
    pub limit: usize,
    /// 偏移量（分页）
    pub offset: usize,
}

impl Default for TraceFilter {
    fn default() -> Self {
        Self {
            session_id: None,
            tool_name: None,
            tag: None,
            capability_id: None,
            from_date: None,
            to_date: None,
            source_type: None,
            include_deleted: false,
            limit: 100,
            offset: 0,
        }
    }
}

// ─── ImportResult ──────────────────────────────────────────────────

/// 单次 import 操作的结果。
#[derive(Debug, Clone)]
pub struct ImportResult {
    /// 导入的源文件路径
    pub source_path: String,
    /// 使用的适配器名称
    pub adapter: String,
    /// 是否自动检测到适配器
    pub auto_detected: bool,
    /// 成功导入的 trace 列表
    pub imported: Vec<TraceSummary>,
    /// 跳过的 trace 数量（解析失败/格式不支持）
    pub skipped_count: usize,
    /// 跳过的原因列表
    pub skip_reasons: Vec<String>,
}

// ─── RedactConfig ──────────────────────────────────────────────────

/// 脱敏配置。
#[derive(Debug, Clone)]
pub struct RedactConfig {
    /// 是否脱敏 Authorization header
    pub redact_auth_header: bool,
    /// 是否脱敏 API key 模式
    pub redact_api_keys: bool,
    /// 是否脱敏环境变量值
    pub redact_env_values: bool,
    /// 额外的自定义正则替换规则 (pattern, replacement)
    pub custom_rules: Vec<(String, String)>,
}

impl Default for RedactConfig {
    fn default() -> Self {
        Self {
            redact_auth_header: true,
            redact_api_keys: true,
            redact_env_values: false,
            custom_rules: Vec::new(),
        }
    }
}
```

### 5.2 类型间关系图

```
BehaviorTrace (1)
  ├── tool_calls: Vec<ToolCall> (0..N)
  │     └── result_id ────────────────────┐
  ├── observations: Vec<Observation> (0..N)│
  │     └── tool_call_id ─────────────────┘
  ├── token_usage: TokenUsage (1)
  ├── source: TraceSource (1)  ← tagged enum
  ├── tags: Vec<String> (0..N)
  └── capability_ids: Vec<String> (0..N)

TraceSummary: BehaviorTrace 的轻量视图，不含 tool_calls/observations
TraceFilter:  SQL 查询条件
ImportResult: 消费 import 命令的返回
RedactConfig: 脱敏规则配置
```

---

## 6. 适配器设计

### 6.1 文件: `src/trace/adapter.rs`

```rust
//! Trace 适配器 trait + 注册与自动检测。
//!
//! 适配器模式将外部 Agent 平台的原生 trace 格式
//! （DeepSeek API log / Claude session log / OpenAI run trace / ...）
//! 转换为 Paporot 标准 BehaviorTrace。

use crate::trace::error::TraceError;
use crate::trace::types::BehaviorTrace;

/// 外部 trace 格式 → BehaviorTrace 的转换适配器。
///
/// # 实现要求
///
/// - `Send + Sync`: 适配器实例可在多线程环境下共享
/// - `can_handle`: 必须快速返回（< 1ms），不应做全量解析
/// - `parse`: 可以较慢，允许执行 IO 或复杂计算
///
/// # 错误处理
///
/// `parse` 的返回策略:
///   - 一条 trace 解析失败 → 跳过该条，继续处理剩余，返回 `TraceError::PartialImport`
///   - 全部解析失败 → 返回 `TraceError::ParseError`
///   - 格式完全不匹配 → `can_handle` 返回 false
pub trait TraceAdapter: Send + Sync {
    /// 适配器唯一名称。
    ///
    /// 用于 CLI `--adapter` 参数和 TraceSource.Imported.adapter 字段。
    /// 约束: 小写字母 + 连字符，如 "deepseek" / "claude-code" / "openai"。
    fn name(&self) -> &str;

    /// 适配器版本号。
    ///
    /// 格式: "major.minor.patch"，如 "1.0.0"。
    /// 当解析逻辑发生变更时递增。
    /// 记录在 TraceSource.Imported.adapter_version 中用于可复现性。
    fn version(&self) -> &str;

    /// 检测输入是否为本适配器支持的格式。
    ///
    /// # 实现约束
    ///
    /// - 不应执行完整解析
    /// - 建议通过探测前 N 字节的特征模式来判断
    ///   - JSON 格式: 检查顶层 key 名称
    ///   - JSONL 格式: 检查首行的顶层 key 名称
    ///   - 其他格式: 检查 magic bytes 或文件头
    /// - 时间复杂度: O(1) 或 O(前 4096 字节)
    fn can_handle(&self, raw: &str) -> bool;

    /// 解析原始 trace 文本，返回 BehaviorTrace 列表。
    ///
    /// # 参数
    ///
    /// - `raw`:     原始文本内容（可能是单条 JSON / JSONL / 自定义格式）
    /// - `file_path`: 原始文件路径（用于 TraceSource 记录和错误消息）
    ///
    /// # 返回
    ///
    /// - `Ok(Vec<BehaviorTrace>)`: 成功解析出的所有 trace
    ///   - 空 Vec 合法，表示没有识别出可转换的 trace
    /// - `Err(TraceError::PartialImport)`: 部分解析成功
    /// - `Err(TraceError::ParseError)`: 全部失败
    ///
    /// # 副作用
    ///
    /// 无。适配器是纯函数，不访问文件系统或网络。
    fn parse(&self, raw: &str, file_path: &str) -> Result<Vec<BehaviorTrace>, TraceError>;

    /// 适配器的人类可读描述。
    ///
    /// 用于 CLI `paporot trace adapter list` 展示。
    fn description(&self) -> &str;
}

// ─── 适配器注册表 ────────────────────────────────────────────────

/// 获取所有已注册的适配器。
///
/// 新适配器只需在此函数中加入一行即可自动注册。
/// 未来可改为过程宏自动发现。
pub fn all_adapters() -> Vec<Box<dyn TraceAdapter>> {
    vec![
        Box::new(crate::trace::adapters::deepseek::DeepSeekAdapter::new()),
        // 未来扩展:
        // Box::new(crate::trace::adapters::claude::ClaudeAdapter::new()),
        // Box::new(crate::trace::adapters::openai::OpenAIAdapter::new()),
    ]
}

/// 按名称查找适配器。
///
/// # 参数
///
/// - `name`: 适配器名称，大小写不敏感
///
/// # 返回
///
/// - `Some(Box<dyn TraceAdapter>)` 找到
/// - `None` 未找到
pub fn find_adapter(name: &str) -> Option<Box<dyn TraceAdapter>> {
    let name_lower = name.to_lowercase();
    all_adapters()
        .into_iter()
        .find(|a| a.name().to_lowercase() == name_lower)
}

/// 自动检测格式，返回第一个 can_handle() 返回 true 的适配器。
///
/// # 检测顺序
///
/// 按 all_adapters() 返回的顺序依次尝试。
/// 建议将差异最大的适配器放在最前面。
pub fn auto_detect(raw: &str) -> Option<Box<dyn TraceAdapter>> {
    all_adapters()
        .into_iter()
        .find(|a| a.can_handle(raw))
}

/// 列出所有适配器的信息。
pub fn list_adapters() -> Vec<AdapterInfo> {
    all_adapters()
        .iter()
        .map(|a| AdapterInfo {
            name: a.name().to_string(),
            version: a.version().to_string(),
            description: a.description().to_string(),
        })
        .collect()
}

/// 适配器元信息（用于 CLI 展示）。
#[derive(Debug, Clone)]
pub struct AdapterInfo {
    pub name: String,
    pub version: String,
    pub description: String,
}

// ─── 单元测试辅助 ───────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod test_helpers {
    use super::*;
    use crate::trace::error::TraceError;

    /// 创建一个用于测试的 mock 适配器。
    pub struct MockAdapter {
        pub name: String,
        pub handles: bool,
        pub results: Vec<BehaviorTrace>,
    }

    impl TraceAdapter for MockAdapter {
        fn name(&self) -> &str { &self.name }
        fn version(&self) -> &str { "0.0.0-test" }
        fn can_handle(&self, _raw: &str) -> bool { self.handles }
        fn parse(&self, _raw: &str, _file_path: &str) -> Result<Vec<BehaviorTrace>, TraceError> {
            Ok(self.results.clone())
        }
        fn description(&self) -> &str { "Mock adapter for testing" }
    }
}
```

### 6.2 文件: `src/trace/adapters/deepseek_types.rs`

DeepSeek 原生 API 响应格式的反序列化类型。此模块仅在 DeepSeek 适配器内部使用，不对外暴露。

```rust
//! DeepSeek API 原生格式的反序列化类型。
//!
//! 基于 DeepSeek API 文档的 Chat Completion 响应格式。
//! 只定义适配器需要用到的字段，其他字段由 serde 自动忽略。

use serde::Deserialize;

/// DeepSeek Chat Completion 单次 API 调用的完整响应。
///
/// 对应 DeepSeek API `POST /chat/completions` 的返回值。
/// 此为简化版，只包含适配器所需的字段。
#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) struct DeepSeekResponse {
    /// API 调用 ID
    pub id: String,
    /// 模型名称
    #[serde(default)]
    pub model: String,
    /// choices 列表
    pub choices: Vec<DeepSeekChoice>,
    /// token 用量
    #[serde(default)]
    pub usage: Option<DeepSeekUsage>,
    /// 创建时间戳（Unix 秒）
    pub created: Option<u64>,
}

/// DeepSeek API 单条 choice。
#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekChoice {
    /// choice 序号
    #[serde(default)]
    pub index: u32,
    /// 消息内容
    pub message: DeepSeekMessage,
    /// 结束原因: "stop" | "length" | "tool_calls" | ...
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// DeepSeek API 消息。
///
/// 可能同时包含 content 和 tool_calls，也可能只有其中之一。
#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekMessage {
    /// 消息角色: "assistant" | "user" | "system" | "tool"
    #[serde(default)]
    pub role: String,
    /// 文本内容（可能为 null 或空）
    #[serde(default)]
    pub content: Option<String>,
    /// tool 调用列表
    #[serde(default)]
    pub tool_calls: Option<Vec<DeepSeekToolCall>>,
}

/// DeepSeek tool 调用。
#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekToolCall {
    /// tool 调用 ID
    pub id: String,
    /// tool 类型: "function"
    #[serde(rename = "type")]
    pub call_type: String,
    /// 函数调用详情
    pub function: DeepSeekFunctionCall,
}

/// DeepSeek 函数调用详情。
#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekFunctionCall {
    /// 函数名称
    pub name: String,
    /// 参数（JSON 字符串，需二次解析）
    pub arguments: String,
}

/// DeepSeek token usage。
#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekUsage {
    /// prompt tokens
    #[serde(default)]
    pub prompt_tokens: u64,
    /// completion tokens
    #[serde(default)]
    pub completion_tokens: u64,
    /// total tokens
    #[serde(default)]
    pub total_tokens: u64,
}

/// 导入格式1: JSONL，每行一个 DeepSeek API response
///
/// 这是最常见的格式——将多次 API 调用的完整 response 对象
/// 以 JSONL 方式保存。
pub(crate) type DeepSeekJsonlFormat = Vec<DeepSeekResponse>;

/// 导入格式2: DeepSeek Platform Run Log（批量导入）。
///
/// DeepSeek Platform 导出的 run 日志可能包含多个 turn，
/// 每个 turn 是一个 DeepSeekResponse。
#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekRunLog {
    /// 运行 ID
    pub run_id: String,
    /// 所有 turn 的 response
    pub turns: Vec<DeepSeekRunTurn>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekRunTurn {
    /// turn 序号
    pub index: u32,
    /// 用户输入 prompt
    pub prompt: Option<String>,
    /// API response
    pub response: DeepSeekResponse,
    /// 时间戳
    pub timestamp: Option<String>,
}
```

### 6.3 文件: `src/trace/adapters/deepseek.rs`

```rust
//! DeepSeek Trace Adapter
//!
//! 将 DeepSeek API response / run log 转换为 Paporot BehaviorTrace。
//!
//! # 支持格式
//!
//! 1. JSONL 格式: 每行一个 DeepSeek Chat Completion response 对象
//! 2. Run Log 格式: DeepSeek Platform 导出的 run 日志（JSON 对象，包含 turns 数组）
//!
//! # 自动检测
//!
//! `can_handle` 通过探测首行的顶层 JSON key 来判定:
//!   - 包含 "choices" 且有 "id" + "model" → JSONL 格式
//!   - 包含 "run_id" + "turns"             → Run Log 格式

use crate::trace::adapter::TraceAdapter;
use crate::trace::error::TraceError;
use crate::trace::types::{BehaviorTrace, Observation, TokenUsage, ToolCall, TraceSource};
use super::deepseek_types::*;

/// DeepSeek 适配器
pub struct DeepSeekAdapter;

impl DeepSeekAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl TraceAdapter for DeepSeekAdapter {
    fn name(&self) -> &str {
        "deepseek"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn can_handle(&self, raw: &str) -> bool {
        // 取前 4096 字节做快速探测
        let head = &raw[..raw.len().min(4096)];

        // JSONL 格式: 首行包含 "choices" + "id"
        if let Some(first_line) = head.lines().next() {
            if first_line.contains("\"choices\"") && first_line.contains("\"id\"") {
                return true;
            }
        }

        // Run Log 格式: 包含 "run_id" + "turns"
        if head.contains("\"run_id\"") && head.contains("\"turns\"") {
            return true;
        }

        false
    }

    fn parse(&self, raw: &str, file_path: &str) -> Result<Vec<BehaviorTrace>, TraceError> {
        let trimmed = raw.trim();

        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        // 检测格式
        if let Some(first_line) = trimmed.lines().next() {
            if first_line.contains("\"choices\"") && first_line.contains("\"id\"") {
                return self.parse_jsonl(trimmed, file_path);
            }
        }

        if trimmed.contains("\"run_id\"") && trimmed.contains("\"turns\"") {
            return self.parse_run_log(trimmed, file_path);
        }

        Err(TraceError::ParseError {
            message: "Unrecognized DeepSeek format. Expected JSONL (lines with \"choices\"+\"id\") or Run Log (object with \"run_id\"+\"turns\")".into(),
            adapter: self.name().into(),
        })
    }

    fn description(&self) -> &str {
        "Parses DeepSeek API Chat Completion responses and Platform Run Logs into Paporot BehaviorTraces"
    }
}

// ─── 私有方法 ─────────────────────────────────────────────────────

impl DeepSeekAdapter {
    /// 解析 JSONL 格式（一行一个 DeepSeekResponse）
    fn parse_jsonl(&self, raw: &str, file_path: &str) -> Result<Vec<BehaviorTrace>, TraceError> {
        let mut traces = Vec::new();
        let mut skipped = 0u32;
        let mut skip_reasons = Vec::new();

        for (line_no, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<DeepSeekResponse>(line) {
                Ok(resp) => {
                    traces.push(self.response_to_trace(&resp, file_path));
                }
                Err(e) => {
                    skipped += 1;
                    skip_reasons.push(format!("Line {}: {}", line_no + 1, e));
                }
            }
        }

        if traces.is_empty() && skipped > 0 {
            return Err(TraceError::ParseError {
                message: format!(
                    "Failed to parse all {} lines. Reasons: {}",
                    skipped,
                    skip_reasons.join("; ")
                ),
                adapter: self.name().into(),
            });
        }

        if skipped > 0 {
            return Err(TraceError::PartialImport {
                imported: traces.len(),
                skipped: skipped as usize,
                reasons: skip_reasons,
            });
        }

        Ok(traces)
    }

    /// 解析 Run Log 格式
    fn parse_run_log(&self, raw: &str, file_path: &str) -> Result<Vec<BehaviorTrace>, TraceError> {
        let log: DeepSeekRunLog = serde_json::from_str(raw).map_err(|e| {
            TraceError::ParseError {
                message: format!("Failed to parse DeepSeek Run Log: {}", e),
                adapter: self.name().into(),
            }
        })?;

        let traces: Vec<BehaviorTrace> = log.turns.iter().map(|turn| {
            let mut trace = self.response_to_trace(&turn.response, file_path);
            // Run Log 有更好的 prompt 和 session 信息
            trace.session_id = log.run_id.clone();
            if let Some(ref prompt) = turn.prompt {
                trace.prompt = prompt.clone();
            }
            if let Some(ref ts) = turn.timestamp {
                trace.started_at = ts.clone();
                trace.finished_at = ts.clone(); // 没有结束时就用同一个
            }
            trace
        }).collect();

        Ok(traces)
    }

    /// 将单个 DeepSeekResponse 转换为 BehaviorTrace
    fn response_to_trace(&self, resp: &DeepSeekResponse, file_path: &str) -> BehaviorTrace {
        // 提取初始 content
        let mut final_output_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut observations = Vec::new();
        let mut call_idx = 0u32;

        let session_id = resp.id.clone();

        // 处理 choice（通常只有 1 个）
        for choice in &resp.choices {
            let msg = &choice.message;

            // 收集 content
            if let Some(ref content) = msg.content {
                if !content.is_empty() {
                    final_output_parts.push(content.clone());
                }
            }

            // 收集 tool_calls
            if let Some(ref d_tool_calls) = msg.tool_calls {
                for d_call in d_tool_calls {
                    call_idx += 1;

                    // 解析 arguments 字符串为 JSON Value
                    let args: serde_json::Value = serde_json::from_str(&d_call.function.arguments)
                        .unwrap_or_else(|_| {
                            serde_json::Value::String(d_call.function.arguments.clone())
                        });

                    let call_id = format!("call_{}_{:03}", session_id, call_idx);
                    let obs_id = format!("obs_{}_{:03}", session_id, call_idx);

                    tool_calls.push(ToolCall {
                        id: call_id.clone(),
                        tool_name: d_call.function.name.clone(),
                        args,
                        timestamp: String::new(), // 下面统一填充
                        duration_ms: 0,
                        result_id: Some(obs_id.clone()),
                    });

                    observations.push(Observation {
                        id: obs_id,
                        tool_call_id: call_id,
                        content: String::new(), // DeepSeek API response 不含 observation
                        truncated: false,
                        truncated_at_bytes: None,
                    });
                }
            }
        }

        let final_output = final_output_parts.join("\n");

        // 时间戳处理
        let timestamp = if let Some(ts) = resp.created {
            // Unix timestamp (seconds) → ISO-8601
            format_timestamp(ts)
        } else {
            now_iso8601()
        };

        let (trace_tools, _) = filter_tool_calls_for_trace(&tool_calls);

        BehaviorTrace {
            id: String::new(), // 将由 TraceStorage 分配
            session_id,
            prompt: String::new(), // JSONL 格式无原始 prompt，由用户后续补充
            tool_calls: trace_tools,
            observations,
            final_output,
            token_usage: TokenUsage {
                input_tokens: resp.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
                output_tokens: resp.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
                cache_read_tokens: None,
                cache_write_tokens: None,
            },
            started_at: timestamp.clone(),
            finished_at: timestamp,
            source: TraceSource::Imported {
                adapter: self.name().into(),
                adapter_version: self.version().into(),
                file_path: file_path.to_string(),
            },
            tags: Vec::new(),
            capability_ids: Vec::new(),
            deleted: false,
        }
    }
}

// ─── 辅助函数 ──────────────────────────────────────────────────

/// 将 Unix 时间戳（秒）转为 ISO-8601 字符串
fn format_timestamp(unix_secs: u64) -> String {
    // 简单实现，避免引入 chrono 依赖
    // 格式: "2026-06-12T14:30:00Z"
    // 精确转换需要 chrono，这里用合理方式处理
    use std::time::{Duration, UNIX_EPOCH};
    let d = UNIX_EPOCH + Duration::from_secs(unix_secs);
    // 使用 chrono 或手写格式化
    // 简化: 存原始 epoch 字符串
    format!("epoch:{}", unix_secs)
}

fn now_iso8601() -> String {
    // 简化的当前时间获取
    "unknown".to_string()
}

/// 从 tool_calls 中分离出 agent tool 和 system tool
///
/// 一些 DeepSeek tool calls 实际上是系统内部调用（如 reasoning），
/// 不应计入 trace 的 tool_calls。
fn filter_tool_calls_for_trace(tool_calls: &[ToolCall]) -> (Vec<ToolCall>, Vec<ToolCall>) {
    // 当前版本不做过滤，全部保留
    (tool_calls.to_vec(), Vec::new())
}

// ─── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_handle_jsonl_format() {
        let adapter = DeepSeekAdapter::new();
        let sample = r#"{"id":"chatcmpl-123","choices":[{"message":{"role":"assistant","content":"Hello"}}]}"#;
        assert!(adapter.can_handle(sample));
    }

    #[test]
    fn test_can_handle_run_log_format() {
        let adapter = DeepSeekAdapter::new();
        let sample = r#"{"run_id":"run-001","turns":[]}"#;
        assert!(adapter.can_handle(sample));
    }

    #[test]
    fn test_can_handle_unknown_format() {
        let adapter = DeepSeekAdapter::new();
        assert!(!adapter.can_handle("just some random text"));
    }

    #[test]
    fn test_can_handle_empty() {
        let adapter = DeepSeekAdapter::new();
        assert!(!adapter.can_handle(""));
    }
}
```

---

## 7. 存储层设计

### 7.1 文件: `src/trace/storage.rs`

```rust
//! Trace 持久化存储层。
//!
//! 双写架构:
//!   - JSONL 文件: 权威数据源（单行 JSON，追加写入）
//!   - SQLite 索引: 加速查询（通过 byte_offset 回源 JSONL 读详情）
//!
//! # 文件管理
//!
//! JSONL 文件按日期分片: `{YYYY-MM-DD}-{seq}.jsonl`
//! seq 从 001 开始，当前文件超过 100 MB 时自动 +1。
//!
//! # 删除语义
//!
//! soft delete: JSONL 行和 SQLite 行中设置 `_deleted: true`，
//! 不物理删除数据。SQLite 查询默认过滤 deleted=1 的行。

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::trace::error::TraceError;
use crate::trace::types::{BehaviorTrace, ImportResult, TraceFilter, TraceSummary};

/// Trace 存储管理器。
///
/// # 线程安全
///
/// TraceStorage 内部使用互斥机制保证 JSONL 写入原子性。
/// Clone 是 cheap 的（PathBuf + Option<Connection> 交给外部管理，
/// 或者 Connection 用 Arc<Mutex<>> 包装）。
#[derive(Clone)]
pub struct TraceStorage {
    /// 存储目录: .Paporot/traces/
    dir: PathBuf,
    /// SQLite 数据库路径（用于延迟连接）
    db_path: PathBuf,
}

impl TraceStorage {
    /// 创建存储实例。
    ///
    /// # 参数
    ///
    /// - `base_dir`: Paporot 根目录（通常是 `.Paporot/`）
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let base = base_dir.into();
        Self {
            dir: base.join("traces"),
            db_path: base.join("trace_index.db"),
        }
    }

    // ─── 生命周期 ─────────────────────────────────────────────

    /// 初始化存储目录和 SQLite 数据库。
    ///
    /// 幂等操作：多次调用不会重复创建。
    pub fn init(&self) -> Result<(), TraceError> {
        fs::create_dir_all(&self.dir).map_err(|e| TraceError::Io {
            message: format!("Failed to create trace dir {}: {}", self.dir.display(), e),
        })?;

        self.init_db()?;

        // 确保 .Paporot/.gitignore 包含 trace 相关条目
        self.ensure_gitignore()?;

        Ok(())
    }

    /// 初始化 SQLite 表结构。
    fn init_db(&self) -> Result<(), TraceError> {
        let conn = self.open_db()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS traces (
                id              TEXT PRIMARY KEY,
                session_id      TEXT NOT NULL,
                tool_names      TEXT NOT NULL,
                prompt_preview  TEXT DEFAULT '',
                started_at      TEXT NOT NULL,
                finished_at     TEXT NOT NULL,
                duration_ms     INTEGER NOT NULL DEFAULT 0,
                input_tokens    INTEGER NOT NULL DEFAULT 0,
                output_tokens   INTEGER NOT NULL DEFAULT 0,
                source_type     TEXT NOT NULL,
                adapter_name    TEXT DEFAULT NULL,
                file_path       TEXT NOT NULL,
                byte_offset     INTEGER NOT NULL,
                capability_ids  TEXT DEFAULT '[]',
                tags            TEXT DEFAULT '[]',
                deleted         INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_traces_session
                ON traces(session_id);
            CREATE INDEX IF NOT EXISTS idx_traces_started
                ON traces(started_at);
            CREATE INDEX IF NOT EXISTS idx_traces_deleted
                ON traces(deleted);
            CREATE INDEX IF NOT EXISTS idx_traces_tool_names
                ON traces(tool_names);
            CREATE INDEX IF NOT EXISTS idx_traces_source_type
                ON traces(source_type);",
        )
        .map_err(|e| TraceError::Database {
            message: format!("Failed to initialize SQLite schema: {}", e),
        })?;

        Ok(())
    }

    /// 确保 .gitignore 忽略 trace 相关文件。
    fn ensure_gitignore(&self) -> Result<(), TraceError> {
        let gitignore_path = self.dir.parent().unwrap().join(".gitignore");
        let entries = ["traces/", "trace_index.db"];

        let mut existing = if gitignore_path.exists() {
            fs::read_to_string(&gitignore_path).unwrap_or_default()
        } else {
            String::new()
        };

        let mut changed = false;
        for entry in &entries {
            if !existing.lines().any(|l| l.trim() == *entry) {
                existing.push_str(entry);
                existing.push('\n');
                changed = true;
            }
        }

        if changed {
            fs::write(&gitignore_path, existing).map_err(|e| TraceError::Io {
                message: format!("Failed to write .gitignore: {}", e),
            })?;
        }

        Ok(())
    }

    /// 打开或创建 SQLite 连接。
    fn open_db(&self) -> Result<Connection, TraceError> {
        Connection::open(&self.db_path).map_err(|e| TraceError::Database {
            message: format!("Failed to open SQLite database {}: {}", self.db_path.display(), e),
        })
    }

    // ─── 写入 ─────────────────────────────────────────────────

    /// 保存单条 BehaviorTrace。
    ///
    /// 执行:
    ///   1. 分配 trace ID（如果为空）
    ///   2. 追加一行到当前 JSONL 文件
    ///   3. 记录 byte_offset
    ///   4. 插入 SQLite 索引行
    ///
    /// JSONL 和 SQLite 在同一事务语义下写入（best effort）。
    ///
    /// # 返回
    ///
    /// 保存的 JSONL 文件路径。
    pub fn save(&self, trace: &BehaviorTrace) -> Result<PathBuf, TraceError> {
        self.init()?;

        let mut trace = trace.clone();

        // 分配 ID
        if trace.id.is_empty() {
            trace.id = self.next_id()?;
        }

        // 获取当前活动的 JSONL 文件（不存在或超过 100MB 则创建新文件）
        let jsonl_path = self.current_jsonl_file()?;

        // 序列化为单行 JSON
        let json_line = serde_json::to_string(&trace).map_err(|e| TraceError::Serialize {
            message: format!("Failed to serialize trace {}: {}", trace.id, e),
        })?;

        // 追加写入 JSONL
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&jsonl_path)
            .map_err(|e| TraceError::Io {
                message: format!("Failed to open {}: {}", jsonl_path.display(), e),
            })?;

        // 记录写入前的文件大小作为 byte_offset
        let byte_offset = file.metadata().map(|m| m.len()).unwrap_or(0);

        writeln!(file, "{}", json_line).map_err(|e| TraceError::Io {
            message: format!("Failed to write to {}: {}", jsonl_path.display(), e),
        })?;

        // 写入 SQLite 索引
        self.insert_index(&trace, &jsonl_path, byte_offset)?;

        Ok(jsonl_path)
    }

    /// 批量保存多条 BehaviorTrace。
    ///
    /// 返回 ImportResult。
    pub fn save_batch(&self, traces: Vec<BehaviorTrace>) -> Result<ImportResult, TraceError> {
        let mut imported = Vec::new();
        let mut skipped = 0usize;
        let mut skip_reasons = Vec::new();

        let total = traces.len();

        for trace in traces {
            match self.save(&trace) {
                Ok(_) => {
                    imported.push(self.trace_to_summary(&trace));
                }
                Err(e) => {
                    skipped += 1;
                    skip_reasons.push(format!("{}: {}", trace.id, e));
                }
            }
        }

        Ok(ImportResult {
            source_path: String::new(), // 由上层填充
            adapter: String::new(),
            auto_detected: false,
            imported,
            skipped_count: skipped,
            skip_reasons,
        })
    }

    /// 将 BehaviorTrace 插入 SQLite 索引。
    fn insert_index(&self, trace: &BehaviorTrace, file_path: &Path, byte_offset: u64) -> Result<(), TraceError> {
        let conn = self.open_db()?;

        let tool_names: HashSet<&str> = trace.tool_calls.iter()
            .map(|tc| tc.tool_name.as_str())
            .collect();
        let tool_names_json = serde_json::to_string(&tool_names.iter().collect::<Vec<_>>())
            .unwrap_or_else(|_| "[]".to_string());

        let prompt_preview = if trace.prompt.len() > 200 {
            format!("{}...", &trace.prompt[..200])
        } else {
            trace.prompt.clone()
        };

        let duration_ms = self.calc_duration_ms(&trace.started_at, &trace.finished_at);

        let source_type = match &trace.source {
            crate::trace::types::TraceSource::Imported { .. } => "imported",
            crate::trace::types::TraceSource::Captured { .. } => "captured",
        };

        let adapter_name = match &trace.source {
            crate::trace::types::TraceSource::Imported { adapter, .. } => Some(adapter.as_str()),
            _ => None,
        };

        let capability_ids_json = serde_json::to_string(&trace.capability_ids)
            .unwrap_or_else(|_| "[]".to_string());
        let tags_json = serde_json::to_string(&trace.tags)
            .unwrap_or_else(|_| "[]".to_string());

        conn.execute(
            "INSERT OR REPLACE INTO traces
                (id, session_id, tool_names, prompt_preview, started_at, finished_at,
                 duration_ms, input_tokens, output_tokens, source_type, adapter_name,
                 file_path, byte_offset, capability_ids, tags, deleted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            rusqlite::params![
                trace.id,
                trace.session_id,
                tool_names_json,
                prompt_preview,
                trace.started_at,
                trace.finished_at,
                duration_ms,
                trace.token_usage.input_tokens,
                trace.token_usage.output_tokens,
                source_type,
                adapter_name,
                file_path.to_string_lossy().to_string(),
                byte_offset as i64,
                capability_ids_json,
                tags_json,
                if trace.deleted { 1 } else { 0 },
            ],
        )
        .map_err(|e| TraceError::Database {
            message: format!("Failed to insert trace index for {}: {}", trace.id, e),
        })?;

        Ok(())
    }

    // ─── 读取 ─────────────────────────────────────────────────

    /// 按 ID 加载单条 trace 完整内容。
    ///
    /// 先从 SQLite 查 byte_offset，再到 JSONL 文件中 seek 读取对应行。
    pub fn load(&self, id: &str) -> Result<BehaviorTrace, TraceError> {
        let conn = self.open_db()?;

        let (file_path_str, byte_offset): (String, i64) = conn
            .query_row(
                "SELECT file_path, byte_offset FROM traces WHERE id = ?1 AND deleted = 0",
                rusqlite::params![id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(|e| TraceError::NotFound {
                message: format!("Trace {} not found: {}", id, e),
            })?;

        let file_path = Path::new(&file_path_str);
        let content = fs::read_to_string(file_path).map_err(|e| TraceError::Io {
            message: format!("Failed to read {}: {}", file_path_str, e),
        })?;

        // 定位到 byte_offset 所在行
        let target_line = &content[byte_offset as usize..];
        let line = target_line.lines().next().ok_or_else(|| TraceError::NotFound {
            message: format!("Trace {} data not found at offset {} in {}", id, byte_offset, file_path_str),
        })?;

        serde_json::from_str(line).map_err(|e| TraceError::Serialize {
            message: format!("Failed to parse trace {}: {}", id, e),
        })
    }

    /// 按 session_id 加载该 session 的所有 trace。
    pub fn load_by_session(&self, session_id: &str) -> Result<Vec<BehaviorTrace>, TraceError> {
        let conn = self.open_db()?;

        let mut stmt = conn
            .prepare(
                "SELECT file_path, byte_offset
                 FROM traces
                 WHERE session_id = ?1 AND deleted = 0
                 ORDER BY started_at ASC",
            )
            .map_err(|e| TraceError::Database {
                message: format!("Failed to query traces by session: {}", e),
            })?;

        let rows: Vec<(String, i64)> = stmt
            .query_map(rusqlite::params![session_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|e| TraceError::Database {
                message: format!("Failed to read trace rows: {}", e),
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut traces = Vec::new();
        for (file_path_str, byte_offset) in &rows {
            let content = fs::read_to_string(file_path_str).map_err(|e| TraceError::Io {
                message: format!("Failed to read {}: {}", file_path_str, e),
            })?;
            let target_line = &content[*byte_offset as usize..];
            if let Some(line) = target_line.lines().next() {
                if let Ok(trace) = serde_json::from_str::<BehaviorTrace>(line) {
                    traces.push(trace);
                }
            }
        }

        Ok(traces)
    }

    /// 按过滤条件列出 trace 摘要。
    pub fn list(&self, filter: &TraceFilter) -> Result<Vec<TraceSummary>, TraceError> {
        let conn = self.open_db()?;

        let mut sql = String::from(
            "SELECT id, session_id, tool_names, prompt_preview, started_at, finished_at,
                    duration_ms, input_tokens, output_tokens, source_type, adapter_name,
                    capability_ids, tags, deleted
             FROM traces WHERE 1=1",
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if !filter.include_deleted {
            sql.push_str(" AND deleted = 0");
        }

        if let Some(ref sid) = filter.session_id {
            sql.push_str(" AND session_id = ?");
            params.push(Box::new(sid.clone()));
        }

        if let Some(ref tool) = filter.tool_name {
            sql.push_str(" AND tool_names LIKE ?");
            params.push(Box::new(format!("%\"{}\"%", tool)));
        }

        if let Some(ref tag) = filter.tag {
            sql.push_str(" AND tags LIKE ?");
            params.push(Box::new(format!("%\"{}\"%", tag)));
        }

        if let Some(ref cap_id) = filter.capability_id {
            sql.push_str(" AND capability_ids LIKE ?");
            params.push(Box::new(format!("%\"{}\"%", cap_id)));
        }

        if let Some(ref from) = filter.from_date {
            sql.push_str(" AND started_at >= ?");
            params.push(Box::new(from.clone()));
        }

        if let Some(ref to) = filter.to_date {
            sql.push_str(" AND started_at <= ?");
            params.push(Box::new(format!("{}T23:59:59Z", to)));
        }

        if let Some(ref st) = filter.source_type {
            sql.push_str(" AND source_type = ?");
            params.push(Box::new(st.clone()));
        }

        sql.push_str(" ORDER BY started_at DESC");

        if filter.limit > 0 {
            sql.push_str(&format!(" LIMIT {}", filter.limit));
        }
        if filter.offset > 0 {
            sql.push_str(&format!(" OFFSET {}", filter.offset));
        }

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql).map_err(|e| TraceError::Database {
            message: format!("Failed to prepare trace list query: {}", e),
        })?;

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                let tool_names_json: String = row.get(2)?;
                let tool_names: Vec<String> =
                    serde_json::from_str(&tool_names_json).unwrap_or_default();

                let caps_json: String = row.get(11)?;
                let caps: Vec<String> =
                    serde_json::from_str(&caps_json).unwrap_or_default();

                let tags_json: String = row.get(12)?;
                let tags: Vec<String> =
                    serde_json::from_str(&tags_json).unwrap_or_default();

                Ok(TraceSummary {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    tool_names,
                    prompt_preview: row.get::<_, String>(3).unwrap_or_default(),
                    tool_call_count: 0, // 不存数字，只存了名字列表
                    total_tokens: row.get::<_, u64>(6)? + row.get::<_, u64>(7)?,
                    started_at: row.get(4)?,
                    finished_at: row.get(5)?,
                    duration_ms: row.get::<_, i64>(6).unwrap_or(0) as u64,
                    source_type: row.get(9)?,
                    adapter_name: row.get::<_, Option<String>>(10).unwrap_or(None),
                    capability_count: caps.len(),
                    tags,
                    deleted: row.get::<_, i32>(13).unwrap_or(0) != 0,
                })
            })
            .map_err(|e| TraceError::Database {
                message: format!("Failed to query trace list: {}", e),
            })?;

        let summaries: Result<Vec<_>, _> = rows.collect();
        summaries.map_err(|e| TraceError::Database {
            message: format!("Failed to collect trace summaries: {}", e),
        })
    }

    // ─── 删除 ─────────────────────────────────────────────────

    /// Soft delete 一条 trace。
    ///
    /// 在 SQLite 索引中标记 deleted=1。
    /// JSONL 文件不做修改（不物理删除行）。
    /// 被标记的 trace 在 list/load 中默认不可见（除非 include_deleted=true）。
    pub fn delete(&self, id: &str) -> Result<(), TraceError> {
        let conn = self.open_db()?;

        let affected = conn
            .execute(
                "UPDATE traces SET deleted = 1 WHERE id = ?1",
                rusqlite::params![id],
            )
            .map_err(|e| TraceError::Database {
                message: format!("Failed to delete trace {}: {}", id, e),
            })?;

        if affected == 0 {
            return Err(TraceError::NotFound {
                message: format!("Trace {} not found for deletion", id),
            });
        }

        Ok(())
    }

    // ─── 实用方法 ─────────────────────────────────────────────

    /// 生成下一个 trace ID。
    ///
    /// 格式: "trace_20260612_001"
    pub fn next_id(&self) -> Result<String, TraceError> {
        let conn = self.open_db()?;

        let date = chrono_now_date(); // "2026-06-12"
        let date_no_dash = date.replace('-', "");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM traces WHERE id LIKE ?1",
                rusqlite::params![format!("trace_{}___", date_no_dash)],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(format!("trace_{}_{:03}", date_no_dash, count + 1))
    }

    /// 获取当前活动的 JSONL 文件路径。
    ///
    /// 如果当天最早序号的 JSONL 文件超过 100MB，则创建下一个序号的文件。
    fn current_jsonl_file(&self) -> Result<PathBuf, TraceError> {
        let date = chrono_now_date();
        let mut seq = 1u32;

        loop {
            let path = self.dir.join(format!("{}-{:03}.jsonl", date, seq));
            if !path.exists() {
                return Ok(path);
            }

            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if size < 100 * 1024 * 1024 {
                // 100 MB
                return Ok(path);
            }

            seq += 1;
            if seq > 999 {
                return Err(TraceError::Io {
                    message: format!("Too many JSONL files for date {}", date),
                });
            }
        }
    }

    /// 计算 started_at 和 finished_at 之间的毫秒差。
    fn calc_duration_ms(&self, started_at: &str, finished_at: &str) -> u64 {
        // 简化实现，使用 chrono 库
        0 // 真实实现需 chrono::DateTime::parse_from_rfc3339
    }

    /// 将 BehaviorTrace 转为 TraceSummary。
    fn trace_to_summary(&self, trace: &BehaviorTrace) -> TraceSummary {
        let tool_names: Vec<String> = trace
            .tool_calls
            .iter()
            .map(|tc| tc.tool_name.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let prompt_preview = if trace.prompt.len() > 200 {
            format!("{}...", &trace.prompt[..200])
        } else {
            trace.prompt.clone()
        };

        let source_type = match &trace.source {
            TraceSource::Imported { .. } => "imported".to_string(),
            TraceSource::Captured { .. } => "captured".to_string(),
        };

        let adapter_name = match &trace.source {
            TraceSource::Imported { adapter, .. } => Some(adapter.clone()),
            _ => None,
        };

        TraceSummary {
            id: trace.id.clone(),
            session_id: trace.session_id.clone(),
            prompt_preview,
            tool_names,
            tool_call_count: trace.tool_calls.len(),
            total_tokens: trace.token_usage.input_tokens + trace.token_usage.output_tokens,
            started_at: trace.started_at.clone(),
            finished_at: trace.finished_at.clone(),
            duration_ms: self.calc_duration_ms(&trace.started_at, &trace.finished_at),
            source_type,
            adapter_name,
            capability_count: trace.capability_ids.len(),
            tags: trace.tags.clone(),
            deleted: trace.deleted,
        }
    }
}

/// 获取当前日期字符串: "2026-06-12"
fn chrono_now_date() -> String {
    // 简化实现；实际依赖 chrono crate
    // chrono::Local::now().format("%Y-%m-%d").to_string()
    "2026-06-12".to_string()
}

// ─── 测试 ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_storage() -> (TraceStorage, TempDir) {
        let tmp = TempDir::new().unwrap();
        let storage = TraceStorage::new(tmp.path().join(".Paporot"));
        storage.init().unwrap();
        (storage, tmp)
    }

    fn create_test_trace(id: &str, session_id: &str) -> BehaviorTrace {
        BehaviorTrace {
            id: id.to_string(),
            session_id: session_id.to_string(),
            prompt: "Test prompt".to_string(),
            tool_calls: Vec::new(),
            observations: Vec::new(),
            final_output: "Test output".to_string(),
            token_usage: Default::default(),
            started_at: "2026-06-12T14:00:00Z".to_string(),
            finished_at: "2026-06-12T14:01:00Z".to_string(),
            source: crate::trace::types::TraceSource::Captured {
                agent_name: "test-agent".to_string(),
            },
            tags: Vec::new(),
            capability_ids: Vec::new(),
            deleted: false,
        }
    }

    #[test]
    fn test_save_and_load() {
        let (storage, _tmp) = create_test_storage();
        let trace = create_test_trace("trace_20260612_001", "sess-001");
        let path = storage.save(&trace).unwrap();
        assert!(path.exists());

        let loaded = storage.load("trace_20260612_001").unwrap();
        assert_eq!(loaded.session_id, "sess-001");
        assert_eq!(loaded.prompt, "Test prompt");
    }

    #[test]
    fn test_list_with_filter() {
        let (storage, _tmp) = create_test_storage();
        storage.save(&create_test_trace("trace_20260612_001", "sess-a")).unwrap();
        storage.save(&create_test_trace("trace_20260612_002", "sess-b")).unwrap();

        let all = storage.list(&TraceFilter::default()).unwrap();
        assert_eq!(all.len(), 2);

        let filtered = storage.list(&TraceFilter {
            session_id: Some("sess-a".into()),
            ..Default::default()
        }).unwrap();
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_soft_delete() {
        let (storage, _tmp) = create_test_storage();
        storage.save(&create_test_trace("trace_20260612_001", "sess-a")).unwrap();
        storage.delete("trace_20260612_001").unwrap();

        // 默认不返回已删除的
        let all = storage.list(&TraceFilter::default()).unwrap();
        assert!(all.is_empty());

        // include_deleted 可以看到
        let all = storage.list(&TraceFilter {
            include_deleted: true,
            ..Default::default()
        }).unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].deleted);
    }

    #[test]
    fn test_next_id() {
        let (storage, _tmp) = create_test_storage();
        let id = storage.next_id().unwrap();
        assert!(id.starts_with("trace_"));
    }
}
```

---

## 8. CLI 子命令（接口级）

### 8.1 文件: `src/commands/trace.rs`

```rust
//! `paporot trace` 子命令实现。
//!
//! 子命令:
//!   import  <file> [--adapter <name>]
//!   list    [--session <id>] [--tool <name>] [--tag <tag>]
//!           [--from <date>] [--to <date>] [--limit <n>]
//!   show    <trace-id> [--format json|summary]
//!   delete  <trace-id>
//!   link    <trace-id> --cap <cap-id>
//!   unlink  <trace-id> --cap <cap-id>
//!   redact  <trace-id> [--rules <config>]
//!   adapter list

use anyhow::Context;
use crate::trace::adapter;
use crate::trace::error::TraceError;
use crate::trace::storage::TraceStorage;
use crate::trace::types::{BehaviorTrace, ImportResult, RedactConfig, TraceFilter, TraceSummary};

// ─── import ────────────────────────────────────────────────────

pub fn cmd_import(
    storage: &TraceStorage,
    file_path: &str,
    adapter_name: Option<&str>,
) -> anyhow::Result<ImportResult> {
    let raw = std::fs::read_to_string(file_path)
        .context("Failed to read input file")?;

    let adapter = if let Some(name) = adapter_name {
        adapter::find_adapter(name)
            .ok_or_else(|| anyhow::anyhow!("Unknown adapter: {}. Use 'paporot trace adapter list'", name))?
    } else {
        adapter::auto_detect(&raw)
            .ok_or_else(|| anyhow::anyhow!("Could not auto-detect format. Specify --adapter"))?
    };

    let traces = match adapter.parse(&raw, file_path) {
        Ok(t) => t,
        Err(TraceError::PartialImport { imported, skipped, reasons }) => {
            eprintln!("  [WARN] Partial import: {} imported, {} skipped", imported, skipped);
            for r in &reasons {
                eprintln!("         {}", r);
            }
            Vec::new() // PartialImport 错误时 traces 由上层处理
        }
        Err(e) => return Err(anyhow::anyhow!("{}", e)),
    };

    // ── 处理 PartialImport 重试 + 实际保存 ──
    let traces = match adapter.parse(&raw, file_path) {
        Ok(t) => t,
        Err(TraceError::PartialImport { imported, skipped, reasons }) => {
            eprintln!("  [WARN] {} traces imported, {} skipped.", imported, skipped);
            for reason in &reasons {
                eprintln!("         {}", reason);
            }
            // 需要重新解析获取成功的那些
            adapter.parse(&raw, file_path)
                .unwrap_or_else(|_| Vec::new())
        }
        Err(e) => return Err(anyhow::anyhow!("Parse error: {}", e)),
    };

    if traces.is_empty() {
        anyhow::bail!("No valid traces found in {}", file_path);
    }

    let result = storage.save_batch(traces)?;

    Ok(ImportResult {
        source_path: file_path.to_string(),
        adapter: adapter.name().to_string(),
        auto_detected: adapter_name.is_none(),
        ..result
    })
}

// ─── list ──────────────────────────────────────────────────────

pub fn cmd_list(storage: &TraceStorage, filter: TraceFilter) -> anyhow::Result<Vec<TraceSummary>> {
    storage.list(&filter).map_err(|e| anyhow::anyhow!("{}", e))
}

// ─── show ──────────────────────────────────────────────────────

pub enum ShowFormat {
    Summary,
    Json,
    Full,
}

pub fn cmd_show(storage: &TraceStorage, trace_id: &str, format: ShowFormat) -> anyhow::Result<()> {
    let trace = storage.load(trace_id)?;

    match format {
        ShowFormat::Summary => {
            let summary = storage.list(&TraceFilter {
                session_id: Some(trace.session_id.clone()),
                limit: 1,
                ..Default::default()
            })?.into_iter().next();

            if let Some(s) = summary {
                print_summary(&s);
            }
        }
        ShowFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&trace)?);
        }
        ShowFormat::Full => {
            print_trace_full(&trace);
        }
    }
    Ok(())
}

fn print_summary(s: &TraceSummary) {
    println!("Trace: {}", s.id);
    println!("  Session    : {}", s.session_id);
    println!("  Prompt     : {}", s.prompt_preview);
    println!("  Tools      : {} ({})", s.tool_names.join(", "), s.tool_call_count);
    println!("  Tokens     : {} (in: {}, out: {})", s.total_tokens, 0u64, 0u64);
    println!("  Duration   : {}ms", s.duration_ms);
    println!("  Source     : {} ({})", s.source_type, s.adapter_name.as_deref().unwrap_or("n/a"));
    println!("  Capabilities: {}", s.capability_count);
    println!("  Tags       : {}", if s.tags.is_empty() { "-".into() } else { s.tags.join(", ") });
}

fn print_trace_full(trace: &BehaviorTrace) {
    println!("Trace: {}", trace.id);
    println!("  Session     : {}", trace.session_id);
    println!("  Prompt      : {}", trace.prompt);
    println!("  Started     : {}", trace.started_at);
    println!("  Finished    : {}", trace.finished_at);
    println!("  Token Usage : in={}, out={}, cache_read={:?}, cache_write={:?}",
        trace.token_usage.input_tokens,
        trace.token_usage.output_tokens,
        trace.token_usage.cache_read_tokens,
        trace.token_usage.cache_write_tokens,
    );
    println!("  Source      : {:?}", trace.source);
    println!("  Tags        : {:?}", trace.tags);
    println!("  Capabilities: {:?}", trace.capability_ids);
    println!("  Deleted     : {}", trace.deleted);
    println!("  ── Tool Calls ({}) ──", trace.tool_calls.len());
    for tc in &trace.tool_calls {
        println!("    [{}] {} @ {} ({}ms)",
            tc.id, tc.tool_name, tc.timestamp, tc.duration_ms);
        println!("      args: {}", serde_json::to_string(&tc.args).unwrap_or_default());
        println!("      result_id: {:?}", tc.result_id);
    }
    println!("  ── Observations ({}) ──", trace.observations.len());
    for obs in &trace.observations {
        let preview = if obs.content.len() > 200 {
            format!("{}...[truncated:{}]", &obs.content[..200], obs.truncated_at_bytes.unwrap_or(0))
        } else {
            obs.content.clone()
        };
        println!("    [{}] <- {}: {}", obs.id, obs.tool_call_id, preview);
    }
    println!("  ── Final Output ──");
    println!("  {}", trace.final_output);
}

// ─── delete ────────────────────────────────────────────────────

pub fn cmd_delete(storage: &TraceStorage, trace_id: &str) -> anyhow::Result<()> {
    storage.delete(trace_id).map_err(|e| anyhow::anyhow!("{}", e))
}

// ─── link / unlink ─────────────────────────────────────────────

pub fn cmd_link(storage: &TraceStorage, trace_id: &str, cap_id: &str) -> anyhow::Result<()> {
    let mut trace = storage.load(trace_id)?;
    if !trace.capability_ids.contains(&cap_id.to_string()) {
        trace.capability_ids.push(cap_id.to_string());
    }
    storage.save(&trace)?;
    Ok(())
}

pub fn cmd_unlink(storage: &TraceStorage, trace_id: &str, cap_id: &str) -> anyhow::Result<()> {
    let mut trace = storage.load(trace_id)?;
    trace.capability_ids.retain(|c| c != cap_id);
    storage.save(&trace)?;
    Ok(())
}

// ─── redact ────────────────────────────────────────────────────

pub fn cmd_redact(
    storage: &TraceStorage,
    trace_id: &str,
    config: &RedactConfig,
) -> anyhow::Result<()> {
    let mut trace = storage.load(trace_id)?;
    apply_redact(&mut trace, config);
    storage.save(&trace)?;
    Ok(())
}

fn apply_redact(trace: &mut BehaviorTrace, config: &RedactConfig) {
    // 脱敏 prompt
    if config.redact_api_keys {
        trace.prompt = redact_pattern(&trace.prompt, "api_key", "***");
        trace.prompt = redact_pattern(&trace.prompt, "apikey", "***");
    }
    if config.redact_auth_header {
        trace.prompt = redact_pattern(&trace.prompt, "Authorization", "***");
        trace.prompt = redact_pattern(&trace.prompt, "Bearer", "***");
    }

    // 脱敏 tool_calls 和 observations
    for tc in &mut trace.tool_calls {
        if let Some(s) = tc.args.as_str() {
            let redacted = redact_value(s, config);
            tc.args = serde_json::Value::String(redacted);
        }
    }
    for obs in &mut trace.observations {
        obs.content = redact_value(&obs.content, config);
    }

    // 自定义规则
    for (pattern, replacement) in &config.custom_rules {
        trace.prompt = trace.prompt.replace(pattern.as_str(), replacement.as_str());
    }
}

fn redact_pattern(text: &str, keyword: &str, replacement: &str) -> String {
    text.replace(keyword, replacement)
}

fn redact_value(value: &str, _config: &RedactConfig) -> String {
    value.to_string() // stub
}

// ─── adapter list ──────────────────────────────────────────────

pub fn cmd_adapter_list() -> anyhow::Result<Vec<adapter::AdapterInfo>> {
    Ok(adapter::list_adapters())
}
```

### 8.2 文件: `src/cli.rs` 新增部分

在 `Commands` 枚举中新增：

```rust
/// 执行轨迹管理
Trace {
    #[command(subcommand)]
    action: TraceAction,
},
```

新增子命令枚举：

```rust
#[derive(Subcommand, Debug)]
pub enum TraceAction {
    /// 从外部文件导入 trace
    Import {
        /// 输入文件或目录路径
        file: String,
        /// 指定适配器（可选，不指定则自动检测）
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
        format: ShowFormatArg,
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

#[derive(Clone, Debug)]
pub enum ShowFormatArg {
    Full,
    Json,
    Summary,
}

impl std::str::FromStr for ShowFormatArg {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "full" => Ok(ShowFormatArg::Full),
            "json" => Ok(ShowFormatArg::Json),
            "summary" => Ok(ShowFormatArg::Summary),
            _ => Err(format!("Unknown format: {}. Valid: full, json, summary", s)),
        }
    }
}

impl std::fmt::Display for ShowFormatArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShowFormatArg::Full => write!(f, "full"),
            ShowFormatArg::Json => write!(f, "json"),
            ShowFormatArg::Summary => write!(f, "summary"),
        }
    }
}
```

### 8.3 CLI 入口对接 (`src/main.rs`)

在 `Commands::Trace { ref action }` 分支中：

```rust
Commands::Trace { action } => {
    let storage = crate::trace::storage::TraceStorage::new(
        std::path::Path::new(".Paporot")
    );
    storage.init()?;

    match action {
        TraceAction::Import { file, adapter } => {
            let result = crate::commands::trace::cmd_import(
                &storage, file, adapter.as_deref()
            )?;
            println!("Paporot Trace Import");
            println!("  source       : {}", result.source_path);
            println!("  adapter      : {}{}",
                result.adapter,
                if result.auto_detected { " (auto-detected)" } else { "" }
            );
            println!("  traces       : {} imported, {} skipped",
                result.imported.len(), result.skipped_count);
            for s in result.skip_reasons.iter().take(5) {
                eprintln!("  [skip] {}", s);
            }
            println!("  ── Imported ──");
            for t in &result.imported {
                println!("  {}  prompt: \"{}\"  tools: {}  tokens: {}",
                    t.id, t.prompt_preview, t.tool_call_count, t.total_tokens);
            }
        }
        TraceAction::List { session, tool, tag, capability, from, to, limit, offset } => {
            let filter = TraceFilter {
                session_id: session,
                tool_name: tool,
                tag,
                capability_id: capability,
                from_date: from,
                to_date: to,
                limit,
                offset,
                ..Default::default()
            };
            let results = crate::commands::trace::cmd_list(&storage, filter)?;
            if results.is_empty() {
                println!("No traces found.");
            } else {
                for s in &results {
                    println!("{}  session: {}  tools: {}  tokens: {}  {}",
                        s.id, s.session_id, s.tool_names.join(","), s.total_tokens, s.started_at);
                }
            }
        }
        TraceAction::Show { trace_id, format } => {
            let fmt = match format {
                ShowFormatArg::Full => ShowFormat::Full,
                ShowFormatArg::Json => ShowFormat::Json,
                ShowFormatArg::Summary => ShowFormat::Summary,
            };
            crate::commands::trace::cmd_show(&storage, &trace_id, fmt)?;
        }
        TraceAction::Delete { trace_id } => {
            crate::commands::trace::cmd_delete(&storage, &trace_id)?;
            println!("Trace {} deleted (soft delete)", trace_id);
        }
        TraceAction::Link { trace_id, cap } => {
            crate::commands::trace::cmd_link(&storage, &trace_id, &cap)?;
            println!("Linked trace {} -> capability {}", trace_id, cap);
        }
        TraceAction::Unlink { trace_id, cap } => {
            crate::commands::trace::cmd_unlink(&storage, &trace_id, &cap)?;
            println!("Unlinked trace {} -> capability {}", trace_id, cap);
        }
        TraceAction::Redact { trace_id } => {
            let config = RedactConfig::default();
            crate::commands::trace::cmd_redact(&storage, &trace_id, &config)?;
            println!("Trace {} redacted", trace_id);
        }
        TraceAction::Adapter { action: AdapterAction::List } => {
            let adapters = crate::commands::trace::cmd_adapter_list()?;
            for a in &adapters {
                println!("  {:<16} v{:<8} {}", a.name, a.version, a.description);
            }
        }
    }
    Ok(())
}
```

---

## 9. Capability 弱关联（接口级）

### 9.1 文件: `src/types.rs` —— Capability 新增字段

```rust
/// 关联的 Execution Trace ID 列表（弱关联，可选）。
///
/// 由用户通过 `paporot trace link` 手动建立，
/// 或未来由 Behavior Eval 模块自动填充。
/// 不影响 snapshot / diff / coverage 逻辑。
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub evidence_trace_ids: Vec<String>,
```

### 9.2 行为规范

- `capability_ids`（在 BehaviorTrace 侧）与 `evidence_trace_ids`（在 Capability 侧）是双向弱关联
- 通过 `paporot trace link` 同时写入两边
- snapshot create / diff / coverage / regression / risk 命令**不读取也不使用**此字段
- 未来 Behavior Eval 模块可通过此字段反向查找 trace

---

## 10. 错误类型

### 10.1 文件: `src/trace/error.rs`

```rust
//! Trace 模块错误类型。

use std::fmt;

/// Trace 模块的所有错误。
#[derive(Debug)]
pub enum TraceError {
    /// I/O 错误
    Io {
        message: String,
    },

    /// SQLite 数据库错误
    Database {
        message: String,
    },

    /// 解析错误
    ParseError {
        message: String,
        adapter: String,
    },

    /// 部分导入成功
    PartialImport {
        imported: usize,
        skipped: usize,
        reasons: Vec<String>,
    },

    /// trace 未找到
    NotFound {
        message: String,
    },

    /// 序列化/反序列化错误
    Serialize {
        message: String,
    },

    /// 不支持的格式
    UnsupportedFormat {
        format: String,
        adapter: String,
    },
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TraceError::Io { message } => write!(f, "I/O error: {}", message),
            TraceError::Database { message } => write!(f, "Database error: {}", message),
            TraceError::ParseError { message, adapter } => {
                write!(f, "Parse error [{}]: {}", adapter, message)
            }
            TraceError::PartialImport { imported, skipped, reasons } => {
                write!(f, "Partial import: {} ok, {} skipped. Reasons: {}",
                    imported, skipped, reasons.join("; "))
            }
            TraceError::NotFound { message } => write!(f, "Not found: {}", message),
            TraceError::Serialize { message } => write!(f, "Serialize error: {}", message),
            TraceError::UnsupportedFormat { format, adapter } => {
                write!(f, "Unsupported format '{}' for adapter '{}'", format, adapter)
            }
        }
    }
}

impl std::error::Error for TraceError {}
```

---

## 11. 非功能性需求

### 11.1 性能

| 指标 | 目标 | 测量方法 |
|------|------|---------|
| 单文件上限 | 100 MB | `current_jsonl_file()` 检测文件大小 |
| 单条 trace 导入耗时 | < 500ms | `save()` 不含适配器解析时间 |
| SQLite 插入耗时 | < 10ms / 条 | `insert_index()` wall-clock |
| 列表查询（1000 条） | < 100ms | `list()` 含 SQLite 查询 + 反序列化 |
| 单条详情读取 | 1 次 SQLite seek + 1 次文件 seek | `load()` 两步定位 |
| 适配器 `can_handle` | < 1ms | 只读前 4096 字节 |

### 11.2 规模

| 指标 | 目标 |
|------|------|
| 单项目 trace 数量 | 100–1000 条 |
| JSONL 文件数 | 按日期分片，≤ 999 个/天 |
| SQLite 数据库 | ≤ 10MB（1000 条 trace） |

### 11.3 安全与隐私

- 不自动脱敏；`paporot trace redact` 手动触发
- 默认脱敏: `Authorization: Bearer ***`, `api_key=***`
- trace 文件通过 `.Paporot/.gitignore` 排除

### 11.4 可靠性

- 记录失败不阻断 Agent 执行
- 损坏 JSONL 行 → skip + stderr 告警
- SQLite 写入失败 → 告警但不阻断 JSONL 写入（索引可重建）

### 11.5 兼容性

- 不修改现有 `snapshot/diff/coverage/regression/risk/graph/feedback/testmap`
- `BehaviorTrace` 新增字段使用 `#[serde(default)]`

---

## 12. 实现计划

### 12.1 阶段与文件清单

```
Phase 1: 数据模型 + 存储
  [NEW] src/trace/mod.rs
  [NEW] src/trace/types.rs        (~250 lines)
  [NEW] src/trace/error.rs        (~50 lines)
  [NEW] src/trace/storage.rs      (~350 lines)
  [MOD] src/types.rs              (+1 field: evidence_trace_ids)
  [MOD] Cargo.toml                (+ rusqlite, + chrono)

Phase 2: DeepSeek 适配器
  [NEW] src/trace/adapter.rs      (~120 lines)
  [NEW] src/trace/adapters/mod.rs
  [NEW] src/trace/adapters/deepseek_types.rs  (~80 lines)
  [NEW] src/trace/adapters/deepseek.rs        (~200 lines)

Phase 3: CLI 子命令
  [NEW] src/commands/trace.rs     (~250 lines)
  [MOD] src/cli.rs                (+ TraceAction, AdapterAction enums)
  [MOD] src/lib.rs                (+ pub mod trace;)
  [MOD] src/main.rs               (+ Commands::Trace 分支)
```

### 12.2 新增依赖

```toml
[dependencies]
rusqlite = { version = "0.31", features = ["bundled"] }
chrono = "0.4"
tempfile = "3"  # dev-dependencies for tests
```

---

## 13. 测试策略

### 13.1 单元测试

| 模块 | 测试文件 | 关键用例 |
|------|---------|---------|
| `types.rs` | 内联 `#[cfg(test)]` | serde 往返序列化；`TraceFilter::default()`；JSON 兼容性 |
| `storage.rs` | 内联 `#[cfg(test)]` | save/load 往返；list 过滤；soft delete；next_id 格式 |
| `adapter.rs` | 内联 `#[cfg(test)]` | `all_adapters()` 非空；`find_adapter` 大小写；`auto_detect` |
| `deepseek.rs` | 内联 `#[cfg(test)]` | `can_handle` JSONL/RunLog/unknown/empty；parse JSONL/RunLog |
| `error.rs` | 内联 `#[cfg(test)]` | `Display` 输出格式 |

### 13.2 集成测试

`tests/integration_tests.rs` 新增：

```rust
#[tokio::test]
async fn test_trace_import_and_list() {
    // 1. 用 fixture 创建 DeepSeek JSONL 文件
    // 2. import
    // 3. list 验证
}

#[tokio::test]
async fn test_trace_lifecycle() {
    // import → show → link → unlink → delete → list(include_deleted)
}
```

### 13.3 Fixture

`tests/fixtures/deepseek_sample.jsonl`:

```jsonl
{"id":"chatcmpl-001","choices":[{"message":{"role":"assistant","content":"Hello!","tool_calls":[{"id":"call_1","type":"function","function":{"name":"grep","arguments":"{\"pattern\": \"login\"}"}}]}}],"usage":{"prompt_tokens":120,"completion_tokens":45,"total_tokens":165},"created":1718000000}
```

---

## 14. 明确不做的事

- 不做 Trajectory Diff（下一个 PRD）
- 不做 Capability Evidence 自动填充（后续 PRD）
- 不做 Behavior Eval（后续 PRD）
- 不做 Claude Code / OpenAI 适配器（首个适配器仅 DeepSeek）
- 不做 `paporot trace record` wrapper 模式（Phase 4）
- 不做自动脱敏
- 不做 trace 可视化 / Web UI
- 不修改现有 `agent.rs` / `analysis/` / `snapshot/diff/coverage` 逻辑