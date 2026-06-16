# Paporot

**AI 生成代码的行为版本控制与审计系统**

Paporot 是一个命令行工具，安装到你的项目里运行。它回答 AI Agent 写代码时两个最基本的问题：

1. **这次 Agent 改了什么能力？**（Capability Version Control）
2. **Agent 的行为变好还是变坏了？**（Behavior Version Control）

---

## 安装

```bash
# 需要 Rust 1.75+
cargo install --path .
```

装好后，在任意 Git 项目根目录下直接使用。Paporot 的所有数据存在项目里的 `.Paporot/` 目录，不会污染项目其他地方。

---

## 典型使用流程

### 场景 1：Agent 提交了代码，我想知道改了什么

你让 Claude Code / Copilot 改了一轮代码，Agent 提交了。你 run：

```bash
paporot review
```

Paporot 会自动分析 `HEAD~1..HEAD` 的 diff，输出：

```
Capability Changes:
  cap_001  Authentication (Modified)  ── login() 签名变了
  cap_002  API Rate Limit  (New)      ── 新增了 check_rate_limit()

Risk: Medium  ── 认证模块的破坏性变更可能影响下游

Dependencies affected: cap_003 (User Profile)
```

如果你们有 PRD，加上 `-p docs/prd.md`，还能看到需求覆盖了百分之几。

### 场景 2：同一个 Bug，Agent 修了两次，行为不一样了

你让 Agent 修了同一个 Bug 两次，代码改动差不多，但第一次用了 3 个工具调用就完了，第二次多跑了一堆测试和 lint。

```
# 先导入 Agent 的执行日志
paporot trace import ~/.claude/sessions/session_001.json
paporot trace import ~/.claude/sessions/session_002.json

# 对比两次执行的轨迹
paporot trajectory diff --trace-a trace_001 --trace-b trace_002
```

输出：

```
Trajectory Diff: trace_001 → trace_002
  Tools: 3 → 8 (Δ +5)
  Segments: +1 -0 ~1 =1
  Tool Churn: 62.5%  Capability Shift: 12.5%

Phase changes:
  + 验证 (test, lint, check)  ← B 版多了测试验证阶段

gantt
  section Version A (3 tools)
  定位问题      :a1, 2026-01-01, 1d
  实施修改      :a2, after a1, 1d
  提交          :a3, after a2, 1d
  section Version B (8 tools)
  定位问题      :b1, 2026-01-01, 1d
  实施修改      :b2, after b1, 1d
  验证          :b3, after b2, 1d    ← 新增
  提交          :b4, after b3, 1d
```

多跑测试是好事，但如果"验证"阶段 tool 调用从 2 个暴增到 15 个，Paporot 会自动标记为退化风险。

### 场景 3：多个版本后，我想看趋势

```bash
# 对比任意两个版本
paporot diff --from v1 --to v5

# 检测是否有能力退化
paporot regression --from v1 --to v5

# 打开仪表盘看全局
open dashboard.html
```

Dashboard 四个标签页按逻辑顺序排列：

| 标签 | 回答的问题 |
|------|-----------|
| Evidence Explorer | 这个 Capability 是怎么推断出来的？ |
| Capability Graph | 改了 A 会影响哪些能力？ |
| Behavior Eval | Agent 行为有没有退化？ |
| Trajectory Diff | 两次执行，Agent 的步骤哪里不一样？ |

---

## 命令速查

### Capability 版本控制

```bash
paporot snapshot create -m "描述这次改动"    # 从 git diff 创建行为快照
paporot snapshot create -m "..." -p prd.md   # 同时计算 PRD 覆盖率
paporot diff --from v1 --to v2               # 对比两个版本的能力差异
paporot diff --from v1 --to v2 --format mermaid  # 输出 Mermaid 图
paporot coverage -p prd.md                   # PRD 覆盖率分析
paporot regression --from v1 --to v2         # 回归检测
paporot risk                                 # 风险评估
paporot review -p prd.md                     # 一键全流程（snapshot + diff + coverage + regression + risk）
paporot status                               # 当前项目状态
```

### 执行轨迹追踪

```bash
paporot trace import agent_log.json                      # 导入 AI Agent 执行日志
paporot trace import session.json --adapter claude-code  # 指定适配器
paporot trace list                                       # 列出所有 trace
paporot trace list --cap cap_auth --from 2026-06-01      # 按条件过滤
paporot trace show <trace_id>                            # 查看 trace 详情
paporot trace link <trace_id> --cap cap_auth             # 关联 trace 到 Capability
paporot trace adapter list                               # 查看支持的 Agent 格式
```

支持的 Agent 格式：DeepSeek · Claude Code · OpenAI Chat Completion

### 行为版本控制（Trajectory Diff & Eval）

```bash
paporot trajectory diff --trace-a <id> --trace-b <id>   # 对比两次执行的轨迹
paporot trajectory diff --capability cap_auth              # 自动关联同 Capability 的两次执行
paporot trajectory diff --format json                     # 输出 JSON（给 Eval 消费）
paporot trajectory diff --format mermaid                  # 输出 Mermaid 时序图
paporot trajectory list                                   # 列出已缓存的轨迹对比
paporot trajectory show <diff_id>                         # 查看某次对比详情
```

### 依赖图

```bash
paporot graph show                                       # 全局依赖图
paporot graph show --capability cap_auth --depth 2       # 查看某个能力的上下游
paporot graph impact --capability cap_auth               # 改了 A 会影响谁
paporot graph evolution --capability cap_auth            # A 的历史演化
paporot graph cycles                                     # 检查循环依赖
```

### 人工反馈

```bash
paporot feedback approve cap_001              # 确认 AI 推断正确
paporot feedback reject cap_001 -r "误报"     # 标记误报
paporot feedback stats                        # 反馈统计
```

### P0: 行为状态机 — 把 trace 变成状态图

将 Agent 执行日志从"事件序列"升级为"状态机系统"，为 P1/P2 提供结构化输入。

```bash
paporot state build --trace <id>              # 从 trace 构建 BehaviorStateGraph
paporot state show <trace_id>                 # 查看状态图（terminal / json / mermaid）
paporot state diff --trace-a <a> --trace-b <b>  # 对比两条 trace 的状态图
paporot state eval --trace <id>               # 评估状态图质量
```

输出示例：

```
BehaviorStateGraph for trace 'trace_001':
  States: 4
  Transitions (events): 6
  Edges (aggregated): 4

  State s0:
    Primary phase: locate
    Stability: 0.85
    Tools: 0..5
  State s1:
    Primary phase: modify
    Stability: 0.92
    Tools: 5..12
  ...

  Transition Graph:
    s0 → s1  (×3)
    s1 → s2  (×4)
    s2 → s1  (×1)    ← loop detected
    s2 → s3  (×2)
```

### P1: 轨迹向量 — 把行为变成可计算的数值

将单条行为轨迹压缩为 10 维数值向量（非 ML embedding），用于聚类、异常检测、趋势分析。

```bash
# 构建向量
paporot trajectory-vector build --trace <id>              # 从 trace 构建 TrajectoryVector
paporot trajectory-vector build --trace <id> -o v1.json   # 输出到文件

# 对比向量
paporot trajectory-vector diff --v1 v1.json --v2 v2.json   # 两向量逐维度对比

# 聚类分析（至少 2 条 trace）
paporot trajectory-vector cluster --traces t1 t2 t3 t4     # DBSCAN-like 聚类

# 异常检测（至少 3 条 trace）
paporot trajectory-vector anomaly --traces t1 t2 t3 t4 t5  # 检测离群 trace
```

输出示例：

```
TrajectoryVector Summary:
  Tool entropy:       0.4521
  Phase entropy:      0.3812
  Transition entropy: 0.2943
  Loop ratio:         0.0833
  Backtrack ratio:    0.0000
  Burst ratio:        0.0714
  State stability:    0.8231

Cluster Analysis (5 traces):
  Clusters found: 2
  Cluster quality: 0.5231
  Cluster 1: trace_001, trace_003, trace_005
  Cluster 2: trace_002, trace_004
  Noise: 0 traces

Anomaly Detection (5 traces):
  trace_004   anomaly_score=2.8412  ← HIGHEST
  trace_002   anomaly_score=1.2034
  ...
```

向量结构（10 个字段）：

| 字段 | 含义 | 类型 |
|------|------|------|
| `tool_entropy` | 工具序列熵 — 原始行为混乱度 | f32 |
| `phase_entropy` | 状态路径熵 — 执行路径不确定性 | f32 |
| `transition_entropy` | 图结构熵 — 拓扑不确定性 | f32 |
| `loop_ratio` | 状态级循环占比 | f32 |
| `backtrack_ratio` | 时间回退占比 | f32 |
| `burst_ratio` | 工具密度聚集程度 | f32 |
| `state_stability_score` | 相邻状态连续性 | f32 |
| `tool_distribution` | 工具类别分布（稀疏向量） | SparseVector |
| `state_distribution` | 状态阶段分布（稀疏向量） | SparseVector |
| `edit_intensity_curve` | 编辑强度时间曲线 | Vec\<f32\> |

### P2: 耦合图 — 把行为变成关系网络

构建多个 capability 之间的行为耦合关系图。"cap_auth 和 cap_profile 经常一起改 + 行为模式相似 → coupling 0.72"。

```bash
# 构建耦合图（trace:capability 对）
paporot coupling build --pairs trace1:cap_auth trace2:cap_auth trace3:cap_profile

# 分析特定 capability 的耦合
paporot coupling analyze --cap cap_auth --pairs trace1:cap_auth trace2:cap_profile

# 导出图（Mermaid / JSON）
paporot coupling export --pairs ... --format mermaid

# 影响分析：修改 cap_auth 会影响谁？
paporot coupling impact --cap cap_auth --pairs ...
```

输出示例：

```
Coupling Analysis: cap_auth
  Edge count:      3
  Total coupling:  1.3500
  Max coupling:    0.7200
  Avg coupling:    0.4500

  Impact analysis (top connected):
  1. cap_profile →  corr=0.7200
  2. cap_api     ←  corr=0.4100
  3. cap_test    →  corr=0.2200

Coupling Graph:
  cap_auth → cap_profile (0.72)  [entropy:0.6, structural:0.3]
  cap_api  → cap_auth    (0.41)  [structural:0.5, temporal:0.2]
  cap_test → cap_auth    (0.22)  [density:0.4, entropy:0.3]
```

---

## 架构：三层行为版本控制

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

---

## 配置

在项目根目录创建 `.Paporot/config.toml`（可选，不配置也能跑）：

```toml
[llm]
endpoint = "https://api.openai.com/v1/chat/completions"
api_key = ""                      # 或用环境变量 Paporot_API_KEY
model = "gpt-4o"
temperature = 0.3

[trace]
auto_redact = false               # 导入时自动脱敏
redact_auth_header = true
redact_api_keys = true
```

---

## 运行测试

```bash
cargo test                                    # 全部测试（423 个）
cargo test --lib                              # 仅单元测试
cargo test --test integration_tests           # 仅集成测试
cargo test trajectory::p1                     # 仅 P1 测试（84 个）
cargo test trajectory::p2                     # 仅 P2 测试（58 个）
cargo test trajectory                         # 所有 trajectory 测试
```

## 文档

| 文档 | 路径 |
|------|------|
| P0 状态机 PRD | `docs/prd/PRD_P0_STATE_MACHINE.md` |
| P1 轨迹向量 PRD | `docs/prd/PRD_P1_STV.md` |
| P2 耦合图 PRD | `docs/prd/PRD_P2_BCG.md` |
| P1+P2 测试说明书 | `docs/tests/TEST_SPEC_P1_P2.md` |
| P1+P2 测试报告 | `docs/tests/TEST_REPORT_P1_P2.md` |

---

## License

MIT
