# Paporot 设计文档

> AI 生成软件的行为版本控制与审计系统

---

## 目录

1. [项目概述](#1-项目概述)
2. [系统架构](#2-系统架构)
3. [三层混合分析引擎 (P0)](#3-三层混合分析引擎-p0)
4. [行为合约化 (P1)](#4-行为合约化-p1)
5. [能力依赖图 (P2)](#5-能力依赖图-p2)
6. [人机验证回路 (P3)](#6-人机验证回路-p3)
7. [行为-测试闭环 (P4)](#7-行为-测试闭环-p4)
8. [CLI 命令系统](#8-cli-命令系统)
9. [存储与持久化](#9-存储与持久化)
10. [Schema 版本策略](#10-schema-版本策略)
11. [测试体系](#11-测试体系)
12. [依赖关系](#12-依赖关系)

---

## 1. 项目概述

### 1.1 问题背景

AI 代码生成工具（Copilot、Claude Code、Cursor 等）正在从根本上改变软件开发方式。代码不再完全由人逐行编写，而是由模型根据 prompt 和上下文自动生成。这带来一个核心问题：

> **当代码由 AI 生成时，我们如何理解"这次改了什么"？**

传统 git diff 只能看到文本差异，无法回答以下问题：

- 这次变更引入了哪些**新的外部可观测行为**？
- 某个 API 端点的行为契约是否被**破坏性修改**了？
- 新增代码是否引入了**安全风险**（如新的认证入口）？
- 某个能力的修改会**影响哪些下游依赖**？
- 相对于 PRD，当前实现了功能的**百分之几**？
- AI 的行为推断是否**准确**？需要人工审核哪些？

Paporot 基于 **Anthropic 的行为版本控制理念**，将 git diff 从"文本差异"提升为"行为差异"。

### 1.2 核心概念

| 概念 | 定义 | 类比 |
|------|------|------|
| **Capability** | 最小可理解的行为单元（如"JWT 登录 API"、"用户创建函数"） | git 中的 file change |
| **BehaviorSnapshot** | 某一时刻全部 Capability 的版本化画像 | git commit |
| **BehaviorDiff** | 两个 Snapshot 之间的行为差异（新增/修改/删除/未变） | git diff |
| **BehaviorContract** | Capability 的可验证接口定义（API 方法+路径、函数签名等） | OpenAPI spec |
| **DependencyGraph** | 跨 Capability 的调用/数据依赖关系网 | 依赖分析工具 |

### 1.3 设计目标

1. **确定性优先**：能通过正则/AST 确定的东西绝不调用 LLM，降低成本和延迟
2. **可验证性**：行为推断的结果应附带证据（文件路径、行号、置信度）
3. **人机协作**：提供审核/纠正/反馈机制，人类最终决策
4. **渐进增强**：Schema 向后兼容，旧快照自动补全新字段

---

## 2. 系统架构

### 2.1 模块拓扑

```
                            CLI Commands
                     (snapshot / diff / coverage
                      regression / risk / review
                      version / graph / feedback
                      testmap)
                            │
                     ┌──────▼──────┐
                     │    Agent    │  调度中心：编排各层，管理存储
                     └──┬───┬───┬─┘
                        │   │   │
         ┌──────────────┤   │   ├──────────────────┐
         ▼              │   │                      ▼
   ┌─────────────┐      │   │              ┌──────────────┐
   │  analysis/  │      │   │              │  graph.rs    │
   │  (三层引擎)  │      │   │              │  依赖图索引   │
   └─────────────┘      │   │              └──────────────┘
         │              │   │
    ┌────┼────┐         │   │
    L1  L2   L3         │   │
         │              │   │
         ▼              ▼   ▼
   ┌──────────────────────────────────┐
   │           types.rs               │
   │    (所有核心数据类型定义)           │
   │  Capability / BehaviorSnapshot   │
   │  BehaviorContract / Dependency*  │
   │  BehaviorReview / TestMapping    │
   └──────────────────────────────────┘
         │              │
         ▼              ▼
   ┌──────────┐  ┌──────────┐
   │ storage  │  │ prompts  │
   │  JSON IO │  │  LLM模板  │
   └──────────┘  └──────────┘
```

### 2.2 数据流（端到端）

```
git diff (原始文本)
  │
  ▼
DiffPreprocessor::parse()
  │  按文件类型分组，解析 unified diff
  │  输出: Vec<FileChange>
  ▼
AstAnalyzer::analyze()
  │  L1: 6 语言正则匹配公开符号
  │  输出: Vec<RawChange>
  ▼
RuleEngine::evaluate()
  │  L2: 16 条语义规则标注
  │  输出: Vec<RuleMatch>
  ▼
LlmBridge::enhance()
  │  L3: 仅处理 L1 低置信度变更
  │  输出: Vec<LlmFragment>
  ▼
Agent::l1_changes_to_capabilities()
  │  聚合 RawChange + RuleMatch → Capability
  │  输出: Vec<Capability>
  ▼
Agent::create_snapshot()
  │  组装 BehaviorSnapshot, 可选 PRD 覆盖率计算
  │  输出: BehaviorSnapshot
  ▼
SnapshotStorage::save()
  │  写入 .Paporot/snapshots/{version_id}.json
  ▼
GraphStorage::update_from_snapshot()
  │  更新 .Paporot/graph/graph.json
```

### 2.3 存储布局

```
项目根目录/
  .Paporot/
    config.toml               # 用户配置
    snapshots/
      v1.json                 # 行为快照
      v2.json
    graph/
      graph.json              # 依赖图索引
    feedback/
      feedback.json           # P3 人机审查记录
    testmap/
      testmap.json            # P4 测试映射
```

---

## 3. 三层混合分析引擎 (P0)

### 3.1 设计动机

传统方案 100% 依赖 LLM 提取行为变更，存在三个问题：

| 问题 | 影响 |
|------|------|
| **成本高** | 每次 diff 都要调用 LLM API |
| **延迟大** | 网络往返 + 推理时间 |
| **不可靠** | LLM 输出不一致，可能漏检或误判 |

三层漏斗从确定性到概率性递进：

```
                  输入: git diff
                        │
              ┌─────────▼─────────┐
              │ L1: 确定性解析      │  覆盖率 ≈ 80%
              │ 零 LLM 调用         │  处理: 公开API/结构体/枚举等
              │ 置信度 = 1.0       │
              └─────────┬─────────┘
                        │ 残留变更
              ┌─────────▼─────────┐
              │ L2: 规则引擎       │
              │ 零 LLM 调用        │  标注: 安全风险/破坏性变更/性能
              └─────────┬─────────┘
                        │ 低置信度变更
              ┌─────────▼─────────┐
              │ L3: LLM 语义增强   │  补充: 复杂业务语义
              │ 仅处理少数边缘情况   │
              └───────────────────┘
```

### 3.2 L1: DiffPreprocessor + AstAnalyzer

#### DiffPreprocessor（`src/analysis/preprocessor.rs`）

解析 `git diff` unified 格式，按文件分组输出结构化变更：

```rust
pub struct FileChange {
    pub path: String,           // 文件路径
    pub language: Language,     // 根据扩展名推断
    pub kind: ChangeKind,       // Added/Deleted/Modified/Renamed
    pub hunks: Vec<Hunk>,       // 差异块
}

pub struct DiffSummary {
    pub files_changed: usize,
    pub additions: usize,
    pub deletions: usize,
    pub by_language: Vec<(Language, usize)>,
}
```

核心 API：

| 方法 | 输入 | 输出 | 用途 |
|------|------|------|------|
| `parse(diff_text)` | `&str` | `Vec<FileChange>` | 按文件拆分 diff |
| `summarize(changes)` | `&[FileChange]` | `DiffSummary` | 变更统计 |

#### AstAnalyzer（`src/analysis/l1_ast.rs`）

用正则表达式匹配公开符号，**不解析 AST**（避免引入语言特定 parser 依赖）。每种语言有独立的正则匹配器，输出统一的 `RawChange`。

**6 种语言 + 通用检测器**：

| 语言 | 检测的符号 | 公开性判定规则 | 文件扩展名 |
|------|-----------|---------------|-----------|
| Rust | `fn` / `struct` / `enum` / `trait` / `use` / `const` | 必须有 `pub` 关键字 | `.rs` |
| TypeScript | `function` / `class` / `import` / `export` | 必须有 `export` 关键字 | `.ts`, `.tsx` |
| JavaScript | `function` / `class` / `import` / `export` | 必须有 `export` 关键字 | `.js`, `.jsx`, `.mjs` |
| Python | `def` / `class` | 跳过 `_` 前缀（私有约定） | `.py` |
| Go | `func` / `type struct` | 首字母大写（Go 公开性约定） | `.go` |
| Java | `class` / method | 必须有 `public` 修饰符 | `.java` |
| 通用 | HTTP 路由（`.get(`/`.post(`...） | 全部捕获 | 所有文件 |
| 通用 | 配置项（`const`/`let` 赋值） | 全部捕获 | 所有文件 |

**L1 产出 —— `RawChange`**：

```rust
pub struct RawChange {
    pub id: String,                 // "rc_001"
    pub source: ChangeSource,       // Ast（L1 来源）
    pub change_type: ChangeType,    // FunctionAdded / FunctionRemoved / ...
    pub file_path: String,          // "src/auth.rs"
    pub language: Language,         // Rust
    pub line_start: usize,          // 42
    pub line_end: usize,            // 45
    pub symbol_name: String,        // "login_handler"
    pub old_signature: Option<String>,
    pub new_signature: Option<String>,
    pub confidence: f32,            // 0.0 ~ 1.0
    pub module: Option<String>,
    pub tags: Vec<String>,
}
```

**27 种变更类型（`ChangeType`）**：

| 大类 | 变体 |
|------|------|
| 函数级 | `FunctionAdded` / `FunctionRemoved` / `FunctionSignatureChanged` |
| 结构体 | `StructAdded` / `StructFieldAdded` / `StructFieldChanged` / `StructFieldRemoved` |
| 枚举 | `EnumAdded` / `EnumVariantAdded` / `EnumVariantRemoved` |
| Trait/接口 | `TraitAdded` / `TraitMethodAdded` / `TraitMethodChanged` |
| HTTP | `HttpRouteAdded` / `HttpRouteChanged` / `HttpRouteRemoved` |
| 依赖 | `ImportAdded` / `ImportRemoved` |
| 常量 | `ConstantAdded` / `ConstantChanged` / `ConstantRemoved` |
| 错误 | `ErrorVariantAdded` / `ErrorVariantRemoved` |
| 其他 | `ConfigFileChanged` / `DependencyVersionChanged` / `DocOnly` / `UnknownChange` |

其中 9 种为**破坏性变更**（`ChangeType::is_breaking() == true`）：

- `FunctionRemoved` / `FunctionSignatureChanged`
- `StructFieldRemoved`
- `EnumVariantRemoved`
- `TraitMethodChanged`
- `HttpRouteRemoved` / `HttpRouteChanged`
- `ConstantRemoved`
- `ImportRemoved`

### 3.3 L2: 规则引擎

`RuleEngine`（`src/analysis/l2_rules.rs`）对 L1 产出的每个 `RawChange` 逐一评估内置规则，纯确定性，**零 LLM 调用**。

**16 条内置规则，5 个类别**：

| 类别 | 规则数 | 规则 ID | 检测内容 |
|------|--------|---------|---------|
| **安全** | 5 | `sec_auth_001` | 认证相关符号变更（login/auth/authenticate/credential/token） |
| | | `sec_authz_001` | 授权/权限相关符号变更（authorize/permission/role/acl） |
| | | `sec_crypto_001` | 加密相关符号变更（encrypt/decrypt/hash/cipher/password） |
| | | `sec_injection_001` | SQL/命令注入风险（sql/query/execute/command + 字符串拼接特征） |
| | | `sec_secrets_001` | 密钥/token 环境变量变更（SECRET/TOKEN/KEY/PASSWORD env var） |
| **破坏性** | 5 | `breaking_001` | 公开函数删除（`ChangeTypeIn([FunctionRemoved])` + 非测试文件） |
| | | `breaking_signature_001` | 签名变更（`ChangeTypeIn([FunctionSignatureChanged])`） |
| | | `breaking_field_001` | 结构体字段删除（`ChangeTypeIn([StructFieldRemoved])` + 非测试文件） |
| | | `breaking_enum_001` | 枚举变体删除（`ChangeTypeIn([EnumVariantRemoved])`） |
| | | `breaking_const_001` | 公开常量删除（`ChangeTypeIn([ConstantRemoved])`） |
| **性能** | 2 | `perf_sql_001` | SQL 文件变更（`FilePathMatches("*.sql")`） |
| | | `perf_db_migration_001` | 数据库迁移文件（`FilePathMatches("*migration*")`） |
| **弃用** | 2 | `deprecated_001` | 包含 `deprecated` 标记的 diff |
| | | `wip_001` | 包含 TODO/FIXME/HACK 的 diff |
| **领域** | 2 | `domain_testing_001` | 测试代码识别（`FilePathMatches("*test*")` 或 `*_test.*`） |
| | | `domain_config_001` | 配置变更（`FilePathMatches("*config*")` 或 `.toml`/`.yaml`/`.json`） |

**规则引擎核心 —— `RuleTrigger` 组合逻辑**：

```rust
pub enum RuleTrigger {
    SymbolMatches { pattern: String },       // 符号名匹配
    ChangeTypeIn(Vec<ChangeType>),           // 变更类型匹配
    FilePathMatches { pattern: String },     // 文件路径匹配
    ContentContains { pattern: String },     // diff 内容匹配
    And(Box<RuleTrigger>, Box<RuleTrigger>), // 逻辑与
    Or(Box<RuleTrigger>, Box<RuleTrigger>),  // 逻辑或
    Not(Box<RuleTrigger>),                   // 逻辑非
}
```

**L2 产出 —— `RuleMatch`**：

```rust
pub struct RuleMatch {
    pub rule_id: String,         // "sec_auth_001"
    pub raw_change_id: String,   // 关联的 RawChange.id
    pub matched_tags: Vec<String>, // ["authentication", "security"]
    pub severity: Severity,      // Low / Medium / High
    pub category: RuleCategory,  // Security / Breaking / Performance / Deprecation / Domain
    pub description: String,
}
```

### 3.4 L3: LLM 桥接器

`LlmBridge`（`src/analysis/l3_llm_bridge.rs`）作为最后的语义补充层：

- **仅处理 L1 置信度 < 0.5 的变更**和 L1+L2 完全未覆盖的残留 diff
- 将残留 diff 和低置信度变更组装为 LLM prompt
- LLM 返回 JSON → 解析为 `BehaviorSnapshot` → 提取其中 `Capability` 列表
- `merge_fragments()` 函数纯确定性（JSON 解析合并），可独立测试

---

## 4. 行为合约化 (P1)

### 4.1 从文本到契约

P0 版的 `Capability` 只有纯文本 `description` 字段，无法程序化验证。P1 引入了结构化的**行为契约**。

### 4.2 BehaviorContract 枚举

```rust
#[serde(tag = "type")]
pub enum BehaviorContract {
    HttpEndpoint {
        method: String,           // GET / POST / PUT / DELETE
        path_template: String,    // /api/users/{id}
        auth_required: bool,
    },
    Function {
        name: String,             // handle_login
        visibility: String,       // public / private
        is_async: bool,
    },
    DataSchema {
        kind: SchemaKind,         // Struct / Enum / TypeAlias
        derives: Vec<String>,     // ["Debug", "Clone", "Serialize"]
    },
}
```

使用 serde 的 `#[serde(tag = "type")]` 序列化，JSON 中通过 `"type"` 字段区分变体：

```json
{ "type": "http_endpoint", "method": "POST", "path_template": "/api/login", "auth_required": true }
{ "type": "function", "name": "handle_login", "visibility": "public", "is_async": true }
{ "type": "data_schema", "kind": "struct", "derives": ["Debug", "Clone"] }
```

### 4.3 条件系统

每个 Capability 可附带三类条件：

```rust
pub struct Condition {
    pub kind: ConditionKind,    // Precondition / Postcondition / Invariant
    pub expression: String,     // "user.is_authenticated == true"
    pub severity: Severity,     // Low / Medium / High
}
```

### 4.4 能力类别

```rust
pub enum CapabilityCategory {
    Functional,      // 功能逻辑
    Security,        // 安全相关
    Performance,     // 性能相关
    Ux,              // 用户体验
    Operational,     // 运维/部署
    DataIntegrity,   // 数据完整性
}
```

---

## 5. 能力依赖图 (P2)

### 5.1 设计

依赖图独立于快照存储，维护在 `.Paporot/graph/graph.json` 中。每次快照保存后增量更新。

### 5.2 数据结构

```rust
pub struct DependencyGraph {
    pub edges: Vec<DependencyEdge>,       // 所有依赖边
    pub nodes: HashMap<String, NodeMeta>, // capability_id → 节点元数据
    pub evolution_chains: HashMap<String, Vec<String>>, // capability_id → 版本历史
}

pub struct DependencyEdge {
    pub from: CapabilityRef,
    pub to: CapabilityRef,
    pub relation: DependencyRelation,
    pub confidence: f32,
    pub source: RelationSource,
}
```

**7 种依赖关系类型（`DependencyRelation`）**：

| 变体 | 含义 | 示例 |
|------|------|------|
| `Calls` | 函数调用 | `login_handler()` 调用 `validate_token()` |
| `ConsumesEvent` | 消费事件 | Payment Service 消费 `OrderPlaced` 事件 |
| `ReadsData` | 读取数据 | `get_user()` 读取 `users` 表 |
| `WritesData` | 写入数据 | `create_order()` 写入 `orders` 表 |
| `PostconditionDepends` | 后置条件依赖 | B 的前置条件 = A 的后置条件 |
| `SharesState` | 共享状态 | 两个 handler 共享同一个全局状态 |
| `ImplementsOrComposes` | 实现或组合 | `UserService` 实现 `AuthTrait` |

**4 种关系来源（`RelationSource`）**：`AstInferred` / `RuleInferred` / `LlmInferred` / `Manual`

### 5.3 核心查询 API

| API | 功能 | 算法 |
|-----|------|------|
| `impact_analysis(capability_id)` | 查询下游所有被影响的能力 | DFS 遍历出边 |
| `evolution_trace(capability_id)` | 追溯能力在各快照版本中的变化 | evolution_chains 查表 |
| `detect_cycles()` | 检测循环依赖 | DFS + 三色标记 |

### 5.4 Capability 的依赖字段

```rust
pub struct Capability {
    // P2 新增:
    pub depends_on: Vec<DependsOn>,     // 我依赖的上游
    pub depended_by: Vec<DependedBy>,   // 依赖我的下游（由 graph 自动填充）
    pub evolved_from: Option<CapabilityRef>, // 跨快照演化链
}
```

---

## 6. 人机验证回路 (P3)

### 6.1 设计动机

AI 生成的行为推断（尽管有多层验证）仍可能出现：

- **误报**：将无关代码变更识别为新的行为
- **漏报**：未能识别重要的行为变更
- **分类错误**：将破坏性变更标记为非破坏性

P3 提供了结构化的人工审核机制。

### 6.2 审查裁决

```rust
pub enum ReviewVerdict {
    Approved,   // 确认正确
    Rejected,   // 标记为误报
    Corrected,  // 修正后接受（附带 corrected: Capability）
    Flagged,    // 无法判断，标记为待定
}
```

### 6.3 反馈存储

```rust
pub struct FeedbackStore {
    pub reviews: Vec<BehaviorReview>,
    pub stats: FeedbackStats,      // total / approved / rejected / corrected / flagged
}
```

API：`add_review()` / `reviews_for(capability_id)` / `load_or_new()` / `save()`

### 6.4 CLI 反馈命令

```
paporot feedback approve <capability_id> [--comment "reason"]
paporot feedback reject <capability_id> [--comment "reason"]
paporot feedback correct <capability_id> [--corrected-json <path>]
paporot feedback flag <capability_id> [--comment "reason"]
paporot feedback show [--capability <id>]
paporot feedback stats
```

---

## 7. 行为-测试闭环 (P4)

### 7.1 核心概念

将行为 Capability 与对应的测试代码建立双向映射：

```
Capability "JWT Login API"  ←→  tests/auth/login_test.rs::test_jwt_valid_token
                                     tests/auth/login_test.rs::test_jwt_expired_token
```

### 7.2 测试映射

```rust
pub struct TestMapping {
    pub map_id: String,
    pub capability_id: String,
    pub test_file: String,          // 测试文件路径
    pub test_name: String,          // 测试函数名
    pub framework: Option<String>,  // cargo-test / pytest / jest / go-test
    pub test_status: TestStatus,    // Passing / Failing / Unknown / Missing
    pub confidence: f32,
    pub source: TestMappingSource,  // FileName / NameConvention / LLM / Manual
}
```

**映射来源**：

| 来源 | 说明 |
|------|------|
| `FileName` | 从 diff 文件名推断：`src/auth.rs` 变更 → 关联 `tests/auth_test.rs` |
| `NameConvention` | 从命名约定推断：`test_login_success` → 关联 `login` 能力 |
| `Llm` | LLM 推断 |
| `Manual` | 人工设定 |

### 7.3 测试框架自动推断

根据文件扩展名自动识别测试框架：

| 测试文件模式 | 推断框架 |
|-------------|---------|
| `*_test.rs` / `test_*.rs` / `tests/*.rs` | `cargo-test` |
| `test_*.py` / `*_test.py` / `tests/test_*.py` | `pytest` |
| `*.test.ts` / `*.spec.ts` / `__tests__/*.ts` | `jest` |
| `*_test.go` | `go-test` |
| `*Test.java` / `*Tests.java` | `junit` |

### 7.4 测试文件推断

从测试文件路径反向推断源代码路径：

```
tests/auth/login_test.rs → src/auth/login.rs
tests/auth/LoginTest.java → src/auth/Login.java
test_user_service.py → user_service.py
```

### 7.5 CLI 测试映射命令

```
paporot testmap scan [--diff <range>]     # 从 diff 扫描测试文件
paporot testmap add <cap_id> <test_file> <test_name>  # 手动添加
paporot testmap show [--capability <id>]  # 查看映射
paporot testmap stats                     # 统计覆盖率
paporot testmap verify [--capability <id>] # 验证映射有效性
```

---

## 8. CLI 命令系统

### 8.1 命令总览

| 命令 | 文件 | 功能 | 是否调用 LLM |
|------|------|------|-------------|
| `snapshot create` | `commands/snapshot.rs` | 创建行为快照 | 是（可配置 L1+L2 预处理） |
| `diff` | `commands/diff.rs` | 对比两个快照 | 否（纯确定性） |
| `coverage` | `commands/coverage.rs` | PRD 覆盖率分析 | 是 |
| `regression` | `commands/regression.rs` | 检测功能回归 | 是 |
| `risk` | `commands/risk.rs` | 风险评估 | 是 |
| `review` | `commands/review.rs` | 完整审查流水线 | 是 |
| `graph` | `commands/graph.rs` | 依赖图查询 | 否 |
| `feedback` | `commands/feedback.rs` | 人工审查记录 | 否 |
| `testmap` | `commands/testmap.rs` | 测试映射管理 | 否 |
| `version` | `commands/version.rs` | 版本信息 | 否 |
| `status` | `commands/version.rs` | 当前状态 | 否 |

### 8.2 审查流水线

`review` 命令整合了完整流水线：

```
1. 获取 git diff         → 确认变更范围
2. L1 确定性提取         → 公开符号变更
3. L2 规则标注           → 安全/破坏性标签
4. 可选: L3 LLM 补充     → 语义增强
5. 创建 BehaviorSnapshot  → 持久化
6. 可选: PRD 覆盖率计算   → 功能完整性
7. 可选: 回归检测         → 能力是否退化
8. 可选: 风险评估         → 整体风险评分
9. 更新依赖图             → GraphStorage
```

---

## 9. 存储与持久化

### 9.1 配置管理（`src/config.rs`）

```rust
pub struct Config {
    pub llm: LlmConfig,         // 模型/API key/超时/重试
    pub storage: StorageConfig, // 快照存储目录
    pub agent: AgentConfig,     // diff 阈值/截断
}
```

支持三层优先级：环境变量 > `.Paporot/config.toml` > 默认值。

### 9.2 快照持久化（`src/storage.rs`）

```rust
pub struct SnapshotStorage {
    dir: PathBuf,  // ".Paporot/snapshots/"
}

// API:
fn save(&self, snapshot: &BehaviorSnapshot) -> Result<PathBuf>
fn load_by_version(&self, version_id: &str) -> Result<BehaviorSnapshot>
fn load_latest(&self) -> Result<BehaviorSnapshot>
fn list_versions_sorted(&self) -> Result<Vec<String>>
fn next_version_id(&self) -> Result<String>     // v1 → v2 → v3 ...
```

---

## 10. Schema 版本策略

### 10.1 版本历史

| 版本 | 变更内容 | `schema_version` |
|------|---------|-----------------|
| v1 | 基础快照：纯文本 Capability | 1（旧文件无此字段） |
| v2 | P1: + contract / preconditions / postconditions / invariants / categories | 2 |
| v3 | P2: + depends_on / depended_by / evolved_from / verified_by / verified_at | 3 |

### 10.2 向后兼容

所有新增字段使用 `#[serde(default)]` 注解：

```rust
pub struct BehaviorSnapshot {
    #[serde(default = "default_schema_version")]  // 旧文件缺失时默认为 3
    pub schema_version: u32,
    // ...
}

pub struct Capability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<BehaviorContract>,  // 旧快照为 None
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<DependsOn>,          // 旧快照为空数组
    // ...
}
```

旧 v1 快照 JSON 可以直接被当前版本加载，缺失字段自动补全为默认值。

---

## 11. 测试体系

### 11.1 测试策略

```
                        系统级测试 (4)
                       / 完整 Agent pipeline
                      /  多文件 L1 全链路
                     /   SchemaVersion 兼容
                    /    Contract 三变体
                   /
          集成测试 (21)
         /  L1+L2 全流水线
        /   安全规则联动
       /    依赖图操作
      /     序列化/兼容性
     /      DiffPreprocessor 边界
    /
  单元测试 (112) ── 每个 pub fn 都有独立测试
                   测试代码在对应源文件的 #[cfg(test)] 块中
```

### 11.2 测试覆盖矩阵

| 源文件 | 测试数 | 测试的接口/功能 |
|--------|--------|---------------|
| `src/analysis/l1_ast.rs` | 18 | Rust fn/struct/enum/trait/use/const, TS/JS fn/class/import, Python def/class, Go func/struct, HTTP route，L1+L2 集成 ×2 |
| `src/analysis/l2_rules.rs` | 4 | 安全规则命中、破坏性规则、测试文件标签、普通函数不误报 |
| `src/analysis/preprocessor.rs` | 3 | parse 单文件、parse 多文件、summarize |
| `src/analysis/types.rs` | 7 | Language::from_extension/from_filename, ChangeType::is_breaking(×28变体), label(×27变体), RawChange 构造, RuleTrigger 组合(×2) |
| `src/analysis/l3_llm_bridge.rs` | 4 | merge_fragments: 空/非JSON/有效JSON/多片段 |
| `src/agent.rs` | 12 | compute_diff(×6), truncate_diff(×3), l1_changes_to_capabilities(×5) |
| `src/types.rs` | 8 | 快照序列化, P3 review(×3), P4 testmap(×3), status_name |
| `src/graph.rs` | 3 | 保存加载, 循环检测, 无假阳性 |
| `src/storage.rs` | 2 | 保存加载, next_version_id |
| `src/config.rs` | 2 | default_config, sample toml |
| `src/prompts.rs` | 3 | extraction prompt min/full, diff prompt |
| `src/llm/client.rs` | 2 | JSON code block 提取, plain JSON 提取 |
| `src/commands/graph.rs` | 6 | impact_analysis(×2), evolution_trace(×2), module_query(×2) |
| `src/commands/feedback.rs` | 6 | approve/reject/correct/flag, reviews_for, 持久化往返 |
| `src/commands/testmap.rs` | 9 | extract_test_file(×4), infer_source(×2), infer_framework(×4), add/map/stats/scan/往返 |
| `src/commands/diff.rs` | 1 | agent.compute_diff 完整分类 |
| `src/commands/version.rs` | 6 | mask_api_key(×4), CARGO_PKG_VERSION, CARGO_PKG_NAME |
| `src/commands/review.rs` | 3 | Agent 结构体完整性, diff_range 默认值, 空 diff 检测 |
| `src/commands/coverage.rs` | 1 | coverage_icon 四种状态映射 |
| `src/commands/risk.rs` | 2 | 空存储、单版本回退（带隔离存储） |
| `src/commands/regression.rs` | 2 | 版本不足、prev/curr 选出（带隔离存储） |
| `src/commands/snapshot.rs` | 4 | Agent 持有 storage, diff_range 格式, next_version_id 格式, 阈值 |
| `tests/integration_tests.rs` | 21 | L1+L2 流水线 ×4, 安全规则联动 ×3, 依赖图操作 ×3, 序列化 ×3, 兼容性 ×2, 边界 ×4, 系统级 ×4 |
| **总计** | **134** | + 1 个文档测试 |

### 11.3 测试隔离

- 单元测试使用纯函数或独立 temp 目录（`isolated_config()` 生成 PID 唯一的临时路径）
- 无外部依赖（git / LLM API 均不参与单元测试）

---

## 12. 依赖关系

### 12.1 外部依赖

| 依赖 | 用途 |
|------|------|
| `clap 4 (derive)` | CLI 命令行解析 |
| `serde 1` / `serde_json 1` | JSON 序列化/反序列化 |
| `chrono 0.4` | 时间戳生成 |
| `reqwest 0.12` | LLM API HTTP 调用 |
| `tokio 1` | 异步运行时 |
| `anyhow 1` | 错误处理 |
| `uuid 1 (v4)` | 唯一 ID 生成 |
| `regex 1` | L1 正则匹配 |
| `toml 0.8` | 配置文件解析 |
| `dirs 5` | 跨平台配置目录查找 |
| `colored 2` | 终端彩色输出 |

### 12.2 项目结构

```
Paporot/
├── Cargo.toml
├── src/
│   ├── main.rs                # CLI 入口
│   ├── lib.rs                 # 库入口（供 integration tests）
│   ├── cli.rs                 # clap 子命令定义
│   ├── config.rs              # 配置管理
│   ├── types.rs               # 核心数据类型（600+ 行）
│   ├── agent.rs               # 调度中心
│   ├── storage.rs             # 快照持久化
│   ├── graph.rs               # 依赖图存储
│   ├── prompts.rs             # LLM prompt 模板
│   ├── analysis/
│   │   ├── mod.rs
│   │   ├── types.rs           # 分析层内部类型
│   │   ├── preprocessor.rs    # DiffPreprocessor
│   │   ├── l1_ast.rs          # AstAnalyzer（900+ 行，6 语言正则）
│   │   ├── l2_rules.rs        # RuleEngine（16 条内置规则）
│   │   └── l3_llm_bridge.rs   # LlmBridge
│   ├── llm/
│   │   ├── mod.rs
│   │   └── client.rs          # OpenAI 兼容 HTTP 客户端
│   └── commands/
│       ├── mod.rs
│       ├── snapshot.rs
│       ├── diff.rs
│       ├── coverage.rs
│       ├── regression.rs
│       ├── risk.rs
│       ├── review.rs
│       ├── graph.rs
│       ├── feedback.rs
│       ├── testmap.rs
│       └── version.rs
├── tests/
│   └── integration_tests.rs   # 21 个集成测试
└── docs/
    ├── DESIGN.md              # 本文档
    └── TEST_SPEC.md           # 测试详细说明
```

---

*文档版本: 2026-06-11 | 项目版本: 0.1.0 | 测试: 134 passed / 0 failed*
