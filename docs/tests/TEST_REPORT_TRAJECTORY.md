# Paporot Trajectory Diff 测试报告

> 日期: 2026-06-12 | 测试工具: `cargo test` | 环境: rustc 1.96.0 (WSL Ubuntu 24.04)

---

## 测试结果总览

| 类别 | 数量 | 通过 | 失败 | 忽略 |
|------|------|------|------|------|
| 单元测试 | 235 | 235 | 0 | 0 |
| 集成测试 | 33 | 33 | 0 | 0 |
| 文档测试 | 2 | 1 | 0 | 1 |
| **总计** | **270** | **269** | **0** | **1** |

**通过率: 99.6%**（1 个忽略的 doctest，属于现有代码）

---

## 新增测试清单

### 1. trajectory 模块单元测试（57 个）

#### `src/trajectory/types.rs` (5 个)

| 测试 | 验证内容 |
|------|---------|
| `test_trajectory_diff_serde_roundtrip` | TrajectoryDiff JSON 序列化往返 |
| `test_segment_kind_serde` | SegmentKind 枚举序列化 |
| `test_tool_diff_kind_serde` | ToolDiffKind 枚举序列化 |
| `test_diff_summary_default` | DiffSummary 默认值全为零 |
| `test_phase_segment_serde` | PhaseSegment + ToolIndexInfo 序列化 |

#### `src/trajectory/hash.rs` (5 个)

| 测试 | 验证内容 |
|------|---------|
| `test_semantic_hash_deterministic` | 相同输入 → 相同 hash |
| `test_semantic_hash_different_tool_names` | read vs write → 不同 hash |
| `test_semantic_hash_different_args` | 同 name 不同 args → 不同 hash |
| `test_semantic_hash_same_tool_same_args` | 完全相同的 tool → 相同 hash |
| `test_semantic_hashes_batch` | 批量 hash 计算 |

#### `src/trajectory/classifier.rs` (8 个)

| 测试 | 验证内容 |
|------|---------|
| `test_classify_empty_trace` | 空 trace → 空 segment |
| `test_classify_single_tool` | 单个 tool → 1 个 segment |
| `test_classify_phase_transition` | read→edit→test → 3 segments |
| `test_classify_unknown_tool` | 未知 tool → default phase |
| `test_classifier_name_version` | name/version 正确 |
| `test_classify_consecutive_same_phase` | read+grep+ls → 1 segment |
| `test_classify_alternating_phases` | read→edit→grep→write → 4 segments |
| `test_english_classifier` | English labels: locate/modify/verify/commit |

#### `src/trajectory/align/scorer.rs` (5 个)

| 测试 | 验证内容 |
|------|---------|
| `test_default_costs` | 默认代价 1.0/1.0/1.0 |
| `test_tool_substitution_same_hash` | 同 hash → cost 0.0 |
| `test_tool_substitution_different_name` | 不同 name → cost 1.0 |
| `test_tool_substitution_same_name_diff_hash` | 同 name 不同 hash → 0.5 |
| `test_tool_substitution_different_name_same_hash` | 不同 name 同 hash(碰撞) → 0.0 |

#### `src/trajectory/align/segment.rs` (7 个)

| 测试 | 验证内容 |
|------|---------|
| `test_match_identical_segments` | 相同序列 → Unchanged |
| `test_match_added_segment` | B 多段 → Added |
| `test_match_deleted_segment` | A 多段 → Deleted |
| `test_match_mixed` | 混合场景：Unchanged+Added+Unchanged |
| `test_match_empty_segments` | 双空 → 空结果 |
| `test_match_all_deleted` | B 空 → 全部 Deleted |
| `test_match_english_labels` | 内置多个 label 场景 |

#### `src/trajectory/align/tool.rs` (8 个)

| 测试 | 验证内容 |
|------|---------|
| `test_align_identical_tools` | 相同序列 → Unchanged |
| `test_align_added_tool` | B 多 tool → Added |
| `test_align_deleted_tool` | A 多 tool → Deleted |
| `test_align_args_changed` | 同 name 不同 args → ArgsChanged |
| `test_align_empty_traces` | 双空 → 空结果 |
| `test_greedy_fallback_used_for_long_sequences` | 250 tools → 贪心降级 |
| + Levenshtein 路径覆盖测试 |

#### `src/trajectory/align/engine.rs` (3 个)

| 测试 | 验证内容 |
|------|---------|
| `test_engine_full_diff` | 完整双层对齐流程 |
| `test_engine_empty_traces` | 双空 trace → 空 diff |
| `test_engine_identical_traces` | 相同 trace → 全部 Unchanged |

#### `src/trajectory/analysis.rs` (7 个)

| 测试 | 验证内容 |
|------|---------|
| `test_from_diff_empty` | 空 diff → 全 0 score |
| `test_from_diff_phase_additions` | Added segment → phase_additions |
| `test_from_diff_phase_deletions` | Deleted segment → phase_deletions |
| `test_from_diff_phase_modifications` | Modified segment → phase_modifications |
| `test_tool_churn_score` | (add+del)/total → 0.5 |
| `test_phase_reorder_score` | (add+del)/total_phases → 0.5 |
| `test_capability_shift_score` | modified/total → 0.25 |

#### `src/trajectory/report.rs` (4 个)

| 测试 | 验证内容 |
|------|---------|
| `test_mermaid_generation` | Mermaid 包含 gantt + trace refs |
| `test_json_report` | JSON 输出包含原始 trace id |
| `test_terminal_summary` | 终端摘要包含关键字段 |
| `test_dashboard_json` | Dashboard JSON 包含 mermaid + diff |

#### `src/evaler/rules.rs` 段级规则（重构新增）

现有 10 个测试保持不变，新增段级规则逻辑（含 S001–S005 规则定义），集成测试覆盖段级规则检测行为。

### 2. 集成测试 — trajectory 模块（8 个新增）

| 测试 | 验证内容 |
|------|---------|
| `test_trajectory_diff_end_to_end` | 3-tool vs 5-tool → 检测 Added phase |
| `test_trajectory_diff_identical_traces` | 相同 trace → 全部 Unchanged |
| `test_trajectory_diff_empty_traces` | 双空 → 无 segment |
| `test_trajectory_to_analysis_pipeline` | Diff → Analysis 完整流程 |
| `test_segment_rules_with_analysis` | 构造 Added phase → 段级规则不误报 |
| `test_segment_rules_critical_phase_deletion` | 删除 critical phase → S002 命中 |
| `test_trajectory_mermaid_output` | Mermaid 输出格式验证 |
| `test_classifier_trait_usage` | PhaseClassifier trait 多态调用 |

---

## 测试覆盖的关键路径

```
PhaseClassifier::classify()
    → SegmentMatcher::match_segments()
        → ToolMatcher::align_tools()   (Levenshtein / Greedy)
            → AlignmentEngine::diff() → TrajectoryDiff
                → TrajectoryAnalysis::from_diff()
                    → evaluate_segment_rules() → Vec<DegradeRuleHit>
```

## 边界条件覆盖

| 边界条件 | 测试 |
|----------|------|
| 空 trace | `test_classify_empty_trace`, `test_align_empty_traces`, `test_engine_empty_traces` |
| 单 tool 序列 | `test_classify_single_tool` |
| 长序列 (>200) | `test_greedy_fallback_used_for_long_sequences` |
| 未知 tool 名称 | `test_classify_unknown_tool` |
| Hash 碰撞场景 | `test_tool_substitution_different_name_same_hash` |
| 全零值 | `test_from_diff_empty`, `test_diff_summary_default` |

---

## 运行指令

```bash
# 全部测试（WSL）
wsl -d ubuntu-24.04 -- bash -c "source ~/.cargo/env && cd /mnt/d/ai/trae_projects/Paporot && cargo test"

# 仅 trajectory 模块
wsl -d ubuntu-24.04 -- bash -c "source ~/.cargo/env && cd /mnt/d/ai/trae_projects/Paporot && cargo test trajectory"

# 仅 evaler 模块
wsl -d ubuntu-24.04 -- bash -c "source ~/.cargo/env && cd /mnt/d/ai/trae_projects/Paporot && cargo test evaler"
```
