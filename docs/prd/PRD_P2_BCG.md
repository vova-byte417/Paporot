# PRD: Paporot P2 — Behavior Coupling Graph (BCG)

> 版本: 1.0 | 日期: 2026-06-13
>
> 一句话定义: **构建多个 capability / trajectory 之间的行为耦合关系图（correlational，非 causal）**

---

## 1. 背景与目标

P1 产出了 `TrajectoryVector`（单条 trace → 数值向量），但无法回答：
- 哪些 capability 在行为上高度相关？
- 修改 cap_auth 时哪些 capability 会被连带影响？
- 行为耦合是 structural（共变）还是 temporal（同步）？

P2 目标：多 trajectory vector → capability 耦合图（`CouplingGraph`）。

核心思想：**P2 graph is not a similarity graph; it is a survivorship graph.** 边由 co-change 证据定义存在性，由 behavior similarity 调制强度。

```
co-change = edge existence ("有没有关系")
similarity = edge strength modifier ("关系有多像")
```

---

## 2. 输入

来自 P1：
- `TrajectoryVector`（per trace）
- cluster label（per trace）
- time series summary（per trace）

来自工程系统：
- git commit log（co-occurrence 证据）
- session logs（co-occurrence 证据）
- file modification history

---

## 3. 核心产物：CouplingGraph

```rust
pub type CapabilityId = String;

pub struct FeatureContribution {
    pub entropy: f32,       // phase_entropy + tool_entropy + transition_entropy
    pub structural: f32,    // loop_ratio
    pub temporal: f32,      // backtrack_ratio
    pub density: f32,       // burst_ratio
}

pub struct CouplingEdge {
    pub from_capability: CapabilityId,
    pub to_capability: CapabilityId,
    pub cochange_score: f32,         // primary: 3-layer log-saturated evidence
    pub similarity_score: f32,       // secondary: cosine(P1_vec_A, P1_vec_B)
    pub correlation_score: f32,      // derived: cochange × (1 + λ × similarity)
    pub feature_contribution: FeatureContribution,  // 4 semantic groups
}

pub struct CouplingGraph {
    pub capabilities: Vec<CapabilityId>,
    pub edges: Vec<CouplingEdge>,
    pub version: u64,
}
```

### 3.1 融合公式（D9）

```
correlation_score = cochange_score × (1 + λ × similarity_score)
λ ∈ [0.2, 0.4], clamp max = 0.5
```

co-change **定义边存在性**（ground truth anchor），similarity **调制边强度**（解释层）。禁止 symmetry 加权：两种信号尺度不同（cosine∈[0,1], cochange∈[0,∞)），linear sum 会爆炸。

### 3.2 cochange_score 定义（D11）

```
cochange_score = log(1 + w1×commit + w2×file + w3×session)
w = [1.0, 1.5, 0.5]
```

三层证据语义：

| 层级 | 语义 | 噪声 | 权重 | 特殊处理 |
|------|------|------|------|----------|
| commit | 原子工程决策 | 高（batch commit） | 1.0 | 降权: `1/log(1+other_caps_in_commit)` |
| file | 结构耦合 | 中 | 1.5 | Jaccard(caps_in_file_A, caps_in_file_B) |
| session | 行为共现 | 低但弱约束 | 0.5 | `cooccurrence / sqrt(total_events)` |

log 饱和：防止高频 co-change 导致 hub 节点度失控。

### 3.3 feature_contribution 定义（D12）

4 个语义 group，底层 per-field linear projection（`wi × Ai × Bi`）聚合：

| Group | P1 features | 语义 |
|-------|------------|------|
| `entropy` | tool_entropy + phase_entropy + transition_entropy | 行为不确定性 |
| `structural` | loop_ratio | 结构循环 |
| `temporal` | backtrack_ratio | 时间回退 |
| `density` | burst_ratio | 密度聚集 |

存储为 group，可展开回 per-field（计算在 field，存储在 group）。原因：schema 稳定性 + 消除 feature redundancy + P2 可解释性。

---

## 4. Pruning Pipeline（D10）

4 层 survivorship filter：

```
raw edges
   ↓
(1) Hard Existence Filter
    if cochange < ε in ALL three layers: DROP
   ↓
(2) Signal Purity Filter
    purity = max(commit, file, session) / (sum + 1e-6)
    if purity < τ_purity: DROP
   ↓
(3) Structural Consistency Filter
    stability = count(edge in different traces) / total_traces
    if stability < τ_stability: DROP
   ↓
(4) Top-K Sparsification
    per node: keep top-K edges by correlation_score
    K = 10-20 (small) / 20-50 (medium) / 50-100 (large)
   ↓
P2 CouplingGraph
```

单 threshold 必然导致 hub explosion（common tools like edit/test 连接所有 capability → O(n²) degree）。

---

## 5. 模块清单

### 5.1 `trajectory/p2/similarity.rs`

向量相似性计算：
- cosine similarity（主方法，在 P1 normalized space 上）
- Jaccard on distributions（稀疏分布专用）
- weighted feature distance（per-group weighting）

### 5.2 `trajectory/p2/cochange.rs`

Co-change 证据提取：
- git commit co-occurrence（commit 内文件 → capability 映射）
- same-session modification coupling
- temporal proximity correlation
- 三层融合（log-saturated）

### 5.3 `trajectory/p2/coupling_builder.rs`

耦合图构建：
- 从 capability → trace vector 映射建 edges
- aggregate multiple traces per capability
- normalize scores
- apply fusion: `cochange × (1 + λ × similarity)`

### 5.4 `trajectory/p2/graph.rs`

图操作：
- merge edges（去重 + 聚合）
- threshold pruning（4 层 survivorship filter）
- stability filtering
- top-K sparsification

### 5.5 `trajectory/p2/correlation.rs`

相关性分析：
- feature correlation matrix（跨 capability）
- cross-trajectory similarity（多 trace 聚合）
- coupling strength scoring

---

## 6. Attribution 计算（D13）

主路径：**weighted linear projection**（确定性、可缓存）。

```
P1 vectors
   ↓
cosine(VA, VB)
   ↓
contribution_i = wi × VA_i × VB_i
   ↓
normalize: contrib_i / Σ contrib
   ↓
aggregate into 4 feature_groups
   ↓
FeatureContribution (stored on edge)
```

禁止 cosine decomposition 做主方法（normalization 后语义失真）。SHAP 仅用于离线诊断（`O(n_features × n_edges)` 不可扩展）。

---

## 7. CLI（P2 新增）

```
paporot coupling build                        # 构建耦合图
paporot coupling analyze --cap cap_auth       # 分析某 capability 的耦合
paporot coupling graph export                 # 导出图结构 (Mermaid / JSON)
paporot coupling impact cap_x                 # 影响分析
```

---

## 8. P2 输出

| 输出 | 格式 | 用途 |
|------|------|------|
| `CouplingGraph.json` | JSON | 图数据结构 |
| Mermaid 图 | markdown | 可视化 |
| impact report | text/md | 影响分析 |

### 8.1 示例 Graph 结构

```
cap_auth → cap_profile (0.72)  [entropy:0.6, structural:0.3, temporal:0.1, density:0.0]
cap_auth → cap_api     (0.41)  [entropy:0.2, structural:0.5, temporal:0.2, density:0.1]
cap_test → cap_auth    (0.63)  [entropy:0.4, structural:0.3, temporal:0.2, density:0.1]
```

---

## 9. P2 核心能力

- capability 之间"行为相关性"
- change propagation analysis（修改 cap_x → 哪些 cap 可能受影响）
- weak dependency discovery（latent coupling）
- trend: coupling strength over time

---

## 10. Decision Log

| # | 决策 | 核心理由 |
|---|------|----------|
| D9 | `correlation = cochange × (1 + λ × similarity)`，λ∈[0.2,0.4] | co-change 定义存在性，similarity 调制强度 |
| D10 | 4 层 survivorship filter: hard → purity → stability → top-K | 单 threshold 导致 hub explosion |
| D11 | `cochange = log(1 + w1×commit + w2×file + w3×session)`，commit 降权 batch | log 饱和防 hub，三层对应三种因果置信度 |
| D12 | `feature_contribution` = 4 语义 group，底层 per-field linear projection | 稳定性 + 消除 redundancy + 可解释 |
| D13 | 主 attribution = weighted linear projection（`wi×Ai×Bi`），SHAP 仅离线 | 确定性、可缓存、与 P1 空间一致 |

---

## 11. 关键工程原则

1. **P2 graph is not a similarity graph; it is a survivorship graph.**
2. **Co-change is not frequency. It is multi-resolution evidence aggregation with saturation.**
3. **Feature contribution in P2 is a semantic attribution layer over P1 feature space, not a raw vector decomposition.**
4. **P2 feature attribution is a deterministic projection of P1 vector interactions, not a perturbation-based estimation problem.**

---

## 12. 与 P0 / P1 的系统关系

```
P0: State Graph (structure)
        ↓  提取行为特征
P1: Trajectory Vector (geometry)
        ↓  向量 → 聚类 + co-change 证据
P2: Coupling Graph (network)
        ↓  耦合图 → impact / trend / anomaly
P3: (future) Causal inference
```

### P1 vs P2 本质区别

| 层级 | 本质 | 对象 |
|------|------|------|
| P1 | single trajectory → vector space | 1 trace = 1 point |
| P2 | multiple trajectories → interaction graph | N traces = N×N edges |
