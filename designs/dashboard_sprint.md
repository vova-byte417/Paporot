# Dashboard V2 详细 PRD & 执行计划

> **目标**: 将 `generate_dashboard_v2()` 生成的 HTML 完全对齐 `paporot-refactor.md` Section 10 设计规格
> **状态**: ✅ Sprint 1-3 完成 — 编译通过，全部 27 个集成测试通过

---

## 一、当前状态 (2026-06-30)

### ✅ 已交付

| Sprint | 内容 | 状态 |
|--------|------|------|
| Sprint 1 | Rust 数据准备：`extract_dependencies()`, `parse_imports()`, `scan_git_history()`, `build_file_tree()` | ✅ |
| Sprint 2 | HTML/CSS/JS：瀑布图(3层+Bézier)、全景图(真实数据+力导向)、LLM解读卡片、侧边栏、入场动画 | ✅ |
| Sprint 3 | 替换 main.rs 旧函数、`--full`/`--change` CLI、删除 `paporot dashboard`、编译+测试通过 | ✅ |
| Sprint 4 | test_space/devika + e-commerce 实测 | ⏳ 待执行 |

### 代码结构

```
src/main.rs
├── cmd_analyze()              # 7步流程（含 --full 时 git 历史扫描）
├── print_behavior_narrative() # 终端行为叙事输出
├── extract_dependencies()     # 从源文件解析 import/use/require
├── parse_imports()            # 多语言导入语句解析器
├── scan_git_history()         # git log 提取模块历史活跃度
├── build_file_tree()          # 文件树JSON构建
├── generate_dashboard_v2()    # 入口，委托给子函数
├── build_dashboard_data()     # JSON数据块构建
├── build_dashboard_html()     # HTML文档拼装
├── build_dashboard_css()      # CSS样式（#111827暗色主题）
└── build_dashboard_js()       # JS逻辑（瀑布图+全景图+侧边栏+Tasks）
```

### 用户使用流程

```
$ paporot analyze              # 默认 --change：行为变更叙事
$ paporot analyze --full       # 包含 git 历史能力全景
# 完成后自动打开 reports/dashboard.html
```

### 行为变更叙事页面包含
- Billboard 标题区 (headline + subtitle + 标签)
- 统计栏 (files/additions/deletions/symbols)
- **三层瀑布图** (符号→模块→下游依赖，Bézier曲线，层延迟入场动画)
- **LLM 解读卡片** (每模块一张，中文描述 + 风险等级)
- **侧边栏** (任务历史 + 模块索引，点击跳转全景)

### 能力全景页面 (--full)
- D3.js 力导向图，真实 git 历史数据
- 本次变更模块=靛蓝 pulse 动画，频繁变更=青色，低活跃=暗灰
- 逐个弹出入场动画 (80ms 间隔)
- 支持拖拽、缩放、聚焦
| `paporot analyze` 终端输出 | ✅ 完成 | 有行为变化叙事，不是只有 PASS/FAIL |

### 1.2 有问题的、不符合规格的

**问题A: Dashboard HTML 与 Section 10 设计规格脱节**

当前 `generate_dashboard_v2()` (main.rs:833-1025) 生成的 HTML 存在以下具体差距：

| Section 10 要求 | 当前实现 | 差距 |
|----------------|---------|------|
| 瀑布式融合影响图：3层布局（内圈符号→外圈模块→下游扩散） | 仅2层简单圆形布局，无树状瀑布扩散 | **缺失树状扩散层** |
| 瀑布图节点大小=变更行数 | 节点大小恒定（circle r=6, rect size=14） | **无数据驱动的尺寸** |
| 瀑布图文件树叠加（hover模块节点展开文件树缩略图） | 无此功能 | **完全缺失** |
| 瀑布图 zoom/pan 交互 | 无 | **完全缺失** |
| 瀑布图 click 聚焦节点 | 无 | **完全缺失** |
| 能力全景图：真实耦合数据 | 使用 `Math.random()` 随机生成连线（第1003行） | **糊弄用户，必须修复** |
| 能力全景图：入场动画（中心扩散） | 无 | **完全缺失** |
| 能力全景图：颜色编码（本次变更=靛蓝脉冲/历史活跃=青/低活跃=暗灰） | 仅按 action 分色（新增=靛蓝/删除=红/混合=琥珀） | **不符合规格的颜色编码** |
| 能力全景图：hover flyout 含"最近变更列表+关联模块" | 仅显示名称和符号数 | **信息不完整** |
| 侧边栏：Task历史含日期+标题+彩色圆点 | 仅"当前分析"一行 | **缺少历史记录展示** |
| 侧边栏：模块索引含最近变更日期和Task数 | 仅模块名称列表 | **缺少日期和计数** |
| 首页大字报区：影响范围缩略图（迷你模块关系图 200×200） | 无 | **完全缺失** |
| LLM解读卡片：无风险颜色标签 | 有 risk 文字但颜色不标准 | **需调整** |

**问题B: 数据准备不充分**

当前传给 JavaScript 的数据（第844-861行）只包含：
```json
{
  "headline", "subtitle", "overallRisk",
  "symbols": { "added": [...], "removed": [...] },
  "modules": [...], "files": [...],
  "interpretations": [...],
  "filesChanged", "additions", "deletions", "prevTrials",
  "graders": [...], "eval": {...}
}
```

缺少瀑布图和全景图需要的关键数据：
- 按模块分组的符号（含所属文件、行号）
- 模块间耦合边（共享文件数 = 耦合强度）
- 文件树结构（目录/文件层级）
- 全景图节点元数据（isChanged, changeCount, addedCount, removedCount）

---

## 二、数据流图

```
Rust 侧 (generate_dashboard_v2)                    JS 侧 (浏览器)
┌──────────────────────────────┐           ┌──────────────────────────────┐
│ EvalResult.code_change       │           │ 变更叙事 Tab                  │
│   ├─ symbols_added[]         │           │   ├─ Billboard 大字报        │
│   ├─ symbols_removed[]       │──JSON──▶  │   ├─ Stats 统计条           │
│   ├─ files_changed[]         │  嵌入     │   ├─ Waterfall 瀑布图        │
│   ├─ modules[]               │  HTML     │   │    (D3.js 3层布局)      │
│   └─ NarrativeData           │           │   └─ LLM 解读卡片           │
│       ├─ headline            │           │                             │
│       ├─ subtitle            │           │ 能力全景 Tab                 │
│       └─ interpretations[]   │           │   └─ Force Graph (D3.js)    │
│                              │           │       真实耦合边 + 入场动画  │
│ 附加数据结构（新增）         │           │                             │
│   ├─ symbolsByModule{}       │           │ Tasks Tab                   │
│   ├─ moduleEdges[]           │           │   └─ 评估任务详情表          │
│   ├─ fileTree[]              │           │                             │
│   └─ panorama{ nodes, links }│           │ 侧边栏                      │
└──────────────────────────────┘           │   ├─ Task 历史（日期+标题） │
                                           │   └─ 模块索引（日期+计数）  │
                                           └──────────────────────────────┘
```

---

## 三、逐 Sprint 详细计划

### Sprint 1: Rust 数据准备增强

**目标**: 从 `EvalResult.code_change` 中提取瀑布图和全景图所需的完整数据

**要修改的文件**: `src/main.rs` 中的 `generate_dashboard_v2` 函数

**具体改动**（Rust 代码，在 `let cc = &eval.code_change;` 之后，`let data = json!({...})` 之前插入）:

1. **`symbolsByModule`**: 按文件路径提取模块名，将每个符号归类到对应模块
   ```rust
   // BTreeMap<String, Vec<Value>>
   // key: "src/auth", value: [{name:"login", kind:"fn", file:"src/auth/login.rs", action:"added", line:42}]
   ```

2. **`moduleEdges`**: 模块间耦合边。两个模块如果都出现在本次变更中且共享文件，创建一条边
   ```rust
   // Vec<{source: "src/auth", target: "src/middleware", strength: 3}>
   // strength = 两个模块共享的文件个数
   ```

3. **`fileTree`**: 文件树结构，用于 hover 模块节点时展示
   ```rust
   // Vec<FileTreeNode{name, path, children[], is_file}>
   // 例如: [{name:"src", children:[{name:"auth", children:[
   //   {name:"login.rs", is_file:true}, {name:"register.rs", is_file:true}
   // ]}]}]
   ```

4. **`panorama.nodes`**: 全景图节点
   ```rust
   // [{id:"src/auth", label:"auth", changeCount:5, isChanged:true, 
   //   addedCount:3, removedCount:2}]
   ```

5. **`panorama.links`**: 全景图耦合边（复用 moduleEdges）

6. **`allModules`**: 所有涉及模块的扁平列表

**预期输出**: `designs/dashboard_v2_new.txt` 已包含上述数据准备的 Rust 代码，需确认无误

**验证方式**: 编译通过即可（数据准备是纯逻辑，无运行时依赖）

---

### Sprint 2: HTML/CSS/JS 模板重写

**目标**: 生成一个完整的、自包含的 HTML 文件，所有 D3.js 可视化都基于真实数据

**HTML 结构** (从 `designs/dashboard_html_template.txt` 扩建):

#### 2.1 CSS 样式（暗色主题 #111827）

| 组件 | 规格要求 | 实现 |
|------|---------|------|
| 整体背景 | `#111827` | `--bg: #111827` |
| 卡片 | 半透明 `rgba(255,255,255,0.03)` + border `rgba(255,255,255,0.06)` | `--card-bg` / `--card-border` |
| 品牌色 | `#6366F1` 靛蓝 | `--brand` |
| 强调色 | `#06B6D4` 青 / `#F59E0B` 琥珀 / `#EF4444` 红 / `#22C55E` 绿 | 对应 CSS 变量 |
| 布局 | 顶栏 56px + 左侧栏 280px + 主内容区 | flex 布局 |
| 动画 | `@keyframes pulse` (呼吸), `@keyframes fadeInUp` (入场) | CSS animations |

#### 2.2 变更叙事 Tab

**(a) Billboard 大字报区**
- `<h1>` 用 LLM 生成的 headline，靛蓝渐变色
- `<p>` subtitle，白色 dim
- 标签组：文件数、+N/-N 行、模块数、风险等级（带颜色）

**(b) Stats 统计条**
- 4 格：Files Changed / Lines Added / Lines Deleted / Pass Rate
- 数值大号字体，标签小号大写

**(c) 瀑布式融合影响图 (D3.js)**

这是最核心的图表，需实现3层布局：

```
Y=60   Layer 1 (符号层)
       ┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐
       │ fn A │  │fn B  │  │fn C  │  │fn D  │   ← 内圈符号节点
       └──┬───┘  └──┬───┘  └──┬───┘  └──┬───┘
          │         │         │         │
          ▼         ▼         ▼         ▼         ← 贝塞尔曲线连线
Y=220  Layer 2 (模块层)
       ┌──────────┐    ┌──────────────┐
       │ src/auth │◄──▶│src/middleware│          ← 外圈模块节点 + 横向耦合边
       └────┬─────┘    └──────┬───────┘
            │                 │
            ▼                 ▼                   ← 树状瀑布扩散
Y=380  Layer 3 (下游扩散层)
       ┌──────────┐    ┌──────────────┐
       │ (models) │    │ (rate_limit) │          ← 从模块向下游扩散
       └──────────┘    └──────────────┘
```

**D3.js 实现要点**:
- 符号节点：`<circle>` + `<text>`，颜色=靛蓝(added)/红色(removed)，大小=名长/行数
- 模块节点：`<rect>` + `<text>`，颜色=青色，宽度=符号数*10
- 下游节点：`<rect>` + `<text>`，颜色=灰暗，虚线连接
- 连线：`<path>` 贝塞尔曲线，渐变透明度，箭头标记
- 模块间耦合边：`<line>` 横向，粗细=耦合强度
- 交互：
  - hover 符号→显示 tooltip (名称、类型、文件、行号、动作)
  - hover 模块→显示 tooltip + 文件树列表
  - click 模块→zoom 聚焦
  - 整体支持 zoom/pan (d3.zoom)

**(d) LLM 解读卡片**
- 按模块分组，每张卡片含：
  - 模块名（渐变色）
  - 风险标签（high=红/medium=琥珀/low=绿）
  - 描述文字
  - 关键符号标签（等宽字体小标签）

#### 2.3 能力全景 Tab

**(a) 力导向网络图 (D3.js)**

- 节点：方块 `<rect>`，按模块名，大小=变更符号数
- 连线：`<line>`，粗细=耦合强度
- 颜色编码（**修复当前问题**）：
  - 本次变更模块：靛蓝 `#6366F1` + pulse 动画
  - (预留) 历史活跃模块：青 `#06B6D4`
  - (预留) 低活跃模块：暗灰 `#374151`
- **入场动画**：所有节点初始坐标=画布中心，simulation 启动后扩散到力导向位置
- 交互：
  - 拖拽节点 (d3.drag)
  - zoom/pan (d3.zoom)
  - hover 显示模块详情 flyout：模块名、变更符号数、新增N个/删除N个

#### 2.4 Tasks Tab

- 表格展示当前 EvalResult
- 列：Eval ID(截断) / 描述 / 分类 / 模块 / 结果(PASS/FAIL badge) / 日期

#### 2.5 侧边栏

**(a) Task 历史**
- 显示当前分析记录
- 格式：彩色圆点 + 日期 + 标题 + 文件数/行数/结果

**(b) 模块索引**
- 以标签形式列出所有模块
- 每个标签：模块简称 + 变更符号数
- 点击跳转能力全景页并聚焦该节点

---

### Sprint 3: 替换 main.rs + WSL 编译验证

**目标**: 将 Sprint 1+2 的产物合并成一个新的 `generate_dashboard_v2` 函数，替换 main.rs 中的旧函数

**操作步骤**:
1. 确认 `designs/dashboard_v2_new.txt`（Rust 数据准备）+ `designs/dashboard_html_template.txt`（HTML/JS模板）都完整
2. 用 SearchReplace 将 main.rs 第833-1025行替换为新函数
3. 在 WSL 中运行 `cargo build 2>&1`
4. 如有编译错误，逐一修复

**预期风险点**:
- HTML 中的 `{` 和 `}` 需要正确转义（`{{` / `}}`）
- JS 模板字面量中的 `${}` 需要转义
- JSON 嵌入需确保序列化正确

---

### Sprint 4: 在 test_space/devika 实测

**目标**: 用真实项目验证完整流程

**操作步骤**:
1. 进入 `test_space/devika`
2. 运行 `paporot analyze`（需配置 API key）
3. 检查终端输出：
   - [3/6] 行为变化叙事是否正确展示
   - [4/6] LLM 叙事是否成功生成
4. 打开生成的 `reports/dashboard.html`
5. 验证：
   - Billboard 大字报显示正确
   - 瀑布图有3层布局、hover 有文件树、可 zoom
   - 全景图有真实耦合边、入场动画、hover flyout
   - 侧边栏有日期+标题+模块索引
   - LLM 解读卡片显示正确

---

## 四、当前已有文件清单

| 文件 | 用途 | 状态 |
|------|------|------|
| `designs/paporot-refactor.md` | 设计规格（Section 10 是 Dashboard） | 参考 |
| `designs/dashboard_sprint.md` | 本文件，执行计划 | **刚重写完成** |
| `designs/dashboard_v2_new.txt` | Sprint 1 产物：Rust 数据准备代码 | 已写，待确认 |
| `designs/dashboard_html_template.txt` | Sprint 2 产物草稿：HTML/CSS 部分 | **不完整，527行截断** |
| `src/main.rs` (833-1025) | 当前旧版 generate_dashboard_v2 | 待替换 |

---

## 五、下一步

**请审核此计划**。确认后我按 Sprint 1→2→3→4 顺序执行，每完成一个 Sprint 汇报结果。

关键待确认问题：
1. 瀑布图的"下游扩散"层在当前数据中只能模拟（我们没有真实的依赖图），是否接受基于模块间共享文件的推断？
2. 模板文件 `dashboard_html_template.txt` 中的 CSS 结构是否满意？还是需要调整？
