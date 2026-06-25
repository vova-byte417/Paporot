# Paporot v3 开发说明

> AI 生成软件行为版本控制与审计系统
>
> 文档版本：2026-06-24 | v3.0 — Loop Engineering 闭环

---

## 目录

1. [系统概览](#1-系统概览)
2. [架构全景](#2-架构全景)
3. [Phase 0：死代码清理与共享类型 Crate](#3-phase-0死代码清理与共享类型-crate)
4. [Phase 1：预处理器回收与 L3 LLM Bridge](#4-phase-1预处理器回收与-l3-llm-bridge)
5. [Phase 2：Snapshot 引擎与 Trace-Snapshot 三级匹配](#5-phase-2snapshot-引擎与-tracesnapshot-三级匹配)
6. [Phase 3：CLI 重构与 Feedback TOML 系统](#6-phase-3cli-重构与-feedback-toml-系统)
7. [Phase 4：Trajectory 子系统与三级匹配算法](#7-phase-4trajectory-子系统与三级匹配算法)
8. [Phase 5：端到端测试与质量保障](#8-phase-5端到端测试与质量保障)
9. [Phase 6：v3 Loop Engineering — 反馈闭环](#9-phase-6v3-loop-engineering--反馈闭环)
10. [项目结构](#10-项目结构)
11. [构建与测试](#11-构建与测试)

---

## 1. 系统概览

Paporot 是一个**AI 生成软件的行为版本控制与审计系统**，基于 WASM 沙箱架构。

### 核心概念

| 概念 | 说明 |
|------|------|
| **Capability** | 软件的可观测行为单元（API 端点、公共函数、配置项等） |
| **BehaviorSnapshot** | 某一版本的所有 Capability 集合 + 元数据 |
| **BehaviorDiff** | 两个 Snapshot 之间的差异（新增/删除/修改/未变） |
| **ExecutionTrace** | Agent（Claude/OpenAI/DeepSeek）的一次完整执行轨迹 |
| **Skill** | 运行在 WASM 沙箱中的分析模块，通过 host function 与外界交互 |

### 三层分析漏斗 (L1 → L2 → L3)

```
┌─────────────────────────────────┐
│ L1: AST 确定性分析   → 高置信度  │   置信度 ≥ 0.85
├─────────────────────────────────┤
│ L2: 规则引擎          → 中置信度  │   置信度 0.5-0.85
├─────────────────────────────────┤
│ L3: LLM Bridge        → 低置信度  │   置信度 < 0.5 → 交给 LLM
│    (host_llm_call)                │   LLM 不可用时返回空，系统降级运行
└─────────────────────────────────┘
```

### 核心架构：双目标编译

```
┌─────────────────┐     wasmtime 加载     ┌──────────────────┐
│  Native Binary  │ ───────────────────→  │  paporot-core    │
│  (x86_64)       │                       │  (wasm32-wasip1) │
│                 │ ←─── host functions ── │                  │
│ • CLI dispatch  │   host_read_file       │ • L1 AST 分析    │
│ • Feedback      │   host_write_file      │ • L2 Rules 引擎  │
│ • SnapshotStore │   host_llm_call        │ • L3 LLM Bridge  │
│ • Trajectory    │   host_verify_contract │ • SnapshotAnalyzer│
│ • Skill Host    │   host_capture_evidence│ • Skill Pipeline │
└─────────────────┘                       └──────────────────┘
```

---

## 2. 架构全景

### 2.1 分层架构

```
┌──────────────────────────────────────────────────────┐
│  CLI Layer (src/)                                     │
│  main.rs · cli.rs · config.rs                         │
├──────────────────────────────────────────────────────┤
│  Analysis Layer ───────────── WASM Sandbox ──────────│
│  src/analysis/              crates/paporot-core/     │
│  L1,L2,L3 (native client)   L1,L2,L3 (wasm32)       │
├──────────────────────────────────────────────────────┤
│  Shared Types ── crates/paporot-analysis-types/ ────│
│  Capability, BehaviorSnapshot, RawChange, etc.       │
├──────────────────────────────────────────────────────┤
│  Subsystems                                          │
│  src/trace/         Trajectory Tracing               │
│  src/trajectory/    Trajectory Analysis (P1/P2)     │
│  src/verification/  Contract Verification            │
│  src/evaler/        Behavioral Evaluation            │
│  src/skills/        Skill Runtime Host               │
└──────────────────────────────────────────────────────┘
```

### 2.2 共享类型层 (paporot-analysis-types)

**为什么需要单独 crate？**

Native binary 编译为 `x86_64-unknown-linux-gnu`，paporot-core 编译为 `wasm32-wasip1`。两个目标共享同一套类型定义（`Capability`、`BehaviorSnapshot`、`RawChange` 等），放在独立 crate 中避免重复定义和类型不匹配。

关键类型：
- `Capability`：软件行为单元，含 id/name/status/confidence/evidence/tags/contract
- `BehaviorSnapshot`：版本快照，含 version_id/git_commit/capabilities/prd_coverage
- `BehaviorDiff`：差异报告，含 added/modified/deleted/unchanged/impact_summary
- `RawChange`：L1/L2/L3 分析产出的原始变更
- `RuleMatch` / `LlmFragment`：规则匹配和 LLM 分析片段
- `CapabilityStatus`：New / Modified / Deleted / Unchanged

---

## 3. Phase 0：死代码清理与共享类型 Crate

### 3.1 删除的模块

| 删除模块 | 原因 |
|----------|------|
| `src/agent.rs` | 重构为 Trace 子系统 |
| `src/llm/*` (llm_client.rs, prompts.rs 等) | LLM 调用改为 WASM host_llm_call |
| `src/commands/*` 中的 15 个子命令 | 简化为 TOML-based feedback |
| `src/prompts.rs` | L3 prompt 内联到 paporot-core |
| `src/snapshot/types.rs` | 类型迁移到 paporot-analysis-types |

### 3.2 创建的 crate

```
crates/paporot-analysis-types/
├── Cargo.toml
└── src/lib.rs       # 所有共享类型 + Serde 派生
```

`src/types.rs` 改为：`pub use paporot_analysis_types::*;`

---

## 4. Phase 1：预处理器回收与 L3 LLM Bridge

### 4.1 paporot-core 模块迁入

将以下模块从 `src/analysis/` 完整迁移到 `crates/paporot-core/src/analysis/`，并为 `wasm32-wasip1` 目标适配：

| 模块 | 文件 | 适配内容 |
|------|------|----------|
| L1 AST | `l1_ast.rs` | 确定性分析，直接编译 |
| L2 Rules | `l2_rules.rs` | 规则引擎，直接编译 |
| Evidence | `confidence.rs`, `provider.rs` | 证据链系统 |
| Report | `dashboard.rs`, `generator.rs` | HTML 报告生成 |
| Pipeline | `pipeline.rs` | Skill DAG 编排 + 预处理 |

### 4.2 L3 LLM Bridge（关键设计）

**问题**：WASM 目标不支持 `async` 和 `reqwest`。

**方案**：通过 `host::llm_call()` host function 同步调用外部 LLM。

```rust
// crates/paporot-core/src/analysis/l3_llm_bridge.rs

pub struct LlmBridge;

impl LlmBridge {
    /// 增强低置信度变更：将低置信度 RawChange 交给 LLM 分析
    pub fn enhance(low_conf_changes: &[RawChange], diff_text: &str) -> Vec<LlmFragment> {
        let user_prompt = build_prompt(low_conf_changes, diff_text);
        match host::llm_call(SYSTEM_PROMPT, &user_prompt) {
            Some(response) => parse_llm_response(&response),
            None => vec![],  // LLM 不可用时降级返回空
        }
    }
}
```

**7 个单元测试**覆盖：prompt 构建、截断、空输入、fragment 合并等场景。

### 4.3 预处理器文件桥接

L1/L2/L3 分析结果写入 `.Paporot/work/preprocessor_output.json`，供 Skill Pipeline 消费：

```json
{
  "version": 1,
  "l1_changes": [...],       // RawChange[]
  "l2_matches": [...],       // RuleMatch[]
  "l3_llm_fragments": [...], // LlmFragment[]
  "summary": {
    "l1_change_count": 12,
    "l2_match_count": 5,
    "l3_fragment_count": 2,
    "average_confidence": 0.87
  }
}
```

### 4.4 Pipeline 集成

`execute_analyze()` 新增预处理阶段，在 Skill DAG 之前运行：

```
Source Context → L0 (DiffPreprocessor) → L1 (AST) → L2 (Rules) → L3 (LLM)
                                                                    ↓
                    Skill DAG ←── preprocessor_output.json ←── PreprocessorBridge
```

---

## 5. Phase 2：Snapshot 引擎与 Trace↔Snapshot 三级匹配

### 5.1 SnapshotStore（IO 层）

WASM 侧的快照存储，通过 `host::read_file/write_file` 操作文件系统：

```rust
pub struct SnapshotStore;

impl SnapshotStore {
    pub fn save(&self, snapshot: &BehaviorSnapshot) -> Result<()>
    pub fn load_by_version(&self, version_id: &str) -> Result<BehaviorSnapshot>
    pub fn list_versions_sorted(&self) -> Result<Vec<String>>
    pub fn next_version_id(&self) -> Result<String>
}
```

**5 个单元测试**：保存/加载/列表/排序/版本号递增。

### 5.2 SnapshotAnalyzer（纯函数）

无状态分析器，仅做计算：

```rust
pub struct SnapshotAnalyzer;

impl SnapshotAnalyzer {
    pub fn diff(prev: &BehaviorSnapshot, curr: &BehaviorSnapshot) -> BehaviorDiff
    pub fn coverage(snapshot: &BehaviorSnapshot) -> PrdCoverage
    pub fn regression(prev: &BehaviorSnapshot, curr: &BehaviorSnapshot) -> RegressionReport
    pub fn risk(prev: &BehaviorSnapshot, curr: &BehaviorSnapshot) -> RiskAssessment
    pub fn evolution(history: &[BehaviorSnapshot]) -> EvolutionReport
}
```

**7 个单元测试**：diff 新增/删除/修改/未变、覆盖率计算、风险评分。

### 5.3 自动快照（Pipeline 收尾）

`execute_analyze()` 在 Skill DAG 完成后自动调用 `save_analysis_snapshot()`：
- 将 `PreprocessorOutput` 中的 `RawChange[]` 映射为 `Capability[]`
- 生成 `BehaviorSnapshot` 并写入 `snapshots/auto_latest.json`

```rust
fn save_analysis_snapshot(
    pp: &PreprocessorOutput,
    timestamp: &str,
) {
    let capabilities: Vec<Capability> = pp.l1_changes.iter().map(|rc| {
        let status = match rc.change_type {
            // Added 变体 → CapabilityStatus::New
            // Removed 变体 → CapabilityStatus::Deleted
            _ => CapabilityStatus::Modified,
        };
        Capability { id, name: rc.symbol_name, status, confidence: Some(rc.confidence), ... }
    }).collect();

    let snapshot = BehaviorSnapshot {
        version_id: format!("auto_{timestamp}"),
        capabilities,
        ...
    };
    host::write_file("snapshots/auto_latest.json", &serde_json::to_string(&snapshot)?)?;
}
```

---

## 6. Phase 3：CLI 重构与 Feedback TOML 系统

### 6.1 Native Command Registry

`main.rs` 使用 HashMap dispatch 替代旧的平面 match：

```rust
type CmdFn = fn(&[String], &Path);
let commands: HashMap<&str, CmdFn> = HashMap::from([
    ("init",       cmd_init as CmdFn),
    ("analyze",    cmd_native_stub as CmdFn),
    ("snapshot",   cmd_native_stub as CmdFn),
    ("trace",      cmd_native_stub as CmdFn),
    ("feedback",   cmd_feedback as CmdFn),
    ("verify",     cmd_native_stub as CmdFn),
    ("serve",      cmd_native_stub as CmdFn),
]);
```

未注册子命令通过 WASM fallback 转发到 paporot-core。

### 6.2 Feedback TOML 审查系统

**设计理念**：人机协作审查回路。AI 生成 review TOML → 人类编辑 → 系统应用。

**TOML 格式**：

```toml
[approve]
cap_auth = "ok, implementation matches spec"
cap_payment = "ok"

[reject]
cap_legacy_sync = "this module was removed in PR #42"

[correct.cap_db]
name = "Database Layer V2"
confidence = 0.95

[flag]
cap_cache = "needs additional integration test"
```

**工作流**：

```
                 generate_review_toml()
BehaviorSnapshot ──────────────────────→ review_v1.toml
                                               │
                                         人类编辑 TOML
                                               │
                 apply_review_toml()           ↓
BehaviorSnapshot ←────────────────────── review_v1.toml (已编辑)
  (已修正)
```

**关键函数**：

| 函数 | 作用 |
|------|------|
| `generate_review_toml(snapshot, reviews_dir)` | 生成包含所有 Capability 的 review TOML 模板 |
| `apply_review_toml(snapshot, toml_path, reviewer)` | 解析 TOML，应用审批/拒绝/修正/标记 |

**5 个单元测试 + 2 个集成测试**：generate、approve、reject、correct、flag、full roundtrip。

---

## 7. Phase 4：Trajectory 子系统与三级匹配算法

### 7.1 Trace↔Snapshot 三级自动匹配算法

实现 PRD §10 决策日志要求的匹配策略：

```
Level 1 ── Git Commit Hash 精确匹配 ── 置信度 1.0
   ↓ 失败
Level 2 ── 文件重叠度 Jaccard 相似度 ── 置信度 [0, 1]
   ↓ 失败
Level 3 ── 时间窗口最近匹配 ── 置信度 [0, 1]（按 24h 窗口衰减）
```

**Jaccard 计算**：

```
Jaccard(A, B) = |A ∩ B| / |A ∪ B|
```

**时间窗口置信度**：

```
confidence = 1.0 - min(delta_seconds / 86400, 1.0)
```

### 7.2 trace_map.json 持久化

```json
{
  "v1": [
    { "trace_id": "trace_001", "confidence": 1.0, "match_level": "commit" }
  ],
  "v2": [
    { "trace_id": "trace_003", "confidence": 0.75, "match_level": "file_overlap" }
  ]
}
```

**11 个单元测试**：commit 精确/不匹配、文件重叠全量/部分/无、时间窗口近/远、全链路三级回退、持久化读写。

### 7.3 Trajectory Adapters

三个 Agent 平台的 trace 解析适配器：

| Adapter | 平台 | 输入格式 |
|---------|------|----------|
| `claude.rs` | Anthropic Claude | Claude API trace JSON |
| `openai.rs` | OpenAI | OpenAI API trace JSON |
| `deepseek.rs` | DeepSeek | DeepSeek API trace JSON |

---

## 8. Phase 5：端到端测试与质量保障

### 8.1 测试矩阵

| 层级 | 文件 | 数量 | 覆盖范围 |
|------|------|------|----------|
| 单元测试 (lib) | `src/*.rs` | 351 | 所有模块单元测试 |
| 单元测试 (bin) | `src/commands/feedback.rs` | 5 | Feedback TOML 操作 |
| WASM 测试 | `crates/paporot-core/src/*.rs` | 71 | 沙箱内各模块 |
| 集成测试 | `tests/integration_tests.rs` | 41 | E2E + Snapshot + Feedback + Trajectory |
| Doc 测试 | 各模块 | 2 | API 文档示例 |

**总计：398 passed / 2 ignored / 0 failed**

### 8.2 端到端测试覆盖

| 测试 | 场景 | 验证点 |
|------|------|--------|
| `test_full_analysis_pipeline_no_llm` | L1→L2→Snapshot 全链路 | Snapshot 存储、版本列表、diff 计算 |
| `test_llm_unavailable_fallback` | LLM 不可用降级 | 系统不崩溃，L1/L2 结果仍正常 |
| `test_trace_snapshot_matching_full_pipeline` | Trace↔Snapshot 匹配 | 三级回退、trace_map.json 持久化 |
| `test_feedback_to_snapshot_full_roundtrip` | Feedback→Snapshot 回路 | generate→edit→apply→reload |
| `test_apply_toml_suppress_rule` | v3: suppress_rule TOML 解析 | suppressions.toml 写入验证 |

### 8.3 WASM 二进制体积

```
paporot-core.wasm (release, lto + opt-level=s): 1.6 MB
```

- 已启用 `lto = true` 和 `opt-level = "s"`
- 接近 PRD 1MB 目标，后续可通过 `wasm-opt -Os` 进一步压缩

### 8.4 构建命令

```bash
# 全量测试
cargo test

# paporot-core WASM 构建
cd crates/paporot-core
cargo build --target wasm32-wasip1 --release

# 仅集成测试
cargo test --tests

# 仅 paporot-core 测试
cd crates/paporot-core && cargo test --target wasm32-wasip1
```

---

## 9. Phase 6：v3 Loop Engineering — 反馈闭环

> **从一次性分析到持续学习。** v3 把 v2 的 Feedback TOML 系统升级为完整的 "分析→审核→学习→抑制" 闭环。
>
> 设计文档：[designs/paporot-v3-loop-engineering.md](designs/paporot-v3-loop-engineering.md)

### 9.1 动机

v2 的 feedback 流存在断点：`paporot feedback apply` 把人类评审写入 `reviews.json`，但 **没有任何下游消费者读取它**。下次 `analyze` 照样报告同类误报。

### 9.2 架构：三层抑制

分析管线在 L2（RuleEngine）之后插入反馈检查：

```
L1 AST → RawChange[]
    ↓
L2 Rules → RuleMatch[] (带 rule_id)
    ↓
┌─────────────────────────────────────────────────────┐
│  Feedback Check                                     │
│                                                     │
│  L1 · Exact Match                                    │
│  (symbol, file, type) ∈ rejected → confidence=0.2   │
│                                                     │
│  L2 · Rule-Level Suppress                            │
│  (rule_id, file_pattern) ∈ suppressions.toml        │
│  → confidence=0.2（需人类审批）                      │
│                                                     │
│  L3 · Prefix Warning                                 │
│  file_path 匹配 reject 历史前缀 → tag "fp-history"  │
└─────────────────────────────────────────────────────┘
    ↓
L3 LLM（仅处理非抑制项）
```

### 9.3 数据通道

```
Native Side                              WASM Side
──────────                               ────────
main.rs: build_feedback_index()
  ├── 读取 reviews.json
  ├── 读取 suppressions.toml
  └── 序列化 → work/feedback_index.json
                                              ↓
                              pipeline.rs: run_preprocessor()
                                  ↓
                              suppressor::apply_suppressions()
                                  ↓
                              write_feedback_index()
                              write_suppressions_toml()  ← hit_count++
```

### 9.4 新增模块

| 文件 | 功能 |
|------|------|
| `src/feedback_loop/feedback_loader.rs` | Native 侧：加载 reviews.json + suppressions.toml，构建 FeedbackIndex |
| `crates/paporot-core/src/suppressor.rs` | WASM 侧：三层抑制逻辑 + hit_count 回写 |

### 9.5 数据结构变更

**Capability 新增溯源字段**（paporot-analysis-types）：

```rust
pub struct Capability {
    // ... existing fields
    pub source_change_type: Option<String>,  // "FunctionRemoved"
    pub triggered_by_rules: Vec<String>,     // ["breaking_001"]
}
```

**新增类型**：`RuleSuppression`, `SuppressionEffect`, `SuppressionStatus`, `FeedbackIndex`

### 9.6 Feedback TOML 扩展

```toml
# v3 新增 section
[suppress_rule.breaking_001]
reason = "src/legacy/ 下公开 API 删除是预期废弃"
file_pattern = "src/legacy/*"
effect = "suppress"              # suppress | warn
change_type = "HttpRouteRemoved" # optional
```

### 9.7 抑制生命周期

```
单条 reject → [自动] L1 exact match 生效
    ↓
人类决定升级 → 写 [suppress_rule."rule_id"] → feedback apply
    ↓
[自动] L2 rule-level 生效 → hit_count++, last_hit 更新
    ↓
连续 30 天 hit_count=0 → status="stale"
```

### 9.8 模块注册

```rust
// src/lib.rs
pub mod feedback_loop;   // v3 新增

// crates/paporot-core/src/lib.rs
pub mod suppressor;      // v3 新增
```

---

## 10. 项目结构

```
Paporot/
├── crates/
│   ├── paporot-analysis-types/   # 共享类型定义（native + wasm 共用）
│   │   └── src/lib.rs            # Capability, Snapshot, RawChange, 枚举
│   └── paporot-core/             # WASM 沙箱核心
│       └── src/
│           ├── analysis/         # L1 AST / L2 Rules / L3 LLM / Preprocessor
│           ├── evidence/         # Confidence / Provider
│           ├── report/           # Dashboard / Generator
│           ├── pipeline.rs       # Skill DAG + 预处理 + 自动快照
│           ├── suppressor.rs     # v3: 三层反馈抑制
│           ├── snapshot_store.rs # Snapshot I/O
│           ├── snapshot_analyzer.rs # Snapshot 纯函数分析
│           ├── host.rs           # Host function FFI bindings
│           ├── types.rs          # paporot-analysis-types 封装
│           └── lib.rs
├── src/
│   ├── analysis/                 # Native 侧分析（客户端调用）
│   ├── commands/
│   │   └── feedback.rs           # TOML review 系统（含 suppress_rule）
│   ├── feedback_loop/            # v3: Native 侧反馈加载器
│   │   └── feedback_loader.rs
│   ├── trace/
│   │   ├── adapters/             # Claude/OpenAI/DeepSeek 适配器
│   │   ├── trace_snapshot_map.rs # 三级匹配算法
│   │   └── types.rs / storage.rs
│   ├── trajectory/               # 轨迹分析（P1/P2 统计）
│   ├── verification/             # 合约验证
│   ├── skills/                   # Skill 运行时宿主
│   ├── snapshot.rs               # Snapshot diff 封装
│   ├── storage.rs                # Snapshot 本地文件存储
│   ├── main.rs                   # CLI 入口 + feedback index 构建
│   └── lib.rs
├── tests/
│   ├── integration_tests.rs      # 含 v3_loop_tests 模块
│   └── fixtures/                 # 测试数据（JSONL traces）
├── designs/
│   ├── paporot-v2-prd.md
│   └── paporot-v3-loop-engineering.md  # v3 设计文档
├── Cargo.toml
└── DEVELOPMENT.md                 # 本文档
```

---

## 11. 构建与测试

### 环境要求

- Rust 1.96+
- wasm32-wasip1 target：`rustup target add wasm32-wasip1`
- WSL (Windows) 或 Linux

### 快速开始

```bash
# 安装 WASM target
rustup target add wasm32-wasip1

# 全量测试
cargo test

# 构建 WASM 模块
cargo build -p paporot-core --target wasm32-wasip1 --release

# 构建 native binary
cargo build --release
```

### 测试结果 (2026-06-24, v3)

```
lib tests:      ~350 passed, 0 failed
bin tests:      ~5 passed, 0 failed
integration:    ~45 passed, 0 failed
─────────────────────────────────────
Total:         ~400 passed, 0 failed
```

### v3 新增测试

| 测试 | 场景 |
|------|------|
| `test_loop_exact_suppression` | E2E: reject → BehaviorReview 溯源 → FeedbackIndex exact match |
| `test_loop_rule_suppression` | E2E: suppress_rule TOML → suppressions.toml → FeedbackIndex L2 |
| feedback_loader 单元测试 ×3 | 空索引、带 reject、exact key 构建 |
| suppressor 单元测试 ×6 | L1 exact、L2 suppress、L2 scope mismatch、L3 prefix、L2 stale skip |
