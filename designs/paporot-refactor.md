# Paporot v0.4.0 重构设计文档

> **Paporot —— AI Coding Agent Behavior Evaluation Platform**
>
> Task 管执行，Paporot 管评估。
>
> 版本：v0.4.0 | 日期：2026-06-29

---

## 目录

1. [重构动机](#1-重构动机)
2. [PRD：产品需求文档](#2-prd产品需求文档)
3. [核心概念定义](#3-核心概念定义)
4. [整体架构](#4-整体架构)
5. [数据模型](#5-数据模型)
6. [CLI 接口](#6-cli-接口)
7. [WASM Skill 系统](#7-wasm-skill-系统)
8. [Host Functions](#8-host-functions)
9. [Grader 评分框架](#9-grader-评分框架)
10. [Dashboard 可视化](#10-dashboard-可视化)
11. [Trajectory 轨迹分析](#11-trajectory-轨迹分析)
12. [实现路线图](#12-实现路线图)
13. [从 v0.1.0 迁移](#13-从-v010-迁移)

---

## 1. 重构动机

### 1.1 v0.4.0 准备做成什么样

Paporot v0.4.0 的核心定位：**一个用户只需 `paporot init` 再配好 API Key，一条 `paporot analyze` 就能看懂"AI Agent 对我的项目做了什么"的工具。**

与 v0.1.0 最大的不同——v0.1.0 分析的是"代码变成什么样了"，v0.4.0 分析的是"Agent 这一次做了什么、项目因此发生了什么变化"。

#### 用户一天的使用流程

```
上午 10:00  Agent 提交了一轮代码（3 个 commit，改了 auth 和 middleware）
上午 10:05  用户敲 paporot analyze
上午 10:06  Dashboard 自动在浏览器打开
           ├── 首页大字：「认证模块架构升级」
           ├── 副标题：Agent 重构了密码验证流程并新增 MFA 支持
           ├── 一张大图：从 validate_password() 扩散到 auth → middleware → models
           └── 滚动到底：每个受影响模块的详细解读
上午 10:08  用户切换 Tab 到「能力全景」
           └── 力导向网络图，看到 auth 和 middleware 因为这次变更耦合加强了
上午 10:10  用户理解了这次变更的全貌，继续工作
```

#### v0.4.0 不做什么

- **不做实时 Agent 监控**：Paporot 消费已完成的 commit 和 Trace，不拦截 Agent 执行
- **不做代码质量 CI gate**：Grader 结果供参考，不会因为测试没通过阻止后续操作
- **不替代 Git**：Git 管代码行级差异，Paporot 管业务级的变更叙事
- **不深入 Agent 工程指标**：tool call 数量/token 消耗只在 Dashboard 侧栏出现，不作为首页主要内容

#### 对齐 Anthropic 定义

v0.1.0 的 `BehaviorSnapshot` 将"Agent 行为"定义为**代码变更的 Capability 契约**（函数签名的增删改）。Anthropic 在 *Demystifying evals for AI agents* (2026-01) 中明确定义了 Agent 评估的核心概念：

> A **transcript** (also called a trace or trajectory) is the complete record of a trial. The **outcome** is the final state in the environment. Grade what the agent *produced*, not the path it took.

v0.4.0 严格遵循这一定义重构核心数据模型——`EvalResult` = Task + Transcript + Outcome + CodeChange。

### 1.2 旧版存在的问题

v0.1.0 在设计上存在以下问题，驱动了本次重构：

**1. "行为"概念混淆**

v0.1.0 将"Agent 行为"定义为代码变更的 Capability 契约（函数签名的增删改），但同时又实现了 `trajectory/` 模块（P0/P1/P2）分析 Agent 的 tool call 序列。同一个词 "Behavior" 指代了两件事：

- **代码契约**（Skill 管道）：这次 commit 新增/修改/删除了哪些函数、API、结构体
- **执行模式**（Trajectory 分析）：Agent 用了多少 tool call、有没有循环重试、行为状态是否稳定

两者在代码中没有桥接，`Capability.evidence_trace_ids` 字段标注为 "P4 预埋" 但从未实现。

**2. 双路径架构未统一**

项目中存在两套并行的分析机制——WASM Skill 管道（`analyze` 命令）和 Native Agent 三层分析（`review` 命令）。两者的目标高度重叠，但实现完全不同，增加了维护成本和概念混乱。

**3. 核心逻辑不完整**

P2 行为耦合层（`cochange.rs`）中 `cooccur_count`、`cap_a_total`、`cap_b_total` 等关键变量声明但从未使用，实际上 P2 的耦合图计算是未完成的骨架代码。PRD Coverage Skill 目录只存在 `skill.toml` 空壳。

**4. 缺少 Task 概念**

整个系统围绕"代码变更"运行——从 git diff 提取符号变更，记录为 Capability 快照。但 Anthropic 的评估框架的核心锚点是 Task（一次有明确定义的开发任务）。没有 Task，就无法回答"Agent 这次成功了吗"这个最基本的问题。

**5. Skill 功能不对称**

6 个 Skill 中大部分是 JSON 聚合 + LLM 调用（如 `architecture-doc-generator`），几乎没有真正的确定性分析。L1 的 AST 分析、L2 的规则引擎在 Native 端已产出结果，但没有充分流入 Skill 管道。

**6. 工程规范缺失**

编译输出 23 个警告（未使用变量、dead_code 字段等），根目录散落临时文件（`changes.patch`、`final_test.txt`），`speech/` 演示目录混入项目代码库。

### 1.3 新旧对比

| 维度 | v0.1.0 | v0.4.0 |
|------|--------|--------|
| 核心对象 | `BehaviorSnapshot`（代码能力快照） | `EvalResult`（Task 执行评估） |
| "行为"定义 | 代码契约新增/修改/删除 | Agent 完成任务的表现 |
| 数据源 | Git diff | ExecutionTrace + Git diff |
| 评估维度 | L1 AST / L2 规则 / L3 LLM | Graders（确定性/静态/LLM Rubric） |
| 版本控制对象 | Capability 变更 | Task 通过率变化 |
| Skill 职责 | 分析代码架构 | 消费缓存做 LLM 质量评判 |
| Skill 安全 | 可读源文件 | 仅读 `.Paporot/cache/` |

---

## 2. PRD：产品需求文档

### 2.1 产品定位

Paporot 是一个 **AI Coding Agent 的行为评估与版本控制平台**。它回答三个问题：

1. **这次 Agent 完成了什么 Task？成功了吗？**（EvalResult）
2. **Agent 的行为模式变好还是变坏了？**（Trajectory 版本控制）
3. **不同 Task 之间，Agent 的表现趋势是什么？**（Dashboard 跨时间聚合）

### 2.2 核心用户场景

| 场景 | 做法 | 产出 |
|------|------|------|
| 刚配好环境，想看 Agent 干了什么 | `paporot analyze` | Dashboard 自动打开，变更叙事 + 能力全景 |
| Agent 刚完成一次 commit | `paporot eval auto` | Outcome 评分 + Agent 行为摘要 |
| 对比同一 Task 两次执行 | `paporot eval compare --task auth-bug --from v1 --to v2` | 行为退化/改进报告 |
| 查看系统能力全景 | `paporot dashboard` | 力导向网络图 + 能力趋势 |
| 自定义任务评测 | `paporot task new "修复认证"` + `paporot eval run --task my-auth-fix` | 自定义 Graders 评测 |
| 自动持续分析 | 配置 `.git/hooks/post-commit` 自动触发 | 每次 commit 自动评估 |

### 2.3 设计原则

1. **底层零耦合**：Timeline 事件溯源，不强加预定义关联模型
2. **上层按需聚合**：Capability 聚类、趋势分析均为查询产物，非持久化实体
3. **宿主做机械，Skill 做判断**：宿主提取数据 → `.Paporot/cache/` → Skill 消费
4. **安全默认**：Skill 不接触源码，用户可签发本地 Skill 扩展权限
5. **高摩擦时可选，低摩擦时自动**

### 2.4 功能需求（PRD Items）

| ID | 需求 | 优先级 |
|----|------|--------|
| P0-01 | Task 自动创建（git commit hook） | P0 |
| P0-02 | `paporot eval auto` — 自动评估单个 commit | P0 |
| P0-03 | 确定性 Grader：test pass/fail + lint 检查 | P0 |
| P0-04 | EvalResult 存储与查询 | P0 |
| P0-05 | `paporot eval compare` — Task 间对比 | P0 |
| P0-06 | Trajectory 分析（P0/P1/P2 保留并适配新数据模型） | P0 |
| P1-01 | LLM Rubric Grader（代码质量评判） | P1 |
| P1-02 | Dashboard HTML 可视化（左右分屏 + 网络图） | P1 |
| P1-03 | Capability 自动聚类（按模块分组） | P1 |
| P1-04 | 用户自定义 Task 定义与命名 | P1 |
| P2-01 | WASM Skill 社区签名机制 | P2 |
| P2-02 | CI/CD 集成（GitHub Actions 输出） | P2 |
| P2-03 | 多 Agent 会话对比分析 | P2 |

---

## 3. 核心概念定义

### 3.1 概念层次

```
Task（一次开发任务）
  └─ Trial（一次 Task 执行尝试）
       ├─ Transcript（Agent 的完整 tool call 记录）
       │    ├─ P0 状态机（行为阶段）
       │    ├─ P1 轨迹向量（10维行为特征）
       │    └─ P2 耦合图（跨能力行为关联）
       ├─ Outcome（最终环境状态 + Grader 判定）
       └─ CodeChange（代码变更摘要）
```

### 3.2 严格定义

| 术语 | 定义 | 来源 |
|------|------|------|
| **Task** | 一个有明确目标的一次开发任务。一个 commit 是原子 Task，多个 commit 可组成更大 Task | 用户定义 / git 自动 |
| **Trial** | 一个 Task 的一次执行。因 Agent 非确定性，同一 Task 可能多 Trial | Anthropic 定义 |
| **Transcript** (=Trace=Trajectory) | 一次 Trial 的完整 tool call 序列 + 推理过程 + 中间结果 | Anthropic 定义 |
| **Outcome** | Trial 结束时环境的最终状态 + Grader 判定结果 (PASS/FAIL/PARTIAL) | Anthropic 定义 |
| **Grader** | 评分器。三类：代码评分（确定性测试）、静态分析、LLM Rubric | Anthropic 定义 |
| **EvalResult** | 一次 Trial 的完整评估记录 = Task 引用 + Transcript + Outcome + CodeChange | Paporot 顶层对象 |
| **CodeChange** | Agent 产生的代码变更（文件列表、符号变更、diff 摘要）。是 Outcome 的辅助说明，不等同于"行为" | Paporot 定义 |
| **Capability** | 系统能力的聚合视图。不持久化，由 Task 按模块聚类动态生成 | Paporot 定义（查询层） |

### 3.3 Task 与 Capability 的关系

```
Task "commit abc123" 改了 src/auth/ 和 src/middleware/
  ├─ 出现在 Capability "auth" 的聚类视图下
  ├─ 出现在 Capability "middleware" 的聚类视图下
  └─ 两个 Capability 因共用 Task 而产生隐含耦合边
```

一个 Task 可以属于多个 Capability。Capability 不是持久化实体，是 Dashboard 查询层对 Task 的聚合视图。

---

## 4. 整体架构

### 4.1 架构图

```
┌─────────────────────────────────────────────────────────────────┐
│                          用户交互层                              │
│                                                                 │
│  CLI: paporot analyze | paporot eval | paporot task |           │
│       paporot dashboard | paporot state | paporot trajectory    │
│                                                                 │
│  Dashboard: 内嵌 Web 应用（axum + 原生 HTML/JS/D3.js）          │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  [变更叙事] [能力全景]               Paporot Dashboard    │   │
│  ├──────────┬──────────────────────────────────────────────┤   │
│  │ 侧边栏   │  主内容区                                     │   │
│  │          │  ┌ 大字报：标题故事 + 副标题 ─────────────┐   │   │
│  │ Task     │  │                                        │   │   │
│  │ 历史     │  └────────────────────────────────────────┘   │   │
│  │          │  ┌ 瀑布式融合影响图（D3.js）─────────────┐   │   │
│  │ 模块     │  │ 环形关系 + 树状瀑布 + 文件树叠加       │   │   │
│  │ 索引     │  └────────────────────────────────────────┘   │   │
│  │          │  ┌ LLM 详细解读（按模块分段）────────────┐   │   │
│  │          │  └────────────────────────────────────────┘   │   │
│  └──────────┴──────────────────────────────────────────────┘   │
└────────────────────────────────┬────────────────────────────────┘
                                 │
┌────────────────────────────────▼────────────────────────────────┐
│                      Eval Engine (Native)                        │
│                                                                 │
│  ┌─────────────┐  ┌──────────────┐  ┌───────────────────┐     │
│  │ Task Manager │  │ Grader Runner │  │ Transcript        │     │
│  │              │  │              │  │ Analyzer          │     │
│  │ - 自动创建   │  │ - Test Grader│  │ - Tool 模式分析   │     │
│  │ - 手动命名   │  │ - Lint Grader│  │ - Token 趋势      │     │
│  │ - 查询历史   │  │ - Build Check│  │ - 错误模式        │     │
│  └─────────────┘  └──────────────┘  └───────────────────┘     │
│                                                                 │
│  ┌─────────────┐  ┌──────────────┐  ┌───────────────────┐     │
│  │ Code Exporter│  │ Timeline      │  │ LLM Client        │     │
│  │              │  │ Storage        │  │                   │     │
│  │ - AST 符号   │  │ (SQLite)       │  │ - Chat API        │     │
│  │ - Diff 提取  │  │               │  │ - Retry           │     │
│  │ - Lint 结果  │  │ EvalResult    │  │ - JSON Schema校验 │     │
│  │ - Test 结果  │  │ GitEvent       │  │                    │     │
│  └──────┬───────┘  └──────────────┘  └───────────────────┘     │
│         │                                                        │
│         │  写入 .Paporot/cache/                                   │
│         ▼                                                        │
└─────────────────────────────────────────────────────────────────┘
                                 │
┌────────────────────────────────▼────────────────────────────────┐
│                  WASM Skill Runtime (wasmtime)                   │
│                                                                 │
│  安全约束：Skill 仅能访问 .Paporot/ 目录                         │
│                                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────────────┐     │
│  │ LLM Rubric   │  │ Report       │  │ Custom Analysis   │     │
│  │ Grader       │  │ Generator    │  │ Skill             │     │
│  │              │  │              │  │                   │     │
│  │ 代码质量评判 │  │ HTML/MD/JSON │  │ 用户自定义分析    │     │
│  │ 过度工程检测 │  │ 报告生成     │  │ 逻辑              │     │
│  └──────────────┘  └──────────────┘  └───────────────────┘     │
│                                                                 │
│  3 个 Host Function:                                             │
│    host_read_file   → .Paporot/ 内读取                           │
│    host_write_file  → .Paporot/ 内写入                           │
│    host_llm_call    → 用户配置的 LLM endpoint                    │
└─────────────────────────────────────────────────────────────────┘
```

### 4.2 数据流

```
Git Commit（Agent 代码变更）
   │
   ├─► Code Exporter（Native 机械层）
   │     ├─ 解析 AST → 符号表
   │     ├─ 提取 diff → 变更摘要
   │     ├─ 运行 test/lint → 结果
   │     ├─ 写入 .Paporot/cache/
   │     └─ 写入 GitEvent 到 Timeline
   │
   ├─► 如果关联 Transcript（Agent 执行日志）
   │     └─ 导入 BehaviorTrace → Timeline
   │
   ├─► Grader Runner（Native，有权执行 shell）
   │     ├─ DeterministicTestGrader
   │     ├─ StaticAnalysisGrader
   │     └─ 产出 Outcome
   │
   ├─► WASM Skill（沙盒内，只读 .Paporot/cache/）
   │     ├─ LLM Rubric Grader（代码质量评判）
   │     └─ Report Generator（聚合所有结果 → 报告）
   │
   └─► 写入 EvalResult → Timeline (SQLite)
```

### 4.3 与旧架构的关键差异

1. **Evaluator 而非 Analyzer**：旧架构分析"代码变成了什么样"；新架构评估"Agent 做得好不好"
2. **宿主不做判断**：旧 Native 通过 L2 规则引擎做判断；新 Native 只做机械提取，判断留给 Graders 和 Skill
3. **Timeline 事件溯源**：旧架构用文件系统 JSON 存快照；新架构用 SQLite 存事件流
4. **Skill 不碰源码**：旧 Skill 可读任意文件；新 Skill 只能读 `.Paporot/cache/`

---

## 5. 数据模型

### 5.1 EvalResult（顶层评估对象）

```rust
/// 一次 Agent Task 执行的完整评估记录
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvalResult {
    /// 唯一标识符
    pub eval_id: String,

    /// 关联的 Task
    pub task: TaskSpec,

    /// 第几次 Trial（同一 Task 多次执行）
    pub trial_index: u32,

    // ── Transcript：Agent 执行面 ──
    /// Agent 执行轨迹（如果捕获了）
    pub transcript: Option<BehaviorTrace>,

    /// Tool 调用模式摘要
    pub tool_pattern: Option<ToolPattern>,

    // ── Outcome：结果面 ──
    /// 最终裁定
    pub outcome: OutcomeVerdict,

    /// 各 Grader 结果
    pub grader_results: Vec<GraderResult>,

    // ── CodeChange：产出面 ──
    /// 代码变更摘要
    pub code_change: CodeChangeSummary,

    /// 时间戳
    pub created_at: String,

    /// 关联的 GitEvent ID
    pub git_event_id: Option<String>,

    /// 关联的 Session ID
    pub session_id: Option<String>,

    /// 用户标签
    pub tags: Vec<String>,
}
```

### 5.2 TaskSpec

```rust
/// 任务规格
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TaskSpec {
    /// 唯一标识符
    pub id: String,

    /// 任务描述
    pub description: String,

    /// 成功标准
    pub success_criteria: Vec<String>,

    /// 任务类别
    pub category: TaskCategory,

    /// 相关模块（文件路径前缀）
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TaskSource {
    /// 自动从 git commit 创建
    Auto { commit_sha: String },
    /// 用户手动创建
    Manual { created_by: String },
    /// 从已有 Task 拆分/合并
    Derived { parent_ids: Vec<String> },
}
```

### 5.3 OutcomeVerdict

```rust
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
```

### 5.4 GraderResult

```rust
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
```

### 5.5 ToolPattern（Agent 行为摘要）

```rust
/// Tool 调用模式摘要（从 BehaviorTrace 提取）
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ToolPattern {
    /// Tool 调用总数
    pub total_tool_calls: usize,

    /// Tool 类型分布: tool_name → count
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
    pub state_count: Option<usize>,

    /// P1 向量（如果有）
    pub trajectory_vector: Option<serde_json::Value>,
}
```

### 5.6 CodeChangeSummary（代码变更摘要）

```rust
/// 代码变更摘要（从 diff + AST 提取，纯机械）
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CodeChangeSummary {
    /// 变更文件列表
    pub files_changed: Vec<String>,

    /// 新增行数
    pub additions: u32,

    /// 删除行数
    pub deletions: u32,

    /// 新增符号（函数/结构体/接口）
    pub symbols_added: Vec<SymbolChange>,

    /// 删除符号
    pub symbols_removed: Vec<SymbolChange>,

    /// 修改符号
    pub symbols_modified: Vec<SymbolChange>,

    /// 涉及的模块（文件路径前缀去重）
    pub modules: Vec<String>,

    /// L1 置信度
    pub confidence: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SymbolChange {
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub line_start: u32,
    pub line_end: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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
}
```

### 5.7 Storage 存储层

```
SQLite: .Paporot/paporot.db

Tables:
  eval_results    — EvalResult 事件流
  traces          — BehaviorTrace 数据
  git_events      — Git commit 事件
  tool_patterns   — ToolPattern 缓存
  task_defs       — 用户定义的 Task 元数据
  grader_results  — 评分历史

Timeline 查询原则：
  - 不预计算跨事件关联
  - 按 session_id + timestamp 窗口动态关联 EvalResult ↔ GitEvent
  - Capability 聚类为查询视图，不持久化
```

---

## 6. CLI 接口

### 6.1 命令总览

```bash
paporot 0.4.0

Commands:
  analyze     一键全量分析（推荐首选）
  eval        Task 评估
  task        Task 管理
  trace       Execution Trace 管理（保留）
  state       行为状态机构建（P0，保留）
  trajectory  轨迹向量分析（P1，保留）
  coupling    行为耦合图分析（P2，保留）
  skill       Skill 管理（保留，接口简化）
  dashboard   启动可视化面板
  init        初始化 .Paporot/
  config      LLM 配置管理
  version     显示版本信息
```

### 6.2 `paporot analyze` — 一键全量分析（推荐首选）

用户配置好 API Key 后，一条命令完成全流程评估并生成 Dashboard。

```bash
# 基本用法：分析最新 commit
paporot analyze

# 指定 commit
paporot analyze --commit HEAD~1

# 指定 PRD
paporot analyze --prd docs/prd.md

# 指定 LLM API Key
paporot analyze --api-key sk-xxxxx

# 跳过 LLM Rubric（更快，纯确定性评分）
paporot analyze --no-llm
```

**执行流程**：

```
paporot analyze
  ├─ 1. 检测最新 git commit → 自动创建 Task
  ├─ 2. CodeExporter：提取 AST 符号 + diff 摘要 + 运行 test/lint → .Paporot/cache/
  ├─ 3. Graders 评分：确定性测试 + 静态分析 + 构建检查
  ├─ 4. (可选) LLM Rubric Grader (WASM Skill)
  ├─ 5. Trajectory 分析（如有关联 Trace）
  ├─ 6. 生成 Dashboard HTML + EvalResult JSON
  └─ 7. 自动打开浏览器预览 Dashboard
```

**输出**：

```
.Paporot/reports/
  ├── dashboard.html       ← 图文并茂的可视化面板
  ├── eval_result.json     ← 结构化评估数据
  └── summary.md           ← Markdown 摘要报告
```

### 6.3 `paporot eval` — Task 评估

```bash
# 自动评估最新 commit
paporot eval auto

# 指定 commit 评估
paporot eval auto --commit HEAD~1

# 评估指定 Task 的最新 Trial
paporot eval run --task "fix-auth-bypass"

# 对比同一 Task 的两个 Trial
paporot eval compare --task "fix-auth-bypass" --from v1 --to v2

# 查看某 Task 的趋势
paporot eval trend --task "fix-auth-bypass"

# 批量回归检测（所有 Task 的最新 Trial vs 基线）
paporot eval regression

# 指定 Graders
paporot eval auto --graders test,lint,llm-rubric

# 指定 LLM API Key
paporot eval auto --api-key sk-xxxxx
```

#### `paporot eval compare` — 行为退化/改进对比

当用户对同一个 Task 执行了多次（如 Agent 前后两次修复同一个 bug），`eval compare` 回答核心问题：**Agent 这次比上次好还是坏？**

**输出结构**：

```
paporot eval compare --task auth-bug --from v1 --to v2

═══════════════════════════════════════════
  对比报告: auth-bug  v1 → v2
═══════════════════════════════════════════

  总体趋势: ↑ 改进（行为模式更高效）

  ── 一级指标（Anthropic tracked_metrics）──
  工具调用数:     12 → 8   ↓33%  改进
  Token 消耗:   4500 → 3200 ↓29%  改进
  执行耗时:     120s → 85s  ↓29%  改进

  ── 深度诊断（P1 行为向量）──
  行为稳定性:    0.72 → 0.85  ↑18%  改进
  工具混乱度:    0.64 → 0.48  ↓25%  改进
  循环比例:      0.18 → 0.06  ↓67%  改进

  ── LLM 对比解读 ──
  v2 版本的 Agent 比 v1 减少了 33% 的工具调用。关键改进在于：
  不再在 auth 和 middleware 之间反复切换（phase_entropy 下降），
  v1 的"读文件→改代码→又读回来→再改"循环消失了。
  整体行为从探索型转变为目标导向型，Agent 的执行更确定、更高效。
```

**指标来源与解释**：

##### 一级指标（Anthropic 官方 tracked_metrics）

直接来自 Anthropic *Demystifying evals for AI agents* 定义的 Transcript 级指标：

| 指标 | 含义 | 用户解读 |
|------|------|---------|
| `n_toolcalls` | 这次用了多少次工具调用 | Agent 做事需要多少步骤 |
| `n_total_tokens` | 消耗了多少 token（输入+输出） | 推理成本 |
| `duration_ms` | 执行耗时 | 完成任务的速度 |

##### 深度诊断指标（P1 轨迹向量）

P1 的 7 个标量指标不是随意选的——它们来自一个统一思想：**熵 = 混乱度，比率 = 行为模式特征。三熵 + 三比率 + 稳定性构成互补的"行为体检"**。

| 指标 | 用户可理解的含义 | 底层计算 |
|------|-----------------|---------|
| `tool_entropy` | **Agent 是否在乱用工具？** 数字越高 = tool 类型切换越随机，可能在无目的地探索 | 统计 tool 类型出现频率的 Shannon 熵。如 90% 时间只做一件事，熵低；五种 tool 均匀使用，熵高 |
| `phase_entropy` | **Agent 是否在来回跳？** 数字越高 = 缺乏清晰计划，反复切阶段。如 edit→test→edit 是直线，edit→read→edit→grep→read→edit 是迷路 | 统计阶段转移路径的熵 |
| `transition_entropy` | **Agent 的状态空间是否变得更复杂了？** 上次只有 3 种状态间转移，这次 8 种。涨了说明行为结构变复杂了（不一定是坏事，但需要关注） | 把转移图当网络，算边分布的熵 |
| `loop_ratio` | **Agent 是不是在兜圈子？** 一大串操作最后回到了起点 | 检测 tool 序列中重复片段的占比。`[read,grep,read,grep]` 含循环；`[read,edit,test,commit]` 没有 |
| `backtrack_ratio` | **Agent 是不是在改已经改过的东西？** 修了又修，改回原样 | 检测向已访问状态回退的频率 |
| `burst_ratio` | **Agent 的工作节奏。** 集中爆发 = 思考后大量改；均匀分布 = 边想边改。没有好坏，是风格 | 编辑操作的时域聚集度 |
| `state_stability_score` | **Agent 的执行是否稳定？** 同一 Task 跑三次，行为结构是否一致。低了 = 不确定性太高 | 对比多次 run 的 StateGraph 结构相似度 |

**退化检测三角**：

```
稳定性 (stability_score)  ← Agent 能不能复现自己的行为？
    │
    ▼
混乱度 (三个熵)           ← Agent 的思路有没有乱？
    │
    ▼
行为模式 (三个比率)       ← Agent 在怎样执行？
```

典型退化模式：tool_entropy ↑ + phase_entropy ↑ + loop_ratio ↑ = **Agent 在迷路**。  
如果只 burst_ratio 变化但熵不变 = **只是换了工作节奏，没退化**。

##### Dashboard 展示策略

这些指标在 Dashboard 上的分层展示：

```
变更叙事页（首页）         → 不展示这些指标（用户说首页要讲项目故事）
eval compare 结果页        → 一级指标（Anthropic）大字展示，深度诊断折叠展开
侧边栏 "Task 详情" flyout  → P1 指标趋势迷你图，hover 提示含义
能力全景页                 → 不展示
```

### 6.4 `paporot task` — Task 管理

```bash
# 创建新 Task
paporot task new "修复认证绕过漏洞" \
    --criteria "test_empty_pw_rejected 通过" \
    --criteria "lint 无新增警告" \
    --module src/auth

# 列出所有 Task
paporot task list

# 查看 Task 详情
paporot task show "fix-auth-bypass"

# 合并两个 Task
paporot task merge --parent task_a --parent task_b --name "统一认证修复"

# 从已有 trials 自动创建 Task
paporot task auto-create
```

### 6.5 保留的旧命令（内部适配）

以下命令保留，但内部适配到新数据模型：

| 旧命令 | v0.4.0 映射 |
|--------|------------|
| `paporot trace import` | 不变（仍导入 Agent 日志） |
| `paporot trace list/show` | 不变 |
| `paporot state build/eval/diff` | 不变（P0 分析） |
| `paporot trajectory-vector build/diff/cluster` | 不变（P1 分析） |
| `paporot coupling build/analyze` | 不变（P2 分析） |

---

## 7. WASM Skill 系统

### 7.1 Skill 重新定位

v0.4.0 中 Skill 的职责从"分析代码架构"转变为"消费缓存做质量判断"。

```
旧：repository-understanding / module-discovery / dependency-analysis / ...
新：llm-rubric-grader / report-generator / (用户自定义分析 Skill)
```

### 7.2 Skill 物理形态

```
.paporot/skills/
  llm-rubric-grader/
    skill.toml        ← 元数据
    skill.wasm        ← 计算逻辑
  report-generator/
    skill.toml
    skill.wasm
```

### 7.3 skill.toml 格式

```toml
[skill]
name = "llm-rubric-grader"
version = "0.1.0"
requires_paporot = ">=0.4.0"
description = "使用 LLM 对代码变更进行质量评分"
timeout_secs = 60

[inputs]
required = ["code_change_summary", "diff_content", "test_results", "lint_results"]
optional = ["prd_content"]

[outputs]
schema = "llm_rubric_output"
format = "json"

[llm_calls]
max_calls = 3
preferred_model = "deepseek-chat"

# 质量检查规则
[quality]
checks = [
    "code_quality_has_score",
    "over_engineering_detected",
    "evidence_cites_source"
]
```

### 7.4 安全模型

| 权限级别 | 可访问 | 获取方式 |
|---------|--------|---------|
| 默认（未签名） | `.Paporot/cache/`、`.Paporot/reports/` | 自动授予 |
| 签名（用户签发） | 项目源文件 + 默认权限 | `paporot skill sign` |
| 签名（社区审核） | 项目源文件 + 默认权限 | 外部审核流程（P2） |

### 7.5 WASM 接口（不变）

Skill 必须导出：

| 函数 | 签名 | 说明 |
|------|------|------|
| `paporot_skill_execute` | `fn() -> i32` | 执行入口 |
| `paporot_skill_output_ptr` | `fn() -> *const u8` | 输出 JSON 指针 |
| `paporot_skill_output_len` | `fn() -> usize` | 输出 JSON 长度 |
| `paporot_skill_error_ptr` | `fn() -> *const u8` | 错误信息指针 |
| `paporot_skill_error_len` | `fn() -> usize` | 错误信息长度 |

---

## 8. Host Functions

### 8.1 现有 Host Functions（保留）

| Function | 说明 | 安全约束 |
|----------|------|---------|
| `host_read_file` | 读取文件 | 仅允许 `.Paporot/` 内 |
| `host_write_file` | 写入文件 | 仅允许 `.Paporot/` 内 |
| `host_llm_call` | LLM 推理 | 仅调用用户配置的 endpoint |

### 8.2 新增 Host Functions（v0.4.0+）

| Function | 说明 | 安全约束 |
|----------|------|---------|
| `host_exec_command` | 执行 shell 命令 | 仅对**签名 Skill** 开放；命令白名单校验 |

`host_exec_command` 对未签名 Skill 不可用。签名 Skill 可执行受限命令集。

---

## 9. Grader 评分框架

### 9.1 Grader 类型

#### 确定性测试 Grader

```rust
/// 运行项目测试套件
pub struct DeterministicTestGrader {
    /// 测试命令
    pub command: String,         // "cargo test" / "pytest" / "npm test"
    /// 工作目录
    pub cwd: Option<String>,
    /// 超时时间（秒）
    pub timeout: u32,
}

// 输出
{
  "passed": true,
  "total_tests": 42,
  "passed_tests": 42,
  "failed_tests": 0,
  "duration_ms": 3200,
  "stderr_summary": ""
}
```

#### 静态分析 Grader

```rust
pub struct StaticAnalysisGrader {
    pub linters: Vec<LintCommand>,
}

pub struct LintCommand {
    pub name: String,            // "rustfmt" / "clippy" / "eslint"
    pub command: String,
}

// 输出
{
  "passed": true,
  "checks": [
    {"name": "clippy", "passed": true, "warnings": 0, "errors": 0},
    {"name": "rustfmt", "passed": true, "issues": 0}
  ]
}
```

#### 构建检查 Grader

```rust
pub struct BuildCheckGrader {
    pub command: String,
    pub timeout: u32,
}

// 输出
{
  "passed": true,
  "exit_code": 0,
  "duration_ms": 4500,
  "artifact_size_bytes": 12480000
}
```

#### LLM Rubric Grader（WASM Skill 实现）

Skill 读取 `.Paporot/cache/` 中的分析结果，通过 `host_llm_call` 调用 LLM 进行质量评判。

```yaml
# 默认 Rubric（可自定义）
dimensions:
  - name: correctness
    weight: 0.4
    description: "代码逻辑是否正确"
  - name: code_quality
    weight: 0.3
    description: "代码风格、命名、结构"
  - name: over_engineering
    weight: 0.15
    description: "是否存在不必要的抽象/复杂度"
  - name: test_coverage
    weight: 0.15
    description: "是否有足够的测试"
```

### 9.2 Grader 注册与运行

```rust
// Grader trait
pub trait Grader {
    fn name(&self) -> &str;
    fn ty(&self) -> GraderType;
    fn run(&self, context: &EvalContext) -> Result<GraderResult>;
}

pub struct EvalContext {
    pub project_root: PathBuf,
    pub paporot_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub commit_sha: String,
    pub diff_content: String,
}
```

---

## 10. Dashboard 可视化

### 10.1 设计定位

Dashboard 是**被分析项目的业务级故事讲述工具**，不是 Paporot 自身运行状态的监控面板。核心目标：

- 回答用户的问题："我的项目发生了什么？"
- 演示级视觉效果——大气、现代极简深色、色彩区分度高
- 不展示内部评分细节（Pass/Fail 不突出），聚焦项目本身的变更叙事

### 10.2 视觉风格

| 属性 | 值 |
|------|-----|
| 风格 | 现代极简深色（Linear / Vercel 风格） |
| 背景色 | `#111827` |
| 主文字色 | `#F9FAFB` |
| 品牌高亮 | `#6366F1` 靛蓝 |
| 强调色 | `#06B6D4`（青）、`#F59E0B`（琥珀）、`#EF4444`（红） |
| 卡片/面板 | 半透明毛玻璃 `rgba(255,255,255,0.03)` + border `rgba(255,255,255,0.06)` |
| 字体 | 系统默认等宽（数字）/ Inter（正文，Google Fonts CDN） |
| 图表库 | D3.js v7 CDN |

### 10.3 技术栈

```
Rust 后端: axum HTTP 服务器
  ├── GET  /              → Dashboard SPA 入口 HTML
  ├── GET  /api/tasks     → Task 列表 JSON
  ├── GET  /api/eval/:id  → 单个 EvalResult JSON
  ├── GET  /api/capabilities → Capability 聚合数据 JSON
  └── GET  /api/trend     → 趋势数据 JSON

前端: 原生 HTML + JS + D3.js CDN（无构建工具、无 npm）
数据源: .Paporot/paporot.db (SQLite)
```

### 10.4 页面布局

```
┌───────────────────────────────────────────────────────────────┐
│  [变更叙事] [能力全景]                    Paporot Dashboard     │
├─────────────┬─────────────────────────────────────────────────┤
│ 侧边栏      │  主内容区                                       │
│             │                                                │
│ Task 列表   │  ┌─ 首屏大字报区 ────────────────────────────┐  │
│ ├─ 06-29   │  │                                            │  │
│ │  认证升级 │  │  「认证模块架构升级」                        │  │
│ ├─ 06-27   │  │   Agent 重构了密码验证流程并新增 MFA 支持    │  │
│ │  修复Bug  │  │   涉及 3 个模块 · 影响范围：auth, middleware│  │
│ ├─ ...     │  │        [影响范围缩略图]                      │  │
│             │  └────────────────────────────────────────────┘  │
│ 模块列表    │                                                │
│ ├─ auth    │  ┌─ 瀑布式统一融合影响图 ────────────────────┐  │
│ ├─ payment │  │                                            │  │
│ ├─ api     │  │  内圈 = 核心变更项（函数/结构体）           │  │
│ └─ ...     │  │  外圈 = 受影响模块                         │  │
│             │  │  向下 = 树状瀑布扩散                      │  │
│             │  │  节点叠加 = 文件树缩略图                   │  │
│             │  └────────────────────────────────────────────┘  │
│             │                                                │
│             │  ┌─ 底部 LLM 详细解读 ──────────────────────┐  │
│             │  │  auth 模块                                │  │
│             │  │  本次变更重构了密码验证流程...              │  │
│             │  │  ---                                      │  │
│             │  │  middleware 模块                          │  │
│             │  │  新增的 MFA 中间件拦截...                  │  │
│             │  └────────────────────────────────────────────┘  │
└─────────────┴─────────────────────────────────────────────────┘
```

### 10.5 变更叙事页（默认首页）

#### 首屏大字报区

页面顶部，占视口约 30%。包含：

- **标题**：LLM 生成的一句话总结（如「认证模块架构升级」），大号 Inter 字体，品牌靛蓝色
- **副标题**：LLM 生成的 1-2 句解释（如「Agent 重构了密码验证流程并新增 MFA 支持，涉及 3 个模块」），白色，中等字号
- **影响范围缩略图**：迷你模块关系图（200×200px），受影响模块高亮

#### 瀑布式统一融合影响图

占视口约 45%，一张融合三种视觉元素的大图：

```
                    ┌─────────────────┐
           ┌───────│  validate_pwd()  │───────┐
           │       │  MfaConfig       │       │
           │       └─────────────────┘       │
           ▼                                 ▼
    ┌──────────┐                    ┌──────────────┐
    │  auth    │ ◄──────────────────│ middleware   │
    └────┬─────┘                    └──────┬───────┘
         │                                 │
         ▼                                 ▼
    ┌──────────┐                    ┌──────────────┐
    │  models  │                    │  rate_limit  │
    └──────────┘                    └──────────────┘
```

- **内圈**：核心变更实体（函数/结构体节点），用环形布局。节点大小 = 变更行数
- **外圈**：受影响模块，按依赖方向分布在内圈周围
- **向下扩散**：从外圈模块向它们的下游依赖继续展开，形成树状瀑布
- **节点叠加**：每个模块节点可 hover 展开其文件树缩略图（变更文件高亮）
- **连线**：白线 + 渐变透明度，表示影响方向
- **交互**：hover 展示详情，click 聚焦该节点，zoom/pan

#### 底部 LLM 详细解读

占视口约 25%，按受影响模块分段，每段包括：

- 模块名称（大字）
- LLM 生成的变更说明（「本次变更重构了密码验证流程，将 argon2 升级为默认哈希算法...」）
- 涉及的关键符号（函数/结构体列表，小字标注）
- 风险提示（如有）

### 10.6 能力全景页（顶部 Tab 切换）

D3.js 力导向网络图，展示整个项目的模块关系全景：

- **Capability 节点**：方块，按模块名，大小 = 关联的 Task 历史数
- **耦合连线**：模块间的依赖/co-change 关系，粗细 = 耦合强度
- **颜色编码**：
  - 本次变更涉及的模块：靛蓝 `#6366F1`（脉搏动画）
  - 历史活跃模块：青色 `#06B6D4`
  - 低活跃模块：暗灰 `#374151`
- **交互**：拖拽节点、缩放、hover 显示模块详情 flyout（最近变更列表 + 关联模块）
- **布局**：启动时带有入场动画（从中心扩散）

### 10.7 侧边栏

左侧固定 280px 宽，包含两个区域：

**Task 历史列表**：
- 显示最近 10 条 Task
- 每条：日期 + 简短标题 + 彩色圆点（模块颜色）
- 点击跳转到该 Task 的变更叙事页
- "查看全部"链接

**模块索引**：
- 列出所有模块/Capability
- 点击跳转能力全景页并聚焦该模块
- 每个模块旁显示最近变更日期和 Task 数

### 10.8 CLI 命令

```bash
# 启动 Dashboard Web 服务（默认 localhost:9494）
paporot dashboard

# 指定端口
paporot dashboard --port 3000

# 指定时间范围
paporot dashboard --from 2026-06-01 --to 2026-06-29
```

`paporot analyze` 执行分析后自动提示 Dashboard URL，用户可以立即打开。

---

## 11. Trajectory 轨迹分析

### 11.1 保留现有模块

`src/trajectory/` 模块全部保留，它基于 `BehaviorTrace` 做分析，不依赖旧 Capability 定义。

```
P0: 行为状态机 (BehaviorStateGraph)
    ├─ state/builder       — 状态构建
    ├─ state/features      — 特征提取
    ├─ state/segmentation  — 状态分割
    ├─ state/merge         — 合并判决
    └─ state/transition    — 转移图

P1: 轨迹向量 (TrajectoryVector)
    ├─ p1/vector           — 10维向量
    ├─ p1/feature_extractor— 特征提取
    ├─ p1/cluster          — 聚类
    └─ p1/timeseries       — 时序分析

P2: 行为耦合图 (Capability Coupling Graph)
    ├─ p2/cochange         — co-change 检测
    ├─ p2/correlation      — 相关性分析
    └─ p2/graph            — 图构建

align/                     — 轨迹对齐引擎
evaler/                    — 图形/状态/转换评估
projection/                — 状态→差异投影
```

### 11.2 在新架构中的位置

P0/P1/P2 分析的是 `BehaviorTrace`（tool call 序列），属于 **Dashboard 左半（Agent 行为面）**。数据流入方式：

```
BehaviorTrace (已有)
  └─► P0 StateGraph ←─ 结果写入 ToolPattern.state_count
  └─► P1 TrajectoryVector ←─ 结果写入 ToolPattern.trajectory_vector
  └─► P2 CouplingGraph ←─ 聚类数据供 Dashboard 网络图使用
```

---

## 12. 实现路线图

### Phase 1：核心数据模型 (P0)

- [ ] 新建 `src/eval/` 模块，定义 `EvalResult` / `TaskSpec` / `OutcomeVerdict` / `GraderResult` 等类型
- [ ] 新建 `src/storage/timeline.rs`（SQLite 事件存储）
- [ ] 实现 `TaskManager`（自动从 git commit 创建 Task + 手动创建）
- [ ] 实现 `CodeExporter`（AST 符号提取 + diff 摘要 + test/lint 运行 → `.Paporot/cache/`）
- [ ] 更新 `src/lib.rs` 和模块声明
- [ ] 更新 `Cargo.toml` 版本号为 0.4.0

### Phase 2：Grader 框架 (P0)

- [ ] 定义 `Grader` trait
- [ ] 实现 `DeterministicTestGrader`
- [ ] 实现 `StaticAnalysisGrader`
- [ ] 实现 `BuildCheckGrader`
- [ ] 实现 `paporot eval auto` CLI 命令

### Phase 3：CLI 迁移 (P0)

- [ ] 实现 `paporot task` 子命令
- [ ] 实现 `paporot eval compare` / `paporot eval trend` / `paporot eval regression`
- [ ] 更新 `paporot init`（新数据目录结构）
- [ ] 删除旧命令模块和旧类型
- [ ] 适配保留的命令（trace/state/trajectory-vector/coupling）到新 lib.rs

### Phase 4：Dashboard + Analyze (P1)

- [ ] 添加 `axum` 依赖到 `Cargo.toml`
- [ ] 实现 `paporot analyze` 命令（编排 CodeExporter + Graders + Trajectory + LLM Rubric → EvalResult）
- [ ] 实现 Dashboard HTTP 服务器（`src/dashboard/server.rs`）
  - [ ] `/` → SPA 入口 HTML
  - [ ] `/api/tasks`、`/api/eval/:id`、`/api/capabilities`、`/api/trend` JSON API
- [ ] 实现前端 HTML/JS（`src/dashboard/static/`）
  - [ ] 页面骨架（导航 Tab + 侧边栏 + 主内容区布局）
  - [ ] 变更叙事页：大字报区 + 瀑布式融合影响图（D3.js）
  - [ ] 能力全景页：力导向网络图（D3.js）
  - [ ] LLM 详细解读区
  - [ ] 入场动画和交互
- [ ] 实现 Capability 聚类引擎（`src/dashboard/cluster.rs`）
- [ ] 实现 `paporot dashboard` CLI 命令

### Phase 5：WASM Skill 升级 (P1)

- [ ] 实现 LLM Rubric Grader Skill
- [ ] 实现 Report Generator Skill
- [ ] 删除 `behavior-boundary-discovery` 和 `prd-coverage` 空壳
- [ ] 更新 Skill SDK 文档

### Phase 6：生态就绪 (P2)

- [ ] Skill 签名机制（`paporot skill sign`）
- [ ] `host_exec_command` host function（签名 Skill 专用）
- [ ] CI/CD 集成模板

---

## 13. 从 v0.1.0 迁移

### 13.1 删除清单

| 文件/目录 | 原因 |
|-----------|------|
| `src/types.rs` | `BehaviorSnapshot` 等旧类型 |
| `src/commands/snapshot.rs` | 概念错误 |
| `src/commands/diff.rs` | 概念错误 |
| `src/commands/coverage.rs` | 概念错误 |
| `src/commands/regression.rs` | 用 eval regression 替代 |
| `src/commands/risk.rs` | 用 eval regression 替代 |
| `src/commands/review.rs` | 用 eval auto 替代 |
| `src/commands/graph.rs` | 依赖旧 DependsOn 类型 |
| `src/commands/feedback.rs` | 概念错误 |
| `src/commands/testmap.rs` | 概念错误 |
| `src/agent.rs` | 绑定旧 BehaviorSnapshot |
| `src/prompts.rs` | 旧行为提取 prompt |
| `src/report/` | 依赖旧类型 |
| `src/evaler/` | 依赖旧 Capability |
| `src/skills/registry.rs` | 依赖旧类型 |
| `src/skills/schema_compat.rs` | 旧 schema compat |
| `crates/skills/src/bin/behavior_boundary_discovery.rs` | Skill 概念错误 |
| `.Paporot/skills/behavior-boundary-discovery/` | 同上 |
| `.Paporot/skills/prd-coverage/` | 空壳 |

### 13.2 保留清单

| 文件/目录 | 说明 |
|-----------|------|
| `src/trace/` | Transcript 适配器（DeepSeek/Claude/OpenAI） |
| `src/trajectory/` | P0/P1/P2 全子模块 |
| `src/analysis/` | L1 AST / L2 规则 / L3 LLM 桥接（适配为 CodeExporter 服务） |
| `src/evidence/` | 能力证据收集（适配新模型） |
| `src/llm/` | LLM 客户端 |
| `src/config.rs` | 配置管理 |
| `src/storage.rs` | 保留或适配为 Timeline 存储 |
| `src/skills/runtime/` | WASM 运行时（DAG / host_bridge / wasm_host） |
| `src/skills/error_log.rs` | 错误日志 |
| `src/skills/types.rs` | Skill 类型 |
| `crates/paporot-core/` | WASM 核心管线 |
| `crates/skill-sdk/` | Skill 开发 SDK |
| `crates/paporot-validation/` | Golden Dataset + Benchmark |
| `src/cli.rs` | 保留框架，更新命令 |

### 13.3 新增文件

```
src/
  eval/
    mod.rs           — 模块声明
    types.rs         — EvalResult, TaskSpec, OutcomeVerdict 等
    task.rs          — TaskManager
    grader.rs        — Grader trait + TestGrader + LintGrader + BuildGrader
    runner.rs        — EvalRunner：编排 Graders 执行
    compare.rs       — eval compare 逻辑
    trend.rs         — eval trend 逻辑
    regression.rs    — 批量回归检测
    exporter.rs      — CodeExporter：源码→结构化缓存的导出逻辑
  storage/
    mod.rs           — 存储模块
    timeline.rs      — SQLite Timeline 事件存储
    cache.rs         — .Paporot/cache/ 读写
  dashboard/
    mod.rs           — 模块声明
    server.rs        — axum HTTP 服务 + API 路由
    cluster.rs       — Capability 聚类引擎
    static/
      index.html     — Dashboard SPA 入口页面
      app.js         — 前端主逻辑（Tab 切换、侧边栏、大字报渲染）
      narrative.js   — 变更叙事页：瀑布式融合影响图（D3.js）
      capability.js  — 能力全景页：力导向网络图（D3.js）
      style.css      — 暗色主题样式（#111827 + #6366F1）
```

---

## 决策日志
| 日期 | 决策 | 理由 |
|------|------|------|
| 2026-06-29 | "行为"= Task Outcome + Transcript，代码变更是辅助 | 对齐 Anthropic 定义 |
| 2026-06-29 | 底层 Timeline 事件溯源 (F)，上层按需聚合 Capability (D) | 最大灵活性，未来可物化为 E |
| 2026-06-29 | 一个 Task 可属于多个 Capability（网络图渲染） | 反映真实影响面 |
| 2026-06-29 | Grader: 基础自动 + 质量可选 | 低门槛起步 |
| 2026-06-29 | 宿主做机械导出，Skill 做判断 | 安全边界 = 智能边界 |
| 2026-06-29 | Skill 默认不能读源码 | 用户数据保护优先 |
| 2026-06-29 | 直接替换旧模型，不保留兼容 | 用户同意舍弃错误定义 |
| 2026-06-29 | 一个 commit = 原子 Task | 用户定义 |
| 2026-06-29 | 命令名为 `paporot`（小写）| 用户要求，与旧版 Paporot 区分 |
| 2026-06-29 | 新增 `paporot analyze` 一键全量分析命令 | 用户要求：init + 配置 API Key 后一条命令出 Dashboard |
| 2026-06-29 | Dashboard = 内嵌 Web 服务（axum + 原生 HTML/JS/D3.js） | 轻量，无构建工具 |
| 2026-06-29 | 视觉风格：现代极简深色（#111827 + 靛蓝 #6366F1） | 用户要求：大气、演示级、色彩区分度高 |
| 2026-06-29 | Dashboard 主题 = 讲被分析项目的故事，不是展示 Paporot 状态 | 用户纠正：用户不关心内部 Pass/Fail |
| 2026-06-29 | 导航：顶部双 Tab（变更叙事/能力全景）+ 左侧边栏 | 展示+探索两用 |
| 2026-06-29 | 变更叙事页：首屏大字报 + 瀑布式融合影响图 + LLM 详细解读 | 瀑布图 = 环形关系 + 树状扩散 + 文件树叠加 |
| 2026-06-29 | 能力全景页：D3.js 力导向网络图 | 交互性强，一眼看清模块关系 |
| 2026-06-29 | LLM 文案由 report-generator Skill 生成，写入 cache | 内容 = 标题故事 + 模块级解释 + 风险提示 |
