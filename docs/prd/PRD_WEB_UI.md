# PRD: Paporot Web UI（仪表盘）

> 统一的行为版本控制仪表盘，单文件 HTML

---

## 目录

1. [背景与动机](#1-背景与动机)
2. [设计决策记录](#2-设计决策记录)
3. [架构](#3-架构)
4. [页面设计](#4-页面设计)
5. [数据源](#5-数据源)
6. [CLI 子命令](#6-cli-子命令)
7. [测试策略](#7-测试策略)

---

## 1. 背景与动机

### 1.1 问题

Paporot 的各个模块（Trace、Trajectory Diff、Evidence、Behavior Eval）各自有 CLI 命令和独立的输出格式。用户需要在多个命令之间切换才能看到完整的"行为版本"全景。

### 1.2 目标

一个统一的仪表盘页面，将所有模块的数据汇总呈现：

- Trace 列表 → 戳进去看轨迹详情
- Trajectory Diff 历史 → 看行为演变
- Capability Evidence → 看推断可信度
- Behavior Eval → 看退化/改进趋势

---

## 2. 设计决策记录

| # | 决策 | 选择理由 |
|---|------|---------|
| D1 | Tab 切换（Trace / Diff / Evidence / Eval） | 一个页面覆盖所有模块 |
| D2 | 单文件 HTML，离线可用，CDN 加载 Mermaid.js 和 Chart.js | 不需要服务器，浏览器直接打开 |
| D3 | `paporot ui` 命令：生成 HTML + 可选启动本地 HTTP 服务 | 两种使用方式：离线看静态文件 / 启动服务实时预览 |
| D4 | 不连接后端 API，纯静态数据嵌入 | 安全、离线、零依赖 |

---

## 3. 架构

### 3.1 生成方式

```
paporot ui           ← CLI 命令
  ├── 读取 .Paporot/trace_index.db     (Trace 数据)
  ├── 读取 .Paporot/trajectory/        (Diff 历史)
  ├── 读取 .Paporot/evidence/          (Evidence 历史)
  ├── 读取 .Paporot/eval/              (Eval 历史)
  └── 生成单一 HTML 文件
       └── 所有数据内嵌为 <script> JSON
       └── CDN 加载 mermaid + chart.js
```

### 3.2 文件位置

```
.Paporot/
  ui/
    dashboard.html       ← 最新生成的仪表盘
    dashboard_{timestamp}.html  ← 历史版本（可选保留）

src/
  ui/                    ← HTML 模板 + 生成逻辑
    mod.rs
    generator.rs         ← 数据采集 + HTML 拼接
    templates.rs         ← HTML 模板字符串
```

---

## 4. 页面设计

### 4.1 整体布局

```
┌──────────────────────────────────────────────────────────┐
│  Paporot Dashboard                          [刷新] [导出]│
│  Behavior Version Control 仪表盘                         │
├──────────────────────────────────────────────────────────┤
│  [Trace]  [Trajectory Diff]  [Evidence]  [Behavior Eval]│
├──────────────────────────────────────────────────────────┤
│                                                          │
│  ┌──────────┬──────────┬──────────┬──────────────────┐  │
│  │ 总 Trace │ 总 Diff  │ 总 Eval  │ 退化 Capability  │  │
│  │   42     │   15     │   8      │   2 ⚠            │  │
│  └──────────┴──────────┴──────────┴──────────────────┘  │
│                                                          │
│  (根据当前 Tab 显示不同内容)                              │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

### 4.2 Tab 1: Trace 列表

```
┌──────────────────────────────────────────────────────────┐
│  [Trace]  Trajectory Diff  Evidence  Behavior Eval      │
├──────────────────────────────────────────────────────────┤
│  搜索: [________]  标签: [all ▼]  日期: [2026-06-01]-[__]│
│                                                          │
│  ┌──────┬──────────┬──────────┬───────┬──────┬──────────┐│
│  │ ID   │ Session  │ Prompt   │ Tools │Token │ Duration ││
│  ├──────┼──────────┼──────────┼───────┼──────┼──────────┤│
│  │ t_001│ sess-01  │ fix bug  │ 12    │ 320  │ 45s     ││
│  │ t_002│ sess-02  │ add feat │ 8     │ 210  │ 32s     ││
│  │ t_003│ sess-01  │ fix bug  │ 15    │ 450  │ 62s     ││
│  │ ...  │ ...      │ ...      │ ...   │ ...  │ ...     ││
│  └──────┴──────────┴──────────┴───────┴──────┴──────────┘│
│                                                          │
│  点击行 → 展开 Trace 详情（Tool Calls 时间线）           │
└──────────────────────────────────────────────────────────┘
```

### 4.3 Tab 2: Trajectory Diff 历史

```
┌──────────────────────────────────────────────────────────┐
│  Trace  [Trajectory Diff]  Evidence  Behavior Eval      │
├──────────────────────────────────────────────────────────┤
│  ┌──────┬──────────┬──────────────┬──────────────────┐   │
│  │ Cap  │ 版本     │ 变化摘要      │ 操作              │   │
│  ├──────┼──────────┼──────────────┼──────────────────┤   │
│  │ Bug  │ t_001→   │ +1段 +5调用  │ [查看详情]        │   │
│  │ Fix  │ t_003    │ M-1 +130 tok │ [Mermaid图]       │   │
│  ├──────┼──────────┼──────────────┼──────────────────┤   │
│  │ Auth │ t_005→   │ +2段 +3调用  │ [查看详情]        │   │
│  │      │ t_008    │ -0 M-0       │ [Mermaid图]       │   │
│  └──────┴──────────┴──────────────┴──────────────────┘   │
│                                                          │
│  点击 [Mermaid图] → 弹出模态框展示时序图                  │
│  点击 [查看详情] → 跳到该 Capability 的完整 Diff 页面     │
└──────────────────────────────────────────────────────────┘
```

### 4.4 Tab 3: Capability Evidence

```
┌──────────────────────────────────────────────────────────┐
│  Trace  Trajectory Diff  [Evidence]  Behavior Eval      │
├──────────────────────────────────────────────────────────┤
│  置信度概览                                              │
│  ┌──────────────┬──────────┬──────────┬────────────────┐ │
│  │ Capability   │ L1       │ L2       │ L3             │ │
│  ├──────────────┼──────────┼──────────┼────────────────┤ │
│  │ User Login   │ 🟢 0.85 │ 🟡 0.72 │ 🟢 0.90        │ │
│  │ Bug Fix      │ 🟢 0.92 │ 🟢 0.80 │ — (未配置)     │ │
│  │ Payment      │ 🟡 0.65 │ 🟡 0.58 │ 🟢 0.88        │ │
│  └──────────────┴──────────┴──────────┴────────────────┘ │
│                                                          │
│  点击行 → 展开溯源树（可交互 L1→L2→L3 连线）            │
└──────────────────────────────────────────────────────────┘
```

### 4.5 Tab 4: Behavior Eval

```
┌──────────────────────────────────────────────────────────┐
│  Trace  Trajectory Diff  Evidence  [Behavior Eval]      │
├──────────────────────────────────────────────────────────┤
│  退化趋势                                                │
│  ┌──────┬──────────┬──────────┬──────────────────────┐   │
│  │ Cap  │ Verdict  │ 命中规则  │ 趋势                 │   │
│  ├──────┼──────────┼──────────┼──────────────────────┤   │
│  │ Bug  │ ⚠ Watch  │ R002×1   │ tool_calls ↑         │   │
│  │ Fix  │          │ R005×1   │ duration ↗           │   │
│  ├──────┼──────────┼──────────┼──────────────────────┤   │
│  │ Auth │ ✅ Pass  │ —        │ —                    │   │
│  ├──────┼──────────┼──────────┼──────────────────────┤   │
│  │ Pay  │ 🔴 Degr  │ R001×1   │ token_usage ↑↑       │   │
│  │      │          │ R004×1   │ confidence ↓         │   │
│  └──────┴──────────┴──────────┴──────────────────────┘   │
│                                                          │
│  点击行 → 展开趋势图（Chart.js 折线图）                  │
└──────────────────────────────────────────────────────────┘
```

---

## 5. 数据源

### 5.1 数据嵌入

所有数据打包为单文件 HTML 中的 `<script type="application/json">`：

```html
<script type="application/json" id="paporot-data">
{
  "generated_at": "2026-06-12T14:30:00Z",
  "counts": {
    "total_traces": 42,
    "total_diffs": 15,
    "total_evals": 8,
    "degraded_capabilities": 2
  },
  "traces": [ ... TraceSummary[] ... ],
  "trajectory_diffs": [ ... TrajectoryDiff[] ... ],
  "evidences": [ ... Evidence[] ... ],
  "eval_results": [ ... EvalResult[] ... ]
}
</script>
```

### 5.2 读取逻辑

```rust
/// 收集仪表盘所需的所有数据。
fn collect_dashboard_data(base_dir: &Path) -> DashboardData {
    let trace_storage = TraceStorage::new(base_dir);
    let traces = trace_storage.list(&TraceFilter {
        limit: 1000,
        ..Default::default()
    }).unwrap_or_default();

    // 读取 .Paporot/trajectory/ 中的 diff JSON 文件
    let diffs = load_diff_history(base_dir);

    // 读取 .Paporot/evidence/ 中的 evidence JSON 文件
    let evidences = load_evidence_history(base_dir);

    // 读取 .Paporot/eval/ 中的 eval JSON 文件
    let evals = load_eval_history(base_dir);

    DashboardData { traces, diffs, evidences, evals, ... }
}
```

---

## 6. CLI 子命令

### 6.1 `paporot ui`

```rust
pub enum UiAction {
    /// 生成统一的仪表盘 HTML 文件
    Generate {
        /// 输出路径（默认 .Paporot/ui/dashboard.html）
        #[arg(short, long)]
        output: Option<String>,
    },

    /// 生成仪表盘并启动本地 HTTP 服务器
    Serve {
        /// 端口（默认 9090）
        #[arg(short, long, default_value = "9090")]
        port: u16,
    },
}
```

使用示例：

```bash
# 生成静态 HTML
paporot ui generate

# 生成到指定路径
paporot ui generate --output ./paporot-dashboard.html

# 启动本地服务
paporot ui serve
# → 浏览器打开 http://localhost:9090
```

### 6.2 Serve 模式实现

```rust
/// 启动轻量 HTTP 服务。
fn serve_dashboard(port: u16) -> Result<()> {
    // 使用 tiny_http 或仅依赖 std::net::TcpListener
    // 单线程、只接受本地连接
    let listener = std::net::TcpListener::bind(format!("127.0.0.1:{}", port))?;
    println!("Dashboard available at http://localhost:{}", port);
    println!("Press Ctrl+C to stop");

    for stream in listener.incoming() {
        let mut stream = stream?;
        // 返回预生成的 HTML
        // HTTP 200 OK + Content-Type: text/html
    }
}
```

---

## 7. 测试策略

### 单元测试

| 测试 | 内容 |
|------|------|
| 数据采集 | 从空目录采集 → 空数据；从有数据的目录采集 → 正确计数 |
| HTML 生成 | 生成的文件包含 `<script id="paporot-data">` |
| HTML 完整性 | 生成的文件是合法的 HTML（有 `<html>` `</html>`） |

### 集成测试

- 完整流程：创建 trace → 生成 UI → 打开 HTML 验证数据嵌入
- serve 模式：启动服务 → HTTP GET 返回 200
