# Paporot v2 PRD

> **Paporot — AI Generated Software Behavior Version Control & Auditing System**
>
> Git 管代码，Paporot 管理解。v2 重构目标：以确定性算法为骨架，以 LLM 为语义皮肤，以轨迹分析为差异化壁垒。

---

## 目录

1. [重构动机](#1-重构动机)
2. [调整后架构概览](#2-调整后架构概览)
3. [子系统详解](#3-子系统详解)
   - [3.1 内置预处理器（确定性）](#31-内置预处理器确定性)
   - [3.2 Snapshot 版本化引擎](#32-snapshot-版本化引擎)
   - [3.3 Evidence 决策溯源系统](#33-evidence-决策溯源系统)
   - [3.4 Skill Pipeline（5 个 WASM Skill）](#34-skill-pipeline5-个-wasm-skill)
   - [3.5 报告生成器](#35-报告生成器)
   - [3.6 Trajectory 轨迹子系统](#36-trajectory-轨迹子系统)
4. [CLI 命令映射](#4-cli-命令映射)
5. [数据流图](#5-数据流图)
6. [LLM 调用预算分析](#6-llm-调用预算分析)
7. [文件系统布局](#7-文件系统布局)
8. [死代码清理清单](#8-死代码清理清单)
9. [实现阶段划分](#9-实现阶段划分)
10. [决策日志](#10-决策日志)

---

## 1. 重构动机

### 发现的问题

当前 Paporot v1 的 WASM Skill 系统存在以下问题：

1. **LLM 过度依赖**：每个 Skill 至少调用 1 次 LLM，即使核心分析逻辑可以用确定性算法完成。
2. **能力定位重合**：`behavior-boundary-discovery` Skill 用正则提取函数名后，又调用 LLM 做"二次确认分类"——LLM 覆盖了确定性结果。
3. **死代码积累**：原生架构的 `agent.rs`、`commands/`（16 个子命令）、`trace/`、`trajectory/`、`evaler/`、`evidence/` 等约 10,000+ 行代码完全未被 WASM 管道使用。
4. **Snapshot 版本化能力缺失**：WASM 管道每次 `analyze` 是一次性的，没有版本历史，无法做 diff/regression 等核心行为版本控制操作。
5. **核心差异化功能未接入**：`trace/` 和 `trajectory/` 是 Paporot "Behavior Version Control" 定位的关键，但完全未被集成。

### 设计原则

| 原则 | 说明 |
|------|------|
| **确定性优先** | 能用正则/规则/图算法的，绝不用 LLM |
| **LLM 仅做语义** | LLM 只用于"理解"（推断项目用途、描述模块职责、命名 flow、写总结） |
| **证据链透明** | 每个 Capability 的推断过程完全可溯源（L1/L2/L3 三级证据） |
| **版本化内置** | Snapshot 版本链是基础设施，不是可选功能 |
| **轨迹为壁垒** | Agent 行为轨迹分析是 Paporot 区别于所有代码分析工具的独特能力 |

---

## 2. 调整后架构概览

```
┌────────────────────────────────────────────────────────────┐
│                    外部输入源                               │
│  Git Repo (diff)  │  LLM API  │  Agent Traces  │  PRD     │
└────────────────────────┬───────────────────────────────────┘
                         │
┌────────────────────────▼───────────────────────────────────┐
│              Paporot Native Binary (wasmtime)               │
│                                                            │
│  Host Functions:                                           │
│  host_read_file  host_write_file  host_llm_call            │
│  host_verify_contract  host_capture_evidence  [NEW]        │
│  collect_sources  call_deepseek_api                        │
└────────────────────────┬───────────────────────────────────┘
                         │
┌────────────────────────▼───────────────────────────────────┐
│          WASM Sandbox (paporot-core.wasm)                  │
│                                                            │
│  ┌──────────────────────────────────────────────┐         │
│  │  内置能力 [NEW - 从死代码回收]                │         │
│  │                                              │         │
│  │  DiffPreprocessor → L1 AST 正则引擎          │         │
│  │  → L2 Rules 规则引擎 → L3 LLM Bridge(可选)   │         │
│  │  → Snapshot Engine (v1/v2/v3...)             │         │
│  │  → Evidence 决策溯源                         │         │
│  └──────────────────────────────────────────────┘         │
│                         │                                  │
│  ┌──────────────────────▼────────────────────────┐        │
│  │  Skill Pipeline (DAG 编排, 5 个 WASM Skill)   │        │
│  │                                              │        │
│  │  S1: repository-understanding  LLM x1        │        │
│  │  S2: module-discovery           LLM x1        │        │
│  │  S3: dependency-analysis        LLM x0 (纯算法)│       │
│  │  S4: runtime-flow-analysis      LLM x1        │        │
│  │  S5: architecture-doc-generator LLM x1        │        │
│  │                                              │        │
│  │  ✗ 移除: behavior-boundary-discovery        │        │
│  └──────────────────────────────────────────────┘        │
│                         │                                  │
│              ┌──────────▼──────────┐                      │
│              │  Report Generator   │                      │
│              │  JSON / MD / HTML   │                      │
│              └─────────────────────┘                      │
└──────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────────┐
│         Trajectory Subsystem [NEW - 重点功能, Native]       │
│                                                            │
│  Trace Import → P1 Feature Vector → P2 Coupling Graph     │
│  → State Graph Builder → Evaler 退化检测                   │
│  → Alignment Engine (轨迹差异对比)                         │
│                                                            │
│  全确定性算法，零 LLM 调用                                  │
└────────────────────────────────────────────────────────────┘
```

### 颜色约定

| 颜色 | 含义 |
|------|------|
| 绿色 | 新增 / 从死代码回收 |
| 蓝色 | 已有（保持不变） |
| 红色 | 已废弃 / 被取代 |
| 紫色 | 轨迹子系统（重点功能） |

---

## 3. 子系统详解

### 3.1 内置预处理器（确定性）

**来源**：回收自 `src/analysis/`（l1_ast、l2_rules、l3_llm_bridge、preprocessor、types）

**定位**：paporot-core 内置能力，在所有 Skill 执行前自动运行。

**类型定义**：预处理器类型（`RawChange`、`RuleMatch`、`FileChange`、`ChangeType` 等）统一放置在新 crate `paporot-analysis-types`（`serde` + `serde_json`）中，同时被 paporot-core（wasm32-wasip1）和 native binary（x86_64）引用，保证类型一致性。

**数据通道**：预处理器执行完毕后，将 `RawChange[]` + `RuleMatch[]` + `Evidence[]` 序列化为 JSON，通过 `host_write_file` 写入 `.Paporot/work/preprocessor_output.json`。Skill Pipeline 通过已有的输入键机制读取该文件。

**WASM 体积**：引入 `regex` crate 预计增加 100-200KB，WASM 二进制体积上限设定为 1MB，当前预估 ~500KB，可接受。

#### 3.1.1 DiffPreprocessor

```
输入:  git diff (unified format)
输出:  FileChange[]

功能:
- 按文件边界拆分 unified diff
- 解析 hunk header (@@ -old +new @@)
- 识别文件语言 (Rust/TS/Python/Go/Java/Unknown)
- 识别变更类型 (Added/Deleted/Modified/Renamed)
- 生成 DiffSummary (files_changed, additions, deletions, by_language)
```

#### 3.1.2 L1 AST 正则引擎

```
输入:  FileChange[]
输出:  RawChange[] (带置信度分数)

覆盖范围:
┌──────────┬────────────────────────────────────────────┐
│ 语言     │ 支持的语法结构                              │
├──────────┼────────────────────────────────────────────┤
│ Rust     │ pub fn, pub struct, pub enum, pub trait,   │
│          │ use, pub const                             │
│ TypeScript│ export function, class, import             │
│ Python   │ def, class                                 │
│ Go       │ func (大写), type struct                    │
│ Java     │ public/private method, class                │
│ 通用     │ HTTP 路由注册, 配置文件键值变更              │
└──────────┴────────────────────────────────────────────┘

置信度计算:
- 完整匹配公开函数签名: 0.9
- 匹配函数名但不完整签名: 0.7
- 匹配类/结构体: 0.8
- 匹配 import: 0.85
- HTTP 路由: 0.75
- 配置变更: 0.6

输出 RawChange 包含:
- id, source, change_type (18 种变更类型)
- file_path, language, line_start, line_end
- symbol_name, old_signature, new_signature
- confidence, module, tags
```

#### 3.1.3 L2 Rules 规则引擎

```
输入:  RawChange[]
输出:  RuleMatch[]

规则类别:
┌──────────────┬──────┬──────────────────────────────────┐
│ 类别         │ 数量 │ 示例规则                          │
├──────────────┼──────┼──────────────────────────────────┤
│ Security     │  3   │ 认证逻辑变更、权限/guard 变更、   │
│              │      │ 加密相关变更                       │
│ Breaking     │  4   │ 公共 API 签名变更、数据结构字段   │
│              │      │ 删除、Trait 方法变更、枚举变体删除 │
│ Performance  │  2   │ 关键路径函数变更、循环内新增 I/O  │
│ Data         │  2   │ Schema 变更、序列化格式变更       │
│ Error        │  2   │ 错误类型新增/删除、错误处理变更    │
│ Dependency   │  2   │ 外部依赖版本变更、新增外部依赖     │
│ Architecture │  2   │ 跨层依赖、循环依赖引用             │
└──────────────┴──────┴──────────────────────────────────┘

触发器系统:
- SymbolMatches { pattern }      — 符号名模糊匹配
- ChangeTypeIn [types]           — 变更类型白名单
- FilePathMatches { pattern }    — 文件路径 glob 匹配
- ContentContains { pattern }    — 签名内容匹配
- And(a, b), Or(a, b), Not(inner) — 组合触发器

每条规则命中输出:
- rule_id, raw_change_id, matched_tags
- severity (Critical/High/Medium/Low/Info)
- category, description
```

#### 3.1.4 L3 LLM Bridge（可选，低置信度残留）

```
触发条件: L1+L2 已覆盖所有高置信度变更时跳过
输入:     RawChange[] (confidence < 0.5)
输出:     LlmFragment[] → Capability[]

LLM 调用: 1 次（仅处理低置信度残留）
LLM 通道: 通过 WASM FFI 调用 `host_llm_call`（而非直接 reqwest），复用现有 LLM 基础设施
用途:     对 L1 正则无法完全确定的变更片段做语义补充
Prompt:   从废弃的 `src/prompts.rs` 中提取 `SYSTEM_PROMPT_BEHAVIOR_EXTRACTOR` 和 `build_extraction_prompt`，迁移到 paporot-core
回退:     LLM 不可用时降级为 L1+L2 only（不影响核心功能）
```

### 3.2 Snapshot 版本化引擎

**来源**：回收自 `src/storage.rs` + `src/graph.rs` + `src/types.rs`

**定位**：paporot-core 内置能力，每次 `analyze` 自动持久化。

**架构**：拆分为两个职责清晰的模块：
- `SnapshotStore`：纯存储（save / load_by_version / load_latest / list_versions_sorted），管磁盘 I/O
- `SnapshotAnalyzer`：纯分析（diff / coverage / regression / risk / graph / evolution），只消费 `&[BehaviorSnapshot]` 切片，纯函数，零 I/O 依赖，极其易测

#### 数据模型

```json
{
  "schema_version": 3,
  "version_id": "v42",
  "git_commit": "abc1234",
  "git_ref": "refs/heads/main",
  "message": "添加 JWT 认证",
  "created_at": "2026-06-24T10:30:00Z",
  "capabilities": [
    {
      "id": "cap_auth_001",
      "name": "JWT Token-based Authentication",
      "description": "用户登录后颁发 JWT，支持短时有效期和刷新",
      "status": "new",
      "module": "auth",
      "confidence": 0.95,
      "evidence": ["src/auth/jwt.rs:15-45", "L1:FunctionAdded"],
      "categories": ["authentication", "security"],
      "depends_on": [{"capability_id": "cap_user_001", "relation": "uses"}],
      "evolved_from": null
    }
  ],
  "prd_coverage": {
    "percentage": 80.0,
    "total_items": 5,
    "covered_items": 4,
    "details": [...]
  },
  "dependency_graph": {
    "edges": [...],
    "nodes": {...},
    "evolution_chains": {...}
  }
}
```

#### 功能

| 功能 | 说明 | 实现优先级 |
|------|------|-----------|
| `save` | 保存 Snapshot 到 `.Paporot/snapshots/v{N}.json` | P0 |
| `load_by_version` | 按版本 ID 加载 | P0 |
| `load_latest` | 加载最新 Snapshot | P0 |
| `list_versions_sorted` | 按自然序 (v1 < v2 < v10) 列出所有版本 | P0 |
| `diff` | 两个 Snapshot 的行为差异（Capability 新增/修改/删除） | P1 |
| `coverage` | PRD 需求覆盖率计算 | P1 |
| `regression` | 基于版本链回归检测 | P1 |
| `risk` | 当前 Snapshot 风险评估（基于规则命中） | P1 |
| `graph` | 独立依赖图索引（循环检测、扇入扇出、影响分析） | P1 |
| `evolution` | Capability 跨版本演化链追踪 | P2 |

#### 依赖图索引

**两类依赖图**：

| 依赖图 | 来源 | 粒度 | 用途 |
|--------|------|------|------|
| 模块级依赖图 | Skill `dependency-analysis` 产出 | 模块级 | 架构文档、Excalidraw 可视化 |
| Capability 级依赖图 | Snapshot Engine 维护 | Capability 级 | 行为影响分析、演化链追踪 |

**关联机制**：Skill 产出的模块级依赖关系作为 Capability 依赖推断的上游输入——Capability 的 `depends_on` 自动推导自其所涉及模块的依赖关系。两个图独立存储但单向餵入。

```
DependencyGraph {
    edges:  DependencyEdge[]     // 所有依赖边
    nodes:  HashMap<capability_id, NodeMeta>  // 节点元数据
    evolution_chains: HashMap<capability_id, Vec<version_id>>
    // 每周合入时自动更新: 合入 → 新 snapshot → graph.json + evolution_chains 刷新
}

DependencyEdge {
    from_capability_id, to_capability_id
    relation: Uses | Extends | Implements | Calls | Publishes
    confidence: f32
}
```

---

### 3.3 Evidence 决策溯源系统

**来源**：回收自 `src/evidence/`

**定位**：绑定到 L1+L2 预处理器，为每个 Capability 提供三层推断证据链。

```
Evidence {
    capability_id:  String
    snapshot_version: String
    l1:  L1Evidence[]     // AST 符号提取证据（确定性）
    l2:  L2Evidence[]     // 规则匹配证据（确定性）
    l3:  Option<L3Evidence> // LLM 补充证据（可选）
    confidence: EvidenceConfidence
    generated_at: String
}

L1Evidence {
    symbol, file_path, line
    kind: Function | Struct | Enum | Trait | Implementation | Module
    visibility: String
}

L2Evidence {
    rule_id, rule_name, severity
    matched_tags: Vec<String>
}

L3Evidence (only when LLM available) {
    prompt_hash: String    // 提示词指纹（可复现）
    fragment: String       // LLM 原始输出片段
    model: String          // 使用的模型
}
```

**用途**：
- 用户审查时可以溯源"Paporot 为什么认为这是一个新 Capability"
- 合约验证失败的 root cause 分析
- 回归检测的变更根因定位

---

### 3.4 Skill Pipeline（5 个 WASM Skill）

**保留 5 个 Skill，移除 `behavior-boundary-discovery`（被 L1+L2 取代）。**

#### DAG 编排

```
Layer 0 (独立执行):
  S1: repository-understanding     ─┐
  S2: module-discovery             ─┤ 并行
                                     │
Layer 1 (依赖 S1, S2):              │
  S3: dependency-analysis          ←┤
                                     │
Layer 2 (依赖 S3):                  │
  S4: runtime-flow-analysis        ←┘
                                     │
Layer 3 (聚合所有上游):
  S5: architecture-doc-generator
```

#### 各 Skill 详细规格

| Skill | LLM 调用 | 确定性部分 | LLM 的用途 | 输入 | 输出 |
|-------|---------|-----------|-----------|------|------|
| **S1: repository-understanding** | 1 次 | 语言/框架检测、入口文件扫描 | 推断项目用途和架构风格 | repo_tree, repo_files, git_meta | project_name, purpose, languages, frameworks, entrypoints |
| **S2: module-discovery** | 1 次 | 目录分组、符号提取、category 预分类 | 为每个模块生成 2 句职责描述 | repo_tree, ast_symbols, import_graph | modules[], module_count |
| **S3: dependency-analysis** | **0 次** | DFS 环检测、扇入扇出统计、Excalidraw 依赖图生成 | 无 | import_graph, symbol_references, call_graph | dependencies[], cycles[], excalidraw, total_dependencies |
| **S4: runtime-flow-analysis** | 1 次 | DFS 调用链追踪、关键词分 5 个 phase、Mermaid 生成 | 为每条 flow 生成人类可读的描述名 | ast, call_graph, entry_points | flows[], mermaid, flow_count |
| **S5: architecture-doc-generator** | 1 次 | 聚合上游结果、section 状态统计 | 综合所有分析结果生成 2-3 段高层架构总结 | 所有上游 Skill 输出 | generated_files[], sections[], summary |

---

### 3.5 报告生成器

**已有 (`src/report/`)，无缝接入。**

**报告章节结构**：预处理结果独立成章在前，Skill 产出作为深度分析在后：

```
Chapter 1: Behavior Changes (L1 AST + L2 Rules 产出)
  - 变更摘要 (DiffSummary)
  - RawChange[] 分类展示
  - RuleMatch[] 风险标注
  - Risk Level 总评

Chapter 2: Architecture Deep Dive (Skill Pipeline 产出)
  - Project Overview (S1)
  - Module Catalog (S2)
  - Dependency Graph (S3)
  - Runtime Flows (S4)
  - Architecture Summary (S5)
```

```
输入: 预处理器输出 + Skill 执行结果 + DAG 层描述
输出:
  .Paporot/reports/analysis_result.json   — 结构化 JSON
  .Paporot/reports/architecture.md        — Markdown 报告
  .Paporot/reports/dashboard.html         — 暗色主题可视化面板
```

---

---

### 3.5-bis Feedback 人机验证回路

**来源**：回收自 `src/commands/feedback.rs` + `types.rs` 中的 FeedbackStore/BehaviorReview/ReviewVerdict

**定位**：Native 命令，纯 CRUD 操作。用户在 Paporot 自动分析后快速审查纠正。

**工作流**：

```bash
paporot analyze                              # 自动生成 .Paporot/reviews/review_v42.toml
# 用户用编辑器打开 review_v42.toml，花 2 分钟填写
paporot feedback apply                       # Paporot 读取 TOML 并写回 Snapshot
paporot feedback stats                       # 查看审查统计
```

**TOML 审查文件格式**（`.Paporot/reviews/review_v42.toml`）：

```toml
# 自动生成于 2026-06-24T10:30:00Z
# 版本: v42, Capabilities: 15 个
# 只改你要纠正的行，不改的留空即可

[approve]
cap_auth_001 = "ok"
cap_pay_002 = "ok"

[reject]
cap_pay_003 = "仅重构，无行为变更"

[correct.cap_user_005]
name = "User Profile Cache Invalidation"
description = "用户资料更新后自动刷新缓存"
status = "new"

[flag]
cap_weird_001 = "可能是实验性代码，下个版本再确认"
```

**纠错操作**：

| 操作 | 含义 | 持久化效果 |
|------|------|-----------|
| `[approve]` | 确认正确 | Capability.verified_by / verified_at 标记 |
| `[reject]` | 标记为误报 | Capability.status = Deleted，删除原因写入注释 |
| `[correct]` | 修正名称/描述/状态 | 直接覆盖 Capability 对应字段 |
| `[flag]` | 标记待定 | Capability.tags 附加 "needs-review" |

---

### 3.6 Trajectory 轨迹子系统

**来源**：回收自 `src/trace/` + `src/trajectory/` + `src/evaler/`

**定位**：Native 二进制独立子系统，Paporot 的核心差异化能力。

**原则：全确定性算法，零 LLM 调用。**

**Trace ↔ Snapshot 关联机制**：

两个系统紧耦合——`paporot analyze` 执行时，Snapshot 创建后自动寻找对应的 Agent Trace 并关联，无需用户手动指定。

**三级自动匹配算法**：

```
Level 1: Commit hash 精确匹配 (确定性最高)
  从 Trace 的 tool_calls 中扫描 bash 命令，提取 git commit hash
  → 与 Snapshot.git_commit 直接匹配，命中则直接关联

Level 2: 文件集重叠度 (L1 未命中时回退)
  计算 Trace 中 write 操作的文件路径集合
  ∩ Snapshot 中 Capability.evidence 涉及的文件路径集合
  → Jaccard 相似度 ≥ 0.5 则认为匹配

Level 3: 时间窗口 (L1/L2 均未命中时兜底)
  |Trace.finished_at - Snapshot.created_at| ≤ 30 分钟
  → 取时间最接近的 Trace
```

匹配结果持久化到 `.Paporot/snapshots/trace_map.json`：

```json
{
  "v1": "trace_20260624_001",
  "v2": "trace_20260624_002",
  "v3": null
}
```

`null` 表示该 Snapshot 没有找到对应 Trace（Agent 未导出轨迹或匹配度不够）。Capability 通过 `evidence_trace_ids` 字段关联匹配到的 Trace。

#### 3.6.1 Trace Import & Storage

```
功能: 解析 Claude/OpenAI/DeepSeek 的 Agent 执行日志，存入结构化存储
输入: JSONL 文件 (Agent 执行日志)
输出: BehaviorTrace[] → 持久化到 .Paporot/traces/

适配器:
- Claude Adapter   (claude_types.rs)
- OpenAI Adapter   (openai_types.rs)
- DeepSeek Adapter (deepseek_types.rs)
- 自动检测: auto_detect() 根据内容特征自动选择适配器

BehaviorTrace 包含:
- trace_id, tool_call_sequence
- timing (每个 step 的耗时)
- token_usage (prompt/completion tokens)
- file_changes (每次 write 操作的文件路径和行数)
- error_events (异常/重试记录)
- capability_id (可选，关联到 Snapshot 中的 Capability)
```

#### 3.6.2 P1: 轨迹特征向量

```
功能: 从 BehaviorTrace 提取数值特征向量，做序列分析
输入: BehaviorTrace[]
输出: FeatureVector[] (聚类结果 + 异常标记)

特征维度:
- tool_call_count          (工具调用总数)
- tool_call_diversity      (不同工具类型数)
- total_duration_secs      (总耗时)
- total_tokens             (总 Token 消耗)
- output_length            (最终输出长度)
- error_count              (错误/重试次数)
- file_change_count        (修改文件数)
- avg_thinking_duration    (平均思考时间)
- tool_call_sequence_entropy (调用序列熵值)
- phase_transition_count   (阶段切换次数)

算法:
- 序列度量: DTW / Edit Distance 对比两条轨迹的相似度
- 聚类: K-Means / DBSCAN 发现"好轨迹"和"坏轨迹"的模式
- 异常检测: Isolation Forest / Z-Score 标记偏离正常模式的轨迹
```

#### 3.6.3 P2: 行为耦合图

```
功能: 分析 Capability 之间的共变关系，构建行为耦合网络
输入: Trace[] + Capability[] 映射
输出: CouplingGraph

分析维度:
- Co-change Analysis: 哪些 Capability 经常被同一个 Trace 修改
- Coupling Strength: 共变频率 → 归一化耦合强度
- Similarity: 两条 Trace 的 Capability 变更模式相似度
- Impact Analysis: 修改 Capability A 对 Capability B/C/D 的波及概率

输出结构:
CouplingGraph {
    nodes:  CapabilityNode[]    // 能力节点
    edges:  CouplingEdge[]      // 耦合边
    clusters: CapabilityCluster[] // 耦合群组
    matrix:  f64[][]            // 耦合矩阵 (全连接)
}
```

#### 3.6.4 State Graph Builder

```
功能: 从 BehaviorTrace 构建 Agent 行为状态图
输入: BehaviorTrace[]
输出: BehaviorStateGraph

状态定义 (基于 tool_call 序列自动分段):
- Phase::Planning     (搜索、读文件、分析)
- Phase::Coding       (写文件、编辑代码)
- Phase::Testing      (运行测试、检查输出)
- Phase::Debugging    (读错误日志、修改代码)
- Phase::Reviewing    (git diff、自我审查)

状态过渡:
- 从 Trace 中提取相邻 tool_call 的类型变化
- 构建状态转移矩阵 (5x5)
- 发现常见的状态转移路径
- 识别异常的状态跳跃
```

#### 3.6.5 Evaler 退化检测规则引擎

```
功能: 对比同一 Capability 在不同 Trace 中的表现，检测行为退化
输入: BehaviorTrace[] (同 Capability 的历史轨迹)
输出: EvalVerdict

内置规则 (R001-R005):
┌────────┬──────────────────────────┬──────────┬────────────┐
│ 规则ID │ 描述                      │ 严重程度 │ 阈值        │
├────────┼──────────────────────────┼──────────┼────────────┤
│ R001   │ 工具调用次数暴增          │ High     │ +100%       │
│ R002   │ 工具调用次数小幅增长      │ Medium   │ +50%        │
│ R003   │ 输出长度严重缩减          │ Critical │ -50%        │
│ R004   │ Token 消耗翻倍            │ High     │ +100%       │
│ R005   │ Output Token 大幅减少     │ Medium   │ -30%        │
└────────┴──────────────────────────┴──────────┴────────────┘

EvalVerdict:
- status: Pass | Degraded | Critical
- hits: DegradeRuleHit[]       // 触发的退化规则
- score: f32                  // 综合退化评分 0-100
```

#### 3.6.6 Alignment Engine

```
功能: 对齐两条 BehaviorTrace，生成 TrajectoryDiff
输入: BehaviorTrace A, BehaviorTrace B
输出: TrajectoryDiff

对齐策略:
- 按 tool_call 类型对齐 (Step Alignment)
- 按代码修改文件对齐 (File Alignment)
- 按时间阶段对齐 (Phase Alignment)

TrajectoryDiff:
- aligned_steps: 对齐后的步骤对
- unmatched_in_a: A 有 B 没有的步骤
- unmatched_in_b: B 有 A 没有的步骤
- similarity_score: 整体相似度 0-1
- diff_by_phase: 按阶段分解的差异
```

---

## 4. CLI 命令映射

### Dispatch 机制

采用 **native command registry + WASM 兜底转发** 模式：

```rust
// main.rs
let native_commands: HashMap<&str, fn(&[String])> = HashMap::from([
    ("trace", cmd_trace),
    ("trajectory", cmd_trajectory),
    ("feedback", cmd_feedback),
]);

if let Some(handler) = native_commands.get(command) {
    handler(args);           // Native 直接执行
} else {
    run_wasm(args);          // 转 WASM 沙盒 (analyze, skill, snapshot, diff, coverage, ...)
}
```

- 新增 native 命令只需加一行到 registry
- 新增 WASM 命令（如 `snapshot list`、`diff`）完全不需要改 `main.rs`，paporot-core 内部新增分支即可

### 当前命令（保持不变）

```bash
paporot analyze                    # 完整分析管线（预处理器 + Skill）
paporot analyze --prd docs/prd.md # 含 PRD 覆盖率
paporot skill list                # 列出已安装的 Skill
```

### 从 dead code 恢复的命令（功能逻辑迁移到 paporot-core 内置能力）

```bash
# Snapshot 版本管理
paporot snapshot list              # 列出所有 Snapshot 版本
paporot snapshot show --version v3 # 查看指定版本的详情

# 行为差异
paporot diff --from v1 --to v2    # Capability 级行为差异
paporot diff --latest             # 最新版本 vs 上一版本

# PRD 覆盖率
paporot coverage -p docs/prd.md   # 精准覆盖率分析

# 回归检测
paporot regression                 # 自动检测最新 vs 上一版本的退化

# 风险评估
paporot risk                       # 基于规则命中的风险评分
paporot risk --version v3

# 依赖图
paporot graph show                 # 展示依赖图
paporot graph cycles              # 检测循环依赖
paporot graph impact --cap cap_x  # 影响分析
```

### Feedback 审查命令（Native）

```bash
paporot feedback apply             # 读取 .Paporot/reviews/review_v{N}.toml 并写回 Snapshot
paporot feedback stats             # 查看审查统计
paporot feedback show --version v3 # 查看指定版本的审查记录
```

### 轨迹子系统独立命令（Native）

```bash
# Trace 管理
paporot trace import <file.jsonl>                  # 导入 Agent 执行日志
paporot trace import --adapter claude <file.jsonl> # 指定适配器
paporot trace list                                  # 列出已导入的 Trace
paporot trace show <trace_id>                       # 查看 Trace 详情

# Trajectory 分析
paporot trajectory diff --trace-a <id> --trace-b <id>  # 轨迹差异对比
paporot trajectory diff --capability cap_auth_001       # 同 Capability 不同版本对比
paporot trajectory vector build --trace <id>            # 构建特征向量
paporot trajectory cluster --traces id1 id2 id3...      # 聚类分析
paporot trajectory coupling build --pairs trace1:cap1... # 构建耦合图
paporot trajectory coupling export --format excalidraw   # 导出耦合图 (Excalidraw)

# 退化检测
paporot trajectory eval --capability cap_auth_001       # 单 Capability 退化检测
paporot trajectory eval --all                           # 全量退化扫描
```

---

## 5. 数据流图

### Flow A: 代码分析管线

```
Git Repo (git diff)
    │
    ▼
DiffPreprocessor ──→ FileChange[]
    │
    ▼
┌───────────────────────────────┐
│  L1 AST 正则引擎               │
│  5 语言 × 13 语法结构          │
│  → RawChange[] (带置信度)      │
└───────┬───────────────────────┘
        │
  ┌─────┴─────┐
  │ 置信度判断 │
  └─────┬─────┘
        │
   ≥ 0.5 │        < 0.5
   ┌─────▼─────┐   ┌──────────────┐
   │ L2 Rules  │   │ L3 LLM Bridge│
   │ 规则引擎   │   │ (可选,1次LLM) │
   └─────┬─────┘   └──────┬───────┘
         │                │
         └───────┬────────┘
                 │
          ┌──────▼──────┐
          │  合并 L1+L2+L3│
          │  + Evidence  │
          └──────┬──────┘
                 │
          ┌──────▼──────┐
          │ Snapshot     │
          │ Engine       │
          │ v1/v2/v3...  │
          └──────────────┘
```

### Flow B: Skill 分析管线

```
repo_tree + repo_files    .Paporot/work/preprocessor_output.json
         │                      │
         ▼                      │
┌─────────────────┐             │
│ S1: repository-  │◄────────────┘
│   understanding  │  (通过 input 键读取)
│   LLM x1         │
└────────┬────────┘
         ▼
┌─────────────────┐
│ S2: module-      │
│   discovery      │
│   LLM x1         │
└────────┬────────┘
         ▼
┌─────────────────┐
│ S3: dependency-  │
│   analysis       │
│   LLM x0 (纯算法) │
└────────┬────────┘
         ▼
┌─────────────────┐
│ S4: runtime-flow-│
│   analysis       │
│   LLM x1         │
└────────┬────────┘
         ▼
┌─────────────────┐
│ S5: architecture-│
│   doc-generator  │
│   LLM x1         │
└────────┬────────┘
         ▼
┌─────────────────┐
│ Report Generator │
│ JSON/MD/HTML     │
└─────────────────┘
```

### Flow C: Trajectory 轨迹分析管线

```
Agent Execution Logs
(Claude/OpenAI/DeepSeek)
         │
         ▼
┌────────────────────┐
│ Trace Import &      │
│ Storage             │
│ → BehaviorTrace[]   │
└────────┬───────────┘
         │
    ┌────┴────────────┐
    ▼                 ▼
┌────────┐    ┌───────────────┐
│ P1:    │    │ Alignment     │
│ Feature│    │ Engine        │
│ Vector │    │ → TrajectoryDiff│
│ → 聚类, │   └───────────────┘
│   异常  │
└───┬────┘
    ▼
┌────────────┐
│ P2:        │
│ Coupling   │
│ Graph      │
│ → 共变分析 │
└───┬────────┘
    ▼
┌────────────┐
│ State Graph│
│ Builder    │
│ → 行为状态 │
└───┬────────┘
    ▼
┌────────────┐
│ Evaler     │
│ 退化检测    │
│ R001-R005  │
│ → EvalVerdict│
└────────────┘
```

---

## 6. LLM 调用预算分析

### 最坏情况（所有 Skill 可用 + L3 触发）

| 管线 | LLM 调用次数 | 用途 |
|------|-------------|------|
| Flow A: 预处理器 | 0-1 次 | L3 处理低置信度残留（高置信度充足时可跳过） |
| Flow B: Skill Pipeline | 4 次 | 语义理解（推断用途、模块职责、flow 命名、架构总结） |
| Flow C: Trajectory | **0 次** | 全确定性算法 |
| **总计** | **4-5 次** | — |

### 对比旧架构

| 指标 | 旧架构 (v1) | 新架构 (v2) | 改善 |
|------|------------|------------|------|
| LLM 调用次数 | 6-7 次 (含 behavior-boundary-discovery 的 1-2 次) | 4-5 次 | -30% |
| 确定性覆盖率 | ~30% (dependency-analysis 仅一个 Skill 零 LLM) | ~60% (L1+L2 预处理器 + dependency-analysis + 轨迹全部) | +100% |
| 行为退化检测 | 无 | R001-R005 确定性规则引擎 | 新增 |
| 决策溯源 | 无 | L1/L2/L3 三层证据链 | 新增 |

---

## 7. 文件系统布局

```
.Paporot/
├── config.toml                  # LLM 配置
├── skills/                      # WASM Skill (5 个)
│   ├── repository-understanding/
│   ├── module-discovery/
│   ├── dependency-analysis/
│   ├── runtime-flow-analysis/
│   └── architecture-doc-generator/
├── contracts/                   # 合约校验规则
│   ├── json.contract.yaml
│   ├── html.contract.yaml
│   └── excalidraw.contract.yaml
├── work/                        # [NEW] 预处理器输出
│   ├── sources/                 # 源文件副本
│   └── preprocessor_output.json # RawChange[] + RuleMatch[] + Evidence[]
├── snapshots/                   # [NEW] Snapshot 版本存储
│   ├── v1.json
│   ├── v2.json
│   ├── graph.json               # Capability 级依赖图索引
│   └── trace_map.json           # Snapshot ↔ Trace 关联映射
├── reviews/                     # [NEW] Feedback 审查文件
│   └── review_v42.toml
├── evidence/                    # [NEW] 决策溯源证据
│   └── cap_auth_001.evidence.json
├── traces/                      # [NEW] Agent 执行轨迹
│   ├── 2026-06-24_claude_trace_001.jsonl
│   └── trajectory_index.json
├── reports/                     # 报告输出
│   ├── analysis_result.json
│   ├── architecture.md
│   └── dashboard.html
└── logs/                        # 运行日志
    └── 2026-06-24_analyze.log
```

---

## 8. 死代码清理清单

### 确认删除

| 文件/模块 | 原因 |
|-----------|------|
| `src/agent.rs` | 调度逻辑已被 paporot-core DAG 引擎取代 |
| `src/commands/` (除 `feedback.rs` 外的 15 个) | CLI 壳逻辑，功能迁移到 paporot-core 内置能力 |
| `src/llm/client.rs` | `main.rs` 的 host_llm_call 已覆盖 LLM 调用 |
| `src/llm/mod.rs` | 同上 |
| `src/prompts.rs` | Prompt 常量迁移到 paporot-core |
| `.Paporot/skills/behavior-boundary-discovery/` | 被 L1+L2 确定性算法完全取代 |
| `crates/skills/` (6 个 bin) | 旧版 Rust 原生 Skill 实现，WASM Skill 已替代 |
| `crates/skill-sdk/` | 旧版 Skill SDK 接口，WASM SDK 已替代 |

### 回收并重构

| 文件/模块 | 去向 | 重构要点 |
|-----------|------|---------|
| `src/analysis/` (5 个文件) | paporot-core 内置预处理器 | target 改为 wasm32-wasip1, 移除 reqwest 依赖 |
| `src/storage.rs` | paporot-core 内置 (SnapshotStore) | 同上 |
| `src/graph.rs` | paporot-core 内置 (SnapshotAnalyzer) | 同上 |
| `src/types.rs` | paporot-analysis-types crate | 抽取共享类型 |
| `src/evidence/` | paporot-core 内置 | 同上 |
| `src/report/` | paporot-core 内置 | 已在 SkillRuntime 中接入 |
| `src/commands/feedback.rs` | Native Feedback 命令 | 保持 native target，TOML 文件方式 |
| `src/trace/` | Native 轨迹子系统 | 保持 native target，作为独立子系统 |
| `src/trajectory/` | Native 轨迹子系统 | 同上 |
| `src/evaler/` | Native 轨迹子系统 | 同上 |

---

## 9. 实现阶段划分

### Phase 0: 死代码清理 + 新建 crate（1-2 天）
- [ ] 删除 `src/agent.rs`
- [ ] 删除 `src/commands/` 全部
- [ ] 删除 `src/llm/client.rs`、`src/llm/mod.rs`、`src/prompts.rs`
- [ ] 删除 `.Paporot/skills/behavior-boundary-discovery/`
- [ ] 删除 `crates/skills/`、`crates/skill-sdk/`
- [ ] 新建 `crates/paporot-analysis-types/`（`serde` + `serde_json`），移入 `RawChange`、`ChangeType`、`RuleMatch`、`FileChange`、`Evidence` 等共享类型定义
- [ ] 更新各 crate 的 `Cargo.toml` 依赖
- [ ] 更新 `src/lib.rs` 和 `src/main.rs` 的 `mod` 声明
- [ ] 确保 `cargo check` 通过

### Phase 1: 预处理器回收（3-5 天）
- [ ] 将 `src/analysis/` 移入 `crates/paporot-core/src/analysis/`
- [ ] 将 `src/types.rs`（Capability/Snapshot 相关部分）移入 paporot-core
- [ ] 将 `src/evidence/` 移入 paporot-core
- [ ] 将 `src/report/` 移入 paporot-core
- [ ] L1/L2 编译为 wasm32-wasip1
- [ ] L3 LLM Bridge 改为通过 `host_llm_call` 调用
- [ ] 将 `prompts.rs` 中的 Prompt 常量迁移到 paporot-core
- [ ] 实现预处理器输出的文件桥接（写入 `.Paporot/work/preprocessor_output.json`）
- [ ] 修改 `pipeline.rs`：在 Skill DAG 执行前插入预处理器
- [ ] Evidence 写入通过 host_write_file

### Phase 2: Snapshot 引擎回收（2-3 天）
- [ ] 将 `src/storage.rs` + `src/graph.rs` 移入 paporot-core
- [ ] 实现 `SnapshotStore`（save/load/list）和 `SnapshotAnalyzer`（diff/coverage/regression/risk/graph/evolution）
- [ ] Skill 依赖图产出作为 Capability 依赖推断的上游输入
- [ ] Snapshot 持久化路径改为 `.Paporot/snapshots/`
- [ ] pipeline.rs 在分析完成后自动保存 Snapshot

### Phase 3: CLI 命令与 Feedback（1-2 天）
- [ ] 实现 `main.rs` 的 native command registry + WASM 兜底 dispatch
- [ ] 在 paporot-core 的 main 中新增分散子命令：snapshot list/show, diff, coverage, regression, risk, graph
- [ ] 实现 Feedback Native 命令：analyze 后自动生成 `.Paporot/reviews/review_v{N}.toml`，`feedback apply` 读取并写回 Snapshot
- [ ] 报告生成器重组章节结构（预处理独立成章 + Skill 深度分析）

### Phase 4: Trajectory 子系统回收（5-7 天）
- [ ] 清理并激活 `src/trace/`、`src/trajectory/`、`src/evaler/`
- [ ] 实现 Trace ↔ Snapshot 三级自动匹配算法
- [ ] 实现 `trace_map.json` 持久化
- [ ] `dependency-analysis` Skill 输出格式改为 Excalidraw
- [ ] 实现完整的 CLI 命令（trace import/list/show, trajectory diff/vector/cluster/coupling/eval）
- [ ] 回归测试

### Phase 5: 集成与质量保障（2-3 天）
- [ ] 端到端测试（完整 `paporot analyze` 流程）
- [ ] Trajectory 子系统端到端测试
- [ ] LLM 不可用时的降级路径测试
- [ ] WASM 二进制体积验证（≤ 1MB）
- [ ] README 更新

---

## 10. 决策日志

| 日期 | 决策 | 理由 |
|------|------|------|
| 2026-06-24 | L1+L2 确定性预处理器作为 paporot-core 内置能力，而非 WASM Skill | 确定性算法不需要沙盒隔离；应作为所有 Skill 共享的前置基础设施 |
| 2026-06-24 | 用 L1+L2 完全取代 behavior-boundary-discovery Skill | L1 正则覆盖 5 种语言 13 种语法结构，L2 有 AND/OR/NOT 组合规则——更可靠、零 LLM 调用 |
| 2026-06-24 | Snapshot 版本化作为 paporot-core 内置能力 | Capability Version Control 是核心定位，不是可选功能 |
| 2026-06-24 | LLM 仅做语义理解 | Skill 中 LLM 的用途严格限定为：推断项目用途、描述模块职责、命名 flow、写架构总结——所有这些本质上是"文字润色"，而非分析核心 |
| 2026-06-24 | Trajectory 子系统回归为 Native 独立子系统 | 轨迹分析需要访问文件系统、大数据量计算，不适合 WASM 沙盒；全确定性算法无需 LLM 隔离 |
| 2026-06-24 | Evidence 系统绑定到 L1+L2 预处理器 | 每个 Capability 的决策链溯源是最基础的透明性要求 |
| 2026-06-24 | Evaler 退化检测回归到轨迹子系统 | R001-R005 规则消费 BehaviorTrace，与轨迹系统强绑定 |
| 2026-06-24 | 废弃 agent.rs、commands/、llm/client.rs、prompts.rs、crates/skills/、crates/skill-sdk/ | 调度逻辑已被 DAG 引擎取代；LLM 调用通过 WASM host function；旧版 Skill 架构已被 WASM Skill 替代 |
| 2026-06-24 | 预处理器输出通过 `.Paporot/work/preprocessor_output.json` 文件桥接给 Skill Pipeline | 文件 I/O 复用现有 `host_read_file`/`host_write_file`，无需新增 FFI；JSON 序列化让 Skill 以松耦合方式消费 |
| 2026-06-24 | 抽出 `crates/paporot-analysis-types/` 共享类型 crate（`serde` + `serde_json`） | paporot-core（wasm32-wasip1）和 native binary（x86_64）都需使用同一套类型定义，抽成独立 crate 保证类型一致性 |
| 2026-06-24 | Snapshot 引擎拆为 `SnapshotStore` + `SnapshotAnalyzer` | 分析逻辑是纯函数（`&[BehaviorSnapshot] → 结果`），不涉及 I/O，拆分后极其易测；CLI/Skill 可直接复用纯分析函数 |
| 2026-06-24 | Trace ↔ Snapshot 紧耦合：三级自动匹配算法（commit hash → 文件重叠度 → 时间窗口） | 紧耦合让"行为版本对比"成为可能：不仅是代码版本对比，还能对比 Agent 行为轨迹变化；三级回退保证匹配鲁棒性 |
| 2026-06-24 | 匹配结果持久化到 `.Paporot/snapshots/trace_map.json` | 避免每次都重新计算匹配，支持增量更新；`null` 表示无匹配 Trace |
| 2026-06-24 | `dependency-analysis` Skill 输出格式改为 Excalidraw | 用户偏好 Excalidraw 可视化（与架构图一致），且可被 `.contract.yaml` 校验 |
| 2026-06-24 | Skill 产出的模块级依赖图作为 Snapshot Capability 依赖推断的上游输入 | 模块依赖关系是 Capability 间 `depends_on` 推断的天然信号之一，避免重复分析 |
| 2026-06-24 | 报告章节结构：预处理结果（Behavior Changes + Risk Analysis）独立成章在前，Skill 产出在后 | 用户首先关心"改了什么 + 有什么风险"，然后再看架构深度分析；符合审阅优先级 |
| 2026-06-24 | Feedback 通过 TOML 审查文件 + `paporot feedback apply` 命令 | 用户只需在终端用编辑器打开自动生成的 TOML 文件，花 2 分钟纠正——比 CLI 逐条交互高效得多 |
| 2026-06-24 | CLI dispatch：native command registry + WASM 兜底转发 | trace/trajectory/feedback 走 native；新增 WASM 命令零改动 `main.rs`；职责分离清晰 |
| 2026-06-24 | paporot-core 命令入口：分散独立分支（snapshot list/show、diff、coverage、...） | 各命令独立分支，失败隔离清晰；不强行统一入口增加不必要的抽象层 |
| 2026-06-24 | WASM 二进制体积上限 1MB，引入 `regex` 后预估 ~500KB | 当前依赖极少，体积增量可接受；无需引入 `regex-lite` 或惰性编译等优化 |

---

## 附录 A: Host Function 变更

| Host Function | 状态 | 说明 |
|---------------|------|------|
| `host_read_file` | 保持 | — |
| `host_write_file` | 保持 | — |
| `host_llm_call` | 保持 | L3 LLM Bridge 通过此函数调 LLM |
| `host_verify_contract` | 保持 | 合约校验 |
| `host_capture_evidence` | **新增** | 证据捕获写入 |
| `host_save_replay_case` | 保持 | 回归用例保存 |
| `host_load_replay_cases` | 保持 | 回归用例加载 |

## 附录 B: 已废弃模块详细路径

```
删除清单:
  src/agent.rs
  src/commands/analyze.rs
  src/commands/coupling.rs
  src/commands/coverage.rs
  src/commands/diff.rs
  src/commands/graph.rs
  src/commands/mod.rs
  src/commands/regression.rs
  src/commands/review.rs
  src/commands/risk.rs
  src/commands/snapshot.rs
  src/commands/state.rs
  src/commands/testmap.rs
  src/commands/trace.rs
  src/commands/trajectory.rs
  src/commands/trajectory_vector.rs
  src/commands/version.rs
  src/llm/client.rs
  src/llm/mod.rs
  src/prompts.rs
  .Paporot/skills/behavior-boundary-discovery/skill.toml
  .Paporot/skills/behavior-boundary-discovery/skill.wasm
  .Paporot/skills/behavior-boundary-discovery/skill.md
  .Paporot/skills/behavior-boundary-discovery/skill.rs
  crates/skills/src/bin/architecture_doc_generator.rs
  crates/skills/src/bin/behavior_boundary_discovery.rs
  crates/skills/src/bin/dependency_analysis.rs
  crates/skills/src/bin/module_discovery.rs
  crates/skills/src/bin/repository_understanding.rs
  crates/skills/src/bin/runtime_flow_analysis.rs
  crates/skills/Cargo.toml
  crates/skills/Cargo.lock
  crates/skill-sdk/src/host.rs
  crates/skill-sdk/src/lib.rs
  crates/skill-sdk/Cargo.toml

回收清单 (不删除):
  src/commands/feedback.rs      → Native Feedback 命令
  src/trace/                     → Native 轨迹子系统
  src/trajectory/                → Native 轨迹子系统
  src/evaler/                    → Native 轨迹子系统
  src/analysis/                  → paporot-core 内置预处理器
  src/storage.rs                 → paporot-core SnapshotStore
  src/graph.rs                   → paporot-core SnapshotAnalyzer
  src/types.rs                   → paporot-analysis-types crate
  src/evidence/                  → paporot-core 内置
  src/report/                    → paporot-core 内置
```
