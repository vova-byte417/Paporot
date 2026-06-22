# Paporot

**AI 生成代码的行为审计与沙盒化分析管道**

Paporot 是一个高度自动化的命令行工具，回答 AI Agent 写代码时两个最基本的问题：

1. **这次 Agent 改了什么能力？**（Capability Version Control）
2. **Agent 的行为变好还是变坏了？**（Behavior Version Control）

核心亮点：
- **完全自动化** — AI Agent 提交代码后自动触发全管道分析，无需人工干预
- **Skill 编排** — 6 个可组合的 WASM Skill 按 DAG 依赖自动编排执行，每个 Skill 完成一个分析维度
- **总指挥 Skill** — `architecture-doc-generator` 作为总组织者，聚合所有上游 Skill 输出，统一生成报告
- **WASM 沙盒隔离** — 所有分析逻辑在 wasmtime 沙盒内运行，仅通过 3 个受控 host function 与外部交互
- **三层行为版本控制** — 从结构层(P0 状态机) → 几何层(P1 轨迹向量) → 网络层(P2 行为耦合图) 逐层深入

---

## 项目架构

![Paporot Architecture](docs/paporot-architecture.png)

> 架构图源文件：`docs/paporot-architecture.excalidraw`（可用 Excalidraw 打开编辑）

### 架构概览

Paporot 采用 **两级架构**，通过 wasmtime 运行时将分析逻辑隔离在 WASM 沙盒内：

| 层级 | 组件 | 编译目标 | 职责 |
|------|------|----------|------|
| **宿主层** | `src/main.rs` + 命令模块 | native | 加载 .wasm、注册 3 个 host function、转发 CLI 参数 |
| **沙盒层** | `crates/paporot-core/` | wasm32-wasip1 | Skill 扫描 → DAG 拓扑排序 → 逐层执行 → 报告生成 |

### 模块关系

```
src/
├── main.rs               ← 宿主入口：加载 WASM + 注册 host functions + 转发 CLI
├── cli.rs                ← CLI 命令行定义 (16 个子命令)
├── config.rs             ← LLM 配置解析
│
├── commands/             ← 16 个分析命令的实现
│   ├── analyze.rs        ← 运行完整 Skill 分析管道 (← 核心入口)
│   ├── trace.rs          ← 执行轨迹导入与管理
│   ├── trajectory.rs     ← 行为轨迹对比 (diff)
│   ├── state.rs          ← P0 状态机构建
│   ├── trajectory_vector.rs ← P1 轨迹向量
│   ├── coupling.rs       ← P2 行为耦合图
│   ├── snapshot.rs       ← 能力快照
│   ├── diff.rs           ← 版本差异
│   ├── coverage.rs       ← PRD 覆盖率
│   ├── regression.rs     ← 回归检测
│   ├── risk.rs           ← 风险评估
│   ├── review.rs         ← 整合审查
│   ├── graph.rs          ← 依赖图
│   ├── feedback.rs       ← 人机验证回路
│   └── ...
│
├── trace/                ← Agent 日志适配器（DeepSeek / Claude / OpenAI）
├── trajectory/           ← 三层行为版本控制（P0/P1/P2 全部子模块）
│   ├── state/            ← P0 行为状态机
│   ├── p1/               ← P1 轨迹向量（特征提取/聚类/时序）
│   ├── p2/               ← P2 行为耦合图（co-change/相似度/图构建）
│   ├── align/            ← 轨迹对齐引擎
│   ├── evaler/           ← 评估器（状态/转换/图评估）
│   └── projection/       ← 状态到差异投影
│
├── analysis/             ← L1 AST / L2 规则 / L3 LLM 桥接
├── evidence/             ← 能力证据收集与置信度评估
├── evaler/               ← 通用评估（规则/趋势）
├── llm/                  ← DeepSeek API 客户端
├── report/               ← 报告生成器（JSON / MD / HTML Dashboard）
├── skills/               ← Skill 运行与管理（注册表/DAG/WASM host）
├── storage.rs            ← SQLite 持久化
└── agent.rs              ← Agent 抽象
```

### WASM 沙盒隔离模型

沙盒内的 `paporot-core.wasm` 只能通过 3 个受控 host function 与外部交互：

| Host Function | 说明 | 安全约束 |
|---------------|------|----------|
| `host_read_file` | 读取文件 | 仅允许 `.Paporot/` 目录内，路径遍历防护 |
| `host_write_file` | 写入文件 | 仅允许 `.Paporot/` 目录内 |
| `host_llm_call` | LLM 推理 | 仅调用配置的 endpoint |

### 自动化的 Skill 编排管道

分析过程完全自动化，无需人工干预。管道由 **DAG 依赖拓扑排序** 驱动：

```
触发 (AI Agent 提交代码)
  │
  ▼
┌─────────────────────────────────────────────────┐
│  L1  repository-understanding                   │  识别项目目标、技术栈、入口
│  L2  module-discovery                           │  发现业务/技术模块，聚类文件
│  L3  dependency-analysis                        │  构建依赖图、计算耦合指标
│  L4  runtime-flow-analysis                      │  端到端执行路径追踪
│  L5  behavior-boundary-discovery                │  行为组件边界发现
│  L6  architecture-doc-generator  ★ 总指挥       │  聚合上游输出 → 生成 JSON/MD/HTML
└─────────────────────────────────────────────────┘
  │
  ▼
输出报告 (.Paporot/reports/)
  ├── analysis_result.json    ← 结构化机器可读数据
  ├── architecture.md         ← 开发者可读架构报告
  └── dashboard.html          ← 暗色主题交互式可视化面板
```

> **总指挥 Skill** (`architecture-doc-generator`, L6)：作为管道的最后一层，它不独立分析，而是消费 L1-L5 所有上游 Skill 的输出，负责：
> - 整合所有分析维度（项目概要、模块划分、依赖关系、执行流程、行为边界）
> - 生成三种格式的报告（JSON / Markdown / HTML Dashboard）
> - 统一风险评估与建议输出

---

## 三层行为版本控制

当 Agent 多次执行同一任务时，Paporot 能自动检测行为变化——Agent 用了更多工具调用？执行了更复杂的流程？行为模式退化了吗？

```
P0: 行为状态机 (结构层)
    BehaviorTrace → BehaviorStateGraph
          │  状态分割 / 合并判决 (threshold-based)
          ▼
P1: 轨迹向量 (几何层)
    StateGraph → 10维 TrajectoryVector
          │  序列熵 / 循环占比 / 突发性 (projection)
          ▼
P2: 行为耦合图 (网络层)
    Vectors → Capability Coupling Graph
          co-change 边定义 + similarity 调制强度
```

| 层 | 命令 | 核心指标 |
|----|------|----------|
| **P0** | `paporot state build/eval` | 状态分割、合并判决、图结构差异 |
| **P1** | `paporot trajectory-vector` | 10 维向量（tool_entropy, phase_entropy, transition_entropy, loop_ratio, backtrack_ratio, burst_ratio, state_stability_score 等） |
| **P2** | `paporot coupling` | co-change 频率、capability 间耦合强度 |

P0 用 features 做判决（merge/split, threshold-based），P1 用相同 features 做测量（projection, no threshold），P2 用 co-change 定义边存在性，用 similarity 调制强度。

---

## 快速开始

### 前置条件

- **Rust** 1.96+ 工具链（在 WSL 中使用）
- **wasm32-wasip1** target：
  ```bash
  rustup target add wasm32-wasip1
  ```
- **LLM API Key**（可选，未配置时 Skill 返回 stub 输出）

### 编译 Release

```bash
# Step 1: 编译 WASM 沙盒核心
cargo build --manifest-path crates/paporot-core/Cargo.toml --target wasm32-wasip1 --release

# Step 2: 编译 native loader
cargo build --release

# Step 3: 部署 WASM 到项目数据目录
mkdir -p .Paporot/bin
cp crates/paporot-core/target/wasm32-wasip1/release/paporot-core.wasm .Paporot/bin/
```

构建产物：
- `target/release/Paporot` — Native 可执行文件（可直接分发）
- `crates/paporot-core/target/wasm32-wasip1/release/paporot-core.wasm` — 沙盒 WASM

### 配置 LLM

在项目根目录创建 `.Paporot/config.toml`：

```toml
[llm]
api_key = ""                      # 留空从环境变量 PAPOROT_API_KEY 读取
model = "deepseek-chat"
endpoint = ""                     # 留空使用默认 endpoint
temperature = 0.3
max_tokens = 4096
```

或直接设置环境变量：

```bash
export PAPOROT_API_KEY="sk-xxxxxxxxxxxxxxxx"
```

> 未配置 LLM 时分析仍可运行，Skill 会返回 stub 输出。

---

## 使用说明

### 1. 运行完整分析（推荐首选）

```bash
# 基本分析
./target/release/Paporot analyze

# 指定 PRD + 输入文件
./target/release/Paporot analyze --prd docs/prd.md --input src/main.rs
```

自动执行 6 个 Skill（按 DAG 顺序）并生成报告到 `.Paporot/reports/`：
- `analysis_result.json` — 结构化 JSON 数据
- `architecture.md` — Markdown 架构报告
- `dashboard.html` — 暗色主题可视化面板（含 Skill DAG、风险等级、执行状态）

### 2. 查看已安装的 Skill

```bash
./target/release/Paporot skill list
```

### 3. 行为版本控制

```bash
# 创建快照
paporot snapshot create -m "修复了登录 Bug"

# 对比两个版本
paporot diff --from v1 --to v2

# PRD 覆盖率分析
paporot coverage -p docs/prd.md

# 回归检测
paporot regression --from v1 --to v2

# 风险评估
paporot risk
```

### 4. 执行轨迹追踪

```bash
# 导入 Agent 日志
paporot trace import agent_log.json
paporot trace import session.json --adapter claude-code

# 查看导入的轨迹
paporot trace list
paporot trace show <trace_id>

# 轨迹对比（检测行为变化）
paporot trajectory diff --trace-a <id> --trace-b <id>
paporot trajectory diff --capability cap_auth
```

### 5. 三层行为分析

```bash
# P0: 行为状态机
paporot state build --trace <id>
paporot state eval --trace <id>
paporot state diff --trace-a <a> --trace-b <b>

# P1: 轨迹向量（10 维）
paporot trajectory-vector build --trace <id>
paporot trajectory-vector cluster --traces t1 t2 t3 t4
paporot trajectory-vector anomaly --traces t1 t2 t3 t4 t5

# P2: 行为耦合图
paporot coupling build --pairs trace1:cap_auth trace2:cap_auth
paporot coupling analyze --cap cap_auth
paporot coupling impact --cap cap_auth
```

### 6. 依赖图分析

```bash
paporot graph show
paporot graph show --capability cap_auth --depth 2
paporot graph impact --capability cap_auth
paporot graph cycles
```

### 7. 整合审查（一键全流程）

```bash
paporot review -p docs/prd.md
```

一条命令完成：snapshot + diff + coverage + regression + risk 全流程。

---

## 目录结构

```
Paporot/
├── Cargo.toml                    # Native binary 包描述
├── src/
│   ├── main.rs                   # Native loader 入口 (wasmtime + WASI)
│   ├── cli.rs                    # CLI 16 个子命令定义
│   ├── config.rs                 # LLM 配置
│   ├── commands/                 # 所有分析命令实现
│   ├── trace/                    # Agent 日志适配器
│   ├── trajectory/               # P0/P1/P2 行为分析
│   │   ├── state/                # P0 状态机
│   │   ├── p1/                   # P1 轨迹向量
│   │   ├── p2/                   # P2 行为耦合图
│   │   ├── align/                # 对齐引擎
│   │   ├── evaler/               # 评估器
│   │   └── projection/           # 状态投影
│   ├── analysis/                 # L1/L2/L3 分析
│   ├── evidence/                 # 能力证据
│   ├── report/                   # 报告生成 (dashboard.rs)
│   └── skills/                   # Skill 运行管理
│
├── crates/
│   ├── paporot-core/             # WASM 沙盒核心 (wasm32-wasip1)
│   │   └── src/
│   │       ├── main.rs           # 沙盒 CLI 入口
│   │       ├── pipeline.rs       # Skill 扫描 → DAG → 执行 → 报告
│   │       └── host.rs           # Host function FFI
│   ├── skills/                   # Skill WASM 源码
│   └── skill-sdk/                # Skill 开发 SDK
│
├── .Paporot/                     # 项目数据目录
│   ├── bin/                      # paporot-core.wasm
│   ├── config.toml               # LLM 配置
│   ├── skills/                   # 已安装的 Skill (各含 skill.toml + skill.wasm)
│   ├── reports/                  # 分析输出 (JSON / MD / HTML)
│   └── work/                     # 临时文件
│
└── docs/
    ├── architecture.md           # 详细架构文档
    ├── paporot-architecture.excalidraw  # 架构图源文件
    └── prd/                      # 各模块 PRD 文档
```

---

## 安全模型

- **文件隔离**：沙盒仅能访问 `.Paporot/` 目录，host function 内置 canonicalize + starts_with 路径遍历防护
- **网络限制**：仅 `host_llm_call` 可发起 HTTPS 请求，仅调用配置的 LLM endpoint
- **内存安全**：WASM 线性内存由 wasmtime 管理，OOB 访问触发 wasm trap 而非 UB
- **Skill 隔离**：每个 Skill 是独立的 `.wasm` 文件，独立执行，互不干扰

---

## 运行测试

```bash
cargo test                          # 全部测试
cargo test --lib                    # 库测试
cargo test --test integration_tests # 集成测试
cargo test trajectory::p1           # P1 轨迹向量测试
cargo test trajectory::p2           # P2 耦合图测试
cargo test trajectory               # 所有轨迹模块测试
```

---

## 文档

| 文档 | 路径 | 说明 |
|------|------|------|
| 项目 README | `README.md` | 本文档 |
| 架构详细文档 | `docs/architecture.md` | 完整架构与用户手册 |
| 架构图 | `docs/paporot-architecture.excalidraw` | Excalidraw 源文件 |
| P0 状态机 PRD | `docs/prd/PRD_P0_STATE_MACHINE.md` | 行为状态机设计 |
| P1 轨迹向量 PRD | `docs/prd/PRD_P1_STV.md` | 10 维轨迹向量设计 |
| P2 耦合图 PRD | `docs/prd/PRD_P2_BCG.md` | 行为耦合图设计 |
| 系统设计 | `docs/prd/DESIGN.md` | 原始系统设计方案 |
| P1+P2 测试说明书 | `docs/tests/TEST_SPEC_P1_P2.md` | 测试用例说明 |
| P1+P2 测试报告 | `docs/tests/TEST_REPORT_P1_P2.md` | 测试执行结果 |

---

## License

MIT
