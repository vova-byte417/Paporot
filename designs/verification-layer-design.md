# Paporot Verification Layer 设计文档

> 基于 [Verification Layer PRD](../docs/Verification%20Layer%20PRD.md) 的结构化需求澄清与设计方案。
>
> 设计日期：2026-06-22

---

## 1. 理解总结

- **正在构建**：在 Paporot 现有行为分析能力之上，构建 Verification Layer——一套自动验证 AI 生成 Artifact（JSON / Excalidraw / HTML / Screenshot）正确性的框架，包含 Contract Engine、Evidence Engine、Replay Engine、Judge Engine、Regression Engine 五大引擎。
- **为什么存在**：AI Agent 产出速度远超人工审查速度；Skill 输出存在随机性；需要质量把关 + 回归防线并举。
- **目标用户**：开发阶段开发者手动触发 + Pipeline 自动集成；上线后收敛为全自动 Pipeline。
- **关键约束**：MVP 优先结构正确性，JSON/Excalidraw 零容忍；Contract 秒级本地执行，Judge 分钟级可调 LLM；数据仅存 WASM 沙盒生命周期内。
- **明确非目标**：不验证代码（验证可观察行为）；MVP 不含多 Agent Judge 和自动修复。

---

## 2. 假设清单

1. JSON / Excalidraw 的结构验证可通过确定性规则（schema 解析、字段校验、元素计数）在秒级完成，无需 LLM。
2. 现有 Paporot 的 SQLite 和 `.paporot/` 目录结构可直接扩展以容纳 Verification 数据。
3. WASM 沙盒的 host function（`host_read_file` / `host_write_file`）足以支撑 Evidence 的临时存储需求。
4. Contract 配置文件的 YAML 格式对目标用户可接受，学习成本低。
5. 硬阻断策略不会导致 Pipeline 过度敏感——因为 JSON / Excalidraw 的结构检查是确定性的。

---

## 3. 整体架构

```
┌─────────────────────────────────────────────────────────────────┐
│                      Paporot Core (Native / Rust)                │
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐   │
│  │ Contract     │  │ Evidence     │  │ 现有模块              │   │
│  │ Engine       │  │ Engine       │  │ Snapshot / Diff /    │   │
│  │              │  │              │  │ Graph / SQLite /     │   │
│  │ 加载 Contract│  │ 收集 Input   │  │ Analysis             │   │
│  │ YAML 配置    │  │ 收集 Output  │  │                      │   │
│  │ 本地执行     │  │ 暂存沙盒     │  │                      │   │
│  │ 确定性校验   │  │ 生命周期     │  │                      │   │
│  │ 返回结果     │  │ Replay 读取  │  │                      │   │
│  └──────┬───────┘  └──────┬───────┘  └──────────────────────┘   │
│         │                 │                                      │
│         │  host_verify_   │  host_capture_                      │
│         │  contract()     │  evidence()                          │
│         │                 │                                      │
├─────────┼─────────────────┼──────────────────────────────────────┤
│         ▼                 ▼                  WASM Sandbox        │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │              Skill Runtime (现有 DAG Engine)              │   │
│  │                                                          │   │
│  │  Layer 1-5: 现有 6 个分析 Skill                          │   │
│  │                        ↓ Artifact Output                 │   │
│  │  ┌─────────────────────────────────────────────────┐    │   │
│  │  │  verification-runner (门面 Skill, Layer 7)       │    │   │
│  │  │    → host_verify_contract()                     │    │   │
│  │  │    → host_capture_evidence()                    │    │   │
│  │  │    → FAIL → 阻断, 输出修正方案                   │    │   │
│  │  │    → PASS → 继续                                │    │   │
│  │  └─────────────────────────────────────────────────┘    │   │
│  │                                                          │   │
│  │  Layer 8: judge-verifier / replay-engine /              │   │
│  │           regression-engine (Phase 2/3 Skill)           │   │
│  └──────────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────────┘
```

### 关键设计决策

- **混合架构**：Contract Engine 和 Evidence Engine 放在 Paporot Core（Rust 本地执行），Replay / Judge / Regression Engine 作为 WASM Skill 实现。
- **新增 host function**：`host_verify_contract()`、`host_capture_evidence()`、`host_save_replay_case()`、`host_load_replay_cases()`。
- 现有 6 个 Skill 无需修改，产出 Artifact 后由 DAG 自动调度 `verification-runner`。
- Phase 2/3 的 Skill 在同层级追加，不改变基础架构。

---

## 4. Contract Engine 详解

### 4.1 Contract 配置格式

用户在 `.paporot/contracts/` 下为每种 Artifact 类型创建 YAML 文件：

```yaml
# .paporot/contracts/excalidraw.contract.yaml
artifact_type: excalidraw
version: "1.0"
severity: error          # error = 硬阻断, warn = 不阻断

rules:
  syntax:
    valid_json: true
    valid_excalidraw_schema: true       # type/version/elements 必填，elements 是数组

  structure:
    min_elements: 1                     # 至少 1 个元素
    no_deleted_only: true               # 不允许所有元素都 isDeleted=true
    allowed_element_types:              # 允许的元素类型白名单
      - rectangle
      - ellipse
      - diamond
      - text
      - arrow
      - line
      - freedraw
      - image
    max_elements: 500
```

```yaml
# .paporot/contracts/json.contract.yaml
artifact_type: json
version: "1.0"
severity: error

rules:
  syntax:
    valid_json: true
    conforms_to_schema: "analysis_result.schema.json"   # 引用 schema 文件

  structure:
    required_fields: ["project_name", "modules", "dependencies"]
    no_empty_arrays: true
```

### 4.2 执行引擎

```
Artifact + Contract配置 → ContractEngine.verify() → VerificationResult
                                                         ├─ status: PASS | FAIL
                                                         ├─ rule_results: [{rule, pass, detail}]
                                                         └─ suggestions: ["修复建议1", "修复建议2"]
```

### 4.3 内置校验器（纯 Rust，不调用 LLM）

| 校验器 | 用途 | 性能 |
|--------|------|------|
| `JsonSyntaxChecker` | JSON 是否合法、是否符合指定 schema | <1ms |
| `ExcalidrawSchemaChecker` | 验证 `type`/`version`/`elements` 必填字段，elements 为合法数组 | <1ms |
| `HtmlSyntaxChecker` | HTML 是否 valid（DOM 构建成功） | <10ms |
| `StructureAnalyzer` | 元素计数、isDeleted 检测、类型白名单、命名唯一性 | <1ms |
| `SchemaValidator` | 对比 JSON Schema 与产出 JSON | <5ms |

---

## 5. Evidence Engine 详解

### 5.1 证据生命周期

```
Skill DAG 执行开始
       │
       ▼
  host_capture_evidence()    ← 每次 Skill 产出 Artifact 时调用
       │
       ▼ 写入沙盒内存缓冲区
  Evidence Buffer            ← WASM 线性内存，不落宿主机磁盘
       │
       ▼ 用于 Replay / Regression
  host_save_replay_case()    ← FAIL 时持久化到 .paporot/regression/cases/ (SQLite)
       │
       ▼
  沙盒退出 → 内存清空         ← Evidence Buffer 自动销毁
```

### 5.2 证据粒度

| 证据项 | 内容 | 存储位置 |
|--------|------|----------|
| Prompt | LLM 调用的原始 prompt | 内存 → Replay Case（FAIL 时） |
| Skill Input | 上游 Skill 传给本 Skill 的数据 | 内存 → Replay Case（FAIL 时） |
| Skill Output | 本 Skill 产出的 Artifact | 内存 → Replay Case（FAIL 时） |
| Intermediate | 中间态（如 JSON→Excalidraw 的中间表示） | 内存 → Replay Case（FAIL 时） |

PASS 场景下 Evidence 仅在内存中留存，沙盒退出即销毁。FAIL 时自动打包为 Replay Case 落盘。

### 5.3 Host Function 接口

```rust
host_capture_evidence(artifact_id, input_json, output_json, intermediate_json) -> errno
host_save_replay_case(case_json) -> errno
host_load_replay_cases() -> json_array
```

---

## 6. verification-runner Skill（门面）

### 6.1 DAG 位置

```
现有 DAG:
  Layer 1: repository-understanding
  Layer 2: module-discovery
  Layer 3: dependency-analysis
  Layer 4: runtime-flow-analysis
  Layer 5: behavior-boundary-discovery
  Layer 6: architecture-doc-generator  ← 产出 JSON / Excalidraw / HTML
           │
           ▼
  Layer 7: verification-runner         ← 新增，MVP 唯一新 Skill
           │
           ├── Contract 验证（调用 host_verify_contract）
           ├── Evidence 收集（调用 host_capture_evidence）
           │
           ├── PASS → 继续 / 结束
           └── FAIL → 保存 Replay Case → 阻断 → 输出错误+修正方案
```

### 6.2 skill.toml

```toml
[skill]
name = "verification-runner"
version = "0.1.0"
requires_paporot = "0.3.0"
description = "对所有上游 Artifact 执行 Contract 验证、收集 Evidence、在 FAIL 时保存 Replay Case"
timeout_secs = 30

[outputs]
schema = """{
  "type": "object",
  "properties": {
    "overall_status": {"type": "string", "enum": ["PASS", "FAIL"]},
    "results": {"type": "array", "items": {"$ref": "#/definitions/ArtifactResult"}},
    "replay_cases_saved": {"type": "integer"}
  }
}"""
format = "json"

[dependencies]
uses_outputs_from = [
  "architecture-doc-generator",
  "dependency-analysis",
  "runtime-flow-analysis"
]

[llm_calls]
max_calls = 0
```

### 6.3 FAIL 输出示例

```json
{
  "overall_status": "FAIL",
  "results": [
    {
      "artifact_id": "architecture.excalidraw",
      "artifact_type": "excalidraw",
      "status": "FAIL",
      "rule_results": [
        {"rule": "valid_excalidraw_schema", "pass": true},
        {"rule": "min_elements", "pass": false,
         "detail": "expected >= 1, got 0"},
        {"rule": "no_deleted_only", "pass": true}
      ],
      "suggestions": [
        "Excalidraw 图中元素数量为 0，请检查 architecture-doc-generator 是否正确生成了 elements 数组",
        "建议：检查上游 Skill 的 excalidraw 序列化逻辑，确认 elements 非空"
      ]
    }
  ],
  "replay_cases_saved": 1
}
```

---

## 7. Replay Engine 与 Regression Engine（Phase 2 预览）

### 7.1 Replay Engine

```
Replay Case (历史 FAIL) → 重新注入输入 → 当前 Generator 重新执行
                                                    │
                                                    ▼
                                           新 Artifact
                                                    │
                                                    ▼
                                           Contract 验证
                                                    │
                                           PASS → 系统已修复
                                           FAIL → 问题仍存在
```

**Replay Case 数据结构：**

```json
{
  "case_id": "uuid",
  "created_at": "RFC3339",
  "source_commit": "abc123",
  "artifact_type": "excalidraw",
  "upstream_input": { "..." },
  "failed_artifact": { "..." },
  "contract_result": { "..." },
  "suggestions": ["..."]
}
```

### 7.2 Regression Engine

```
每次代码变更后：
  1. 加载所有历史 Replay Case + 所有 Contract 配置
  2. 逐个 Case Replay
  3. 逐条 Contract 重新验证
  4. 生成 Regression Report

Regression Report：
  - 新增 FAIL：原来能通过的，现在 FAIL → 回归！
  - 修复 PASS：原来 FAIL 的，现在 PASS → 修复确认
  - 稳定：状态未变 → 无变化
```

**存储设计：**

```
.paporot/regression/
├── cases/           ← Replay Case (SQLite)
├── reports/         ← Regression Report (JSON)
└── history.csv      ← 每次回归的趋势数据
```

---

## 8. MVP 范围一览

| 维度 | MVP (Phase 1) | Phase 2 | Phase 3 |
|------|:--:|:--:|:--:|
| Artifact 类型 | JSON, Excalidraw | + HTML, Screenshot | — |
| Contract 验证 | 结构正确性 | 语义正确性 | — |
| Evidence 收集 | PASS 内存 / FAIL 落盘 | 同左 | 同左 |
| Replay | — | 自动重放历史 Case | 同左 |
| Regression | — | 自动回归报告 | 同左 |
| Judge Agent | — | LLM 语义审核 | 多 Agent |
| Auto Fix | — | — | 自动修复循环 |
| 用户触发 | CLI + Pipeline | 仅 Pipeline | 仅 Pipeline |

---

## 9. 决策日志

| # | 决策 | 备选方案 | 理由 |
|---|------|----------|------|
| 1 | 混合架构：Contract / Evidence 进 Core，Replay / Judge / Regression 做 Skill | 全部 Skill 化 / 全部 Core | 性能敏感逻辑不宜进 WASM；需 LLM 的逻辑宜复用现有 LLM Bridge |
| 2 | Core 引擎 + Skill 门面（方案 C） | 独立管线(A) / 全内联 DAG(B) | 与决策 1 一致，最小侵入，最大复用 |
| 3 | Contract 配置用 YAML 声明式 + Rust / WASM 校验插件 | AI 推断 / 纯 Rust 代码 | 用户低门槛配置 + 插件保证配置正确，两层防护 |
| 4 | Excalidraw 替代 Mermaid 为目标图格式 | Mermaid | 项目实际使用 Excalidraw |
| 5 | 性能分层：Contract 秒级本地，Judge 分钟级可调 LLM | 全同步 / 全异步 | 结构检查确定性高可本地；语义需 LLM 但可接受延迟 |
| 6 | 硬阻断 FAIL，输出错误 + 修正方案 | 软警告 / 可配置 | 用户明确选择，保证 JSON / Excalidraw 零容忍 |
| 7 | MVP 先结构正确性，Phase 2 再语义 | 同步全覆盖 | 降低 MVP 复杂度，Excalidraw 语义校验需 LLM |
| 8 | JSON / Excalidraw 零容忍优先，HTML / Screenshot 后置 | 全类型平权 | 高风险 Artifact 优先保障 |
| 9 | 证据仅存沙盒内存，FAIL 时才落盘 Replay Case | 全量持久化 / 全量内存 | 安全约束（沙盒退出自动销毁）+ 失败案例才有回溯价值 |
| 10 | 开发阶段 C（双触发），上线后 B（Pipeline 自动） | 仅手动 / 仅自动 | 演进路径清晰，架构需同时支持两种模式 |
| 11 | 本地 SQLite 存储 | PostgreSQL / 远程存储 | 与现有架构一致，MVP 保持简单 |
