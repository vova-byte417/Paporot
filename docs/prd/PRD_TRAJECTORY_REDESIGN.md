# PRD: Trajectory Diff + Eval 重构（接口级）

> 基于 PRD_TRAJECTORY_DIFF.md，融入 TrajectoryAnalysis 中间层 + 架构拆分

---

## 目录

1. [修订概要](#1-修订概要)
2. [模块架构（修订）](#2-模块架构修订)
3. [数据类型定义](#3-数据类型定义)
4. [PhaseClassifier trait](#4-phaseclassifier-trait)
5. [align/ 子模块](#5-align-子模块)
6. [TrajectoryAnalysis（关键新增）](#6-trajectoryanalysis关键新增)
7. [Eval 重构](#7-eval-重构)
8. [存储：trajectory_cache.db](#8-存储trajectory_cachedb)
9. [CLI 命令](#9-cli-命令)
10. [Dashboard Tab 4](#10-dashboard-tab-4)
11. [错误类型](#11-错误类型)
12. [测试策略](#12-测试策略)

---

## 1. 修订概要

### 相对于 PRD_TRAJECTORY_DIFF.md 的变更

| 原设计 | 修订 | 理由 |
|--------|------|------|
| `phases.rs` 硬编码函数 | `classifier.rs` + `PhaseClassifier` trait | 未来可插拔 RuleBased / LLM / Embedding 分类器 |
| `align.rs` 单文件 | `align/` 子模块（segment/tool/scorer/engine） | 未来扩展匹配算法无需重构 |
| Eval 直接消费 `TrajectoryDiff` | Eval 消费 `TrajectoryAnalysis` | Diff 是 IR，Analysis 是 Eval 契约，避免每 Rule 重复遍历 |
| 扩展 `trace_index.db` | 新建 `trajectory_cache.db` | Source of Truth 与 Derived Data 分离 |
| 数据流: Diff → Eval | 数据流: Diff → Analysis → Eval | 后续几十条行为规则可扩展的关键 |

### 完整数据流

```
BehaviorTrace (A)          BehaviorTrace (B)
       │                         │
       └──────────┬──────────────┘
                  ▼
         PhaseClassifier::classify()
                  │
                  ▼
         Vec<PhaseSegment>
                  │
                  ▼
         AlignmentEngine::align()
         ├── SegmentMatcher::match_segments()
         └── ToolMatcher::align_tools()
                  │
                  ▼
            TrajectoryDiff  ──────→  report.rs (Mermaid + HTML)
                  │
                  ▼
         TrajectoryAnalysis::from_diff()
                  │
         ┌────────┼────────┐
         ▼        ▼        ▼
    builtin_rules  segment_rules  trend_detect
         │        │        │
         └────────┼────────┘
                  ▼
             EvalResult
```

---

## 2. 模块架构（修订）

```
src/
  trajectory/                        ← NEW
    mod.rs
    types.rs                         ← TrajectoryDiff / SegmentDiff / ToolDiff / PhaseSegment ...
    classifier.rs                    ← PhaseClassifier trait + RuleBasedClassifier
    hash.rs                          ← SemanticHash
    align/
      mod.rs                         ← re-export
      engine.rs                      ← AlignmentEngine (orchestrator)
      segment.rs                     ← SegmentMatcher
      tool.rs                        ← ToolMatcher
      scorer.rs                      ← Cost function (InsertionCost / DeletionCost / SubstitutionCost)
    analysis.rs                      ← TrajectoryAnalysis + from_diff()
    report.rs                        ← Mermaid + JSON + HTML 数据生成
    cache.rs                         ← trajectory_cache.db (rusqlite)
  evaler/                            ← REFACTOR
    mod.rs
    types.rs                         ← UNCHANGED
    rules.rs                         ← + segment_rules(analysis: &TrajectoryAnalysis)
    trends.rs                        ← UNCHANGED
  commands/
    trajectory.rs                    ← NEW: paporot trajectory diff
    evaler.rs                        ← REFACTOR: + --mode trajectory
  cli.rs                             ← + Commands::Trajectory, + eval --mode
  lib.rs                             ← + pub mod trajectory;
```

### 依赖关系

```
commands/trajectory.rs
  → trajectory/engine.rs
       → trajectory/classifier.rs   (PhaseClassifier trait)
       → trajectory/align/segment.rs
       → trajectory/align/tool.rs
       → trajectory/align/scorer.rs
       → trajectory/hash.rs
       → trajectory/types.rs
       → trajectory/report.rs
       → trajectory/cache.rs

commands/evaler.rs
  → trajectory/analysis.rs
       → trajectory/types.rs
  → evaler/rules.rs
       → trajectory/analysis.rs     (segment_rules)
       → trace/types.rs              (builtin_rules 保持不变)
       → evaler/types.rs
  → evaler/trends.rs
```

- `trajectory/` 模块消费 `trace/types.rs::BehaviorTrace`，只读
- `evaler/` 模块消费 `trajectory/analysis.rs::TrajectoryAnalysis`（段级规则）和 `trace/types.rs::BehaviorTrace`（全局规则）
- `trajectory/` 不依赖 `analysis/`、`agent.rs`、`config.rs`

---

## 3. 数据类型定义

### 3.1 文件：`src/trajectory/types.rs`

```rust
use serde::{Deserialize, Serialize};

// ─── TrajectoryDiff ───────────────────────────────────────────────

/// 两条 BehaviorTrace 的完整差异对比结果。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrajectoryDiff {
    /// 对比的 Capability ID
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SegmentDiff {
    /// 段标签，如 "定位问题"、"实施修改"、"验证"、"提交"
    pub label: String,
    pub kind: SegmentKind,
    pub tool_diffs: Vec<ToolDiff>,
    pub index_a: Option<usize>,
    pub index_b: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum SegmentKind {
    Unchanged,
    Modified,
    Added,
    Deleted,
}

// ─── ToolDiff ─────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolDiff {
    pub tool_name: String,
    pub kind: ToolDiffKind,
    pub index_a: Option<usize>,
    pub index_b: Option<usize>,
    pub args_diff: Option<ArgsDiff>,
    pub duration_ms: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ToolDiffKind {
    Unchanged,
    Added,
    Deleted,
    ArgsChanged,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ArgsDiff {
    pub args_a: serde_json::Value,
    pub args_b: serde_json::Value,
}

// ─── DiffSummary ──────────────────────────────────────────────────

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

// ─── DiffInput ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum DiffInput {
    ByCapability { capability_id: String },
    Manual { trace_id_a: String, trace_id_b: String },
}

// ─── PhaseSegment (分类器输出) ─────────────────────────────────────

/// PhaseClassifier 的输出：一条 trace 被切割为多个 PhaseSegment。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseSegment {
    /// 阶段标签（如 "定位问题"）
    pub label: String,
    /// 该段包含的 tool_calls（原始索引 + 名称，不复制完整数据）
    pub tool_indices: Vec<ToolIndexInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolIndexInfo {
    /// 在原始 trace.tool_calls 中的索引
    pub index: usize,
    pub tool_name: String,
}
```

---

## 4. PhaseClassifier trait

### 4.1 文件：`src/trajectory/classifier.rs`

```rust
use super::types::PhaseSegment;
use crate::trace::types::BehaviorTrace;

/// 将 BehaviorTrace 中的 tool_calls 分类为语义阶段。
///
/// 设计为 trait 以支持未来插拔:
/// - RuleBasedClassifier (当前实现)
/// - LLMClassifier (未来)
/// - EmbeddingClassifier (未来)
pub trait PhaseClassifier: Send + Sync {
    /// 分类器名称（用于序列化/缓存标识）
    fn name(&self) -> &str;

    /// 分类器版本（缓存失效用）
    fn version(&self) -> &str;

    /// 对 trace 进行阶段分类
    fn classify(&self, trace: &BehaviorTrace) -> Vec<PhaseSegment>;
}

/// 内置的基于规则的分类器。
pub struct RuleBasedClassifier {
    /// 阶段名 → 匹配的 tool 名称列表
    pub rules: Vec<PhaseMapping>,
    /// 默认阶段名（未匹配的 tool 归入此阶段）
    pub default_phase: String,
}

#[derive(Debug, Clone)]
pub struct PhaseMapping {
    pub phase: String,
    pub tool_names: Vec<String>,
}

impl RuleBasedClassifier {
    /// 使用默认规则创建
    pub fn default() -> Self {
        Self {
            rules: vec![
                PhaseMapping {
                    phase: "定位问题".into(),
                    tool_names: vec![
                        "read", "grep", "glob", "search_codebase",
                        "web_search", "web_fetch", "ls", "list",
                    ].into_iter().map(String::from).collect(),
                },
                PhaseMapping {
                    phase: "实施修改".into(),
                    tool_names: vec![
                        "write", "edit", "search_replace", "delete_file",
                        "bash", "run_command",
                    ].into_iter().map(String::from).collect(),
                },
                PhaseMapping {
                    phase: "验证".into(),
                    tool_names: vec![
                        "test", "cargo", "check", "lint", "clippy",
                        "build", "compile",
                    ].into_iter().map(String::from).collect(),
                },
                PhaseMapping {
                    phase: "提交".into(),
                    tool_names: vec![
                        "commit", "git", "push", "pull_request",
                    ].into_iter().map(String::from).collect(),
                },
            ],
            default_phase: "其他".into(),
        }
    }
}

impl PhaseClassifier for RuleBasedClassifier {
    fn name(&self) -> &str { "rule_based" }
    fn version(&self) -> &str { "1.0.0" }

    fn classify(&self, trace: &BehaviorTrace) -> Vec<PhaseSegment> {
        // 算法: 遍历 tool_calls，遇到 phase 变化时切分新段
        // ...
    }
}
```

---

## 5. align/ 子模块

### 5.1 `align/scorer.rs` — 代价函数

```rust
/// 对齐操作的代价定义。
pub struct AlignmentCosts {
    /// 插入一个 tool/segment 的代价
    pub insertion: f64,
    /// 删除一个 tool/segment 的代价
    pub deletion: f64,
    /// 替换一个 tool/segment 的代价
    pub substitution: f64,
}

impl Default for AlignmentCosts {
    fn default() -> Self {
        Self { insertion: 1.0, deletion: 1.0, substitution: 1.0 }
    }
}

/// 计算两个 tool 之间的替换代价。
/// 相同 SemanticHash → 0, 同名称但不同 args → 0.5, 不同名称 → 1.0
pub fn tool_substitution_cost(
    hash_a: u64, hash_b: u64,
    name_a: &str, name_b: &str,
) -> f64 {
    if hash_a == hash_b { 0.0 }
    else if name_a == name_b { 0.5 }
    else { 1.0 }
}
```

### 5.2 `align/segment.rs` — 段级匹配

```rust
use super::types::{PhaseSegment, SegmentKind};

/// 段匹配结果。
pub struct SegmentMatch {
    pub kind: SegmentKind,
    pub index_a: Option<usize>,
    pub index_b: Option<usize>,
}

/// 用贪心算法对齐两个 PhaseSegment 序列。
/// 匹配条件: 段 label 相同。
///
/// 未来可扩展为 Hungarian 或 Needleman-Wunsch。
pub fn match_segments(
    segments_a: &[PhaseSegment],
    segments_b: &[PhaseSegment],
) -> Vec<SegmentMatch> {
    // 当前实现: Greedy matching by label
    // ...
}
```

### 5.3 `align/tool.rs` — Tool 级匹配

```rust
use crate::trace::types::ToolCall;

/// Tool 级对齐结果。
pub struct ToolMatch {
    pub kind: ToolDiffKind,
    pub index_a: Option<usize>,
    pub index_b: Option<usize>,
}

/// 用编辑距离（Levenshtein）对齐 tool 序列。
/// 匹配条件: SemanticHash 相同。
///
/// 性能保护: tool 序列长度 > 200 时降级为贪心匹配。
pub fn align_tools(
    tools_a: &[ToolCall],
    tools_b: &[ToolCall],
    hashes_a: &[u64],
    hashes_b: &[u64],
) -> Vec<ToolMatch> {
    const MAX_EDIT_DISTANCE_LEN: usize = 200;
    if tools_a.len() > MAX_EDIT_DISTANCE_LEN || tools_b.len() > MAX_EDIT_DISTANCE_LEN {
        return greedy_align_tools(tools_a, tools_b, hashes_a, hashes_b);
    }
    levenshtein_align_tools(tools_a, tools_b, hashes_a, hashes_b)
}
```

### 5.4 `align/engine.rs` — 编排器

```rust
use super::{
    segment::match_segments,
    tool::align_tools,
    scorer::AlignmentCosts,
};
use crate::trajectory::{classifier::PhaseClassifier, hash, types::*};
use crate::trace::types::BehaviorTrace;

/// 对齐引擎：编排 segment → tool 双层对齐。
pub struct AlignmentEngine {
    pub costs: AlignmentCosts,
}

impl AlignmentEngine {
    pub fn new(costs: AlignmentCosts) -> Self { Self { costs } }

    /// 计算两条 trace 的 TrajectoryDiff。
    pub fn diff(
        &self,
        classifier: &dyn PhaseClassifier,
        trace_a: &BehaviorTrace,
        trace_b: &BehaviorTrace,
        capability_id: Option<String>,
    ) -> TrajectoryDiff {
        let segments_a = classifier.classify(trace_a);
        let segments_b = classifier.classify(trace_b);

        let segment_matches = match_segments(&segments_a, &segments_b);

        let mut segment_diffs = Vec::new();
        let mut summary = DiffSummary::default();

        for sm in &segment_matches {
            // 对 Unchanged/Modified 段做 tool 级对齐
            let tool_diffs = match sm.kind {
                SegmentKind::Unchanged | SegmentKind::Modified => {
                    // 从 segments_a/b 中取出对应段的 tool_calls
                    // 调用 tool::align_tools()
                    // ...
                }
                SegmentKind::Added | SegmentKind::Deleted => {
                    // 所有 tool 分别标记为 Added / Deleted
                    // ...
                }
            };
            segment_diffs.push(SegmentDiff { /* ... */ });
        }

        // 聚合 summary
        // ...

        TrajectoryDiff {
            capability_id,
            version_a: TrajectoryVersion { /* from trace_a */ },
            version_b: TrajectoryVersion { /* from trace_b */ },
            segments: segment_diffs,
            summary,
        }
    }
}
```

---

## 6. TrajectoryAnalysis（关键新增）

### 6.1 文件：`src/trajectory/analysis.rs`

```rust
use serde::{Deserialize, Serialize};
use super::types::{TrajectoryDiff, SegmentKind, ToolDiffKind};

/// 对 TrajectoryDiff 的结构化分析。
///
/// Eval 规则消费此类型，而非直接消费 TrajectoryDiff。
/// 避免每条规则重复遍历 Diff 数据结构。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrajectoryAnalysis {
    pub trace_id_a: String,
    pub trace_id_b: String,

    // ── 阶段变化 ──
    /// 新增的阶段
    pub phase_additions: Vec<PhaseChange>,
    /// 删除的阶段
    pub phase_deletions: Vec<PhaseChange>,
    /// 修改的阶段
    pub phase_modifications: Vec<PhaseModification>,

    // ── 评分类指标 (0.0 ~ 1.0) ──
    /// Tool churn: 编辑距离 / max(len_a, len_b)
    pub tool_churn_score: f32,
    /// 阶段重排序程度
    pub phase_reorder_score: f32,
    /// 相同 tool 但 args 变化的比例
    pub capability_shift_score: f32,

    // ── 统计摘要 ──
    pub tool_count_a: usize,
    pub tool_count_b: usize,
    pub shared_tool_count: usize,
    pub added_tool_count: usize,
    pub deleted_tool_count: usize,
    pub args_changed_tool_count: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PhaseChange {
    pub label: String,
    pub tool_names: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PhaseModification {
    pub label: String,
    pub tool_count_before: usize,
    pub tool_count_after: usize,
    pub added_tools: Vec<String>,
    pub deleted_tools: Vec<String>,
}

impl TrajectoryAnalysis {
    /// 从 TrajectoryDiff 计算分析结果。纯确定性计算，不调用 LLM。
    pub fn from_diff(diff: &TrajectoryDiff) -> Self {
        let mut analysis = TrajectoryAnalysis {
            trace_id_a: diff.version_a.trace_id.clone(),
            trace_id_b: diff.version_b.trace_id.clone(),
            phase_additions: Vec::new(),
            phase_deletions: Vec::new(),
            phase_modifications: Vec::new(),
            tool_churn_score: 0.0,
            phase_reorder_score: 0.0,
            capability_shift_score: 0.0,
            tool_count_a: diff.version_a.tool_count,
            tool_count_b: diff.version_b.tool_count,
            shared_tool_count: diff.summary.tool_calls_unchanged,
            added_tool_count: diff.summary.tool_calls_added,
            deleted_tool_count: diff.summary.tool_calls_deleted,
            args_changed_tool_count: diff.summary.tool_calls_modified,
        };

        for seg in &diff.segments {
            match seg.kind {
                SegmentKind::Added => {
                    analysis.phase_additions.push(PhaseChange {
                        label: seg.label.clone(),
                        tool_names: seg.tool_diffs.iter().map(|t| t.tool_name.clone()).collect(),
                    });
                }
                SegmentKind::Deleted => {
                    analysis.phase_deletions.push(PhaseChange {
                        label: seg.label.clone(),
                        tool_names: seg.tool_diffs.iter().map(|t| t.tool_name.clone()).collect(),
                    });
                }
                SegmentKind::Modified => {
                    let mut mod_info = PhaseModification {
                        label: seg.label.clone(),
                        tool_count_before: 0,
                        tool_count_after: 0,
                        added_tools: Vec::new(),
                        deleted_tools: Vec::new(),
                    };
                    for td in &seg.tool_diffs {
                        match td.kind {
                            ToolDiffKind::Added => {
                                mod_info.tool_count_after += 1;
                                mod_info.added_tools.push(td.tool_name.clone());
                            }
                            ToolDiffKind::Deleted => {
                                mod_info.tool_count_before += 1;
                                mod_info.deleted_tools.push(td.tool_name.clone());
                            }
                            _ => {
                                mod_info.tool_count_before += 1;
                                mod_info.tool_count_after += 1;
                            }
                        }
                    }
                    analysis.phase_modifications.push(mod_info);
                }
                SegmentKind::Unchanged => {}
            }
        }

        // 计算评分类指标
        let total_tools = (analysis.tool_count_a + analysis.tool_count_b) as f32;
        if total_tools > 0.0 {
            analysis.tool_churn_score =
                (analysis.added_tool_count + analysis.deleted_tool_count) as f32 / total_tools;
            analysis.capability_shift_score =
                analysis.args_changed_tool_count as f32 / total_tools;
        }

        // phase_reorder_score: (additions + deletions) / total phases
        let total_phases = (analysis.phase_additions.len()
            + analysis.phase_deletions.len()
            + analysis.phase_modifications.len()
            + diff.summary.segments_unchanged) as f32;
        if total_phases > 0.0 {
            analysis.phase_reorder_score =
                (analysis.phase_additions.len() + analysis.phase_deletions.len()) as f32 / total_phases;
        }

        analysis
    }
}
```

---

## 7. Eval 重构

### 7.1 文件：`src/evaler/rules.rs` — 新增段级规则

```rust
use crate::trajectory::analysis::TrajectoryAnalysis;
use super::types::{DegradeRule, DegradeRuleHit, RuleSeverity};

/// 基于 TrajectoryAnalysis 的段级退化规则。
pub fn segment_rules() -> Vec<DegradeRule> {
    vec![
        DegradeRule {
            id: "S001".into(),
            name: "phase explosion".into(),
            description: "新增阶段数超过阈值".into(),
            severity: RuleSeverity::High,
            metric: "phase_additions".into(),
            direction: "increase".into(),
            threshold_pct: 0.0, // 新增 >= 2 个阶段即命中（通过 count 判断）
        },
        DegradeRule {
            id: "S002".into(),
            name: "phase missing".into(),
            description: "关键阶段（验证/提交）被删除".into(),
            severity: RuleSeverity::Critical,
            metric: "phase_deletions".into(),
            direction: "increase".into(),
            threshold_pct: 0.0,
        },
        DegradeRule {
            id: "S003".into(),
            name: "tool churn spike".into(),
            description: "tool_churn_score 过高".into(),
            severity: RuleSeverity::High,
            metric: "tool_churn".into(),
            direction: "increase".into(),
            threshold_pct: 50.0, // churn > 0.5
        },
        DegradeRule {
            id: "S004".into(),
            name: "phase reorder".into(),
            description: "阶段执行顺序大幅变化".into(),
            severity: RuleSeverity::Medium,
            metric: "phase_reorder".into(),
            direction: "increase".into(),
            threshold_pct: 50.0, // reorder > 0.5
        },
        DegradeRule {
            id: "S005".into(),
            name: "capability shift".into(),
            description: "相同 tool 但 args 大幅变化".into(),
            severity: RuleSeverity::Medium,
            metric: "capability_shift".into(),
            direction: "increase".into(),
            threshold_pct: 30.0,
        },
    ]
}

/// 对 TrajectoryAnalysis 执行段级规则检测。
pub fn evaluate_segment_rules(
    analysis: &TrajectoryAnalysis,
) -> Vec<DegradeRuleHit> {
    let rules = segment_rules();
    let mut hits = Vec::new();

    for rule in &rules {
        match rule.metric.as_str() {
            "phase_additions" => {
                if analysis.phase_additions.len() >= 2 {
                    hits.push(DegradeRuleHit {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        severity: rule.severity.clone(),
                        description: format!("{} new phases added: {}",
                            analysis.phase_additions.len(),
                            analysis.phase_additions.iter()
                                .map(|p| p.label.as_str())
                                .collect::<Vec<_>>().join(", ")),
                        actual_value_a: 0.0,
                        actual_value_b: analysis.phase_additions.len() as f64,
                    });
                }
            }
            "phase_deletions" => {
                let critical_phases = ["验证", "提交", "verify", "commit"];
                let has_critical = analysis.phase_deletions.iter()
                    .any(|p| critical_phases.contains(&p.label.as_str()));
                if has_critical {
                    hits.push(DegradeRuleHit {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        severity: rule.severity.clone(),
                        description: format!("Critical phases deleted: {}",
                            analysis.phase_deletions.iter()
                                .map(|p| p.label.as_str())
                                .collect::<Vec<_>>().join(", ")),
                        actual_value_a: 0.0,
                        actual_value_b: 1.0,
                    });
                }
            }
            "tool_churn" => {
                let churn_pct = analysis.tool_churn_score * 100.0;
                if churn_pct >= rule.threshold_pct {
                    hits.push(DegradeRuleHit {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        severity: rule.severity.clone(),
                        description: format!("Tool churn score: {:.1}%", churn_pct),
                        actual_value_a: 0.0,
                        actual_value_b: analysis.tool_churn_score as f64,
                    });
                }
            }
            "phase_reorder" => {
                let reorder_pct = analysis.phase_reorder_score * 100.0;
                if reorder_pct >= rule.threshold_pct {
                    hits.push(DegradeRuleHit {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        severity: rule.severity.clone(),
                        description: format!("Phase reorder score: {:.1}%", reorder_pct),
                        actual_value_a: 0.0,
                        actual_value_b: analysis.phase_reorder_score as f64,
                    });
                }
            }
            "capability_shift" => {
                let shift_pct = analysis.capability_shift_score * 100.0;
                if shift_pct >= rule.threshold_pct {
                    hits.push(DegradeRuleHit {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        severity: rule.severity.clone(),
                        description: format!("Capability shift score: {:.1}%", shift_pct),
                        actual_value_a: 0.0,
                        actual_value_b: analysis.capability_shift_score as f64,
                    });
                }
            }
            _ => {}
        }
    }

    hits
}
```

### 7.2 EvalResult 集成（在命令层编排）

```rust
// commands/evaler.rs (伪代码)

fn run_eval(trace_a: &BehaviorTrace, trace_b: &BehaviorTrace, analysis: &TrajectoryAnalysis) -> EvalResult {
    // 1. 全局规则（现有，基于 BehaviorTrace）
    let global_hits = evaluate_builtin_rules(trace_a, trace_b);

    // 2. 段级规则（新增，基于 TrajectoryAnalysis）
    let segment_hits = evaluate_segment_rules(analysis);

    // 3. 趋势检测（现有，基于 TrendPoint）
    let anomalies = detect_anomalies(&trend_points, 2.0);

    // 4. 合并命中 + 判定 verdict
    let all_hits: Vec<_> = global_hits.into_iter().chain(segment_hits).collect();
    let verdict = determine_verdict(&all_hits, &anomalies);

    EvalResult { /* ... */ }
}
```

---

## 8. 存储：trajectory_cache.db

### 8.1 文件：`src/trajectory/cache.rs`

与 `trace_index.db` 分离，独立数据库。

```sql
-- trajectory_cache.db schema

CREATE TABLE IF NOT EXISTS trajectory_diffs (
    id TEXT PRIMARY KEY,              -- "tdiff_{timestamp}_{seq}"
    trace_id_a TEXT NOT NULL,
    trace_id_b TEXT NOT NULL,
    capability_id TEXT,
    diff_json TEXT NOT NULL,          -- TrajectoryDiff 序列化 JSON
    analysis_json TEXT NOT NULL,      -- TrajectoryAnalysis 序列化 JSON
    mermaid_text TEXT,                -- Mermaid 代码
    classifier_name TEXT NOT NULL,
    classifier_version TEXT NOT NULL,
    computed_at TEXT NOT NULL,
    score_tool_churn REAL NOT NULL DEFAULT 0,
    score_phase_reorder REAL NOT NULL DEFAULT 0,
    score_capability_shift REAL NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_diffs_trace_a ON trajectory_diffs(trace_id_a);
CREATE INDEX IF NOT EXISTS idx_diffs_trace_b ON trajectory_diffs(trace_id_b);
CREATE INDEX IF NOT EXISTS idx_diffs_capability ON trajectory_diffs(capability_id);
```

### 8.2 缓存位置

```
.Paporot/
  traces/                          ← Source of Truth
  trace_index.db                   ← Source of Truth
  trajectory_cache.db              ← Derived Data (NEW)
  trajectory/                      ← JSON 导出目录 (NEW, 供 dashboard 读取)
    tdiff_20260612_001.json
```

---

## 9. CLI 命令

### 9.1 `paporot trajectory diff`

```bash
# 通过 Capability 自动关联
paporot trajectory diff --capability cap_bug_fix_001

# 手动指定两条 trace
paporot trajectory diff --trace-a trace_20260612_001 --trace-b trace_20260612_003

# 输出格式
paporot trajectory diff --capability cap_bug_fix_001 --format json
paporot trajectory diff --capability cap_bug_fix_001 --format mermaid
paporot trajectory diff --capability cap_bug_fix_001 --format html

# 指定输出路径
paporot trajectory diff --capability cap_bug_fix_001 --output ./my_report.html

# 使用自定义 phase rules 配置
paporot trajectory diff --capability cap_bug_fix_001 --phases-config phases.toml

# 列出缓存的 diff
paporot trajectory list

# 查看某次 diff 详情
paporot trajectory show <diff_id>
```

### 9.2 `paporot eval run --mode trajectory`

```bash
# 原有模式（仍可用，仅消费 BehaviorTrace）
paporot eval run --trace-a <id> --trace-b <id>

# 新增 trajectory 模式（消费 TrajectoryAnalysis）
paporot eval run --trace-a <id> --trace-b <id> --mode trajectory

# 通过 capability 自动关联
paporot eval run --capability <id> --mode trajectory
```

---

## 10. Dashboard Tab 4: Trajectory Diff

### 10.1 数据来源

```javascript
// dashboard.html 新增标签页读取 .Paporot/trajectory/*.json
async function loadTrajectoryDiffs() {
    // 列出所有 trajectory JSON 文件
    // 提供选择器切换 diff
    const diff = await fetchJSON('.Paporot/trajectory/tdiff_20260612_001.json');
    renderTrajectoryView(diff);
}
```

### 10.2 视图结构

```
┌─────────────────────────────────────────────────────────┐
│ Tab 4: Trajectory Diff                                  │
├─────────────────────────────────────────────────────────┤
│ ┌─────────────── Diff Selector ───────────────────────┐ │
│ │ Capability: cap_bug_fix [v]  Trace A [v] Trace B [v]│ │
│ └──────────────────────────────────────────────────────┘ │
│                                                          │
│ ┌─────────── Summary Cards Row ───────────────────────┐ │
│ │ Added   │ Deleted │ Modified│ Churn   │ Shift      │ │
│ │  3 segs │  0 segs │  2 segs │  0.42   │  0.15      │ │
│ └──────────────────────────────────────────────────────┘ │
│                                                          │
│ ┌──────────────── Mermaid Diagram ────────────────────┐ │
│ │     Timeline A    │   Diff    │    Timeline B        │ │
│ │   ┌──────────┐   │          │   ┌──────────┐       │ │
│ │   │ 定位问题 │   │    =     │   │ 定位问题 │       │ │
│ │   │ read()   │   │          │   │ read()   │       │ │
│ │   │ grep()   │   │          │   │ grep()   │       │ │
│ │   └──────────┘   │          │   └──────────┘       │ │
│ │   ┌──────────┐   │   (+)    │   ┌──────────┐       │ │
│ │   │ 实施修改 │   │          │   │ 验证     │       │ │
│ │   │ edit()   │   │          │   │ test()   │       │ │
│ │   └──────────┘   │          │   └──────────┘       │ │
│ │                  │          │   ┌──────────┐       │ │
│ │                  │   (+)    │   │ 实施修改 │       │ │
│ │                  │          │   │ edit()   │       │ │
│ │                  │          │   └──────────┘       │ │
│ └─────────────────────────────────────────────────────┘ │
│                                                          │
│ ┌──────────── Tool Diff Table ────────────────────────┐ │
│ │ Tool       │ Kind        │ A Index │ B Index │ Args │ │
│ │ read       │ Unchanged   │   0     │   0     │  =   │ │
│ │ grep       │ Unchanged   │   1     │   1     │  =   │ │
│ │ test       │ Added       │   -     │   2     │  +   │ │
│ │ edit       │ ArgsChanged │   2     │   4     │  ~   │ │
│ └─────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

### 10.3 JSON 文件格式（供 dashboard 消费）

```json
{
  "id": "tdiff_20260612_001",
  "capability_id": "cap_bug_fix_001",
  "trace_id_a": "trace_20260612_001",
  "trace_id_b": "trace_20260612_003",
  "diff": { /* TrajectoryDiff */ },
  "analysis": { /* TrajectoryAnalysis */ },
  "mermaid": "sequenceDiagram\n...",
  "computed_at": "2026-06-12T10:00:00Z"
}
```

---

## 11. 错误类型

```rust
// src/trajectory/error.rs

#[derive(Debug, thiserror::Error)]
pub enum TrajectoryError {
    #[error("Trace not found: {0}")]
    TraceNotFound(String),

    #[error("Capability not found: {0}")]
    CapabilityNotFound(String),

    #[error("No traces linked to capability: {0}")]
    NoTracesForCapability(String),

    #[error("Not enough traces for diff (need 2, got {0})")]
    InsufficientTraces(usize),

    #[error("Tool sequence too long for edit-distance alignment: {0} tools")]
    ToolSequenceTooLong(usize),

    #[error("Cache error: {0}")]
    CacheError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

---

## 12. 测试策略

### 12.1 测试层级

| 层级 | 位置 | 测试内容 | 预估数量 |
|------|------|----------|----------|
| **单元测试** | 各源文件 `#[cfg(test)] mod tests` | 每个函数独立行为验证 | 30+ |
| **集成测试** | `tests/integration_tests.rs` | 端到端: import trace → diff → analysis → eval | 8+ |
| **系统测试** | `tests/system_tests.rs` | CLI 命令黑盒测试 | 6+ |
| **Fixtures** | `tests/fixtures/` | 测试用 trace 数据 | 3+ |

### 12.2 单元测试清单

**`src/trajectory/classifier.rs`:**
- `test_rule_classifier_default_rules` — 默认规则注册
- `test_classify_empty_trace` — 空 trace 返回空 Vec
- `test_classify_single_tool` — 单个 tool 正确分类到 phase
- `test_classify_phase_transition` — tool 序列跨 phase 切分
- `test_classify_unknown_tool` — 未知 tool 归入 default_phase
- `test_classifier_name_version` — name/version 返回正确

**`src/trajectory/hash.rs`:**
- `test_semantic_hash_deterministic` — 相同输入产出相同 hash
- `test_semantic_hash_different_tool_names` — 不同 tool name → 不同 hash
- `test_semantic_hash_different_args` — 不同 args → 不同 hash
- `test_semantic_hash_same_tool_different_args` — 同 name 不同 args → 不同 hash

**`src/trajectory/align/scorer.rs`:**
- `test_default_costs` — 默认代价 1.0/1.0/1.0
- `test_tool_substitution_same_hash` — 同 hash → 0
- `test_tool_substitution_different_name` — 不同 name → 1.0
- `test_tool_substitution_same_name_diff_hash` — 同 name 不同 hash → 0.5

**`src/trajectory/align/segment.rs`:**
- `test_match_identical_segments` — 相同序列 → 全部 Unchanged
- `test_match_added_segment` — B 多一个段 → Added
- `test_match_deleted_segment` — A 多一个段 → Deleted
- `test_match_mixed` — 混合场景

**`src/trajectory/align/tool.rs`:**
- `test_align_identical_tools` — 相同序列全部 Unchanged
- `test_align_added_tool` — B 多一个 tool
- `test_align_deleted_tool` — A 少一个 tool
- `test_align_args_changed` — 同 name 不同 args → ArgsChanged
- `test_align_greedy_fallback` — >200 tools → 降级贪心

**`src/trajectory/align/engine.rs`:**
- `test_engine_full_diff` — 完整 diff 流程
- `test_engine_empty_traces` — 两个空 trace

**`src/trajectory/analysis.rs`:**
- `test_from_diff_empty` — 空 diff → 全 0 值
- `test_from_diff_phase_additions` — 阶段新增
- `test_from_diff_phase_deletions` — 阶段删除
- `test_from_diff_phase_modifications` — 阶段修改
- `test_tool_churn_score` — churn score 计算正确
- `test_phase_reorder_score` — reorder score 计算正确
- `test_capability_shift_score` — shift score 计算正确

**`src/evaler/rules.rs` (新增):**
- `test_segment_rules_registered` — 5 条段级规则注册
- `test_s001_phase_explosion_triggered` — >= 2 新增阶段命中
- `test_s001_phase_explosion_not_triggered` — < 2 新增阶段不命中
- `test_s002_critical_phase_deleted` — "验证" 被删除 → Critical hit
- `test_s002_normal_phase_deleted` — "其他" 被删除 → 不命中
- `test_s003_tool_churn_high` — churn > 0.5 → High
- `test_s005_capability_shift` — shift > 0.3 → Medium

### 12.3 集成测试清单

| 测试 | 描述 |
|------|------|
| `test_trajectory_diff_end_to_end` | 导入两条 trace → 计算 diff → 缓存 → 验证 |
| `test_trajectory_diff_by_capability` | 通过 capability_id 自动查找关联 trace |
| `test_trajectory_diff_manual_trace_ids` | 手动指定 trace_a + trace_b |
| `test_trajectory_to_analysis_pipeline` | diff → analysis → segment_rules → EvalResult |
| `test_trajectory_cache_hit` | 相同 trace 对二次请求命中缓存 |
| `test_trajectory_mermaid_output` | 验证 Mermaid 图生成 \\
| `test_eval_with_trajectory_mode` | eval run --mode trajectory 完整流程 |
| `test_dashboard_json_export` | 验证 JSON 文件格式符合 dashboard 期望 |

### 12.4 测试 Fixtures

```
tests/fixtures/
  trace_simple_a.json      ← 3 tool_calls: read → edit → commit
  trace_simple_b.json      ← 4 tool_calls: read → test → edit → commit
  trace_complex_a.json     ← 12 tool_calls，跨 4 个 phase
  trace_complex_b.json     ← 15 tool_calls，跨 5 个 phase
  trace_empty.json         ← 0 tool_calls
```
