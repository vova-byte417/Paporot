# Paporot P0 — Behavior State Machine 测试报告

> 日期: 2026-06-12 | 工具: `cargo test` | 环境: rustc 1.96.0 (WSL Ubuntu 24.04)

---

## 测试结果总览

| 类别 | 数量 | 通过 | 失败 | 忽略 |
|------|------|------|------|------|
| 单元测试 + bin测试 | 319 + 286 | 319 + 286 | 0 | 0 |
| 集成测试 | 33 | 33 | 0 | 0 |
| 文档测试 | 2 | 1 | 0 | 1 |
| **总计（去重）** | **~350** | **~350** | **0** | **1** |

**通过率: 100%**（唯一忽略的 doctest 属于现有代码）

---

## 新增 P0 模块测试清单

### `state/features.rs` (6 tests)

| 测试 | 验证内容 |
|------|---------|
| `test_extract_empty` | 空 tool 序列 → 全 0 features |
| `test_tool_histogram` | 4 tool → locate=0.5, modify=0.25, verify=0.25 |
| `test_edit_density` | read+edit+write+test → 0.5 edit density |
| `test_read_write_ratio` | read+grep+ls+edit → 0.75 read ratio |
| `test_loop_intensity` | read,read,edit,edit,test → 0.5 loops |
| `test_file_clusters` | 2 .rs + 1 .py → rust=0.67, python=0.33 |

### `state/segmentation.rs` (8 tests)

| 测试 | 验证内容 |
|------|---------|
| `test_empty_trace` | 空 → 空 |
| `test_single_tool` | 1 tool → 1 segment |
| `test_tool_type_change` | read→edit → 2 segments (ToolTypeChange) |
| `test_failure_loop` | test→edit → 2 segments (FailureLoop, 优先级高于 ToolTypeChange) |
| `test_file_scope_jump` | src/auth.rs→tests/auth_test.rs → 2 segments |
| `test_idle_gap` | 1 hour gap → 2 segments (IdleGap) |
| `test_consecutive_same_tool_no_cut` | 3 同类型 tool → 1 segment |
| `test_mixed_trace` | 8 tool 混合 → ≥5 segments |

### `state/window.rs` (4 tests)

| 测试 | 验证内容 |
|------|---------|
| `test_window_within_boundary` | Window 不跨 segment 边界 |
| `test_small_segment_one_candidate` | 1-tool segment → 1 candidate |
| `test_stride_behavior` | stride=3 产生 2+ candidates |
| `test_phase_distribution` | 4 tool → locate=0.5, modify=0.25, verify=0.25 |

### `state/merge.rs` (5 tests)

| 测试 | 验证内容 |
|------|---------|
| `test_merge_empty` | 空 → 空 |
| `test_single_candidate` | 1 candidate → 1 state |
| `test_merge_similar_adjacent` | 相似 modify → merge to 1 |
| `test_not_merge_dissimilar` | locate vs commit → split to 2 |
| `test_stability_score` | 稳定性评分范围 0-1 |

### `state/transition.rs` (5 tests)

| 测试 | 验证内容 |
|------|---------|
| `test_empty_event_log` | 0 state → 0 events |
| `test_single_state_no_events` | 1 state → 0 events |
| `test_event_log_sequence` | 4 states → 3 events, 正确 from/to |
| `test_aggregate_edges` | 3 events → 2 edges, count 正确 |
| `test_build_graph` | 完整 graph: trace_id, states, edges |

### `state/builder.rs` (3 tests)

| 测试 | 验证内容 |
|------|---------|
| `test_build_empty_trace` | 空 trace → 空 graph |
| `test_build_simple_trace` | 7 tool 混合 → ≥2 states, ≥1 events |
| `test_build_single_tool_trace` | 1 tool → 1 state, 0 events |

### `similarity/merge_sim.rs` (4 tests)

| 测试 | 验证内容 |
|------|---------|
| `test_merge_identical` | 相同 features → 1.0 |
| `test_merge_different_phases` | 不同 phase → ~0.7 (file/ctrl/fail match) |
| `test_jaccard_identical` | Jaccard 相同 → 1.0 |
| `test_jaccard_disjoint` | Jaccard 无交集 → 0.0 |

### `similarity/align_sim.rs` (3 tests)

| 测试 | 验证内容 |
|------|---------|
| `test_align_identical` | 相同 → 1.0 |
| `test_align_partial_overlap` | 部分重叠 → 0.1~0.9 |
| `test_align_threshold_stricter_than_merge` | align ≤ merge for different states |

### `evaler/state_eval.rs` (3 tests)

| 测试 | 验证内容 |
|------|---------|
| `test_empty_graph` | 空 → state_count=0, entropy=0 |
| `test_single_state` | 1 state → loop_ratio=0 |
| `test_loop_detection` | 连续同 phase → loop_ratio=0.5 |

### `evaler/transition_eval.rs` (3 tests)

| 测试 | 验证内容 |
|------|---------|
| `test_no_oscillation` | 线性序列 → oscillation=0 |
| `test_with_oscillation` | 来回序列 → oscillation≥1 |
| `test_reversal_ratio` | s0→s1→s0 → reversal>0 |

### `evaler/graph_eval.rs` (2 tests)

| 测试 | 验证内容 |
|------|---------|
| `test_empty_graph` | 空 → path_length=0, entropy=0 |
| `test_with_edges` | 3 states + 2 edges → path_length=3, entropy>0 |

### `evaler/mod.rs` (3 tests)

| 测试 | 验证内容 |
|------|---------|
| `test_evaluate_simple_pass` | 线性流程 → Pass |
| `test_evaluate_empty_graph` | 空 graph → Degraded |
| `test_evaluate_oscillation` | 振荡模式 → TE001 hit |

### `projection/state_to_diff.rs` (2 tests)

| 测试 | 验证内容 |
|------|---------|
| `test_identical_graphs` | 相同 graph → segments_unchanged>0 |
| `test_graph_with_changes` | B 多 1 state → tool_count diff |

### 集成测试增量

8 个 trajectory 集成测试继续通过（test_trajectory_diff_end_to_end 等），无需修改。

---

## 测试覆盖的关键路径

```
Trace → StateGraph
  → RuleSegmenter::cut()          (8 tests)
    → WindowBuilder::build()      (4 tests)
      → AdjacentMerger::merge()   (5 tests)
        → TransitionBuilder       (5 tests)
          → Eval                  (8 tests)
            → Projection→Diff     (2 tests)

Shared: StateFeatures extraction   (6 tests)
Shared: Merge/Align similarity     (7 tests)
```

## 边界条件覆盖

| 边界条件 | 测试 |
|----------|------|
| 空 trace | `test_build_empty_trace`, `test_extract_empty`, `test_merge_empty` |
| 单 tool trace | `test_build_single_tool_trace`, `test_single_tool` |
| 零相似度 | `test_jaccard_disjoint` |
| 完全相似 | `test_merge_identical` |
| 振荡检测 | `test_evaluate_oscillation` |
| FailureLoop 优先级 | `test_failure_loop` |

## 运行指令

```bash
# 全部测试
wsl -d ubuntu-24.04 -- bash -c "source ~/.cargo/env && cd /mnt/d/ai/trae_projects/Paporot && cargo test"

# 仅 state 模块
wsl -d ubuntu-24.04 -- bash -c "source ~/.cargo/env && cd /mnt/d/ai/trae_projects/Paporot && cargo test trajectory::state"

# 仅 evaler
wsl -d ubuntu-24.04 -- bash -c "source ~/.cargo/env && cd /mnt/d/ai/trae_projects/Paporot && cargo test trajectory::evaler"
```
