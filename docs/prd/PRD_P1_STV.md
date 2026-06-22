# PRD: Paporot P1 — Statistical Trajectory Vector (STV)

> 版本: 1.0 | 日期: 2026-06-13
>
> 一句话定义: **把"行为轨迹"压成可计算的数值向量（非 ML embedding），用于 clustering / anomaly / trend**

---

## 1. 背景与目标

P0 产出了 `BehaviorStateGraph`（结构层），但结构无法直接用于：
- 跨 trace 聚类
- 异常检测
- 趋势分析
- P2 coupling graph 的向量输入

P1 目标：将单条 behavior trace → 数值向量（`TrajectoryVector`），建立"行为测量空间"。

核心设计原则：**Feature space is shared across layers, but decision functions are strictly layer-local.**

```
             Shared Feature Space (StateFeatures)
                     ↓
         ┌───────────┴───────────┐
         ↓                       ↓
    P0 Decision              P1 Projection
  (merge_similarity)        (vectorization)
   → thresholded            → continuous
   → categorical            → no threshold
         ↓                       ↓
    State Graph           Trajectory Vector
```

P0 用 features 做**判决**（merge/split，weighted + thresholded）；P1 用同样的 features 做**测量**（projection + normalization，无 threshold）。决不复用 P0 的 `merge_similarity` 等判决函数。

---

## 2. 输入依赖（来自 P0）

| 输入 | 来源 | 用途 |
|------|------|------|
| `BehaviorStateGraph` | P0 `build_state_graph()` | states + edges + event_log |
| `TransitionEventLog` | `graph.event_log` | 序列分析（entropy, loop, backtrack） |
| `ToolLog` | 可选回溯（via `BehaviorTrace.tool_calls`） | tool_entropy, burst 检测 |

---

## 3. 核心产物：TrajectoryVector

```rust
pub struct SparseVector {
    pub indices: Vec<u32>,
    pub values: Vec<f32>,
    pub registry_version: u64,
}

pub struct TrajectoryVector {
    // ── Distribution features (global registry indexed) ──
    pub tool_distribution: SparseVector,
    pub state_distribution: SparseVector,

    // ── Entropy triad (3-resolution decomposition) ──
    pub tool_entropy: f32,         // H(tool sequence), raw event disorder
    pub phase_entropy: f32,        // H(state bigram sequence), path disorder
    pub transition_entropy: f32,   // H(aggregated edge graph), structural disorder

    // ── Structural metrics (orthogonal 3-axis) ──
    pub loop_ratio: f32,           // state-level structural cycles
    pub backtrack_ratio: f32,      // k-step temporal regression
    pub burst_ratio: f32,          // tool-level temporal density spike

    // ── Continuity ──
    pub state_stability_score: f32, // mean cosine similarity(adjacent states), P1-recomputed

    // ── Temporal curve ──
    pub edit_intensity_curve: Vec<f32>,  // raw curve over time windows
}
```

### 3.1 Feature 定义

| 字段 | 粒度 | 定义 | 范围 |
|------|------|------|------|
| `tool_distribution` | global tool registry | normalized frequency per tool category | sparse |
| `state_distribution` | global phase registry | proportion per state phase | sparse |
| `tool_entropy` | tool sequence | H(tool category sequence) | [0, log N] |
| `phase_entropy` | state bigram seq | H(S1→S2→S3...) | [0, log E] |
| `transition_entropy` | aggregated edges | H(edge distribution) | [0, log E] |
| `loop_ratio` | state-level cycles | #cycle_transitions / total_transitions | [0, 1] |
| `backtrack_ratio` | k-step temporal | count(Si→Sj where j<i-k) / total | [0, 1] |
| `burst_ratio` | tool density | max_run_length(category) / total_length | [0, 1] |
| `state_stability_score` | adjacent states | mean(cosine(State_i, State_{i+1})) | [0, 1] |
| `edit_intensity_curve` | time windows | per-window mean edit_density | [0, 1] |

### 3.2 Entropy 三层分解（D4）

`tool_entropy` → `phase_entropy` → `transition_entropy` 是同一 trace 在不同概率空间上的层次投影，不是重复测量。

| entropy | probability space | order-sensitive | 用途 |
|---------|------------------|-----------------|------|
| tool_entropy | tool events | yes | raw disorder, sanity check |
| phase_entropy | state sequence | yes | execution path uncertainty |
| transition_entropy | edge graph | no | structural topology uncertainty |

三者不能合并——单 entropy 无法区分 loop bug（三者都高）vs exploratory（phase 高、transition 低）vs state fragmentation（transition 极高）。

### 3.3 结构三轴正交（D6）

`loop_ratio` / `backtrack_ratio` / `burst_ratio` 是三维正交行为轴：

- `loop_ratio`: 结构循环（state-level graph-based）
- `backtrack_ratio`: 时间回退（temporal, order-based）
- `burst_ratio`: 密度聚集（tool-level, sequence-based）

`oscillation`（A↔B 两状态来回）作为 loop 的子类型吸收进 `loop_ratio`，不独立成字段（避免 collinearity）。

---

## 4. 模块清单

### 4.1 `trajectory/p1/feature_extractor.rs`

从 `BehaviorStateGraph` 提取统计特征快照：
- tool histogram（按 P0 tool_category 聚合）
- state histogram（按 primary_phase 聚合）
- transition counts
- 归一化 entropy 指标

### 4.2 `trajectory/p1/sequence_metrics.rs`

序列级行为度量：
- loop detection（DFS on state transition graph，min_len=2, max_len=5）
- oscillation detection → 吸收入 loop
- backtracking ratio（k-step temporal regression）
- burst detection（连续同 tool category ≥ 3 视为 burst）

### 4.3 `trajectory/p1/timeseries.rs`

时序聚合：
- tool usage over time window
- state transitions over time
- edit intensity curve（per-window mean edit_density）
- flatten / stats 方法供 vector 层使用

### 4.4 `trajectory/p1/vector.rs`

`TrajectoryVector` 组装 + 标准化：
1. bounded normalization（entropy: `/log(N)`, ratio: [0,1] 不变）
2. log compression（burst heavy-tail: `log(1+burst)/log(max)`）
3. robust scaling（median/IQR）
4. 组装最终 vector

### 4.5 `trajectory/p1/cluster.rs`

聚类分析：
- DBSCAN-like density clustering（deterministic，无随机种子）
- similarity grouping（基于 cosine distance on P1 vector）
- cluster label assignment

### 4.6 `trajectory/p1/registry.rs`（新增）

全局 Feature Registry：
- 版本化（append-only）
- tool category registry
- phase label registry
- 逻辑重投影（view mapping）：1→1 / 1→many / many→1 / unknown

```rust
pub struct FeatureRegistry {
    pub version: u64,
    pub tool_mapping: HashMap<String, u32>,
    pub phase_mapping: HashMap<String, u32>,
    pub parent_version: Option<u64>,
}
```

**核心约束**：
- Registry is a versioned coordinate system, not a mutable taxonomy
- Vectors are immutable snapshots tied to registry versions
- All cross-version comparisons must be performed in a unified projected space

---

## 5. Normalization Pipeline（D7）

```
raw trace
   ↓
feature extraction
   ↓
bounded normalization (entropy: H/log(N), ratios: [0,1])
   ↓
log compression (burst heavy-tail)
   ↓
robust scaling (median/IQR)
   ↓
P1 TrajectoryVector
```

**禁止 whitening**：破坏语义轴可解释性；P2 coupling graph 依赖 feature correlation 本身作为信号。

---

## 6. CLI（P1 新增）

```
paporot trajectory vector build --trace <id>
paporot trajectory vector diff <v1> <v2>
paporot trajectory cluster analyze
paporot trajectory anomaly detect
```

---

## 7. P1 输出

| 输出 | 格式 | 用途 |
|------|------|------|
| `TrajectoryVector.json` | JSON | P2 输入 |
| similarity matrix | CSV/JSON | cluster 分析 |
| anomaly score | f32 | 异常检测 |
| trend score | f32 | 趋势分析 |

---

## 8. P1 核心价值

- 替代"纯 diff 思维" → "行为数值空间"
- 为 P2 correlation 提供 stable vector space
- 支持 cross-version comparison（via registry reprojection）
- 可解释：每个 feature 有明确的行为语义

---

## 9. Decision Log

| # | 决策 | 核心理由 |
|---|------|----------|
| D1 | `phase_entropy` = transition sequence Shannon entropy | P1 独立测量，避免 P0 bootstrap bias |
| D2 | `state_stability_score` = 相邻 state cosine similarity 均值（P1 重算） | P0 stability_score 含 merge heuristic artifact |
| D3 | `edit_intensity_curve` = 完整 Vec，P2 用一阶导数统计 | 保留波形形态，避免绝对值偏差 |
| D4 | 三层 entropy: tool → phase → transition | 三个不同概率空间，合并导致信号坍塌 |
| D5 | 全局版本化 append-only registry + SparseVector | dense resize O(n²) 爆炸；registry 是坐标系非分类系统 |
| D6 | `loop_ratio`(state) / `backtrack_ratio`(temporal) / `burst_ratio`(tool density)，oscillation 吸收进 loop | 三轴正交，避免 collinearity |
| D7 | bounded norm + log compression + robust scaling，禁止 whitening | 保留语义轴可解释性 |
| D8 | Registry 逻辑重投影（4 种映射），历史 vector 不变 | 物理重写 O(N×dataset) 不可行 |

---

## 10. 关键工程原则

1. **Feature space is shared across layers, but decision functions are strictly layer-local.**
2. **Entropy in P1 is a multi-resolution decomposition of a single behavior trace across different probability spaces, not duplicated measurement.**
3. **P1 vector must preserve semantic axes while controlling scale, not decorrelate them.**
4. **Registry is a versioned coordinate system, not a mutable taxonomy.**

---

## 11. 与 P0 / P2 的关系

```
P0: State Graph (structure)
        ↓  StateFeatures + TransitionEventLog
P1: Trajectory Vector (geometry)
        ↓  TrajectoryVector + cluster labels
P2: Coupling Graph (network)
```
