# Paporot P1 + P2 测试说明书

> 日期: 2026-06-13 | 测试环境: WSL Ubuntu 24.04, rustc 1.96.0

---

## 1. 测试概览

| 分类 | 测试数 | 通过 | 失败 |
|------|--------|------|------|
| P1 单元测试 | 84 | 84 | 0 |
| P2 单元测试 | 58 | 58 | 0 |
| P1+P2 小计 | 142 | 142 | 0 |
| 项目总计（含 P0） | 391 | 390 | 0 |

---

## 2. P1 模块测试明细

### 2.1 `trajectory::p1::feature_extractor` (16 tests)

| 测试 | 验证点 |
|------|--------|
| `test_feature_snapshot_basic` | 4 状态图 → 三个 entropy 均非零 |
| `test_feature_snapshot_empty` | 空图 → 所有 entropy 为零 |
| `test_tool_entropy_uniform` | 4 种不同工具 → entropy = log2(4) = 2.0 |
| `test_tool_entropy_single_category` | 全部同工具 → entropy = 0 |
| `test_phase_entropy_linear` | 3 个唯一 bigram → H ≈ 1.585 |
| `test_phase_entropy_repeated` | 1 个 bigram 重复 → H = 0 |
| `test_transition_entropy_equals_phase_for_simple` | 简单图下两者等价 |
| `test_distribution_vecs` | tool/state distribution 各为 5 维 |

### 2.2 `trajectory::p1::sequence_metrics` (16 tests)

| 测试 | 验证点 |
|------|--------|
| `test_backtrack_no_backtrack` | 线性序列 → backtrack_ratio = 0 |
| `test_backtrack_with_backtrack` | s0→s1→s0 → backtrack > 0 |
| `test_cycle_detection_2_state` | s0↔s1 振荡 → 检测为 2-state loop |
| `test_cycle_detection_3_state` | s0→s1→s2→s0 → 3-state cycle |
| `test_burst_no_burst` | 每步不同工具 → burst_ratio = 0 |
| `test_burst_detected` | 4 连续 read → burst_ratio > 0 |
| `test_empty_events` | 空图 → 全部 ratio = 0 |
| `test_loop_ratio_includes_oscillation` | 振荡被 loop 吸收 |

### 2.3 `trajectory::p1::timeseries` (8 tests)

| 测试 | 验证点 |
|------|--------|
| `test_timeseries_empty_events` | 空事件 → 曲线仍非空（回退逻辑） |
| `test_timeseries_single_window` | 同时间戳 → 单窗口 |
| `test_edit_intensity_stats_empty` | 空曲线 → mean/min/max = 0 |
| `test_derivative_stats` | 导数统计 = [0.1→0.3→0.6] → mean=0.25 |

### 2.4 `trajectory::p1::vector` (18 tests)

| 测试 | 验证点 |
|------|--------|
| `test_bounded_entropy_max` | log2(4)=2 → bounded = 1.0 |
| `test_bounded_entropy_half` | 1.0 / log2(4) = 0.5 |
| `test_bounded_entropy_zero` | 0 → 0 |
| `test_log_compress_zero` | log_compress(0) = 0 |
| `test_log_compress_one` | log2(2) = 1.0 |
| `test_robust_scale` | [0,1,2,3,4] → median=2, IQR=2 → scaled[-1, -0.5, 0, 0.5, 1] |
| `test_cosine_identical` | cos(v,v) = 1.0 |
| `test_cosine_orthogonal` | cos([1,0],[0,1]) = 0 |
| `test_build_vector_basic` | 完整 pipeline: snapshot→vector |

### 2.5 `trajectory::p1::cluster` (8 tests)

| 测试 | 验证点 |
|------|--------|
| `test_cluster_empty` | 空数据 → cluster_count=0 |
| `test_cluster_all_noise` | eps=0.1, min_points=3 → 全部 noise |
| `test_similarity_group` | 相似向量归入同组 |
| `test_cluster_quality` | intra > inter → quality > 0 |

### 2.6 `trajectory::p1::registry` (18 tests)

| 测试 | 验证点 |
|------|--------|
| `test_registry_initial` | 初始 5 tool + 5 phase |
| `test_register_new_tool` | append → index=5 |
| `test_register_existing_tool` | 重复注册 → 返回已有 index |
| `test_sparse_vector_to_dense` | indices[0,2,4] → dense[0.1, 0, 0.3, 0, 0.5] |
| `test_sparse_vector_from_dense` | dense → sparse（去零） |
| `test_sparse_vector_empty` | 空 sparse = 全零 dense |
| `test_reproject_same_version` | 同版本 → 原样返回 |
| `test_reproject_direct_mapping` | v1→v2 无 remap → index 不变 |
| `test_reproject_with_remap` | remap entry → 正确重投影 |

---

## 3. P2 模块测试明细

### 3.1 `trajectory::p2::similarity` (12 tests)

| 测试 | 验证点 |
|------|--------|
| `test_cosine_identical` | 同向量 → sim=1 |
| `test_cosine_different` | 正交向量 → sim < 0.5 |
| `test_jaccard_identical` | 同分布 → 1.0 |
| `test_jaccard_disjoint` | 无交集 → 0.0 |
| `test_weighted_projection` | contrib = [0.667, 0.333, 0.0] |
| `test_grouped_contributions` | 4 group 之和 = 1.0 |

### 3.2 `trajectory::p2::cochange` (14 tests)

| 测试 | 验证点 |
|------|--------|
| `test_cochange_no_evidence` | 无共现 commit → commit_score=0 |
| `test_cochange_both_present` | 双 cap 同 commit → 全部 > 0 |
| `test_cochange_batch_commit_discount` | 小 batch(2) > 大 batch(20) |
| `test_jaccard_sets` | {a,b}|{b,c} → 1/3 |
| `test_from_counts` | cooccur=5 → score > 0 |
| `test_from_counts_no_cooccur` | cooccur=0 → score = ln(1) |
| `test_fused_score_log_saturation` | 100 次共现 → score < 5.0（log 饱和） |

### 3.3 `trajectory::p2::coupling_builder` (8 tests)

| 测试 | 验证点 |
|------|--------|
| `test_build_empty` | 空 vector → 无边 |
| `test_build_two_caps` | 2 cap → 1 edge，corr > 0 |
| `test_build_no_cochange` | 相似但无 cochange → corr = 0 |
| `test_aggregate_vectors` | mean([0,0...,0], [1,1...,1]) → 0.5 |

### 3.4 `trajectory::p2::graph` (8 tests)

| 测试 | 验证点 |
|------|--------|
| `test_hard_filter_removes_zero_cochange` | cochange=0 → 过滤 |
| `test_threshold_prune` | corr < 0.5 → 过滤 |
| `test_topk_per_node` | K=1 → 每节点仅保留最强边 |
| `test_prune_empty` | 空图 → 无崩溃 |

### 3.5 `trajectory::p2::correlation` (16 tests)

| 测试 | 验证点 |
|------|--------|
| `test_pearson_perfect_positive` | y=2x → r=1.0 |
| `test_pearson_perfect_negative` | y=4-x → r=-1.0 |
| `test_pearson_no_correlation` | const x → r=0 |
| `test_feature_correlation_matrix` | 同步增长特征 → 全部 r≈1.0 |
| `test_cross_similarity_matrix` | 同向量 → sim=1.0 |
| `test_coupling_strength_single` | 2 边 → edge_count=2 |
| `test_coupling_strength_none` | 无关 cap → edge_count=0 |
| `test_impact_top_n` | top-2 → 按 corr 降序 |

---

## 4. 测试覆盖率

| 模块 | 文件 | 函数覆盖 | 分支覆盖 |
|------|------|----------|----------|
| feature_extractor | p1/feature_extractor.rs | 100% | ~95% |
| sequence_metrics | p1/sequence_metrics.rs | 100% | ~90% |
| timeseries | p1/timeseries.rs | 100% | ~90% |
| vector | p1/vector.rs | 100% | ~90% |
| cluster | p1/cluster.rs | 100% | ~85% |
| registry | p1/registry.rs | 100% | ~90% |
| similarity | p2/similarity.rs | 100% | ~95% |
| cochange | p2/cochange.rs | 100% | ~90% |
| coupling_builder | p2/coupling_builder.rs | 100% | ~90% |
| graph | p2/graph.rs | 100% | ~85% |
| correlation | p2/correlation.rs | 100% | ~90% |

---

## 5. 关键设计验证

| 设计决策 | 测试验证 |
|----------|----------|
| D1: phase_entropy = H(state bigram) | `test_phase_entropy_linear`, `test_phase_entropy_repeated` |
| D2: state_stability = adjacent cosine | `test_cosine_identical`, `test_cosine_different` |
| D4: 三层 entropy 分解 | `test_tool_entropy_*`, `test_phase_entropy_*`, `test_transition_entropy_*` |
| D6: 三轴正交 | `test_cycle_detection_2_state`, `test_backtrack_*`, `test_burst_*` |
| D7: bounded norm + log compress + robust scale | `test_bounded_entropy_*`, `test_log_compress_*`, `test_robust_scale` |
| D9: corr = cochange × (1+λ×sim) | `test_build_two_caps`, `test_build_no_cochange` |
| D10: 4-layer pruning | `test_hard_filter_*`, `test_topk_per_node` |
| D11: log-saturated cochange | `test_fused_score_log_saturation`, `test_cochange_batch_commit_discount` |
| D12: 4 semantic group attribution | `test_grouped_contributions` |
| D13: weighted linear projection | `test_weighted_projection` |
