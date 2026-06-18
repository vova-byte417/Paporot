# Paporot Skill System 设计文档

> **Paporot —— AI Generated Software Understanding Platform**
>
> Git 管代码，Paporot 管理解。

---

## 目录

1. [产品定位](#1-产品定位)
2. [使用说明](#2-使用说明)
3. [整体架构](#3-整体架构)
4. [三层设计详解](#4-三层设计详解)
5. [Skill 物理形态](#5-skill-物理形态)
6. [WASM 标准接口](#6-wasm-标准接口)
7. [Host Functions](#7-host-functions)
8. [DAG 编排引擎](#8-dag-编排引擎)
9. [核心数据结构](#9-核心数据结构)
10. [Schema 兼容层](#10-schema-兼容层)
11. [错误处理与日志](#11-错误处理与日志)
12. [6 个 Skill 规格](#12-6-个-skill-规格)
13. [报告输出与 Dashboard](#13-报告输出与-dashboard)
14. [实现阶段划分](#14-实现阶段划分)
15. [决策日志](#15-决策日志)

---

## 1. 产品定位

### 问题

用户使用 AI Agent（Claude Code / Copilot / Cursor）提交代码时，真正痛苦的不是"AI 改了代码"，而是：

- 我根本不知道 AI 改了什么
- 我不知道为什么改
- 我不知道会影响什么
- 我不知道需求实现了多少

AI 的提交速度远超人类审查速度——人的瓶颈已经不是写代码，而是**理解代码**。

### 解决

Paporot 压缩"理解成本"到分钟级。它从 Git 仓库出发，自动构建四层理解：

```
Code → Behavior Layer → Architecture Layer → Requirement Layer → Human-readable Reports
```

### 与 Git 的分工

| | Git | Paporot |
|---|---|---|
| 管理什么 | 代码（行级差异） | 理解（行为级差异） |
| 回答什么问题 | 谁改了哪一行 | 他改了哪个能力、为什么、影响什么 |
| 用户 | 所有开发者 | 使用 AI Agent 的开发者 |

---

## 2. 使用说明

### 安装

```bash
# 需要 Rust 1.96+
cargo install --path .
```

### 配置 LLM

编辑 `.paporot/config.toml`：

```toml
[llm]
endpoint = "https://api.deepseek.com/v1/chat/completions"
api_key = "sk-your-deepseek-api-key"
model = "deepseek-chat"
temperature = 0.3
max_tokens = 4096
max_retries = 3
timeout_secs = 120
```

### 日常使用

```bash
# 一键全量分析（最常用）
paporot analyze

# 含需求覆盖率分析
paporot analyze --prd docs/prd.md

# 保留的原有命令
paporot review -p docs/prd.md        # 等价于 analyze --prd
paporot coverage -p docs/prd.md      # 只跑 PRD 覆盖率（轻量、快速）
paporot snapshot create -m "v2"      # 创建行为快照
paporot diff --from v1 --to v2       # 行为差异对比
paporot graph show                   # 依赖图
```

### 输出

```
.paporot/reports/
  dashboard.html          ← 双击打开，可视化面板
  data/
    analysis_result.json  ← 机器可读的完整数据
    dependency_graph.json
    runtime_flows.json
    behavior_boundary.json
  architecture.md         ← 架构报告
  behavior.md             ← 行为边界报告
  coverage.md             ← PRD 覆盖率报告 (有 --prd 时)

.paporot/logs/
  2026-06-18T15_30_00_analyze.log  ← 错误诊断日志
```

### 典型场景

| 场景 | 命令 | 得到什么 |
|------|------|---------|
| Agent 刚提了一轮代码 | `paporot analyze` | 架构变化 + 行为变化 + 风险评估 |
| 同时有 PRD | `paporot analyze --prd prd.md` | 上面 + "需求实现了多少" |
| 只看需求覆盖率 | `paporot coverage -p prd.md` | 快速覆盖率报告 |
| 多版本趋势 | 多次 `paporot analyze` | Dashboard 时间线图 |

---

## 3. 整体架构

### 架构图

```
┌──────────────────────────────────────────────────────────┐
│                      用户                                 │
│                                                          │
│   paporot analyze [--prd prd.md]                        │
│   paporot review  [--prd prd.md]                        │
│   paporot coverage -p prd.md                             │
└──────────────────────┬───────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────┐
│                   CLI Layer                               │
│   cli.rs  ──  命令解析、参数校验                          │
└──────────────────────┬───────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────┐
│               Skill Runtime                               │
│                                                          │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────────┐  │
│  │ Skill        │  │ DAG Engine   │  │ Schema        │  │
│  │ Registry     │  │ (编排调度)    │  │ Compat Layer  │  │
│  │              │  │              │  │ (版本兼容)     │  │
│  │ 扫描         │  │ 拓扑排序     │  │               │  │
│  │ .paporot/    │  │ 并行调度     │  │ Core v3 →     │  │
│  │ skills/*.toml│  │ 超时/降级    │  │ Skill v1      │  │
│  └──────────────┘  └──────────────┘  └───────────────┘  │
│                                                          │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────────┐  │
│  │ WASM Host    │  │ LLM Bridge   │  │ Report        │  │
│  │ (wasmtime)   │  │              │  │ Generator     │  │
│  │              │  │ 统一 API 调  │  │               │  │
│  │ 加载/执行    │  │ 用、重试、   │  │ Markdown +    │  │
│  │ .wasm        │  │ Token 预算   │  │ HTML Dashboard│  │
│  └──────────────┘  └──────────────┘  └───────────────┘  │
└──────────────────────┬───────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────┐
│                  Paporot Core                              │
│                                                          │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌─────────┐ │
│  │ Snapshot │  │  Diff    │  │  Graph   │  │ Storage │ │
│  │          │  │          │  │          │  │         │ │
│  │ 行为快照 │  │ 行为差异 │  │ 依赖图   │  │ SQLite  │ │
│  └──────────┘  └──────────┘  └──────────┘  └─────────┘ │
│                                                          │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌─────────┐ │
│  │ Analysis │  │  Types   │  │  Config  │  │  Audit  │ │
│  │          │  │          │  │          │  │  Record │ │
│  │ L1/L2/L3 │  │ 核心类型 │  │ 配置管理 │  │ 审计记录│ │
│  └──────────┘  └──────────┘  └──────────┘  └─────────┘ │
└──────────────────────────────────────────────────────────┘
```

### 架构说明

**Paporot Core** 是"护城河"——负责任何情况下都不能变的底层能力：

- `Snapshot`：从 Git Commit 生成行为快照
- `Diff`：两个快照的行为差异（哪些能力新增/修改/删除）
- `Graph`：能力之间的依赖图
- `Audit Record`：每次变更的审计记录

**Skill Runtime** 是"扩展层"——利用 Core 的数据做增值分析：

- `Skill Registry`：扫描 `.paporot/skills/` 加载 Skill 元数据
- `DAG Engine`：根据 Skill 依赖关系自动编排执行
- `WASM Host`：通过 wasmtime 加载执行 `skill.wasm`
- `LLM Bridge`：统一管理 LLM 调用（API 请求、重试、Token 预算）
- `Schema Compat Layer`：保证 Core 数据结构升级后旧 Skill 仍能运行
- `Report Generator`：将 Skill 产出组装为 Markdown 报告 + HTML Dashboard

**CLI Layer** 是用户唯一入口——所有分析最终收敛到 `paporot analyze`。

---

## 4. 三层设计详解

### 第一层：Paporot Core

Core 只负责三件事，这是产品护城河：

```
Git Commit
    ↓
Behavior Snapshot  ──→  { commit, behavior_hash, changed_functions }
    ↓
Behavior Diff      ──→  { added, modified, removed, risk_level }
    ↓
Audit Record       ──→  不可变审计日志
```

Core 不做任何"分析"——不做架构推断、不做需求覆盖、不做风险评估。这些全交给 Skill。

### 第二层：Skill Runtime

类似 Git Hook 的机制，但更结构化：

```
Paporot Event (analyze / commit)
    ↓
Skill Runtime
    ├── 扫描 skill.toml，构建 DAG
    ├── Schema Compat（版本适配）
    ├── WASM Host 加载执行
    ├── LLM Bridge 统一调用
    └── 收集结果，生成报告
```

用户一次 `paporot analyze`，Runtime 自动执行所有匹配的 Skill。

### 第三层：Skills

每个 Skill = Goal + Inputs + Procedure + Output Schema + Quality Checks。

Skill 以 `.wasm` + `skill.toml` 的形态存在，位于 `.paporot/skills/<skill-name>/`。

---

## 5. Skill 物理形态

### 目录结构

```
.paporot/skills/
  repository-understanding/
    skill.toml       ← 元数据：声明输入/输出/依赖/LLM预算
    skill.wasm       ← 计算逻辑
  module-discovery/
    skill.toml
    skill.wasm
  dependency-analysis/
    skill.toml
    skill.wasm
  runtime-flow-analysis/
    skill.toml
    skill.wasm
  behavior-boundary-discovery/
    skill.toml
    skill.wasm
  architecture-doc-generator/
    skill.toml
    skill.wasm
  prd-coverage/              ← 条件激活（需 --prd）
    skill.toml
    skill.wasm
```

### skill.toml 完整格式

```toml
[skill]
name = "repository-understanding"
version = "0.1.0"
requires_paporot = ">=0.2.0"
description = "识别项目整体目标、技术栈、入口程序、核心业务能力"
timeout_secs = 30

[inputs]
required = ["repo_tree", "repo_files", "git_meta"]
optional = ["language_config"]
schema_version = { repo_tree = "1.0" }

[outputs]
schema = "repository_understanding_output"
format = "json"

[llm_calls]
max_calls = 3
preferred_model = "deepseek-chat"

[dependencies]
uses_outputs_from = []

[quality]
checks = [
    "summary_must_cite_source_files",
    "no_hallucinated_goals"
]
```

### 字段说明

| 字段 | 类型 | 说明 |
|------|------|------|
| `skill.name` | String | 唯一标识，对应目录名 |
| `skill.version` | String | Skill 自身语义版本 |
| `skill.requires_paporot` | String | 对 Paporot Core 的 semver 约束 |
| `skill.timeout_secs` | u32 | 最大执行时间，超时则降级 |
| `inputs.required` | [String] | 必须的数据输入，缺一则跳过该 Skill |
| `inputs.optional` | [String] | 可选输入 |
| `inputs.schema_version` | Map | 每个 input 期望的 schema 版本 |
| `outputs.schema` | String | 输出 schema ID |
| `llm_calls.max_calls` | u32 | LLM 调用次数上界 |
| `dependencies.uses_outputs_from` | [String] | 依赖的上游 Skill 名称 |
| `quality.checks` | [String] | 质量检查规则 ID 列表 |

---

## 6. WASM 标准接口

### Skill 必须导出的函数

| 函数 | 签名 | 说明 |
|------|------|------|
| `paporot_skill_execute` | `fn() -> i32` | Runtime 注入 Inputs 后调用。返回 0=成功 |
| `paporot_skill_output_ptr` | `fn() -> *const u8` | 返回输出 JSON 的内存指针 |
| `paporot_skill_output_len` | `fn() -> usize` | 返回输出 JSON 的字节长度 |
| `paporot_skill_error_ptr` | `fn() -> *const u8` | 返回错误信息的内存指针 |
| `paporot_skill_error_len` | `fn() -> usize` | 返回错误信息的字节长度 |

### 调用时序

```
1. Runtime 解析 skill.toml，完成 Schema Compat
2. 创建 WASM 实例，分配线性内存
3. 写入 required + optional inputs 到共享内存段
4. 调用 paporot_skill_execute()
   ├── Skill 内部通过 paporot_read_input() 读取数据
   ├── 需要时调用 paporot_llm_complete()
   ├── 需要时调用 paporot_cache_put/get()
   └── 返回 0（成功）或 非0（失败）
5. 成功 → 读取 output_ptr/len，反序列化，校验 Schema
6. 失败 → 读取 error_ptr/len，记录日志，降级
7. 缓存输出供下游 Skill 使用
```

### Skill SDK 宏（开发者视角）

Skill 开发者不需要手写导出函数。使用 `paporot-skill-sdk` 宏：

```rust
use paporot_skill_sdk::*;

#[skill(
    name = "repository-understanding",
    version = "0.1.0",
    timeout_secs = 30,
    inputs = ["repo_tree", "repo_files", "git_meta"],
    outputs = "repository_understanding_output"
)]
fn analyze(ctx: SkillContext) -> SkillResult<Value> {
    // 读取输入
    let tree: RepoTree = ctx.read_input("repo_tree")?;
    let files: RepoFiles = ctx.read_input("repo_files")?;

    // 纯计算
    let metadata = locate_metadata(&tree, &files);
    let entrypoints = detect_entrypoints(&tree);

    // LLM 推理
    let purpose = ctx.llm_complete(
        prompt!("推断项目用途", metadata, entrypoints),
        json_schema!({"project_name": String, "purpose": String})
    )?;

    Ok(json!({
        "project_name": purpose.project_name,
        "purpose": purpose.purpose,
        "languages": metadata.languages,
        "frameworks": metadata.frameworks,
        "entrypoints": entrypoints
    }))
}
```

编译：`cargo build --target wasm32-unknown-unknown --release`，产出 `skill.wasm`。

---

## 7. Host Functions

Paporot Runtime 通过 wasmtime 向 WASM 沙箱暴露以下 Host Functions：

### 数据读取

```
fn paporot_read_input(key_ptr: i32, key_len: i32) -> i64

读取 skill.toml 中声明的 input 数据。
参数：指向 key 字符串的指针和长度
返回：打包的 (data_ptr << 32) | data_len
```

### LLM 调用

```
fn paporot_llm_complete(
    prompt_ptr: i32, prompt_len: i32,
    schema_ptr: i32, schema_len: i32
) -> i64

向配置的 LLM 发起一次 Chat Completion 请求。
参数：prompt JSON 指针/长度，output JSON Schema 指针/长度
返回：打包的 (response_ptr << 32) | response_len

Runtime 统一处理：
  - API 调用与重试（最多 3 次）
  - JSON Schema 校验 LLM 输出
  - Token 计数与预算管理
  - 错误分类（网络不通 / 限流 / 服务端错误）
```

### 中间结果缓存

```
fn paporot_cache_put(key_ptr: i32, key_len: i32, value_ptr: i32, value_len: i32)

fn paporot_cache_get(key_ptr: i32, key_len: i32) -> i64

Skill 内部缓存中间计算结果，避免重复计算。
缓存仅在当前管线执行周期内有效。
```

### 日志

```
fn paporot_log(level: i32, msg_ptr: i32, msg_len: i32)

level: 0=DEBUG, 1=INFO, 2=WARN, 3=ERROR
日志写入 .paporot/logs/ 下的当次分析日志文件。
```

---

## 8. DAG 编排引擎

### 功能

DAG Engine 负责：

1. **建图**：读取所有 skill.toml 的 `dependencies.uses_outputs_from`，构建有向无环图
2. **环检测**：发现循环依赖时拒绝执行，报错退出
3. **拓扑排序**：确定执行层级，同层级可并行
4. **调度执行**：逐层执行，每层内并行
5. **超时控制**：每个 Skill 独立计时，超时则降级
6. **失败降级**：任一 Skill 失败，其所有下游 Skill 标记为 SKIPPED，但其他分支继续

### 执行流程

```
输入: skill.toml[]
      + 用户参数 (--prd 等)

步骤:
  1. Registry 扫描 .paporot/skills/*/skill.toml
  2. 版本兼容校验 (requires_paporot semver)
  3. 过滤：缺少 required input 的 Skill 排除
     (如无 --prd 则排除 prd_coverage)
  4. 构建 DAG：
     - 节点 = Skill name
     - 边 = uses_outputs_from
  5. 环检测 → 有环则报错
  6. 拓扑排序 → 执行层级 []
  7. for 每个层级:
       for 每个 Skill in 当前层级 (tokio::spawn 并行):
         a. Schema Compat：转换 input 版本
         b. 创建 WASM 实例
         c. 注入 inputs + 上游 outputs
         d. 调用 paporot_skill_execute()
         e. 校验 output schema
         f. 缓存结果
         g. 失败 → 记录错误日志 → 标记下游
  8. 生成 summary.toml
  9. Report Generator 读取结果
  10. Architecture Doc Generator 合成最终报告
```

### 管线图

```
Repository Understanding ──────────────────────────────┐
         │                                              │
         ▼                                              │
Module Discovery ──────────────────────────────────────┤
         │                                              │
         ▼                                              │
Dependency Analysis ───────┐                            │
         │                 │                            │
         ▼                 ▼                            │
Runtime Flow Analysis  (无其他并行 Skill)               │
         │                                              │
         ▼                                              │
Behavior Boundary Discovery ────→ PRD Coverage ◄────────┘
         │                       (仅 --prd 时激活)
         ▼
Architecture Document Generator
```

同层无并行，但 DAG 引擎天然支持——未来添加只依赖 `repo_tree` 的 Skill（如 Security Analysis）会自动与 Module Discovery 并行。

---

## 9. 核心数据结构

### SkillManifest —— skill.toml 的 Rust 表示

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub name: String,
    pub version: String,
    pub requires_paporot: String,
    pub description: String,
    pub timeout_secs: u32,
    pub inputs: SkillInputs,
    pub outputs: SkillOutputs,
    pub llm_calls: LlmBudget,
    pub dependencies: SkillDeps,
    pub quality: QualityChecks,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInputs {
    pub required: Vec<String>,
    pub optional: Vec<String>,
    pub schema_version: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillOutputs {
    pub schema: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmBudget {
    pub max_calls: u32,
    pub preferred_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDeps {
    pub uses_outputs_from: Vec<String>,
}
```

### SkillRunResult —— 单次 Skill 执行结果

```rust
#[derive(Debug, Clone, Serialize)]
pub struct SkillRunResult {
    pub skill_name: String,
    pub status: SkillRunStatus,
    pub duration_ms: u64,
    pub output_json: Option<String>,
    pub error: Option<SkillError>,
}

#[derive(Debug, Clone, Serialize)]
pub enum SkillRunStatus {
    Ok,
    Skipped,
    TimedOut,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillError {
    pub phase: String,
    pub error_code: String,
    pub detail: String,
    pub suggestion: Option<String>,
}
```

### AnalysisResult —— 一次 `paporot analyze` 的完整结果

```rust
#[derive(Debug, Clone, Serialize)]
pub struct AnalysisResult {
    pub commit_id: String,
    pub timestamp: String,
    pub skill_results: Vec<SkillRunResult>,
    pub summary: AnalysisSummary,
    pub generated_reports: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisSummary {
    pub total_skills: usize,
    pub ok: usize,
    pub skipped: usize,
    pub failed: usize,
    pub total_duration_ms: u64,
    pub risk_level: String,       // "low" | "medium" | "high"
    pub prd_coverage_pct: Option<f32>,
}
```

### DagNode —— DAG 中的节点

```rust
#[derive(Debug, Clone)]
pub struct DagNode {
    pub manifest: SkillManifest,
    pub wasm_path: PathBuf,
    pub deps: Vec<String>,
}
```

---

## 10. Schema 兼容层

### 问题

Paporot Core 升级后，数据结构可能变化（如 `RepoTree` 加了字段），旧 Skill 声明需要 `repo_tree = "1.0"`，但 Core 现在提供 `v3.0`。直接注入会导致 WASM 反序列化失败。

### 策略：Runtime 端适配，Skill 无感

每个 Skill 在 `skill.toml` 中声明期望的 Input Schema 版本。Runtime 维护兼容转换表，在注入数据前自动降级/升级。

### 转换函数示例

```rust
// src/skills/schema_compat.rs

fn compat_repo_tree(raw: &RepoTreeV3, target_version: &str) -> Result<Vec<u8>> {
    match target_version {
        "1.0" => {
            // V3 → V1: 移除 v2/v3 新增字段
            let v1 = RepoTreeV1 {
                root: raw.root.clone(),
                files: raw.files.iter().map(|f| FileEntryV1 {
                    path: f.path.clone(),
                    size: f.size,
                }).collect(),
            };
            Ok(serde_json::to_vec(&v1)?)
        }
        "2.0" => { /* V3 → V2 */ }
        "3.0" => Ok(serde_json::to_vec(raw)?),  // 当前版本，直接透传
        v => Err(anyhow!("unsupported schema version: {}", v)),
    }
}
```

### 兼容结果

- **成功** → 透传转换后的数据，Skill 正常执行
- **失败**（无兼容路径）→ 降级：SKIPPED + 错误日志，不给用户看崩溃

---

## 11. 错误处理与日志

### 降级策略

DAG 管线中任一 Skill 失败时：

- 该 Skill 标记为 Failed / TimedOut
- 所有依赖它的下游 Skill 标记为 Skipped
- 不依赖它的其他分支继续执行
- 最终报告明确标注每个 Skill 状态
- 错误详情写入 `.paporot/logs/`

### 错误分类码

| 错误码 | 含义 | 用户侧排查建议 |
|--------|------|---------------|
| `no_compat_path` | Schema 版本不兼容 | 升级 Skill 或 Core |
| `memory_oob` | WASM 内存越界 | Skill bug，联系开发者 |
| `wasm_trap` | WASM 运行时异常 | 检查 skill.wasm 文件完整性 |
| `network_unreachable` | DNS/连接失败 | 检查网络连接 |
| `http_5xx` | DeepSeek 服务端错误 | 稍后重试 |
| `http_429` | 被限流 | 降低调用频率或升级 API plan |
| `http_401` | API Key 无效 | 检查 `.paporot/config.toml` 中的 api_key |
| `llm_json_parse` | LLM 返回格式不符合 schema | 自动重试，3 次后降级 |
| `timeout` | Skill 执行超时 | 增大 skill.toml 中 timeout_secs |
| `output_schema_fail` | Skill 输出不符合约定 | Skill bug |
| `upstream_skip` | 上游 Skill 失败 | 修复上游问题 |

### 错误日志格式

`.paporot/logs/2026-06-18T15_30_00_analyze.log` 示例：

```
[15:30:01.234] ERROR skill=repository_understanding phase=compat_input
  input_name = "repo_tree"
  skill_requires = "v1.0"
  core_provides = "v3.0"
  reason = "no_compat_path"

[15:30:05.891] ERROR skill=module_discovery phase=wasm_execute
  exit_code = 1
  wasm_error = "memory access out of bounds at offset 0x4A3C"

[15:30:08.123] ERROR skill=dependency_analysis phase=llm_call
  attempt = 3/3
  endpoint = "https://api.deepseek.com/v1/chat/completions"
  model = "deepseek-chat"
  http_status = 502
  reason = "upstream_service_unavailable"
  suggestion = "Check network connectivity or try again later"

[15:30:10.456] WARN  skill=dependency_analysis phase=llm_call
  attempt = 1/3
  http_status = 429
  reason = "rate_limited"
  retry_after_ms = 1000

[15:30:25.789] ERROR skill=behavior_boundary phase=timeout
  timeout_secs = 30
  elapsed_secs = 30.1

[15:30:26.001] WARN  skill=prd_coverage phase=dag_skip
  reason = "upstream 'behavior_boundary' failed"

[15:30:26.500] SUMMARY
  total_skills = 7
  ok = 4
  skipped = 2
  failed = 1
  total_duration_ms = 25498
```

---

## 12. 6 个 Skill 规格

### Skill 1：Repository Understanding

**Goal**：识别项目整体目标、技术栈、入口程序、核心业务能力。

**Inputs**：`repo_tree`, `repo_files`, `git_meta`

**Procedure**：
1. 定位项目元数据（README、Cargo.toml、build scripts）
2. 检测入口点（`src/main.rs`、`bin/*`）
3. 推断项目用途
4. 生成摘要

**Output**：
```json
{
  "project_name": "...",
  "purpose": "...",
  "languages": ["rust"],
  "frameworks": ["clap", "tokio"],
  "entrypoints": ["src/main.rs"],
  "architecture_style_candidates": ["CLI Application", "Modular Pipeline"]
}
```

**Quality Checks**：摘要必须引用实际源文件证据，禁止推断业务目标。

---

### Skill 2：Module Discovery

**Goal**：发现系统中的业务模块和技术模块。

**Inputs**：`repo_tree`, `ast_symbols`, `import_graph`

**Procedure**：
1. 构建命名空间映射
2. 聚类相关文件
3. 推断职责
4. 分类

**Output**：
```json
{
  "modules": [
    {
      "name": "agent",
      "responsibility": "核心调度器，编排三层分析流水线",
      "files": ["src/agent.rs"],
      "category": "Service"
    }
  ]
}
```

**分类**：API | Service | Domain | Storage | Infrastructure | Utility

---

### Skill 3：Dependency Analysis

**Goal**：构建模块依赖图和耦合分析。

**Inputs**：`import_graph`, `symbol_references`, `call_graph`

**Procedure**：
1. 构建 Import Graph
2. 构建 Symbol Graph
3. 计算耦合指标（fan-in、fan-out、循环依赖）
4. 检测架构违规

**Output**：
```json
{
  "dependencies": [
    {"from": "agent", "to": "analysis", "type": "function_call"}
  ],
  "cycles": [],
  "high_coupling_modules": [],
  "architecture_violations": []
}
```

---

### Skill 4：Runtime Flow Analysis

**Goal**：发现端到端业务执行路径。

**Inputs**：`ast`, `call_graph`, `entry_points`

**Procedure**：
1. 识别外部触发器（HTTP、CLI、MQ、Scheduler）
2. 追踪函数调用链
3. 标注 Input → Validation → Business Logic → Persistence → Output
4. 构建执行链

**Output**：
```json
{
  "flows": [
    {
      "name": "paporot analyze",
      "trigger": "CLI",
      "path": ["cli", "agent", "analysis", "llm", "storage"],
      "phases": {
        "input": ["cli"],
        "validation": [],
        "business_logic": ["agent", "analysis"],
        "persistence": ["storage"],
        "output": ["cli"]
      }
    }
  ],
  "mermaid": "flowchart TD\n  CLI --> Agent\n  Agent --> Analysis\n  Analysis --> LLM\n  Agent --> Storage"
}
```

---

### Skill 5：Behavior Boundary Discovery

**Goal**：发现影响可观测行为的组件。这是 Anthropic 风格行为版本控制的核心。

**Inputs**：`ast`, `git_diff`, `call_graph`

**Procedure**：
1. 识别用户可见输出（API Response、File Output、DB State、Event Emission）
2. 沿调用链反向追踪
3. 标注 behavioral / non-behavioral

**Output**：
```json
{
  "behavioral_modules": ["agent", "analysis", "commands"],
  "non_behavioral_modules": ["llm", "storage"],
  "behavioral_functions": ["agent.execute_pipeline", "analysis.analyze_diff"],
  "non_behavioral_functions": ["llm.log_request", "storage.write_cache"],
  "changed_boundaries": [
    {
      "function": "agent.execute_pipeline",
      "change_type": "modified",
      "user_visible": true,
      "risk": "medium"
    }
  ]
}
```

判断标准：
- **Behavioral**：改这里，用户能直接感知变化（API 返回变了、登录流程变了）
- **Non-behavioral**：改这里，用户感知不到（日志格式、内部缓存策略）

---

### Skill 6：Architecture Document Generator

**Goal**：聚合前 5 个 Skill 的输出，生成人类可读的架构文档和 HTML Dashboard。

**Inputs**：前 5 个 Skill 的全部 Output + `AnalysisSummary`

**Output**：
- `architecture.md`
- `behavior.md`
- `dashboard.html`
- `data/analysis_result.json`

### 条件 Skill：PRD Coverage

**Goal**：对比 PRD 需求与代码实现，计算需求覆盖率。

**Inputs**：`prd_content`, `repo_tree`, `ast_symbols`, `behavior_boundary`

**Activation**：仅当用户提供 `--prd` 参数时激活。

**Output**：
```json
{
  "total_requirements": 12,
  "implemented": 7,
  "partial": 3,
  "not_implemented": 2,
  "coverage_pct": 70.8,
  "details": [
    {
      "requirement": "用户登录",
      "status": "implemented",
      "mapped_to": ["auth::login"]
    }
  ]
}
```

---

## 13. 报告输出与 Dashboard

### 输出目录结构

```
.paporot/reports/
  dashboard.html            ← 双击打开，无需服务器
  data/
    analysis_result.json    ← 完整分析数据（机器可读）
    dependency_graph.json   ← 依赖图数据
    runtime_flows.json      ← 运行时流程数据
    behavior_boundary.json  ← 行为边界数据
  architecture.md           ← 架构报告
  behavior.md               ← 行为边界报告
  coverage.md               ← PRD 覆盖率报告
```

### Dashboard 页面（5 个标签页）

| 标签 | 图类型 | 内容 |
|------|--------|------|
| **版本变更** | 时间线图 | 多次 `analyze` 的行为演进，节点颜色=变更幅度，连线颜色=风险等级 |
| **模块地图** | 卡片 | 项目有哪些模块、各自职责、分类 |
| **依赖关系** | 有向网络图 | 模块间调用关系。箭头=调用方向，颜色=耦合强度 |
| **运行时流程** | 泳道图 | 请求如何穿透模块，标注每阶段的职责 |
| **行为边界** | 双色标记 | 红色=行为核心（改了用户能感知），灰色=支撑模块 |

### 图说明（用户友好）

**依赖关系图说明**：
> 这张图展示了模块之间的调用关系。**箭头从"调用者"指向"被调用者"**。
>
> 例如 `Agent → Analysis` 表示 Agent 模块会调用 Analysis 模块的代码。如果 Analysis 改了，Agent 的行为可能受影响。
>
> - 被越多模块指向 → 越底层、越关键 → 改它风险更大
> - 指向越多模块 → "编排者"角色 → 最容易受别人改动影响
> - 出现环（A→B→C→A）→ 模块互相依赖 → 解耦时优先处理

**行为边界图说明**：
> - **红色节点**（行为核心）：改这里，用户能直接感知——登录流程变了、API 返回变了
> - **灰色节点**（支撑模块）：改这里，用户感知不到——日志格式、内部缓存策略
>
> 一次提交同时改红色和灰色 → 功能性变更 + 工程优化 → 分开审查更清晰

**版本时间线图说明**：
> 模仿 Git 提交图。节点颜色深浅 = 变更幅度（深=大改）。连线颜色 = 风险等级（绿=低、黄=中、红=高）。
>
> `v3 → v4` 变红 → 该次 Agent 提交有高风险改动 → 点击节点查看具体改了哪个模块

### 技术实现

- 纯静态 HTML，内嵌 JSON 数据，不依赖服务器
- Mermaid.js 渲染依赖图、流程图
- Chart.js 渲染时间线
- `--format mermaid` 可独立导出图为 `.mmd` 文件

---

## 14. 实现阶段划分

| 阶段 | 内容 | 产出 | 验证标准 |
|------|------|------|---------|
| **Phase 0** | `src/skills/types.rs` 类型定义、Registry（扫描 skill.toml）、Schema Compat 基础 | `paporot skill list` 列出已安装 Skill | 编译通过，单元测试覆盖 |
| **Phase 1** | DAG Engine + WASM Host（wasmtime 集成） | Skill 可被加载并空跑（mock execute） | 集成测试：DAG 拓扑排序正确 |
| **Phase 2** | Host Functions（read_input、llm_complete、cache_put/get、log） | Skill 能拿到真实 Core 数据 | 集成测试：Skill 读取 repo_tree 成功 |
| **Phase 3** | 6 个 Skill 编写（paporot-skill-sdk） + 编译为 .wasm | 管线跑通 | `paporot analyze` 完成全管线 |
| **Phase 4** | Report Generator + Dashboard 模板 + architecture.md 模板 | 完整报告产出 | Dashboard 在浏览器中正常渲染 |
| **Phase 5** | CLI 集成（`analyze` 命令、`review` 改造、`coverage` 保留） | UX 闭环 | 端到端测试通过 |
| **Phase 6** | 错误日志系统 + 降级机制 + 错误码分类 | 容错完整 | 模拟网络断开、LLM 超时，验证降级行为 |

### 不做的（MVP Non-Goals）

- Skill Marketplace 远程分发
- 用户自定义 Skill SDK 工具链
- 多语言 Skill 开发（仅 Paporot 团队编写）
- 10 万行以上大型仓库支持
- 增量分析

---

## 15. 决策日志

| # | 决策点 | 选项 | 选择 | 理由 |
|---|--------|------|------|------|
| 1 | Skill 运行形态 | WASM / 子进程 / HTTP 微服务 / 嵌入脚本 | WASM | 隔离性好，多语言可编译，分发二进制安全 |
| 2 | Skill 分发 | Registry / 本地 / 混合 / GitHub Release | 本地 only | MVP 阶段 Skill 由 Paporot 团队编写，不需要分发平台 |
| 3 | Skill 编排 | DAG 引擎 / 线性流水线 / 事件驱动 | DAG 引擎 | 内部自动调度，用户只需 `paporot analyze` |
| 4 | Core↔Skill 通信 | 共享内存 / Host Functions / 文件交换 | 共享内存预注入 + Host Functions 按需读取 | 性能最优，按需灵活 |
| 5 | Skill 调 LLM | Host Function / 内嵌 Prompt / WASI HTTP | Host Function | 统一管理 API Key、重试、Token 预算 |
| 6 | LLM 配置 | 多种来源 | `.paporot/config.toml` | 用户一次配置，DeepSeek 优先 |
| 7 | 版本兼容 | semver 校验 / Schema 兼容层 / 版本绑定 | Schema 兼容层 + 充分容错 | 用户体验最好，版本不匹配不崩溃 |
| 8 | 错误处理 | 降级继续 / 快速失败 / 重试+降级 / 用户选择 | 降级继续 + 错误日志 | 用户总能看到部分结果，不会空手而归 |
| 9 | Skill 物理形态 | wasm+toml / 自包含wasm / 声明式DSL | `.wasm` + `skill.toml` | 关注点分离，元数据可读可改 |
| 10 | 性能目标 | 30s / 2min / 5min | ~2 分钟全管线 | 可接受泡咖啡的等待，每个 Skill 一次 LLM 调用 |
| 11 | 增量 vs 全量 | 增量 / 全量 | 全量重跑 | MVP 简化，2 分钟可接受 |
| 12 | CLI 兼容 | 统一 analyze / 废弃旧命令 / 保留+改造 | `review` 改为调 Skill Runtime，`coverage` 保留 | 向后兼容，两种场景各有入口 |
