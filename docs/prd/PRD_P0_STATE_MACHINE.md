# PRD: Paporot P0 — Behavior State Machine

> 版本: 2.0-p0 | 日期: 2026-06-12
>
> 一句话定义: **把 Agent execution trace 从"事件序列"升级为"状态机系统"**

---

## 1. 背景问题

当前 Paporot v1 的三项结构性缺陷:

1. **TrajectoryDiff 过度中心化** — Diff 是 pairwise artifact，不是 state representation，无法泛化到 multi-version evolution
2. **Phase 是 rule-based label** — `if tool == test → verify`，这是 brittle heuristic，不是行为建模
3. **Eval 依赖 diff → rule → threshold** — 无 path dependency，无 state abstraction

核心问题: 行为不可建模（只有片段，没有状态），无法表达"路径依赖"，无法扩展 embedding/graph/anomaly detection。

---

## 2. P0 目标

建立纯结构层(non-ML)行为状态模型: 将 `BehaviorTrace` 转换为 `BehaviorStateGraph`。

---

## 3. 核心设计法则

1. *State identity is defined over shared behavioral feature space, while operations (merge, alignment) are context-dependent projections over that space.*
2. *TrajectoryDiff is a backward-compatible projection layer and must not introduce independent semantic meaning.*
3. *Only Tool Log is source of truth. All other representations are derived and non-authoritative.*
4. *Diff is no longer evaluative, only diagnostic.*
5. *State alignment is defined over event-log projections, not direct state label matching or embedding similarity.*

---

## 4. 数据流与依赖方向

```
Layer 0: BehaviorTrace (Tool Log — 唯一真源，immutable)
           │
           ▼
Layer 1: Hard Segmentation (irreversible)
  │  Trigger: tool type change / failure→fix / idle gap > threshold / file scope jump
  │  Output: Vec<RawSegment>
           ▼
Layer 2: Window-based StateCandidate construction
  │  ┌──────────────────────────────┐
  │  │ PhaseLabel annotation (independent) │  → phase_dist: HashMap<PhaseLabel, f32>
  │  └──────────────────────────────┘
  │  Output: Vec<StateCandidate>
           ▼
Layer 3: Adjacent-only merge
  │  merge_similarity (loose, threshold 0.85)
  │  Output: Vec<BehaviorState>
           ▼
Layer 4: Transition construction
  │  TransitionEvent log (Layer A) + TransitionEdge graph (Layer B)
           ▼
    BehaviorStateGraph  (核心输出)
           │
     ┌─────┴──────────────┐
     ▼                    ▼
  State-centric Eval   TrajectoryDiff (projection)
```

### 硬约束

- 每层只依赖上一层，不可跳跃
- TrajectoryDiff 不可反向流入 StateGraph
- Layer 1 边界不可逆，Layer 3 仅允许相邻合并
- PhaseLabel 是 derived attribute，不参与 Layer 1 segmentation

---

## 5. 接口级数据类型

### 5.1 核心输出类型

```rust
/// State 是 stabilized behavioral cluster。
/// Phase 是 probability distribution，不是分类标签。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BehaviorState {
    pub id: String,
    /// Multi-phase distribution
    pub phase_dist: HashMap<String, f32>,
    /// Dominant phase (仅用于 UI 显示)
    pub primary_phase: String,
    /// 统一特征向量
    pub features: StateFeatures,
    /// 状态稳定性评分 (0.0–1.0)
    pub stability_score: f32,
}

/// 统一行为特征空间。Merge 和 Alignment 共享此结构，
/// 但使用不同的 decision function。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct StateFeatures {
    /// 工具类型直方图: tool_type → normalized frequency
    pub tool_histogram: HashMap<String, f32>,
    /// 文件范围向量: file_cluster → normalized frequency
    pub file_clusters: HashMap<String, f32>,
    /// 编辑密度: edit/write tools / total tools
    pub edit_density: f32,
    /// 读写比例: read tools / write tools
    pub read_write_ratio: f32,
    /// 循环强度: 重复 state 的频次密度
    pub loop_intensity: f32,
    /// 失败率: failure/retry tools / total tools
    pub failure_rate: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BehaviorStateGraph {
    pub trace_id: String,
    pub session_id: String,
    /// 状态节点列表
    pub states: Vec<BehaviorState>,
    /// Transition 事件序列 (Layer A — 不可变)  
    pub event_log: Vec<TransitionEvent>,
    /// Transition 聚合图 (Layer B — 分析视图)
    pub edges: Vec<TransitionEdge>,
}
```

### 5.2 Transition 双层模型

```rust
/// Layer A: 不可变事件序列，用于 replay / debug / embedding。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TransitionEvent {
    pub from: String,       // StateId
    pub to: String,         // StateId
    pub trigger_tool: String,
    pub timestamp: u64,
}

/// Layer B: 聚合图边，用于 visualization / evaluation。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TransitionEdge {
    pub from: String,
    pub to: String,
    pub count: u32,
    pub avg_cost: f32,
}
```

### 5.3 State Diff 类型

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StateDiff {
    pub graph_a: String,    // trace_id
    pub graph_b: String,
    /// 对齐后的 state 配对
    pub state_pairs: Vec<StatePair>,
    /// 全局差异指标
    pub metrics: StateDiffMetrics,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StatePair {
    pub state_a: Option<String>,
    pub state_b: Option<String>,
    pub kind: StateDiffKind,
    pub similarity: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum StateDiffKind { Matched, Added, Deleted, Split, Merged }

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct StateDiffMetrics {
    pub state_churn: f32,
    pub transition_churn: f32,
    pub path_divergence: f32,
}
```

### 5.4 内部 Pipeline 类型

```rust
/// Layer 1 输出
#[derive(Debug, Clone)]
struct RawSegment {
    tool_indices: Vec<usize>,
    boundary_reason: BoundaryReason,
}

#[derive(Debug, Clone)]
enum BoundaryReason {
    ToolTypeChange,
    FailureLoop,
    IdleGap { ms: u64 },
    FileScopeJump,
}

/// Layer 2 输出
#[derive(Debug, Clone)]
struct StateCandidate {
    segment_idx: usize,
    window_start: usize,
    features: StateFeatures,
    phase_dist: HashMap<String, f32>,
}
```

---

## 6. State Similarity Metric

### 6.1 统一特征空间 `StateFeatures`

所有相似度计算基于 `StateFeatures` 的六个分量。

### 6.2 分量计算

| 分量 | 算法 | 公式 |
|------|------|------|
| `tool_overlap` | Jaccard | \|A∩B\| / \|A∪B\| |
| `file_scope_overlap` | Cosine similarity on file cluster vectors | COS(A,B) |
| `edit_density_similarity` | Absolute difference | 1 - \|a.density - b.density\| |
| `control_flow_similarity` | n-gram similarity (n=2,3) on tool sequence | ngram_match/total |
| `failure_pattern_similarity` | Normalized count ratio | 1 - \|a.fail - b.fail\|/max(a,b) |

### 6.3 最终公式

```
S(A,B) = w1 * tool_overlap + w2 * file_scope_overlap + w3 * edit_density_sim
       + w4 * control_flow_sim + w5 * failure_pattern_sim
```

默认权重: `w = [0.3, 0.15, 0.15, 0.25, 0.15]`

### 6.4 Merge vs Alignment — 不同决策函数

| 任务 | 目标 | 性质 | Threshold |
|------|------|------|-----------|
| **Merge** | 合并相同行为模式 | many-to-one (宽松) | 0.85 |
| **Alignment** | 精确找对应关系 | one-to-one (严格) | 0.65 |

两者共享 `StateFeatures` 特征空间，使用不同 decision threshold。

---

## 7. 关键 Trait 定义

```rust
/// Layer 1: 硬边界切割
pub trait Segmenter: Send + Sync {
    /// 将 trace 切分为不可穿透的 RawSegment 序列
    fn cut(&self, trace: &BehaviorTrace) -> Vec<RawSegment>;
}

/// Layer 2: 窗口化候选状态生成
pub trait WindowBuilder: Send + Sync {
    /// 在每个 segment 内生成 StateCandidate
    fn build_candidates(
        &self,
        segment: &RawSegment,
        trace: &BehaviorTrace,
    ) -> Vec<StateCandidate>;
}

/// Layer 3: 相邻合并
pub trait StateMerger: Send + Sync {
    /// 仅合并相邻 candidate
    fn merge(&self, candidates: &[StateCandidate]) -> Vec<BehaviorState>;
}

/// 编排器
pub trait StateGraphBuilder: Send + Sync {
    fn build(&self, trace: &BehaviorTrace) -> BehaviorStateGraph;
}
```

---

## 8. State-centric Eval

Eval 从 v1 的 `TrajectoryDiff → Rules` 迁移为三层评估:

### Layer 1: State-level

| 指标 | 含义 | 计算 |
|------|------|------|
| `phase_entropy` | Phase distribution 的信息熵 | −Σ p·log(p) |
| `loop_ratio` | 自环 transition 占比 | self_loops / total_events |
| `tool_diversity` | 使用的不同工具类型数 / total tools | unique_tools / total_tools |

### Layer 2: Transition-level

| 指标 | 含义 | 计算 |
|------|------|------|
| `oscillation_count` | A→B→A 来回次数 | count(A→B→A) |
| `transition_entropy` | 转移分布熵 | −Σ p(t)·log(p(t)) |
| `reversal_ratio` | 反向转移占比 | reverses / total_edges |

### Layer 3: Graph-level

| 指标 | 含义 | 计算 |
|------|------|------|
| `path_length_drift` | 状态路径长度变化 | |len(B) − len(A)| / max |
| `structural_entropy` | 图结构信息熵 | Σ −p(edge)·log(p(edge)) |
| `stability_trend` | 多版本稳定性趋势 | sliding window of path length |

### Eval Pipeline

```
Trace → StateGraph → [StateMetrics, TransitionMetrics, GraphMetrics] → EvalResult
```

### 退化规则迁移

v1 段级规则 (S001–S005) 迁移为 state-centric:

| v1 规则 | v2 状态级对应 |
|---------|--------------|
| S001 阶段爆炸 | oscillation_count > threshold OR state_count > 2x baseline |
| S002 关键阶段删除 | primary_phase "提交" state missing in version B |
| S003 tool churn | tool_diversity drop > 0.5 |
| S004 阶段重排序 | alignment cost > threshold |
| S005 capability shift | file_clusters cosine similarity < 0.5 |

---

## 9. CLI 接口

### 新命令

```bash
# 构建 BehaviorStateGraph
paporot state build --trace <trace_id>
# 输出: states: N, transitions: M

# 可视化
paporot state show <trace_id> --format mermaid
# 输出: state graph diagram

# State diff (canonical)
paporot state diff --trace-a <id> --trace-b <id>
# 输出: StatePair 列表 + StateDiffMetrics

# State-centric eval
paporot eval run --mode state --trace-a <id> --trace-b <id>
```

### 兼容命令 (内部改造)

```bash
# CLI 签名不变，内部改为 StateGraph → projection → Diff
paporot trajectory diff --trace-a <id> --trace-b <id>

# 保留原 mode，内部改为 StateGraph → Eval
paporot eval run --mode trajectory
```

---

## 10. 模块结构

```
src/trajectory/
├── mod.rs
├── types.rs                  # BehaviorState, StateFeatures, StateGraph, StateDiff 等
│
├── state/                    # P0 核心: 三层 builder
│   ├── mod.rs
│   ├── features.rs           # StateFeatures 提取 + 各分量计算
│   ├── segmentation.rs       # Layer 1: hard rule-based boundary detection
│   ├── window.rs             # Layer 2: sliding window candidate generator
│   ├── merge.rs              # Layer 3: adjacent-only merge
│   ├── transition.rs         # TransitionEvent log + TransitionEdge aggregation
│   └── builder.rs            # Trace → BehaviorStateGraph 编排
│
├── similarity/               # 共享 StateFeatures 的两个 decision function
│   ├── mod.rs
│   ├── merge_sim.rs          # 宽松加权 (threshold 0.85)
│   └── align_sim.rs          # 严格加权 (threshold 0.65)
│
├── evaler/                   # v2 state-centric eval
│   ├── mod.rs
│   ├── state_eval.rs         # entropy, loop_ratio, tool_diversity
│   ├── transition_eval.rs    # oscillation, reversal_ratio, entropy
│   └── graph_eval.rs         # path_length_drift, stability_trend
│
├── projection/               # TrajectoryDiff 兼容层
│   ├── mod.rs
│   └── state_to_diff.rs      # BehaviorStateGraph → TrajectoryDiff (单向)
│
├── align/                    # 保留，降级为事件级对齐引擎
│   ├── engine.rs             # 输入: TransitionEventLog → StateDiff
│   └── tool.rs               # Levenshtein (保留复用)
│
├── classifier.rs             # 保留: PhaseLabel 来源 (Layer 2 标注)
├── hash.rs                   # 保留
├── report.rs                 # 输出升级: 支持 StateGraph 格式
└── cache.rs                  # 保留: 新增 state/ 目录读写
```

---

## 11. 存储

```
.Paporot/state/
  {trace_id}.json           # BehaviorStateGraph 序列化
  {trace_id}.graph.json     # TransitionEdge 聚合图 (for dashboard)
```

---

## 12. 验收指标

| 指标 | 目标 |
|------|------|
| Trace → StateGraph 转换 | 空/单/多 tool 全覆盖 |
| State merge rate | < 30%(不过度压缩) |
| Graph size | ≤ trace length |
| CLI latency (1k tool calls) | < 300ms |
| 无 ML/embedding 依赖 | ✓ |
| TrajectoryDiff 不再是核心依赖 | ✓ |
| Eval 可完全基于 StateGraph | ✓ |
| 所有 v1 测试继续通过 | 269 个 |

---

## Decision Log

| ID | 决策 | 替代方案 | 理由 |
|----|------|---------|------|
| D1 | 三层 segmentation: Rule cut → Window → Adjacent merge | 纯 rule / 纯 window | Rule 定义时间拓扑骨架, Window 构造稳定 state, Merge 仅相邻 |
| D2 | PhaseLabel 不参与 segmentation, 是 Layer 2 derived attribute | PhaseLabel 驱动切割 | 标签不稳定, 不可区分同名 state, 切割应基于结构信号 |
| D3 | Transition 双层: EventLog (A) + AggregatedGraph (B) | 仅加权边 / 仅事件序列 | A 保 replay/debug/embedding, B 保 scalability |
| D4 | TrajectoryDiff 保留为兼容层, StateGraph → Diff 单向 | 删除 / 并行双轨 | 保留 v1 投资, v2 语义纯度, 渐进弃用路径 |
| D5 | State diff 基于 TransitionEvent log 序列对齐 | label matching / similarity | label 不稳定, 事件序列对齐可解释、可复用、确定性 |
| D6 | State 允许 multi-phase: phase_dist 是 distribution | 单标签 state | window+merge 天然产生 overlap, 禁止会 graph explosion |
| D7 | Merge 和 Alignment 共享 StateFeatures, 不同 decision function | 共享 similarity 函数 | merge 需宽松(recall), alignment 需严格(precision), 目标冲突 |
| D8 | Eval 三层: state / transition / graph metrics | 单层 eval | 不同维度需要不同指标, 分层可组合 |
