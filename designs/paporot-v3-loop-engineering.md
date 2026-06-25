# Paporot v3 Loop Engineering PRD

> **Paporot — AI Generated Software Behavior Version Control & Auditing System**
>
> v2 建立了三层分析漏斗 + Snapshot 引擎 + Trajectory 追踪 + Feedback TOML。
> v3 要做的：把 v2 的"一次性分析管线"升级为"持续学习回路"。
> 从事后审计到预防性治理。

---

## 目录

1. [动机：v2 断点分析](#1-动机v2-断点分析)
2. [四个回路全览](#2-四个回路全览)
3. [回路 D 详细设计（v3 核心）](#3-回路-d-详细设计v3-核心)
   - [3.1 三层抑制机制](#31-三层抑制机制)
   - [3.2 Layer 1：精确抑制](#32-layer-1精确抑制)
   - [3.3 Layer 2：规则级抑制](#33-layer-2规则级抑制)
   - [3.4 Layer 3：文件前缀警告](#34-layer-3文件前缀警告)
   - [3.5 Feedback TOML 扩展](#35-feedback-toml-扩展)
4. [数据结构变更](#4-数据结构变更)
5. [集成架构](#5-集成架构)
6. [抑制生命周期](#6-抑制生命周期)
7. [实施计划](#7-实施计划)
8. [决策日志](#8-决策日志)

---

## 1. 动机：v2 断点分析

### 1.1 v2 已有的 Feedback 数据流

```
CLI ──→ paporot feedback generate v1
              ↓
         review_v1.toml        ← 人类编辑（approve / reject / correct / flag）
              ↓
         paporot feedback apply
              ↓
         SnapshotStorage.save()    ← BehaviorSnapshot 修正后写入
         FeedbackStore.save()      ← reviews.json 写入
```

`reviews.json` 包含结构化的人类评审记录：

```json
{
  "reviews": [
    {
      "review_id": "rev_001",
      "capability_id": "cap_sync",
      "verdict": "Rejected",
      "comment": "false positive - 同步逻辑已删除",
      "reviewer": "zxgzx",
      "reviewed_at": "2026-06-24T10:00:00Z"
    }
  ],
  "stats": { "approved": 2, "rejected": 1, "corrected": 0, "flagged": 0 }
}
```

### 1.2 三个断点

| 断点 | 问题 | 影响 |
|------|------|------|
| **L2 RuleEngine** | 不读 `reviews.json`。人拒绝了 `cap_sync`，下次分析同类变更照样报告。 | 分析精度不随时间提升 |
| **L3 LLM Bridge** | 不知道"历史上这个类型的变更被人类 reject 过"。 | LLM 冗余调用 |
| **Trajectory 分析** | 不知道某个 agent 历史上被 reject 过多少次。 | Agent 重复犯错无拦截 |

**一句话**：v2 的分析是一次性的。人类反馈数据没有被任何下游消费者读取。需要回路把反馈数据喂回分析管线。

### 1.3 设计原则

1. **假阴性成本远大于假阳性成本** — 宁可多告警，不可漏掉真问题。所有抑制必须可逆、可审计、带 scope 约束。
2. **确定性结论优先于概率性结论** — Exact match（人类确认过的事）→ Rule-level suppress（人类审批过的事）→ LLM 建议（AI 辅助，人等审批）。按可信度排序执行。
3. **不自动学习，只辅助决策** — 对标 Semgrep Assistant Memories 的设计哲学：系统草拟、人类审批、系统执行。不做无人审批的自动规则进化。
4. **粗细分工** — 精确匹配做 safety net，规则级抑制做批量降噪，前缀警告做信号汇聚。三者不竞争，覆盖不同风险区域。

---

## 2. 四个回路全览

Paporot v3 规划四个回路，分 4 个阶段实施：

| 阶段 | 回路 | 依赖 | 内容 |
|------|------|------|------|
| **v3.0（当前）** | **D · Agent 行为优化回路** | Trajectory + Feedback 系统已就绪 | 从反馈数据学习，拦截重复误报；规则级抑制 |
| v3.1 | A · 分析精度自提升 | 依赖 D 的规则通道 | Feedback TOML 纠正 → 规则进化 → 更少误报 |
| v3.2 | B · 变更→验证回路 | 依赖 A 的准确分析 | 自动生成 contract test → wasmtime 执行 |
| v3.3 | C · 预测→核实回路 | 依赖 A + D 的历史积累 | 被动分析 → 主动预测 → 核实准确率 |

**当前 PRD 聚焦 v3.0（回路 D）**，其他回路仅做框架预留。

---

## 3. 回路 D 详细设计（v3 核心）

### 3.0 设计灵感来源

| 工具 | 相关机制 | Paporot 对应 |
|------|---------|-------------|
| **ESLint** | `// eslint-disable-next-line rule-name` | — |
| **Semgrep** | `// nosemgrep: rule-id` + Assistant Memories（AI 草拟→人审批→生效） | 规则级抑制 |
| **SonarQube** | UI Mark False Positive + `// NOSONAR` | 精确抑制 |

核心参考：Semgrep Assistant Memories 2025 年推出的"把 triage 决策提炼为 AI 辅助的记忆单元"模式。

### 3.1 三层抑制机制

分析流水线变为：

```
Source Context → L0 DiffPreprocessor
                     ↓
                L1 AST → RawChange[]
                     ↓
                L2 Rules → RuleMatch[] (带 rule_id)
                     ↓
┌──────────────────────────────────────────────────────────┐
│  Feedback Check ── 三层抑制                               │
│                                                          │
│  Layer 1: Exact Match                                     │
│  (symbol_name, file_path, change_type) ∈ rejected         │
│  → confidence = 0.2, tag "rejected-history"               │
│                                                          │
│  Layer 2: Rule-Level Suppress                             │
│  (rule_id, file_pattern) ∈ suppressions.toml              │
│  → confidence = 0.2, tag "suppressed-by-rule"             │
│                                                          │
│  Layer 3: Prefix Warning                                  │
│  file_path partially matches reject history               │
│  → tag "fp-history-warning" (不影响 confidence)            │
└──────────────────────────────────────────────────────────┘
                     ↓
                L3 LLM（仅处理非抑制低置信度项）
```

**执行顺序**：Layer 1（确定性最高）→ Layer 2（人类审批过）→ Layer 3（纯提示）。Layer 1 命中后跳过 Layer 2/3 检查。

### 3.2 Layer 1：精确抑制

**机制**：以 `(symbol_name, file_path, change_type)` 三元组做 exact match，命中 `reviews.json` 中任意一条 `Rejected` 记录。

**为什么是 exact match？**
- 业界没有自动泛化抑制的先例——ESLint、Semgrep、SonarQube 全部是行级精确抑制
- 假阴性成本太高——泛化可能把真的安全漏洞一起压掉
- 泛化由 Layer 2（规则级，需人类审批）负责

**实现**：

```rust
// src/loop/suppressor.rs

pub struct ExactSuppressor {
    /// (symbol_name, file_path, change_type) → reason
    reject_map: HashMap<(String, String, ChangeType), String>,
}

impl ExactSuppressor {
    pub fn from_feedback_store(fb: &FeedbackStore, snapshot: &BehaviorSnapshot) -> Self {
        let mut map = HashMap::new();
        for review in &fb.reviews {
            if review.verdict == ReviewVerdict::Rejected {
                if let Some(cap) = snapshot.capabilities.iter().find(|c| c.id == review.capability_id) {
                    // 注意：Capability.name 对应 RawChange.symbol_name
                    // Capability 没有 file_path 和 change_type 字段
                    // 需要在 FeedbackStore 中扩展 BehaviorReview 记录溯源信息
                }
            }
        }
        Self { reject_map: map }
    }

    pub fn check(&self, rc: &RawChange) -> Option<SuppressionResult> {
        let key = (rc.symbol_name.clone(), rc.file_path.clone(), rc.change_type);
        self.reject_map.get(&key).map(|reason| SuppressionResult {
            level: SuppressionLevel::Exact,
            reason: reason.clone(),
            new_confidence: 0.2,
        })
    }
}
```

**关键前置条件**：`BehaviorReview` 需要扩展字段记录 RawChange 级别的溯源信息。见 §4。

### 3.3 Layer 2：规则级抑制

**机制**：与 L2 Rules 的 `rule_id` 关联。人类在 Feedback TOML 中通过 `[suppress_rule]` section 显式审批规则级抑制。

**抑制键**（必须有 scope 约束，禁止裸 rule_id）：

| 键 | 示例 | 粒度 |
|----|------|------|
| `(rule_id, file_pattern)` | `("breaking_001", "src/legacy/*")` | 推荐默认 |
| `(rule_id, file_pattern, change_type)` | `("breaking_001", "src/legacy/*", HttpRouteRemoved)` | 更细可选 |

**为什么禁止裸 rule_id？** `sec_auth_001` 在 `src/legacy/` 下是误报，在 `src/auth.rs` 下可能是真漏洞。

**TOML 格式**：

```toml
# feedback TOML 新增 section

[suppress_rule.breaking_001]
reason = "src/legacy/ 下所有公开 API 删除都是预期废弃行为"
file_pattern = "src/legacy/*"
effect = "suppress"                 # suppress | warn

[suppress_rule.sec_token_001]
reason = "src/migrations/ 下 token 字段变更是迁移脚本"
file_pattern = "src/migrations/*"
change_type = "ConstantChanged"
effect = "warn"
```

**抑制文件**：`.Paporot/rules/suppressions.toml`

```toml
[[suppression]]
rule_id = "breaking_001"
file_pattern = "src/legacy/*"
effect = "suppress"
reason = "src/legacy/ 下所有公开 API 删除都是预期废弃行为"
created_by = "zxgzx"
created_at = "2026-06-24T10:00:00Z"
source_review = "review_v1.toml"
hit_count = 12
last_hit = "2026-07-01T14:00:00Z"
status = "active"                  # active | stale | revoked
```

**复检机制**：
- 每次 `analyze` 后更新 `hit_count` 和 `last_hit`
- 连续 30 天零命中 → 状态自动变为 `stale`（不自动删除，人决定是否撤销）
- `paporot feedback stats` 展示每条 suppression 的命中趋势

### 3.4 Layer 3：文件前缀警告

**机制**：当 `file_path` 部分匹配 reject 历史中的文件前缀时，仅打 tag，不改变 confidence。

**为什么只打 tag？** 文件级前缀过于模糊。`src/legacy/sync.rs` 被 reject 过，不代表 `src/legacy/auth.rs` 也需要抑制。

**实现**：对每条 rejected 记录提取文件前缀（取前两级目录），构建前缀集合。RawChange 的 `file_path` 与前缀匹配时追加 tag `fp-history-warning`。

**后续进化路径**：积累足够数据后（v3.1），通过 `host_llm_call` 分析"前缀命中→实际 reject"的转化率，对高转化率前缀自动生成 Layer 2 规则建议。

### 3.5 Feedback TOML 扩展

完整 TOML 格式（新增 section 用 `→` 标记）：

```toml
# ─── 现有 section ───

[approve]
cap_auth = "ok"

[reject]
cap_sync = "false positive - 同步逻辑已删除"

[correct.cap_db]
name = "Database Layer V2"

[flag]
cap_cache = "needs additional test"

# ─── v3 新增 ───

→ [suppress_rule.breaking_001]
→ reason = "src/legacy/ 下公开 API 删除是预期废弃"
→ file_pattern = "src/legacy/*"
→ effect = "suppress"

→ [suppress_rule.sec_token_001]
→ reason = "migrations 下 token 变更是迁移脚本"
→ file_pattern = "src/migrations/*"
→ change_type = "ConstantChanged"
→ effect = "warn"
```

---

## 4. 数据结构变更

### 4.1 `BehaviorReview` 扩展（paporot-analysis-types）

```rust
pub struct BehaviorReview {
    // ── 现有字段 ──
    pub review_id: String,
    pub capability_id: String,
    pub snapshot_version: String,
    pub reviewer: String,
    pub verdict: ReviewVerdict,
    pub comment: Option<String>,
    pub corrected: Option<CorrectedData>,
    pub reviewed_at: String,
    pub tags: Vec<String>,

    // ── v3 新增 ──
    /// 触发此 Capability 的 L2 规则 ID 列表
    /// 用于将 reject 决策关联到具体规则
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggered_by_rules: Vec<String>,

    /// 原始符号名（从 RawChange.symbol_name 反向映射）
    /// 用于 Layer 1 exact match
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_symbol: Option<String>,

    /// 原始文件路径（从 RawChange.file_path 反向映射）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,

    /// 原始变更类型（从 RawChange.change_type 反向映射）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_change_type: Option<String>,
}
```

### 4.2 `RuleSuppression` 新增（paporot-analysis-types）

```rust
/// 规则级抑制条目，持久化到 .Paporot/rules/suppressions.toml
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RuleSuppression {
    pub rule_id: String,
    /// 必填：文件路径 glob pattern，作为 scope 约束
    pub file_pattern: String,
    /// 可选：进一步限制变更类型
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_type: Option<String>,
    pub effect: SuppressionEffect,
    pub reason: String,
    pub created_by: String,
    pub created_at: String,
    pub source_review: String,          // review_v1.toml — 来源追溯
    pub hit_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_hit: Option<String>,
    pub status: SuppressionStatus,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SuppressionEffect {
    /// confidence → 0.2
    Suppress,
    /// 打 tag，不降 confidence
    Warn,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SuppressionStatus {
    Active,
    Stale,
    Revoked,
}
```

### 4.3 `SuppressionResult` 新增

```rust
/// 反馈检查结果，用于 Pipeline 后处理
pub struct SuppressionResult {
    pub level: SuppressionLevel,
    pub reason: String,
    pub new_confidence: f32,
    pub matched_rule: Option<String>,   // Layer 2 命中时填充
}

pub enum SuppressionLevel {
    Exact,      // Layer 1
    Rule,       // Layer 2
    Warning,    // Layer 3
}
```

---

## 5. 集成架构

### 5.1 数据通道

```
                  Native Side                          WASM Side (paporot-core)
                  ───────────                          ────────────────────────
paporot analyze
    ↓
main.rs: load_feedback_index()
    ├── 读取 .Paporot/reviews/reviews.json
    ├── 读取 .Paporot/rules/suppressions.toml
    ├── 构建 FeedbackIndex 结构
    └── 序列化为 JSON → .Paporot/work/feedback_index.json
                                                              ↓
                                          pipeline.rs: run_preprocessor()
                                              L0 → L1 → L2 → L3
                                                              ↓
                                          apply_feedback_suppression()
                                              ├── Layer 1: exact match
                                              ├── Layer 2: rule-level
                                              └── Layer 3: prefix warn
                                                              ↓
                                          save_analysis_snapshot()
```

**通道选择**：不新增 host function。利用已有的 `host::read_file` 读取 `.Paporot/work/feedback_index.json`。native 侧负责构建索引并序列化，WASM 侧负责读取和匹配。WASM 二进制体积不增加。

### 5.2 文件系统布局（新增部分）

```
.Paporot/
├── reviews/
│   ├── review_v1.toml          # 人类编辑的审查文件
│   ├── review_v2.toml
│   └── reviews.json            # 结构化评审记录（存量，扩展字段）
├── rules/
│   └── suppressions.toml       # v3 新增：规则级抑制（人类审批后写入）
└── work/
    ├── preprocessor_output.json # 存量
    └── feedback_index.json      # v3 新增：native → wasm 通道
```

### 5.3 模块结构

```
src/loop/                        # v3 新增
├── mod.rs
├── feedback_loader.rs           # 加载 reviews.json + suppressions.toml
├── suppressor.rs                # Layer 1: Exact match
├── rule_suppressor.rs           # Layer 2: Rule-level
├── prefix_warner.rs             # Layer 3: Prefix warning
└── types.rs                     # SuppressionResult, SuppressionLevel

crates/paporot-core/src/
└── pipeline.rs                  # 修改：run_preprocessor() 末尾插入 apply_feedback_suppression()
```

---

## 6. 抑制生命周期

```
单条 reject 产生
    │
    ├──→ [自动] Layer 1 exact match 生效
    │    当次 analyze 起，同 (symbol, file, type) 自动 confidence=0.2
    │    reviews.json 中 rejected 记录持续有效
    │
    ├──→ [手动] 人类决定升级为规则级抑制
    │    在 TOML 中写 [suppress_rule."rule_id"]
    │    ↓
    │    paporot feedback apply → 写入 suppressions.toml
    │    ↓
    │    [自动] Layer 2 rule-level 生效
    │    命中后 hit_count++，last_hit 更新
    │    ↓
    │    [自动] 连续 30 天 hit_count = 0 → status = "stale"
    │    ↓
    │    [手动] 人选择：
    │      - 保留（stale 不自动删除）
    │      - 撤销 → 在 TOML 中写 [unsuppress."rule_id"]
    │      - 忽略（stale 仅做标记，下次命中时自动恢复 active）
    │
    └──→ [未来] 积累 50+ reject → LLM 记忆建议
         host_llm_call 分析趋势 → memories_suggested.toml → 人等审批
```

---

## 7. 实施计划

### Phase 0：数据结构变更（0.5 天）

- [ ] `BehaviorReview` 扩展 4 个溯源字段
- [ ] `RuleSuppression` / `SuppressionEffect` / `SuppressionStatus` 类型新增到 `paporot-analysis-types`
- [ ] `SuppressionResult` / `SuppressionLevel` 类型新增

### Phase 1：Native 侧反馈加载器（1 天）

- [ ] `src/loop/feedback_loader.rs` — 加载 `reviews.json` + `suppressions.toml`
- [ ] 构建 `FeedbackIndex` 结构（HashMap 索引）
- [ ] 序列化为 JSON 写入 `.Paporot/work/feedback_index.json`
- [ ] 在 `main.rs` 的 `paporot analyze` 入口处调用加载器
- [ ] 单元测试：加载空文件、加载含多条 reject、加载含 suppression

### Phase 2：WASM 侧抑制器（1.5 天）

- [ ] `src/loop/suppressor.rs` — Layer 1 exact match 逻辑
- [ ] `src/loop/rule_suppressor.rs` — Layer 2 rule-level 逻辑
- [ ] `src/loop/prefix_warner.rs` — Layer 3 前缀警告逻辑
- [ ] `crates/paporot-core/src/pipeline.rs` 集成 — `apply_feedback_suppression()` 插入 `run_preprocessor()` 末尾
- [ ] 单元测试：exact match 命中/未命中、rule-level 命中/scope 不匹配、前缀命中/不命中

### Phase 3：TOML 扩展 + CLI（1 天）

- [ ] `src/commands/feedback.rs` 扩展 — 解析 `[suppress_rule]` section
- [ ] `apply_review_toml()` 新增 suppression 写入逻辑
- [ ] `paporot feedback stats` 新增 suppression 命中统计
- [ ] 单元测试：suppression TOML 解析、apply、E2E roundtrip

### Phase 4：E2E 测试 + 文档（1 天）

- [ ] E2E 测试：完整 feedback→analyze→抑制→验证 回路
- [ ] LLM 不可用时降级测试
- [ ] `DEVELOPMENT.md` 更新为 v3 结构
- [ ] 全量测试回归通过

### 预计总工时：4 天

---

## 8. 决策日志

| # | 决策 | 理由 |
|----|------|------|
| D1 | 四个回路全要做，先做 D | 咨询用户后确认。D 数据已就绪，改动最小 |
| D2 | 不用加权求和做"犯错"判定 | 三个信号性质不同（ground truth vs 线索 vs 分类器），性质决定了不适合加权 |
| D3 | 先规则驱动，同时埋点准备 ML 驱动 | 对标 Semgrep。不做无人审批的自动学习 |
| D4 | 从精确抑制→前缀警告→LLM 建议的三层方案 | 最初提议 A+B（Rules + Vectors），调研后改为更务实的三层。精确匹配对标 ESLint/SonarQube，规则级对标 Semgrep Memories |
| D5 | Rules First 执行顺序 | 确定性结论（已被人类否定过的）优先于概率性结论 |
| D6 | 不提供 scope 参数（exact/pattern/module） | 调研 ESLint/Semgrep/SonarQube 后确认：没有工具这样做。粒度由匹配表达式自然决定 |
| D7 | 规则级抑制必须有 scope 约束，禁止裸 rule_id | 同一条规则在不同模块可能是误报或真漏洞。`sec_auth_001` 在 `src/legacy/` vs `src/auth.rs` |
| D8 | 规则级抑制需人类显式审批 | 对标 Semgrep Memories 的 "AI suggests, human approves" |
| D9 | 抑制可逆 —— 30 天零命中 → stale，但不自动删除 | 人去决定是否撤回，系统不做假设 |
| D10 | Native 加载索引 → JSON 文件 → WASM read_file | 不新增 host function。减少接口面，WASM 体积不受影响 |
| D11 | Pipeline 后处理模式（不侵入 L1/L2/L3 代码） | 在 run_preprocessor() 末尾做批量后处理，保持分析模块纯净 |
| D12 | MVC 包含规则级抑制 | 用户确认。不拆分 Layer 2 到后续 Phase |
| D13 | 先写正式设计文档再实施 | 用户确认。本文档即为正式设计文档 |
