# Paporot 架构与用户手册

## 项目概要

Paporot 是一个 **AI 生成软件的沙盒化行为分析管道**。它通过 WebAssembly (WASM) 沙盒隔离执行分析管线，调用一系列可组合的 Skill 对项目进行多维度审计，最终生成 JSON / Markdown / HTML 三份报告。

**核心设计理念**：分析逻辑完全在 WASM 沙盒内运行，只能通过 3 个受控的 host function 与外部交互，确保分析过程不篡改宿主文件、不访问未经授权的网络资源。

---

## 架构总览

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
│  │  │  • CLI 入口 (main)             │  │  │
│  │  │  • pipeline 模块               │  │  │
│  │  │    - Skill 扫描                │  │  │
│  │  │    - DAG 拓扑排序              │  │  │
│  │  │    - 逐层执行 Skill            │  │  │
│  │  │    - 报告生成                  │  │  │
│  │  │  • host 模块 (FFI 绑定)        │  │  │
│  │  └────────────────────────────────┘  │  │
│  │                                      │  │
│  │  Host Functions:                     │  │
│  │  1. host_read_file  (受控文件读)      │  │
│  │  2. host_write_file (受控文件写)      │  │
│  │  3. host_llm_call   (LLM 推理)       │  │
│  │                                      │  │
│  │  WASI Preview1:                      │  │
│  │  • preopen: .Paporot/ → .           │  │
│  │  • stdio 透传                        │  │
│  └──────────────────────────────────────┘  │
└────────────────────────────────────────────┘
```

### 两级架构

| 层级 | 组件 | Target | 职责 |
|------|------|--------|------|
| 宿主层 | `src/main.rs` | native (x86_64-linux) | 加载 .wasm、注册 host function、转发 CLI 参数 |
| 沙盒层 | `crates/paporot-core/` | wasm32-wasip1 | 扫描 Skill → DAG 编排 → 执行分析 → 写报告 |

### Host Function 签名

所有数据通过 WASM 线性内存的 packed pointer 传递，返回值为 `(ptr << 32) | len`：

| 函数 | 签名 | 说明 |
|------|------|------|
| `host_read_file` | `(path_ptr, path_len) → packed` | 读取 .Paporot/ 内文件，带路径遍历防护 |
| `host_write_file` | `(path_ptr, path_len, data_ptr, data_len) → errno` | 写入 .Paporot/ 内文件 |
| `host_llm_call` | `(prompt_ptr, prompt_len, schema_ptr, schema_len) → packed` | 调用 DeepSeek API |

---

## 目录结构

```
Paporot/
├── Cargo.toml                  # Native binary 的包描述
├── src/
│   ├── main.rs                 # Native loader 入口 (wasmtime + WASI)
│   ├── config.rs               # Config/LlmConfig 结构体
│   └── ... (其他原有模块)
│
├── crates/
│   ├── paporot-core/           # WASM 沙盒核心
│   │   ├── Cargo.toml          # 独立包，编译目标 wasm32-wasip1
│   │   └── src/
│   │       ├── main.rs         # 沙盒 CLI 入口
│   │       ├── lib.rs          # 模块导出
│   │       ├── pipeline.rs     # 分析管线 (Skill扫描/DAG/执行/报告)
│   │       └── host.rs         # Host function FFI 声明 + 封装
│   └── skills/                 # Skill 源码 + WASM (原有)
│
├── .Paporot/                   # 项目数据目录
│   ├── bin/                    # paporot-core.wasm 部署位置
│   ├── config.toml             # LLM 配置 (可选)
│   ├── skills/                 # 已安装的 Skill
│   │   ├── repository-understanding/
│   │   │   ├── skill.toml      # Skill 元数据 + 依赖声明
│   │   │   └── skill.wasm      # Skill WASM 二进制
│   │   ├── module-discovery/
│   │   ├── dependency-analysis/
│   │   ├── runtime-flow-analysis/
│   │   ├── behavior-boundary-discovery/
│   │   └── architecture-doc-generator/
│   ├── reports/                # 分析报告输出
│   │   ├── analysis_result.json
│   │   ├── architecture.md
│   │   └── dashboard.html
│   └── work/                   # 临时工作文件
│
└── docs/
    └── architecture.md         # 本文档
```

---

## Skill 体系

### Skill 定义

每个 Skill 由 `skill.toml` 描述：

```toml
[skill]
name = "repository-understanding"
version = "0.1.0"
requires_paporot = "0.2.0"
description = "识别项目整体目标、技术栈、入口程序、核心业务能力"
timeout_secs = 300

[inputs]
required = []
optional = ["prd_content"]

[outputs]
schema = """{"type":"object","properties":{"project_name":{"type":"string"},...}}"""
format = "json"

[dependencies]
uses_outputs_from = []

[llm_calls]
max_calls = 3
preferred_model = "deepseek-chat"
```

### 内置 6 个 Skill

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

## 构建与安装

### 前置条件

- Rust 1.96+ 工具链 (在 WSL 中使用)
- wasm32-wasip1 target: `rustup target add wasm32-wasip1`
- Cargo 镜像配置 (推荐 rsproxy 或 ustc)

### 编译

```bash
# 1. 编译 WASM 沙盒核心
cargo build --manifest-path crates/paporot-core/Cargo.toml \
  --target wasm32-wasip1 --release

# 2. 编译 native loader
cargo build --release

# 3. (可选) 部署 WASM 到 .Paporot/bin/
mkdir -p .Paporot/bin
cp crates/paporot-core/target/wasm32-wasip1/release/paporot-core.wasm \
   .Paporot/bin/
```

构建产物：
- `target/release/Paporot` — native 可执行文件
- `crates/paporot-core/target/wasm32-wasip1/release/paporot-core.wasm` — 沙盒 WASM

---

## 使用手册

### LLM 配置

在 `.Paporot/config.toml` 中配置 DeepSeek API：

```toml
[llm]
api_key = ""          # 留空则从环境变量 PAPOROT_API_KEY 读取
model = "deepseek-chat"
endpoint = ""          # 留空使用默认 https://api.deepseek.com/v1/chat/completions
temperature = 0.3
max_tokens = 4096
```

或通过环境变量：
```bash
export PAPOROT_API_KEY="sk-xxxxxxxxxxxxxxxx"
```

> 未配置 LLM 时，分析仍可运行，Skill 会返回 stub 输出。

### 命令参考

#### `analyze` — 运行完整分析管道

```bash
# 基本分析
Paporot analyze

# 指定 PRD 文件
Paporot analyze --prd docs/prd.md

# 简写
Paporot analyze -p docs/prd.md

# 指定输入文件对
Paporot analyze --input src/main.rs --input Cargo.toml
```

#### `skill list` — 列出已安装的 Skill

```bash
Paporot skill list
```

输出示例：
```
NAME                                VERSION    COMPATIBLE   DESCRIPTION
repository-understanding            0.1.0      YES          识别项目整体目标...
module-discovery                    0.1.0      YES          发现系统中的业务模块...
dependency-analysis                 0.1.0      YES          构建模块依赖图...
runtime-flow-analysis               0.1.0      YES          发现端到端业务执行路径...
behavior-boundary-discovery         0.1.0      YES          发现影响可观测行为的组件边界...
architecture-doc-generator          0.1.0      YES          聚合所有上游 Skill 输出...
6 skills installed
```

### 输出产物

运行 `Paporot analyze` 后，`.Paporot/reports/` 目录下生成 3 份报告：

| 文件 | 格式 | 说明 |
|------|------|------|
| `analysis_result.json` | JSON | 机器可读的结构化分析结果 |
| `architecture.md` | Markdown | 面向开发者的架构分析报告 |
| `dashboard.html` | HTML | 暗色主题的 Dashboard 可视化页面 |

#### analysis_result.json 结构

```json
{
  "project_name": "Paporot Analysis",
  "analyzed_at": "2026-06-18 14:32:00",
  "summary": {
    "total_skills": 6,
    "ok": 6,
    "skipped": 0,
    "failed": 0,
    "risk_level": "low"
  },
  "skill_results": [
    {"name": "repository-understanding", "status": "ok", "duration_ms": 100, "output_summary": "{...}"}
  ],
  "high_level_summary": "6 skills in 6 DAG layers: 6 OK, 0 skipped, 0 failed. Risk: LOW"
}
```

#### Dashboard HTML

开箱即用的暗色主题可视化面板，包含：
- 成功 / 跳过 / 失败 计数仪表
- 风险等级标签 (LOW / MEDIUM / HIGH)
- 高层分析摘要

打开方式：浏览器直接打开 `dashboard.html`

---

## 安全模型

### 文件访问控制

- 沙盒只能访问 `.Paporot/` 目录内的文件
- `host_read_file` 内置路径遍历防护 (canonicalize + starts_with)
- `host_write_file` 同样限制在 `.Paporot/` 内
- 外部项目文件需通过 CLI `--input` 参数显式指定

### 网络访问

- 仅 `host_llm_call` 可发起 HTTPS 请求
- 只能调用配置中的 LLM endpoint
- 无其他网络能力

### 内存安全

- WASM 线性内存由 wasmtime 管理
- Host function 写入数据前自动 grow memory
- OOB 访问会触发 wasm trap 而非 UB

---

## 扩展指南

### 添加新 Skill

1. 在 `.Paporot/skills/<skill-name>/` 创建 `skill.toml` 和 `skill.wasm`
2. 在 `pipeline.rs` 的 `scan_skills()` 中添加新的路径检查
3. 运行 `Paporot analyze` 验证

### 替换 LLM 后端

修改 `src/main.rs` 中的 `call_deepseek_api()` 函数，适配 OpenAI 兼容 API 即可。

### 自定义报告模板

修改 `pipeline.rs` 中的 `build_markdown_report()` 和 `build_dashboard_html()` 函数。

---

## 故障排除

### `paporot-core.wasm not found`

```bash
cargo build --manifest-path crates/paporot-core/Cargo.toml \
  --target wasm32-wasip1 --release
```

### `wasm trap: out of bounds memory access`

通常是 WASM 版本与 native loader 版本不匹配。重新编译两个组件即可。

### `No compatible skills found`

检查 `.Paporot/skills/` 目录下是否有包含 `skill.toml` 的子目录。

### `No API key configured`

配置 `.Paporot/config.toml` 或设置 `PAPOROT_API_KEY` 环境变量。未配置时分析仍可运行（使用 stub 输出）。
