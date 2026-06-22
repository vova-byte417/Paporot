# PRD: Capability Evidence 模块（接口级）

> Paporot 从 Capability Version Control 迈向 Behavior Version Control 的第三步

---

## 目录

1. [背景与动机](#1-背景与动机)
2. [核心概念](#2-核心概念)
3. [设计决策记录](#3-设计决策记录)
4. [模块架构](#4-模块架构)
5. [数据类型定义](#5-数据类型定义)
6. [置信度评分](#6-置信度评分)
7. [证据采集时机](#7-证据采集时机)
8. [L3 证据接口](#8-l3-证据接口)
9. [CLI 子命令](#9-cli-子命令)
10. [HTML 溯源图](#10-html-溯源图)
11. [测试策略](#11-测试策略)

---

## 1. 背景与动机

### 1.1 问题

Paporot 的 Capability 推断管道（L1 AST → L2 Rules → L3 LLM）对用户是黑盒的。

- "为什么这段代码被认为是一个 'User Login' Capability？"
- "这个 Capability 的推断可靠吗？"
- "如果推断错了，从哪个环节开始错的？"

当前 `Capability.confidence` 只是一个数字，没有任何证据支撑。

### 1.2 目标

- **溯源**：每个 Capability 附带 L1 → L2 → L3 的决策链路
- **评分**：三个层级各自给出可信度评分
- **透明**：用户可以审查每个推断步骤

---

## 2. 核心概念

| 概念 | 定义 |
|------|------|
| **Evidence** | 一个 Capability 的推断证据集合（L1 + L2 + L3） |
| **L1Evidence** | AST 层面的符号提取证据：哪个文件、哪个函数/结构体、什么类型 |
| **L2Evidence** | 规则匹配证据：哪条规则、匹配了哪个 L1 符号、触发原因 |
| **L3Evidence** | LLM 推断证据：prompt hash、response、推断的 Capability 描述 |
| **EvidenceConfidence** | 三层独立评分：L1 / L2 / L3，后期可加权合成 |
| **EvidenceHash** | snapshot 创建时采集的 L1/L2/L3 输出 hash，用于事后校验 |
| **LlmEvidenceProvider** | L3 证据采集的抽象接口，由用户注入 LLM 服务 |

---

## 3. 设计决策记录

| # | 决策 | 备选方案 | 选择理由 |
|---|------|---------|---------|
| D1 | 溯源 + 置信度兼做 | 仅溯源 / 仅评分 | 溯源回答"怎么来的"，评分回答"多可靠"，缺一不可 |
| D2 | Snapshot 创建时记录 hash + 事后按需重建详情 | 创建时采集全量 / 完全事后重建 | hash 低开销可校验一致性；详情重建避免 snapshot 创建过重 |
| D3 | 三层独立评分（初期），后期切数据驱动加权 | 手设权重 / 单一数字 | 三层证据维度不同，初期不硬捏；积累数据后可自适应 |
| D4 | L3 证据通过 `LlmEvidenceProvider` trait 注入 | 模块自带 LLM / 只做 L1+L2 | 灵活、不锁定 LLM 服务商；与 Paporot 离线优先一致 |
| D5 | HTML 交互式溯源图 | 终端树 / JSON | 点击展开折叠，L1→L2→L3 连线可视化 |

---

## 4. 模块架构

### 4.1 目录结构

```
src/
  evidence/                            ← 新增一级模块
    mod.rs
    types.rs                           ← Evidence / L1Evidence / L2Evidence / L3Evidence / EvidenceConfidence
    collector.rs                       ← 证据采集器（L1+L2），snapshot 创建时调用
    confidence.rs                      ← 三层评分计算
    provider.rs                        ← LlmEvidenceProvider trait
    report.rs                          ← HTML 溯源图生成
  commands/
    evidence.rs                        ← CLI 子命令 generate / show
  cli.rs                               ← + Commands::Evidence
  lib.rs                               ← + pub mod evidence;
```

### 4.2 依赖关系

```
commands/evidence.rs  →  evidence/collector.rs
                              ├── evidence/types.rs
                              ├── evidence/confidence.rs
                              └── evidence/provider.rs  (trait only)

evidence/ 读取 analysis/l1_ast.rs, analysis/l2_rules.rs 的输出
evidence/ 读取 types::Capability.evidence_trace_ids
```

- `evidence/collector.rs` 消费 `analysis/l1_ast.rs` 和 `analysis/l2_rules.rs` 的输出
- L3 通过 `LlmEvidenceProvider` trait 注入，不硬编码 LLM 依赖

---

## 5. 数据类型定义

### 5.1 文件: `src/evidence/types.rs`

```rust
use serde::{Deserialize, Serialize};

// ─── Evidence ──────────────────────────────────────────────────────

/// 一个 Capability 的完整推断证据。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Evidence {
    /// 关联的 Capability ID
    pub capability_id: String,
    /// 关联的 Snapshot version ID
    pub snapshot_version: String,
    /// L1 AST 证据
    pub l1: Vec<L1Evidence>,
    /// L2 规则证据
    pub l2: Vec<L2Evidence>,
    /// L3 LLM 证据（可选，如果用户未注入 L3 provider 则为 None）
    pub l3: Option<L3Evidence>,
    /// 三层置信度评分
    pub confidence: EvidenceConfidence,
    /// 证据生成时间
    pub generated_at: String,
    /// 证据 hash（与 snapshot 创建时记录的 hash 对比用）
    pub evidence_hash: String,
}

// ─── L1Evidence ────────────────────────────────────────────────────

/// L1 AST 层面的符号提取证据。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct L1Evidence {
    /// 符号名称
    pub symbol: String,
    /// 所在文件
    pub file_path: String,
    /// 行号
    pub line: usize,
    /// 符号类型: fn / struct / enum / trait / impl / mod
    pub kind: SymbolKind,
    /// 可见性: pub / pub(crate) / private
    pub visibility: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Implementation,
    Module,
    Other(String),
}

// ─── L2Evidence ────────────────────────────────────────────────────

/// L2 规则匹配证据。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct L2Evidence {
    /// 匹配的规则 ID
    pub rule_id: String,
    /// 规则名称
    pub rule_name: String,
    /// 被匹配的 L1 符号
    pub matched_symbol: String,
    /// 触发的文件变更
    pub file_change: String,
    /// 匹配原因描述
    pub reason: String,
    /// 严重程度
    pub severity: String,
}

// ─── L3Evidence ────────────────────────────────────────────────────

/// L3 LLM 推断证据。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct L3Evidence {
    /// LLM prompt 的 hash（用于校验一致性，不存原文）
    pub prompt_hash: String,
    /// LLM 输出的推断片段
    pub fragment: String,
    /// LLM 模型名称
    pub model: String,
    /// LLM 调用时间
    pub timestamp: String,
    /// 原始 response 的 hash
    pub response_hash: String,
}

// ─── EvidenceConfidence ────────────────────────────────────────────

/// 三层独立置信度评分（初期方案）。
///
/// 每层 0.0–1.0，不做加权合并。
/// 后期积累数据后切换为自适应加权公式。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvidenceConfidence {
    /// L1 AST 证据可信度
    ///
    /// 基于：符号提取的完整性、跨文件一致性
    pub l1_score: f64,

    /// L2 规则匹配可信度
    ///
    /// 基于：规则命中数、命中规则的 severity、是否有冲突匹配
    pub l2_score: f64,

    /// L3 LLM 推断可信度
    ///
    /// 基于：LLM 输出与 L1/L2 证据的一致性
    /// 如果未配置 L3 provider，则为 None
    pub l3_score: Option<f64>,
}

// ─── EvidenceHash ──────────────────────────────────────────────────

/// Snapshot 创建时记录的轻量证据 hash。
///
/// 存储在 BehaviorSnapshot 或 Capability 中，
/// 用于事后 `evidence generate` 时校验证据是否一致。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvidenceHash {
    pub l1_hash: String,
    pub l2_hash: String,
    pub l3_hash: Option<String>,
}
```

---

## 6. 置信度评分

### 6.1 初期方案：三层独立评分

```rust
/// 计算 L1 置信度 (0.0–1.0)。
fn compute_l1_score(l1_evidence: &[L1Evidence]) -> f64 {
    if l1_evidence.is_empty() {
        return 0.0;
    }

    let total = l1_evidence.len() as f64;
    let pub_count = l1_evidence.iter()
        .filter(|e| e.visibility == "pub")
        .count() as f64;

    // 公开符号占比高 → 推断更可靠
    let visibility_bonus = if total > 0.0 { pub_count / total } else { 0.0 };

    // 符号数量在合理范围内 (1-20) → 满分
    // 太多 (>50) → 推断可能过于宽泛
    let count_score = if total <= 20.0 {
        1.0
    } else if total <= 50.0 {
        0.7
    } else {
        0.4
    };

    (count_score * 0.6 + visibility_bonus * 0.4).clamp(0.0, 1.0)
}

/// 计算 L2 置信度 (0.0–1.0)。
fn compute_l2_score(l2_evidence: &[L2Evidence]) -> f64 {
    if l2_evidence.is_empty() {
        return 0.0;
    }

    let total = l2_evidence.len() as f64;
    let high_severity = l2_evidence.iter()
        .filter(|e| e.severity == "high")
        .count() as f64;

    // high severity 命中比例
    let severity_score = if total > 0.0 { high_severity / total } else { 0.0 };

    // 规则命中数 2-5 视为理想范围
    let count_score = match total as usize {
        0 => 0.0,
        1 => 0.5,
        2..=5 => 1.0,
        6..=10 => 0.7,
        _ => 0.4,
    };

    (count_score * 0.5 + severity_score * 0.5).clamp(0.0, 1.0)
}

/// 计算 L3 置信度 (0.0–1.0)。
///
/// 需要 L1+L2 证据作为交叉校验参照。
fn compute_l3_score(l3: &L3Evidence, l1: &[L1Evidence], l2: &[L2Evidence]) -> f64 {
    // 检查 L3 输出是否引用了 L1 中提取的符号名称
    let mut l1_ref_count = 0u32;
    for l1e in l1 {
        if l3.fragment.contains(&l1e.symbol) {
            l1_ref_count += 1;
        }
    }
    let l1_ref_rate = if !l1.is_empty() {
        l1_ref_count as f64 / l1.len() as f64
    } else {
        0.0
    };

    // L3 输出非空
    let has_content = !l3.fragment.is_empty();

    let base = if has_content { 0.5 } else { 0.0 };
    let ref_bonus = l1_ref_rate * 0.5;

    (base + ref_bonus).clamp(0.0, 1.0)
}
```

### 6.2 后期方案：数据驱动加权

当积累了足够的历史数据后，通过分析"稳定 Capability"（连续多个版本未被标记为 Deleted/Modified）中各层证据的贡献，反推出自适应权重。

---

## 7. 证据采集时机

### 7.1 Hash 记录（snapshot 创建时）

```rust
/// Snapshot 创建时同步记录证据 hash（低开销）。
/// 在 `snapshot create` 过程中，执行完 L1 AST 和 L2 Rules 后，
/// 对输出做 hash 并存储到 Capability 的 evidence_hashes 字段。
struct EvidenceHashRecorder {
    // 对 l1_output 做 hash
    fn record_l1_hash(l1_output: &[AstSymbol]) -> String;
    // 对 l2_output 做 hash
    fn record_l2_hash(l2_output: &[RuleMatch]) -> String;
    // 对 l3_output 做 hash（如果配置了 L3 provider）
    fn record_l3_hash(l3_output: Option<&LlmFragment>) -> Option<String>;
}
```

### 7.2 详情重建（按需）

```bash
# 为指定 snapshot 重建完整证据
paporot evidence generate --snapshot v1

# 为指定 Capability 重建证据
paporot evidence generate --capability cap_auth_001
```

重建逻辑：
1. 找到对应 snapshot 的代码
2. 重新运行 L1 AST 和 L2 Rules
3. 对比 hash：一致 → 证据有效；不一致 → 警告用户代码已变
4. 如果配置了 L3 provider → 调用 L3 推断
5. 计算置信度评分
6. 保存 Evidence 到 `.Paporot/evidence/` 目录

---

## 8. L3 证据接口

### 8.1 `LlmEvidenceProvider` trait

```rust
//! L3 LLM 证据采集的抽象接口。
//!
//! 用户通过实现此 trait 注入自己的 LLM 服务，
//! Paporot 在 evidence generate 时调用它获取 L3 证据。

use crate::evidence::types::L3Evidence;

/// L3 LLM 证据提供者。
///
/// # 实现要求
///
/// - Send + Sync: 可在多线程环境下使用
/// - 错误处理: 返回 None 表示 LLM 不可用，证据降级为 L1+L2 only
pub trait LlmEvidenceProvider: Send + Sync {
    /// 根据 L1 + L2 证据生成 L3 推断证据。
    ///
    /// # 参数
    ///
    /// - `l1_evidence`: L1 AST 符号列表
    /// - `l2_evidence`: L2 规则匹配列表
    /// - `diff_context`: 本次 diff 的上下文（文件变更摘要）
    ///
    /// # 返回
    ///
    /// - `Some(L3Evidence)` 推断成功
    /// - `None` LLM 不可用（跳过 L3 评分）
    fn infer(
        &self,
        l1_evidence: &[crate::evidence::types::L1Evidence],
        l2_evidence: &[crate::evidence::types::L2Evidence],
        diff_context: &str,
    ) -> Option<L3Evidence>;

    /// LLM 服务名称。
    fn name(&self) -> &str;

    /// LLM 模型名称。
    fn model(&self) -> &str;
}
```

### 8.2 配置方式

```toml
# .Paporot/config.toml
[evidence.l3]
# 内置一个基于 DeepSeek API 的默认实现
provider = "deepseek"
api_key = "${DEEPSEEK_API_KEY}"
model = "deepseek-chat"
```

或通过 Rust API 注入自定义实现：

```rust
use Paporot::evidence::provider::LlmEvidenceProvider;
use Paporot::evidence::collector::EvidenceCollector;

let collector = EvidenceCollector::new()
    .with_l3_provider(Box::new(MyCustomLlmProvider::new()));
```

---

## 9. CLI 子命令

### 9.1 `paporot evidence`

```rust
pub enum EvidenceAction {
    /// 生成/重建证据
    Generate {
        /// Snapshot version ID
        #[arg(long, group = "target")]
        snapshot: Option<String>,

        /// Capability ID
        #[arg(long, group = "target")]
        capability: Option<String>,

        /// 输出格式: json | html
        #[arg(long, default_value = "html")]
        format: String,
    },

    /// 查看已有证据
    Show {
        /// Capability ID
        capability: String,
        /// 输出格式
        #[arg(long, default_value = "html")]
        format: String,
    },

    /// 查看所有 Capability 的置信度概览
    Confidence,
}
```

### 9.2 使用示例

```bash
# 生成证据并打开 HTML 溯源图
paporot evidence generate --snapshot v1 --format html

# 查看单个 Capability 的证据
paporot evidence show --capability cap_user_login

# 置信度概览
paporot evidence confidence
```

---

## 10. HTML 溯源图

### 10.1 布局（与 Trajectory Diff 共用 HTML 报告框架）

```
┌──────────────────────────────────────────────┐
│  Capability Evidence: User Login             │
│  cap_user_login  |  snapshot: v1             │
│  ┌──────────┬──────────┬──────────────────┐  │
│  │ L1: 0.85 │ L2: 0.72 │ L3: 0.90 (opt)  │  │
│  │ 4 符号   │ 3 规则   │ deepseek-chat    │  │
│  └──────────┴──────────┴──────────────────┘  │
│──────────────────────────────────────────────│
│  溯源图                                       │
│                                              │
│  ┌── L1 AST ──────────────────────────────┐  │
│  │  login()         src/auth.rs:42  [fn]  │──┤
│  │  AuthService     src/auth.rs:15  [str] │  │
│  │  validate()      src/auth.rs:78  [fn]  │  │
│  │  UserSession     src/auth.rs:102 [str] │  │
│  └──────────────────────────────────────┬─┘  │
│                                         │    │
│  ┌── L2 Rules ──────────────────────────│──┐  │
│  │  auth_pattern    ← login()     HIGH  │  │  │
│  │  security_check  ← AuthService MED   │  │  │
│  │  validation_rule ← validate() HIGH   │──┤  │
│  └──────────────────────────────────────┬─┘  │
│                                         │    │
│  ┌── L3 LLM ────────────────────────────│──┐  │
│  │  prompt_hash: abc123def456           │  │  │
│  │  "email/password based               │  │  │
│  │   authentication flow with           │  │  │
│  │   session management"                │  │  │
│  └──────────────────────────────────────┘  │  │
│──────────────────────────────────────────────│
│  详细证据 (可折叠)                            │
│  ▼ login() – L1 详情                        │
│    文件: src/auth.rs:42                     │
│    类型: pub fn                             │
│    签名: fn login(email: &str, pw: &str) ...│
│  ▼ auth_pattern – L2 详情                   │
│    规则ID: rule_auth_001                    │
│    匹配原因: 检测到身份验证入口函数          │
│  ...                                        │
└──────────────────────────────────────────────┘
```

### 10.2 交互功能

- 点击 L1 符号 → 展开详细 AST 信息
- 点击 L2 规则 → 展开规则匹配详情
- L1→L2 连线高亮当前选中项
- L2→L3 连线表示规则整合
- 置信度颜色梯度：`< 0.4` 红色 → `0.4–0.7` 黄色 → `> 0.7` 绿色

---

## 11. 测试策略

### 单元测试

| 模块 | 关键用例 |
|------|---------|
| `types.rs` | serde 往返；EvidenceHash 校验 |
| `confidence.rs` | L1 空证据 → 0.0；L1 合理范围 → 高分；L2 多 high 规则 → 高分；L3 无 L1 引用 → 低分 |
| `collector.rs` | L1+L2 采集完整性；Hash 记录与校验一致/不一致 |
| `provider.rs` | trait 对象安全；None 返回不崩溃 |

### 集成测试

- 从现有 snapshot 重建证据，验证 hash 一致性
- Mock L3 provider 注入测试
- 变更代码后重建证据 → 警告 hash 不一致
- evidence generate → evidence show 验证
