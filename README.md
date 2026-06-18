# Paporot

**AI 生成代码的行为审计与沙盒化分析管道**

Paporot 是一个命令行工具，回答 AI Agent 写代码时两个最基本的问题：

1. **这次 Agent 改了什么能力？**（Capability Version Control）
2. **Agent 的行为变好还是变坏了？**（Behavior Version Control）

当前已实现 **WASM 沙盒分析管道**：通过 6 个可组合 Skill 对项目进行多维度审计，生成 JSON / Markdown / HTML 三份报告。

---

## 快速开始

```bash
# 前置: Rust 1.96+, wasm32-wasip1 target
rustup target add wasm32-wasip1

# 编译
cargo build --manifest-path crates/paporot-core/Cargo.toml --target wasm32-wasip1 --release
cargo build --release

# 列出现有 Skill
./target/release/Paporot skill list

# 运行完整分析
./target/release/Paporot analyze

# 指定 PRD + 输入文件
./target/release/Paporot analyze --prd docs/prd.md --input src/main.rs
```

分析结果输出到 `.Paporot/reports/`：
- `analysis_result.json` — 结构化 JSON
- `architecture.md` — Markdown 报告
- `dashboard.html` — 暗色主题可视化面板

---

## 架构

分析逻辑完全在 WASM 沙盒内运行，只能通过 3 个受控的 host function 与外部交互。

```
┌────────────────────────────────────────────┐
│           Paporot (Native Binary)          │
│                                            │
│  ┌──────────────────────────────────────┐  │
│  │       wasmtime Runtime Engine        │  │
│  │                                      │  │
│  │  ┌────────────────────────────────┐  │  │
│  │  │  paporot-core.wasm (Sandbox)   │  │  │
│  │  │                                │  │  │
│  │  │  • CLI 入口                    │  │  │
│  │  │  • pipeline (Skill扫描/DAG/报告)│  │  │
│  │  │  • host (FFI 绑定)             │  │  │
│  │  └────────────────────────────────┘  │  │
│  │                                      │  │
│  │  Host Functions:                     │  │
│  │  1. host_read_file  (受控文件读)     │  │
│  │  2. host_write_file (受控文件写)     │  │
│  │  3. host_llm_call   (LLM 推理)      │  │
│  └──────────────────────────────────────┘  │
└────────────────────────────────────────────┘
```

### 两级架构

| 层级 | 组件 | Target | 职责 |
|------|------|--------|------|
| 宿主层 | `src/main.rs` | native (x86_64-linux) | 加载 .wasm、注册 host function、转发 CLI 参数 |
| 沙盒层 | `crates/paporot-core/` | wasm32-wasip1 | 扫描 Skill → DAG 编排 → 执行分析 → 写报告 |

### Host Function 签名

所有数据通过 WASM 线性内存的 packed pointer 传递：

| 函数 | 签名 | 说明 |
|------|------|------|
| `host_read_file` | `(path_ptr, path_len) → (ptr<<32)\|len` | 读取 .Paporot/ 内文件，带路径遍历防护 |
| `host_write_file` | `(path_ptr, path_len, data_ptr, data_len) → errno` | 写入 .Paporot/ 内文件 |
| `host_llm_call` | `(prompt_ptr, prompt_len, schema_ptr, schema_len) → (ptr<<32)\|len` | 调用 LLM API |

---

## Skill 体系

每个 Skill 由 `.Paporot/skills/<name>/skill.toml` 定义，声明输入、输出、依赖和 LLM 预算。

### 内置 6 个 Skill（按 DAG 层排列）

| Skill | 职责 | DAG 层 |
|-------|------|--------|
| `repository-understanding` | 识别项目目标、技术栈、入口 | Layer 1 |
| `module-discovery` | 发现业务/技术模块，聚类文件 | Layer 2 |
| `dependency-analysis` | 构建依赖图、计算耦合指标 | Layer 3 |
| `runtime-flow-analysis` | 端到端执行路径追踪 | Layer 4 |
| `behavior-boundary-discovery` | 行为组件边界发现 | Layer 5 |
| `architecture-doc-generator` | 聚合上游输出，生成架构文档 | Layer 6 |

### DAG 编排规则

- **依赖声明**：Skill 通过 `[dependencies] uses_outputs_from` 声明对上游 Skill 输出的依赖
- **拓扑排序**：自动构建 DAG 并分层执行
- **失败传播**：上游 Skill 失败时，依赖它的下游 Skill 会被跳过
- **输出缓存**：Skill 的输出按名称缓存在 HashMap 中，下游可引用

---

## 目录结构

```
Paporot/
├── Cargo.toml                    # Native binary 包描述
├── src/
│   ├── main.rs                   # Native loader 入口 (wasmtime + WASI)
│   ├── config.rs                 # Config/LlmConfig 结构体
│   └── ...
│
├── crates/
│   ├── paporot-core/             # WASM 沙盒核心
│   │   ├── Cargo.toml            # wasm32-wasip1 target
│   │   └── src/
│   │       ├── main.rs           # 沙盒 CLI 入口
│   │       ├── lib.rs            # 模块导出
│   │       ├── pipeline.rs       # 分析管线
│   │       └── host.rs           # Host function FFI 绑定
│   └── skills/                   # Skill 源码 + WASM (原有)
│
├── .Paporot/                     # 项目数据目录
│   ├── bin/                      # paporot-core.wasm 部署位置
│   ├── config.toml               # LLM 配置 (可选)
│   ├── skills/                   # 已安装的 Skill
│   │   ├── repository-understanding/
│   │   │   ├── skill.toml
│   │   │   └── skill.wasm
│   │   ├── module-discovery/
│   │   ├── dependency-analysis/
│   │   ├── runtime-flow-analysis/
│   │   ├── behavior-boundary-discovery/
│   │   └── architecture-doc-generator/
│   ├── reports/                  # 分析报告输出
│   │   ├── analysis_result.json
│   │   ├── architecture.md
│   │   └── dashboard.html
│   └── work/                     # 临时工作文件
│
└── docs/
    └── architecture.md           # 架构详细文档
```

---

## 典型使用场景

### 场景 1：Agent 提交了代码，我想知道改了什么

```bash
paporot review
```

输出：

```
Capability Changes:
  cap_001  Authentication (Modified)  ── login() 签名变了
  cap_002  API Rate Limit  (New)      ── 新增了 check_rate_limit()

Risk: Medium  ── 认证模块的破坏性变更可能影响下游
Dependencies affected: cap_003 (User Profile)
```

如果你们有 PRD，加上 `-p docs/prd.md` 还能看到需求覆盖了百分之几。

### 场景 2：同一个 Bug，Agent 修了两次，行为不一样了

```bash
paporot trace import ~/.claude/sessions/session_001.json
paporot trace import ~/.claude/sessions/session_002.json
paporot trajectory diff --trace-a trace_001 --trace-b trace_002
```

输出工具调用数、阶段变更、Gantt 图等对比信息。如果"验证"阶段 tool 调用暴增，自动标记为退化风险。

### 场景 3：多个版本后，我想看趋势

```bash
paporot diff --from v1 --to v5
paporot regression --from v1 --to v5
```

Dashboard 四个标签页：Evidence Explorer / Capability Graph / Behavior Eval / Trajectory Diff。

---

## 命令速查

### Capability 版本控制

```bash
paporot snapshot create -m "描述这次改动"
paporot snapshot create -m "..." -p prd.md
paporot diff --from v1 --to v2
paporot diff --from v1 --to v2 --format mermaid
paporot coverage -p prd.md
paporot regression --from v1 --to v2
paporot risk
paporot review -p prd.md
paporot status
```

### 执行轨迹追踪

```bash
paporot trace import agent_log.json
paporot trace import session.json --adapter claude-code
paporot trace list
paporot trace list --cap cap_auth --from 2026-06-01
paporot trace show <trace_id>
paporot trace link <trace_id> --cap cap_auth
paporot trace adapter list
```

支持的 Agent 格式：DeepSeek · Claude Code · OpenAI Chat Completion

### 行为版本控制（Trajectory Diff & Eval）

```bash
paporot trajectory diff --trace-a <id> --trace-b <id>
paporot trajectory diff --capability cap_auth
paporot trajectory diff --format json | mermaid
paporot trajectory list
paporot trajectory show <diff_id>
```

### 依赖图

```bash
paporot graph show
paporot graph show --capability cap_auth --depth 2
paporot graph impact --capability cap_auth
paporot graph evolution --capability cap_auth
paporot graph cycles
```

### 人工反馈

```bash
paporot feedback approve cap_001
paporot feedback reject cap_001 -r "误报"
paporot feedback stats
```

---

## 三层行为版本控制（设计蓝图）

```
P0: State Machine (结构层)
    BehaviorTrace → BehaviorStateGraph
         │
         │  StateFeatures + TransitionEventLog
         ▼
P1: Statistical Vector (几何层)
    StateGraph → TrajectoryVector (10 维数值向量)
         │
         │  TrajectoryVector + co-change 证据
         ▼
P2: Coupling Graph (网络层)
    Vectors → capability 行为耦合图
```

每层独立 decision logic：
- **P0** 用 features 做判决（merge/split, threshold-based）
- **P1** 用相同 features 做测量（projection, no threshold）
- **P2** 用 co-change 定义边存在性，用 similarity 调制强度

### P0：行为状态机

```bash
paporot state build --trace <id>
paporot state show <trace_id>
paporot state diff --trace-a <a> --trace-b <b>
paporot state eval --trace <id>
```

### P1：轨迹向量（10 维）

```bash
paporot trajectory-vector build --trace <id>
paporot trajectory-vector diff --v1 v1.json --v2 v2.json
paporot trajectory-vector cluster --traces t1 t2 t3 t4
paporot trajectory-vector anomaly --traces t1 t2 t3 t4 t5
```

| 字段 | 含义 | 类型 |
|------|------|------|
| `tool_entropy` | 工具序列熵 | f32 |
| `phase_entropy` | 状态路径熵 | f32 |
| `transition_entropy` | 图结构熵 | f32 |
| `loop_ratio` | 状态级循环占比 | f32 |
| `backtrack_ratio` | 时间回退占比 | f32 |
| `burst_ratio` | 工具密度聚集程度 | f32 |
| `state_stability_score` | 相邻状态连续性 | f32 |
| `tool_distribution` | 工具类别分布 | SparseVector |
| `state_distribution` | 状态阶段分布 | SparseVector |
| `edit_intensity_curve` | 编辑强度曲线 | Vec\<f32\> |

### P2：行为耦合图

```bash
paporot coupling build --pairs trace1:cap_auth trace2:cap_auth trace3:cap_profile
paporot coupling analyze --cap cap_auth --pairs ...
paporot coupling export --pairs ... --format mermaid
paporot coupling impact --cap cap_auth --pairs ...
```

---

## 配置

在项目根目录创建 `.Paporot/config.toml`（可选）：

```toml
[llm]
api_key = ""                      # 或用环境变量 Paporot_API_KEY
model = "deepseek-chat"
endpoint = ""                     # 留空使用默认 https://api.deepseek.com/v1/chat/completions
temperature = 0.3
max_tokens = 4096

[trace]
auto_redact = false
redact_auth_header = true
redact_api_keys = true
```

环境变量：

```bash
export Paporot_API_KEY="sk-xxxxxxxxxxxxxxxx"
```

> 未配置 LLM 时，分析仍可运行，Skill 会返回 stub 输出。

---

## 安全模型

- **文件隔离**：沙盒只能访问 `.Paporot/` 目录内的文件，host function 内置 canonicalize + starts_with 路径遍历防护
- **网络限制**：仅 `host_llm_call` 可发起 HTTPS 请求，只能调用配置中的 LLM endpoint
- **内存安全**：WASM 线性内存由 wasmtime 管理，写入前自动 grow，OOB 访问触发 wasm trap

---

## 运行测试

```bash
cargo test
cargo test --lib
cargo test --test integration_tests
cargo test trajectory::p1
cargo test trajectory::p2
cargo test trajectory
```

## 文档

| 文档 | 路径 |
|------|------|
| 架构与用户手册 | `docs/architecture.md` |
| P0 状态机 PRD | `docs/prd/PRD_P0_STATE_MACHINE.md` |
| P1 轨迹向量 PRD | `docs/prd/PRD_P1_STV.md` |
| P2 耦合图 PRD | `docs/prd/PRD_P2_BCG.md` |
| P1+P2 测试说明书 | `docs/tests/TEST_SPEC_P1_P2.md` |
| P1+P2 测试报告 | `docs/tests/TEST_REPORT_P1_P2.md` |

---

## License

MIT
