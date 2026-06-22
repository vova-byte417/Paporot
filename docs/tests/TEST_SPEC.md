# Paporot 测试详细说明

> 版本: 2026-06-11 | 测试总数: **134** | 通过率: **100%**

测试代码位于每个源文件的末尾 `#[cfg(test)] mod tests { ... }` 块中。集成测试位于 `tests/integration_tests.rs`。

运行全部测试：

```bash
cargo test
```

---

## 一、分析层测试 (36 个)

### L1: 差分解析器 (`src/analysis/preprocessor.rs`) — 3 个

| 测试函数 | 测试项 | 输入 | 预期输出 |
|---------|--------|------|---------|
| `test_parse_simple_diff` | 单文件 unified diff 解析 | 1 文件 1 hunk diff 文本 | `changes.len() = 1`, `hunks[0].lines.len() = 5` |
| `test_parse_multiple_files` | 多文件跨语言解析 | .rs + .ts 两文件 diff | `changes.len() = 2`, 语言正确推断为 Rust/TypeScript |
| `test_summarize` | 变更统计 | 2 文件各带加减行 | `files_changed = 2`, `additions = 4`, `deletions = 1` |

### L1: AST 分析器 (`src/analysis/l1_ast.rs`) — 18 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_detect_rust_pub_fn` | 检测 `pub fn` 新增 |
| `test_detect_rust_pub_fn_removed` | 检测 `pub fn` 删除 |
| `test_detect_rust_struct` | 检测 `pub struct` 新增 |
| `test_detect_rust_enum` | 检测 `pub enum` 新增 |
| `test_detect_rust_trait` | 检测 `pub trait` 新增 |
| `test_detect_rust_use_added` | 检测 `use` 导入新增 |
| `test_detect_rust_const` | 检测 `pub const` 新增 |
| `test_skip_private_rust_fn` | 私有函数 `fn foo()` 不应被检测 |
| `test_detect_ts_export_fn` | 检测 `export function` |
| `test_detect_ts_class` | 检测 `export class` |
| `test_detect_ts_import` | 检测 `import ... from ...` |
| `test_detect_python_fn` | 检测 `def func` 且非 `_` 前缀 |
| `test_detect_python_class` | 检测 `class Cls` |
| `test_skip_private_python_fn` | `_` 前缀私有函数不检测 |
| `test_detect_go_pub_fn` | Go 大写函数名检测 |
| `test_skip_private_go_fn` | Go 小写函数名不检测 |
| `test_detect_http_route` | 通用 HTTP 路由检测 (`.get(`/`.post(`...) |
| `test_l1_l2_integration_auth` | L1+L2 联动：login 函数触发安全规则 |
| `test_l1_l2_integration_breaking` | L1+L2 联动：pub fn 删除触发破坏性规则 |

### L2: 规则引擎 (`src/analysis/l2_rules.rs`) — 4 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_auth_rule_hits` | `login` 函数触发 `sec_auth_001` 安全规则 |
| `test_breaking_rule_hits` | `FunctionRemoved` 触发 `breaking_001` |
| `test_test_file_is_tagged` | `*_test.rs` 文件被标记为测试代码 |
| `test_normal_fn_not_flagged` | 普通非破坏性函数不误报 |

### L3: LLM 桥接器 (`src/analysis/l3_llm_bridge.rs`) — 4 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_merge_fragments_empty` | 空片段 → 空 Capability 列表 |
| `test_merge_fragments_non_json` | 非 JSON 内容 → 跳过，返回空 |
| `test_merge_fragments_valid_snapshot` | 有效 BehaviorSnapshot JSON → 解析出 Capability |
| `test_merge_fragments_multiple` | 2 个片段合并 → 2 个 Capability |

### 分析层内部类型 (`src/analysis/types.rs`) — 7 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_language_from_extension_known` | `from_extension` 6 种语言正确映射 |
| `test_language_from_extension_unknown` | 未知扩展名 → `Language::Unknown` |
| `test_language_from_filename` | `from_filename` 从路径推断语言 |
| `test_change_type_is_breaking` | 9 种破坏性 + 若干非破坏性变体的判定 |
| `test_change_type_label_not_empty` | 全部 27 种变体的 `label()` 不为空 |
| `test_raw_change_construction` | `RawChange` 构造和字段访问 |
| `test_rule_trigger_composition` | `RuleTrigger::And` + `::Not` 组合构造 |

---

## 二、核心类型测试 (8 个)

### 类型系统 (`src/types.rs`)

| 测试函数 | 测试项 |
|---------|--------|
| `test_serialize_snapshot` | BehaviorSnapshot JSON 序列化往返 |
| `test_behavior_review_serialization` | P3: BehaviorReview 序列化 |
| `test_review_verdict_serialization` | P3: 4 种 ReviewVerdict 序列化 |
| `test_feedback_store_serialization` | P3: FeedbackStore + FeedbackStats 序列化 |
| `test_test_mapping_serialization` | P4: TestMapping 序列化 |
| `test_test_status_serialization` | P4: 4 种 TestStatus 序列化 |
| `test_test_map_store_serialization` | P4: TestMapStore + TestMapStats 序列化 |
| `test_capability_status_display` | CapabilityStatus 4 种状态的中文名称 |

---

## 三、Agent 调度层测试 (12 个)

### (`src/agent.rs`)

#### compute_diff（6 个）

| 测试函数 | 测试项 |
|---------|--------|
| `test_compute_diff_detects_added` | 新增能力识别：from 无，to 有 (status=New) |
| `test_compute_diff_detects_deleted` | 删除能力识别：from 有，to 无 |
| `test_compute_diff_detects_modified` | 修改能力识别：同 id，to 状态为 Modified |
| `test_compute_diff_unchanged` | 未变化能力识别：同 id，状态 Unchanged |
| `test_compute_diff_mixed_scenario` | 混合场景：新增+修改+删除+未变 + impact_summary 验证 |
| `test_compute_diff_produces_risks` | 风险提示生成：含"兼容性"和"测试覆盖" |

#### truncate_diff（3 个）

| 测试函数 | 测试项 |
|---------|--------|
| `test_truncate_diff_short` | 短 diff 不截断 |
| `test_truncate_diff_long` | 超阈值 diff 截断到阈值 |
| `test_truncate_diff_exact_threshold` | 恰好等于阈值不截断 |

#### l1_changes_to_capabilities（5 个）

| 测试函数 | 测试项 |
|---------|--------|
| `test_l1_changes_to_cap_new_fn` | FunctionAdded → CapabilityStatus::New |
| `test_l1_changes_to_cap_breaking` | FunctionRemoved → CapabilityStatus::Modified |
| `test_l1_changes_tags_from_l2` | L2 标签附到 Capability.tags |
| `test_l1_changes_empty_input` | 空输入不崩溃 |
| `test_l1_changes_multiple` | 3 个 RawChange → 3 个 Capability |

---

## 四、依赖图测试 (`src/graph.rs`) — 3 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_graph_save_and_load` | DependencyGraph 持久化读写往返 |
| `test_cycle_detection` | A→B→C→A 三角循环检测 |
| `test_no_cycle` | A→B→C 线性 DAG 无假阳性 |

---

## 五、命令层测试 (42 个)

### graph 命令 (`src/commands/graph.rs`) — 6 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_impact_analysis_finds_downstream` | 下游影响分析：找到被依赖方 |
| `test_impact_analysis_no_dependents` | 无下游时返回空 |
| `test_evolution_trace` | 演化链追溯 |
| `test_evolution_trace_missing` | 不存在的 capability 返回空 |
| `test_module_query_finds_auth` | 按模块查询找到 auth 模块能力 |
| `test_module_query_no_match` | 不匹配的模块返回空 |

### feedback 命令 (`src/commands/feedback.rs`) — 6 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_approve_adds_review` | approve → stats.approved 递增 |
| `test_reject_adds_review` | reject → stats.rejected 递增 |
| `test_correct_sets_verified_fields` | correct → verified_by/at 更新 |
| `test_flag_adds_review` | flag → stats.flagged 递增 |
| `test_reviews_for_filters` | reviews_for 按 capability_id 过滤 |
| `test_feedback_persistence_roundtrip` | JSON 文件写入→读取往返 |

### testmap 命令 (`src/commands/testmap.rs`) — 9 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_extract_test_file_from_diff_line` | 4 种 diff 行格式提取测试文件路径 |
| `test_infer_source_from_test` | user_test.rs → user.rs 推断 |
| `test_infer_source_java_test` | UserTest.java → User.java 推断 |
| `test_infer_framework` | 4 种语言框架推断 |
| `test_add_mapping` | 添加映射 → stats 更新 |
| `test_mappings_for` | 按 capability 查询映射 |
| `test_stats_after_multiple_adds` | 2 pass + 1 fail + 1 missing 统计 |
| `test_testmap_persistence_roundtrip` | JSON 持久化往返 |
| `test_scan_from_diff` | 完整 diff 扫描映射 |

### diff 命令 (`src/commands/diff.rs`) — 1 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_agent_compute_diff_correct_counts` | 完整 diff：v1(3 caps)→v2(4 caps)，4 种分类计数 |

### version 命令 (`src/commands/version.rs`) — 6 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_mask_api_key_standard` | 标准 key 遮蔽：前 8 + `***` + 后 4 |
| `test_mask_api_key_short` | 短 key → `***` |
| `test_mask_api_key_exact_12` | 12 字符 → `***` |
| `test_mask_api_key_13_chars` | 13 字符 → 15 字符输出 |
| `test_cargo_pkg_version_exists` | `CARGO_PKG_VERSION` 编译期常量 |
| `test_cargo_pkg_name` | `CARGO_PKG_NAME` 为 "Paporot" |

### review 命令 (`src/commands/review.rs`) — 3 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_agent_review_pipeline_existence` | Agent 结构体完整性 |
| `test_default_diff_range` | diff 默认范围 "HEAD~1..HEAD" |
| `test_empty_diff_detection` | 空/非空 diff 判定 |

### coverage 命令 (`src/commands/coverage.rs`) — 1 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_coverage_icon_mapping` | 4 种 CoverageStatus → 图标映射 |

### risk 命令 (`src/commands/risk.rs`) — 2 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_empty_storage_returns_empty_versions` | 空存储返回空版本列表 |
| `test_find_previous_version_single` | 单版本无上一版本（带隔离存储） |

### regression 命令 (`src/commands/regression.rs`) — 2 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_insufficient_snapshots_for_regression` | 版本不足检测（带隔离存储） |
| `test_two_snapshots_select_prev_and_curr` | 两个快照正确选出 prev 和 curr |

### snapshot 命令 (`src/commands/snapshot.rs`) — 4 个

| 测试函数 | 测试项 |
|---------|--------|
| `test_agent_has_storage` | Agent 持有 storage 字段 |
| `test_diff_range_format` | 3 种 git range 格式验证 |
| `test_next_version_id_format` | next_version_id 格式 (v + 数字) |
| `test_default_diff_warn_threshold` | 阈值默认值在合理范围 |

### 基础设施 (`src/storage.rs`, `src/config.rs`, `src/prompts.rs`, `src/llm/client.rs`) — 9 个

| 文件 | 测试数 | 测试项 |
|------|--------|--------|
| `storage.rs` | 2 | `test_save_and_load`, `test_next_version_id` |
| `config.rs` | 2 | `test_default_config`, `test_sample_toml_parses` |
| `prompts.rs` | 3 | `test_build_extraction_prompt_minimal/full`, `test_build_diff_prompt` |
| `llm/client.rs` | 2 | `test_extract_json_code_block`, `test_extract_json_plain` |

---

## 六、集成测试 (`tests/integration_tests.rs`) — 21 个

#### L1+L2 全流水线（4 个）

| 测试函数 | 测试项 |
|---------|--------|
| `test_full_pipeline_rust_new_feature` | 完整 L1+L2：新增 pub fn 通过预处理→分析→规则评估 |
| `test_full_pipeline_removal_detection` | 删除 pub fn 被正确识别为破坏性变更 |
| `test_full_pipeline_mixed_languages` | 多语言混合 diff (Rust + Python) 分析 |
| `test_empty_diff` | 空 diff 不崩溃 |

#### 安全规则联动（3 个）

| 测试函数 | 测试项 |
|---------|--------|
| `test_security_rules_on_auth_changes` | 认证变更触发安全规则 |
| `test_breaking_change_rules` | 破坏性变更触 breaking 规则 |
| `test_no_rule_on_innocuous_change` | 文档变更不误报 |

#### 依赖图操作（3 个）

| 测试函数 | 测试项 |
|---------|--------|
| `test_dependency_relation_serialization` | DependencyRelation 序列化 |
| `test_graph_cycle_detection_complex` | 复杂循环检测 |
| `test_graph_persistence_roundtrip` | 图持久化往返 |
| `test_evolution_chain_across_snapshots` | 跨快照演化链 |

#### 序列化与向后兼容（6 个，含系统级）

| 测试函数 | 测试项 |
|---------|--------|
| `test_snapshot_with_contract_serialization` | 含 contract 的 snapshot 序列化 |
| `test_behavior_contract_variants` | 3 种 contract 变体序列化 |
| `test_load_legacy_v1_snapshot` | 加载旧 v1 snapshot |
| `test_system_schema_version_backward_compat` | v1 JSON 加载后 `schema_version = 3` |
| `test_system_contract_three_variants` | 系统级三变体 contract 解析 |

#### DiffPreprocessor 边界（4 个）

| 测试函数 | 测试项 |
|---------|--------|
| `test_binary_file_diff` | 二进制文件 diff 处理 |
| `test_multiple_hunks_single_file` | 单文件多 hunk 解析 |
| `test_rename_detection` | 重命名检测 |

#### 系统级端到端（4 个）

| 测试函数 | 测试项 |
|---------|--------|
| `test_system_agent_compute_diff_pipeline` | Agent diff 管线：v1→v2→v3 三版本完整 diff |
| `test_system_l1_full_api_change` | L1 完整多文件 API 变更分析（3 文件） |
| `test_system_schema_version_backward_compat` | Schema 向后兼容 |
| `test_system_contract_three_variants` | Contract 三变体 |

#### 文档测试（1 个）

| 位置 | 测试项 |
|------|--------|
| `src/analysis/l1_ast.rs:127` | `AstAnalyzer::analyze()` doc-test |

---

## 测试运行

```bash
# 全部测试
cargo test

# 仅单元测试
cargo test --lib

# 仅集成测试
cargo test --test integration_tests

# 仅文档测试
cargo test --doc

# 按模块过滤
cargo test analysis::
cargo test agent::
cargo test commands::
cargo test types::
```
