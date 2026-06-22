# PRD: Behavior Eval 模块（接口级）

> Paporot 从 Capability Version Control 迈向 Behavior Version Control 的第四步

---

## 目录

1. [背景与动机](#1-背景与动机)
2. [核心概念](#2-核心概念)
3. [设计决策记录](#3-设计决策记录)
4. [模块架构](#4-模块架构)
5. [数据类型定义](#5-数据类型定义)
6. [规则检测引擎](#6-规则检测引擎)
7. [趋势分析](#7-趋势分析)
8. [输出集成](#8-输出集成)
9. [CLI 子命令](#9-cli-子命令)
10. [测试策略](#10-测试策略)

---

## 1. 背景与动机

### 1.1 问题

Trajectory Diff 可以告诉你"Agent 的行为怎么变了"，但不回答：

> 这个变化是好是坏？行为是在改进还是在退化？

举例：

```
Capability "Bug Fix" 的演变：
  v1: 12 tool calls, 45s, 320 tokens
  v2: 15 tool calls, 62s, 450 tokens  ← 多了测试和 lint（正向变化？）
  v3: 25 tool calls, 120s, 800 tokens ← 调用数翻倍（退化警报？）
```

### 1.2 与 Trajectory Diff 和 Capability Evidence 的关系

- **输入**：Trajectory Diff 输出（版本间 tool 变化）+ Capacity Evidence 置信度
- **输出**：退化/改进判定 + 趋势报告

---

## 2. 核心概念

| 概念 | 定义 |
|------|------|
| **EvalResult** | 单次评测结果：通过 / 退化 / 需关注 |
| **EvalVerdict** | 判定结论：Pass / Degraded / Watch |
| **DegradeRule** | 退化检测规则：命中条件 + 严重程度 + 描述 |
| **TrendPoint** | 趋势数据点：一个版本的 tool 调用数 / Token / 耗时 / 置信度 |
| **TrendAnalyzer** | 趋势分析器：检测统计异常（超过 2σ） |

---

## 3. 设计决策记录

| # | 决策 | 备选方案 | 选择理由 |
|---|------|---------|---------|
| D1 | 规则命中 + 趋势异常叠加检测 | 纯规则 / 纯趋势 | 规则 = 已知退化模式（高优先级）；趋势 = 未知异常（低优先级） |
| D2 | HTML + CI exit code + 融入 Regression 报告 | 仅 HTML / 仅 CI | 三种消费场景：人审查、CI 门禁、回归报告汇总 |
| D3 | 趋势阈值 2σ（两倍标准差） | 1σ / 固定阈值 | 2σ 平衡误报率和漏报率 |

---

## 4. 模块架构

### 4.1 目录结构

```
src/
  evaler/                                ← 新增一级模块
    mod.rs
    types.rs                             ← EvalResult / EvalVerdict / DegradeRule / TrendPoint
    rules.rs                             ← 退化规则引擎
    trends.rs                            ← 趋势分析器
    report.rs                            ← HTML 报告生成
  commands/
    evaler.rs                            ← CLI 子命令 eval / rules
  cli.rs                                 ← + Commands::Evaler
  lib.rs                                 ← + pub mod evaler;
```

### 4.2 依赖关系

```
commands/evaler.rs  →  evaler/rules.rs + evaler/trends.rs
                             ├── evaler/types.rs
                             └── 消费 trajectory/types.rs (TrajectoryDiff)
                                 消费 evidence/types.rs (EvidenceConfidence)
```

---

## 5. 数据类型定义

### 5.1 文件: `src/evaler/types.rs`

```rust
use serde::{Deserialize, Serialize};

// ─── EvalResult ────────────────────────────────────────────────────

/// 单次 Behavior Eval 的完整结果。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvalResult {
    /// 被评测的 Capability ID
    pub capability_id: String,
    /// 版本 A 的 trace ID
    pub trace_id_a: String,
    /// 版本 B 的 trace ID
    pub trace_id_b: String,
    /// 生成时间
    pub evaluated_at: String,
    /// 判定结论
    pub verdict: EvalVerdict,
    /// 命中的退化规则
    pub hit_rules: Vec<DegradeRuleHit>,
    /// 趋势异常（如果有）
    pub trend_anomalies: Vec<TrendAnomaly>,
    /// 推荐操作
    pub recommendations: Vec<String>,
}

// ─── EvalVerdict ───────────────────────────────────────────────────

/// 评测判定。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum EvalVerdict {
    /// 通过：无退化迹象
    Pass,
    /// 退化：命中关键退化规则
    Degraded,
    /// 需关注：趋势异常，但未命中关键规则
    Watch,
}

// ─── DegradeRule / DegradeRuleHit ──────────────────────────────────

/// 退化检测规则定义。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DegradeRule {
    /// 规则 ID
    pub id: String,
    /// 规则名称
    pub name: String,
    /// 规则描述
    pub description: String,
    /// 严重程度: critical / high / medium / low
    pub severity: String,
    /// 检测指标: "tool_call_count" | "duration_ms" | "token_usage" | "phase_missing"
    pub metric: String,
    /// 变化方向: "increase" | "decrease" | "missing"
    pub direction: String,
    /// 阈值（百分比，如 50 表示变化超过 50% 则命中）
    pub threshold_pct: f64,
}

/// 规则命中记录。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DegradeRuleHit {
    pub rule_id: String,
    pub rule_name: String,
    pub severity: String,
    pub description: String,
    pub actual_value_a: f64,
    pub actual_value_b: f64,
}

// ─── TrendPoint / TrendAnomaly ─────────────────────────────────────

/// 一个版本的评测趋势数据点。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrendPoint {
    pub version: String,
    pub trace_id: String,
    pub tool_call_count: usize,
    pub duration_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub l1_score: Option<f64>,
    pub l2_score: Option<f64>,
}

/// 趋势异常。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrendAnomaly {
    /// 异常指标
    pub metric: String,
    /// 当前值
    pub current_value: f64,
    /// 历史均值
    pub mean: f64,
    /// 标准差
    pub std_dev: f64,
    /// 偏离标准差倍数
    pub sigma: f64,
}
```

---

## 6. 规则检测引擎

### 6.1 内置退化规则

```rust
/// 默认退化规则集。
pub fn default_degrade_rules() -> Vec<DegradeRule> {
    vec![
        DegradeRule {
            id: "R001".into(),
            name: "工具调用数暴增".into(),
            description: "Tool 调用数相比上一个版本增加超过 100%".into(),
            severity: "high".into(),
            metric: "tool_call_count".into(),
            direction: "increase".into(),
            threshold_pct: 100.0,
        },
        DegradeRule {
            id: "R002".into(),
            name: "执行时间翻倍".into(),
            description: "总执行时间相比上一个版本增加超过 100%".into(),
            severity: "high".into(),
            metric: "duration_ms".into(),
            direction: "increase".into(),
            threshold_pct: 100.0,
        },
        DegradeRule {
            id: "R003".into(),
            name: "Token 消耗暴涨".into(),
            description: "Token 消耗相比上一个版本增加超过 200%".into(),
            severity: "medium".into(),
            metric: "token_usage".into(),
            direction: "increase".into(),
            threshold_pct: 200.0,
        },
        DegradeRule {
            id: "R004".into(),
            name: "跳过验证阶段".into(),
            description: "v1 有验证阶段（test/lint），v2 丢失".into(),
            severity: "critical".into(),
            metric: "phase_missing".into(),
            direction: "missing".into(),
            threshold_pct: 0.0,
        },
        DegradeRule {
            id: "R005".into(),
            name: "新增网络调用".into(),
            description: "v2 新增了 web_search/web_fetch 等网络 tool".into(),
            severity: "medium".into(),
            metric: "new_network_calls".into(),
            direction: "increase".into(),
            threshold_pct: 0.0,
        },
        DegradeRule {
            id: "R006".into(),
            name: "置信度下降".into(),
            description: "Capability Evidence 置信度下降超过 0.3".into(),
            severity: "low".into(),
            metric: "confidence_drop".into(),
            direction: "decrease".into(),
            threshold_pct: 30.0,
        },
    ]
}
```

### 6.2 规则评估流程

```
输入: TrajectoryDiff, Option<Evidence>

for each rule in rules:
    1. 根据 rule.metric 提取 TrajectoryDiff 或 Evidence 中的对应值
    2. 计算变化比例: (value_b - value_a) / value_a * 100%
    3. 如果超过 rule.threshold_pct → 记录 DegradeRuleHit

if 命中任何 critical severity 规则:
    verdict = Degraded
else if 命中任何 high/medium 规则:
    verdict = Watch
else:
    verdict = Pass
```

---

## 7. 趋势分析

### 7.1 趋势数据采集

```rust
/// 为指定 Capability 采集所有历史版本的评测数据点。
fn collect_trend_points(
    storage: &TraceStorage,
    capability_id: &str,
) -> Vec<TrendPoint> {
    // 1. 查找该 capability_id 关联的所有 trace
    // 2. 按 started_at 排序
    // 3. 提取每个 trace 的 key metrics
    // 4. 如果配置了 Evidence，附带 L1/L2 评分
}
```

### 7.2 异常检测

```rust
/// 检测当前版本的值是否偏离历史趋势（> 2σ）。
fn detect_anomalies(
    history: &[TrendPoint],
    current: &TrendPoint,
) -> Vec<TrendAnomaly> {
    let mut anomalies = Vec::new();

    // 检测 tool_call_count
    let counts: Vec<f64> = history.iter()
        .map(|p| p.tool_call_count as f64).collect();
    if let Some(anom) = check_anomaly("tool_call_count",
        current.tool_call_count as f64, &counts) {
        anomalies.push(anom);
    }

    // 检测 duration_ms
    let durations: Vec<f64> = history.iter()
        .map(|p| p.duration_ms as f64).collect();
    if let Some(anom) = check_anomaly("duration_ms",
        current.duration_ms as f64, &durations) {
        anomalies.push(anom);
    }

    anomalies
}

fn check_anomaly(metric: &str, current: f64, history: &[f64]) -> Option<TrendAnomaly> {
    let n = history.len() as f64;
    if n < 3.0 { return None; } // 至少需要 3 个历史数据点

    let mean = history.iter().sum::<f64>() / n;
    let variance = history.iter()
        .map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    let std_dev = variance.sqrt();

    if std_dev == 0.0 { return None; }

    let sigma = (current - mean) / std_dev;

    if sigma.abs() > 2.0 {
        Some(TrendAnomaly {
            metric: metric.into(),
            current_value: current,
            mean,
            std_dev,
            sigma,
        })
    } else {
        None
    }
}
```

---

## 8. 输出集成

### 8.1 HTML 报告（与 Trajectory Diff / Evidence 共用框架）

```
┌──────────────────────────────────────────────┐
│  Behavior Eval: Bug Fix                      │
│  cap_bug_fix_001  |  verdict: ⚠ Watch       │
│──────────────────────────────────────────────│
│  ┌─────────────┐  ┌──────────────┐          │
│  │ 工具调用数   │  │ Token 消耗   │   ...    │
│  │ +25%  ⚠     │  │ +40%  ⚠     │          │
│  └─────────────┘  └──────────────┘          │
│──────────────────────────────────────────────│
│  退化规则命中 (3)                             │
│  ├── 🔴 R004 跳过验证阶段 [critical]          │
│  ├── 🟡 R002 执行时间翻倍 [high]              │
│  └── 🟡 R005 新增网络调用 [medium]            │
│──────────────────────────────────────────────│
│  趋势图                                       │
│  tool 调用数                                 │
│  30 ┤                              ●(v5)     │
│  20 ┤          ●(v3)                         │
│  10 ┤ ●(v1) ●(v2)                            │
│     └──┬────┬────┬────┬────┬──               │
│       v1   v2   v3   v4   v5                 │
│           ─── 均值  ─── 2σ阈值               │
│──────────────────────────────────────────────│
│  建议操作                                     │
│  · v5 跳过验证阶段，建议确认是否为有意变更    │
│  · 如果确认 OK，可通过 `paporot evaler accept`│
│    更新基线                                   │
└──────────────────────────────────────────────┘
```

### 8.2 CI 集成（Exit Code）

```bash
# CI 管道中使用
paporot behavior eval --capability cap_bug_fix_001 --ci-mode

# exit code:
#   0 = Pass
#   1 = Degraded (critical 规则命中 → 阻断 CI)
#   2 = Watch (建议人工审查，不阻断 CI)
```

### 8.3 融入 Regression 报告

现有的 `paporot regression` 报告新增 "行为回归" 章节：

```rust
// 在 src/commands/regression.rs 中
let eval_result = evaler::evaluate(&diff, &evidence)?;
regression_report.behavior = Some(eval_result);
```

---

## 9. CLI 子命令

### 9.1 `paporot behavior`

```rust
pub enum BehaviorAction {
    /// 评测当前版本对上一个版本的行为变化
    Eval {
        /// Capability ID
        #[arg(long)]
        capability: String,

        /// CI 模式（exit code 决定通过/失败）
        #[arg(long)]
        ci_mode: bool,

        /// 输出格式: html | json
        #[arg(long, default_value = "html")]
        format: String,
    },

    /// 列出和管理退化规则
    Rules {
        #[command(subcommand)]
        action: RuleAction,
    },

    /// 接受当前行为作为新基线（重置趋势）
    Accept {
        capability: String,
    },
}

pub enum RuleAction {
    /// 列出所有退化规则
    List,
    /// 禁用某条规则
    Disable { rule_id: String },
    /// 启用某条规则
    Enable { rule_id: String },
}
```

### 9.2 使用示例

```bash
# 评测
paporot behavior eval --capability cap_bug_fix_001

# CI 门禁
paporot behavior eval --capability cap_bug_fix_001 --ci-mode

# 查看当前规则
paporot behavior rules list

# 接受新行为为基线
paporot behavior accept --capability cap_bug_fix_001
```

---

## 10. 测试策略

### 单元测试

| 模块 | 关键用例 |
|------|---------|
| `rules.rs` | 全量退化规则命中；空规则集；阈值边界 |
| `trends.rs` | < 3 个数据点 → 不检测；正好 2σ 边界；远超过 2σ；稳定序列 |
| `types.rs` | EvalVerdict 序列化；TrendPoint 序列化 |

### 集成测试

- 构造一个明显的退化场景（跳过验证阶段），验证 verdict = Degraded
- 构造稳定变化场景（小幅度波动），验证 verdict = Pass
- 趋势分析：用 5 个历史数据点 + 1 个异常点，验证检测到 2σ 异常
- CI mode exit code 验证
