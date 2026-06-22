# Paporot 演讲稿

> 面向 AI 从业者 · 基于项目真实代码 · 约 15-18 分钟

---

大家好，我今天分享的项目叫 **Paporot**——"AI 生成代码的沙盒化行为审计管道"。

先不急着看架构。我想先聊一个我们在座各位可能都遇到过的问题。

你们有没有过这种经历：配了一个 Agent，比如让它修一个 Bug，它确实修好了，代码 diff 看起来也没问题。但你总觉得哪里不对——它这次用了 24 个 tool call，上次修同类型的问题只用了 12 个。它多出来的这 12 步在干什么？它是变得更谨慎了，还是在绕弯路？

**你看得到代码 diff，但你看不到行为 diff。** 这就是 Paporot 要解决的问题。

---

## Paporot 的核心回答：两个问题

Paporot 整个系统只回答两个问题：

1. **Agent 这次提交改了什么能力？**——不只是改了哪些文件，而是影响了哪些业务模块、变了哪些接口契约
2. **Agent 的行为变好还是变坏了？**——两次执行之间，行为模式是进化了还是退化了

怎么回答？通过四个支柱：**完全自动化的 Skill 编排、WASM 沙盒隔离、三层行为版本控制、确定性 + LLM 混合分析**。

---

## 从源头开始：数据的起点是 Trace

在讲架构之前，我必须先讲 Paporot 最底层的概念——**Execution Trace**。

Trace 不是什么复杂的东西。它就是 Agent 的一次完整执行记录。一个 Trace 包含：被要求了什么 (`prompt`)、按时间排序的 tool 调用序列 (`tool_calls`)、每次 tool 调用的参数和耗时、对应的观察结果 (`observations`)、最终输出、token 消耗——全部结构化。

这就是 Paporot 的核心数据源。所有分析——DAG 编排也好、状态机构建也好、向量对比也好——源头都是一个或多个 Trace。

**Trace 怎么来？** 两种方式。你可以用集成 SDK 在 Agent 执行时自动捕获（`TraceSource::Captured`），也可以从日志文件导入（`TraceSource::Imported`）。Paporot 不做 Agent 运行时监控，它只消费已经产生的 Trace 数据做分析。

---

## 架构：为什么必须用 WASM 沙盒

好，有了 Trace 数据，怎么分析？这时候就面临一个安全问题。

Paporot 的分析管线要读你的项目文件、调 LLM、生成报告存盘。如果这一切在宿主进程里跑，意味着分析代码有完整的文件系统访问和网络访问能力——这本身就是安全隐患。Paporot 分析了那么多 Agent 的项目，万一某个分析 Skill 有漏洞，宿主的文件系统就暴露了。

**所以 Paporot 做了一个硬隔离：两层架构。**

**宿主层**是一个极薄的 Rust native binary。它不做任何分析逻辑。它只做四件事：解析 CLI 参数、启动 wasmtime 引擎、加载 `paporot-core.wasm`、注册 3 个 Host Function。

这 3 个 Host Function 是沙盒和外部世界之间**唯一的通道**：

- `host_read_file(path) -> bytes`：宿主层决定要不要给、给哪个文件
- `host_write_file(path, content)`：宿主层决定能不能写、写到哪
- `host_llm_call(prompt) -> response`：宿主层持有 API key，沙盒调 LLM 必须通过它

这就是关键——**沙盒里的 WASM 代码不能直接碰文件系统，不能直接发 HTTP 请求，不能直接做任何系统调用。** 任何对外交互都必须通过这 3 个受控通道。

**WASM 沙盒层**里跑的是 `paporot-core.wasm`，编译目标 `wasm32-wasip1`，一个最小化的 WASI 子集。整个分析管线——6 个 Skill 的编排、状态机构建、向量计算、报告生成——全在这个沙盒里完成。

---

## Skill DAG 编排：一行命令，6 个 Skill 自动跑

Paporot 的分析不是一个大单体，而是拆成 6 个独立 Skill。每个 Skill 是一个独立的 WASM 模块，有 `skill.toml` 声明自己的名字、描述、输出 schema、以及**依赖谁的输出**。

**实际项目中，`.Paporot/skills/` 目录下有 6 个子目录**，每个子目录放一个 `skill.wasm` 和一个 `skill.toml`。你运行 `paporot analyze`，Paporot 自动扫描这个目录，加载所有兼容的 Skill。

6 个 Skill 之间不是并行的，有严格的依赖关系。比如 L3 依赖分析需要 L2 模块发现的输出才知道谁依赖谁，L2 又需要 L1 仓库理解的结果才能把文件聚类成模块。

所以 Paporot 的做法是：**用 Kahn 算法做 DAG 拓扑排序，得到执行层级，同层可以并行，不同层严格串行。** 这个拓扑图在图中有代码证据。

逐层看下来：

- **L1 仓库理解 (repository-understanding)**：识别项目目标、技术栈、入口文件。实际代码里会列出所有框架依赖、语言占比、架构风格
- **L2 模块发现 (module-discovery)**：把文件聚类成业务模块和技术基础设施。会标注每个模块的文件数、职责描述
- **L3 依赖分析 (dependency-analysis)**：构建模块依赖图，算耦合、检测循环依赖
- **L4 执行流分析 (runtime-flow-analysis)**：追踪端到端业务流程，标注副作用（API 调用、数据变更），按风险等级分类
- **L5 行为边界 (behavior-boundary)**：区分核心组件和支撑组件，标注每个模块的边界类型
- **L6 architecture-doc-generator**：这是**总指挥 Skill**。它不自己做分析，而是**消费 L1 到 L5 的全部结构化输出**——模块列表、依赖矩阵、执行流路径、行为边界分类——然后组合成面向不同受众的三份最终报告：JSON（给 CI/CD 消费）、Markdown（给开发者阅读）、HTML Dashboard（给团队共享）

**整个流程完全自动。用户只输一行 `paporot analyze`，剩下的从 DAG 编排到报告生成，没有任何人工干预点。** analyze 命令甚至连可选参数都只有三个：`--prd`（如果你有 PRD 文档要注入覆盖率对比）、`--input`（额外 key=value 输入）、`--api-key`（LLM 密钥）。

---

## 三层行为版本控制：这是 Paporot 最核心的差异化能力

Git 告诉你代码从 v1 变成 v2 改了什么。但 Agent 的行为怎么版本控制？

Paporot 设计了三层，从结构到几何到网络，逐层深入。这是整个项目最核心也最复杂的设计，花点时间讲清楚。

---

### P0：结构层——行为状态机

P0 把你的 Trace 序列抽象成一个**有向状态图**。

想象一下——Agent 的一次执行，tool call 是一条时间线：先 read 几个文件，然后 start a bash server，然后 write 修改，然后 search，然后 write 更多。P0 做的事情就是把这串线性序列**分段**，识别出"这 8 个 read 操作属于一个定位阶段"，"这 6 个 write/edit 属于一个修改阶段"，"这 3 个 search 属于一个探索阶段"。

怎么做的？三层 pipeline：

**第一层，硬分割。** `RuleSegmenter` 按 tool 名称的类别做硬切——read/search/grep 是"locate"类，write/edit/delete 是"modify"类，test/build/lint 是"verify"类，commit/push 是"commit"类。类别变化就是一个切割点。

**第二层，窗口候选。** `WindowBuilder` 在切割后的段内部用滑动窗口找更细粒度的候选状态。每个窗口内的 tool 序列有一个主导 phase 标签。

**第三层，相邻合并。** `AdjacentMerger` 把相邻的相似候选状态合并。比如"locate"后面紧挨着另一个"locate"，只是参数不同——合并成一个状态。

最后构建成 `BehaviorStateGraph`：节点是状态（有 phase 标签和工具数量），边是状态转移。这个图就是你这次执行的行为签名。

**两次执行之间做图结构 diff**——哪些节点新增了、哪些删除了、哪些边变了——这就是 P0 层面回答"行为变了什么"。

---

### P1：几何层——10 维轨迹向量

P0 告诉你"结构变了"，但结构变化有好有坏。怎么量化"变好还是变坏"？

P1 把一次执行的 Trajectory 映射到一个**数值向量空间**。

当前版本的 `TrajectoryVector` 有 7 个标量字段加 2 个分布向量，共计 10 个可比较维度：

- **分布类维度**：`tool_distribution`（工具类别分布向量）、`state_distribution`（状态分布向量）——你的 Agent 是"花 60% 时间在定位、30% 在修改"还是"花 10% 定位、80% 在反复试"？
- **熵类维度**：`tool_entropy`（工具序列的混乱度）、`phase_entropy`（阶段转换的不可预测性）、`transition_entropy`（状态转移的随机程度）——你的 Agent 有规律地切换阶段，还是在乱跳？
- **模式维度**：`loop_ratio`（循环占比，Agent 在多轮迭代圈里绕了几次）、`backtrack_ratio`（回溯比例，修改完又回去读之前的文件）、`burst_ratio`（突发性，所有 tool call 集中在短时间窗口）
- **稳定性维度**：`state_stability_score`（两次执行之间的余弦相似度——直接用数值量化行为一致性）

所有这些值都经过 **D7 规范化的 normalization pipeline**：entropy 做 bounded normalization（除以理论最大值得到 [0,1] 区间），ratios 直接 clamp 到 [0,1]，burst 做 log compression 处理重尾。

注意代码里的注释：**禁止 whitening**。为什么？因为向量轴的语义可解释性是 P1 的核心价值。你不需要一个抽象的主成分，你需要能说"这次的 tool_entropy 从 2.3 升到 3.8，Agent 的工具使用变混乱了"。whitening 会破坏这个可解释性。

---

### P2：网络层——行为耦合图

P1 告诉你"单个 Agent 的行为向量变了"，但一个项目有很多能力区——auth、payment、dashboard、inventory——它们之间的行为有没有联动？

P2 构建的是 **capability 之间的耦合关系**网络。

核心公式在代码里直接写着（D9）：
```
correlation_score = cochange_score × (1 + λ × similarity_score)
```

- **cochange_score**：两个 capability 的文件在同一批 commit 里被一起修改的频率。由 3 层 log-saturated 证据加权：commit 层（同一 commit）+ file 层（同文件）+ session 层（同 session）
- **similarity_score**：两个 capability 的 P1 轨迹向量的余弦相似度（`cosine_sim`）
- **λ ∈ [0.2, 0.4]**：相似度调制因子，防止 similarity 过度放大 cochange，同时又让行为模式相近的能力在耦合图中更近

最后通过 4 层存活过滤（hard → purity → stability → top-K）剪枝，只保留最稳定的耦合边。

这个耦合图告诉你什么？如果一个 Auth 模块的改动高频牵连 Payment 和 Dashboard 两个模块，你就知道——**这个能力是架构瓶颈，改它要小心。** 这就是 P2 的工程价值：不是理论上的"耦合度"，而是**从 Agent 实际行为中自动推导出来的耦合关系**。

---

## 确定性核心 + LLM 辅助

有一点我想特别强调：Paporot 的分析管线在核心部分是**完全确定性**的。

P0 的状态机构建（`build_state_graph`）——确定性的规则引擎，没有 LLM 参与。
P1 的向量计算（`build_vector`）——确定性的数值变换，没有 LLM 参与。
P2 的耦合图构建（`CouplingBuilder::build_edges`）——确定性的公式计算，没有 LLM 参与。
TrajectoryDiff 到 TrajectoryAnalysis 的转换——代码注释直接写"纯确定性计算，不调用 LLM"。

LLM 的入口点是 L6 architecture-doc-generator，它把 L1-L5 的结构化数据"翻译"成人类可读的自然语言——写一段项目概述、总结分析发现、提出改进建议。LLM 是可选的。没有 API key，Paporot 照常输出完整的 JSON/MD/HTML 报告，只是自然语言叙述部分质量下降。

这个设计决策很重要：**你不希望行为审计工具本身就有不确定性。** L1-L5 的结果每次跑出来一定是一样的——同样的 Trace 输入，同样的分析输出。LLM 只在最后一公里的呈现层介入。

---

## 安全模型

Paporot 分析的是 AI Agent 写的代码。这意味着 Paporot 要读取项目文件、可能需要将代码内容通过 LLM 调用发送出去。如果 Paporot 本身不安全，你就是在用一个不安全的工具去审计另一个 Agent——这本身就是一个笑话。

安全模型有三个层次：

1. **WASM 沙盒硬隔离**。所有分析逻辑编译到 `wasm32-wasip1` 目标，在 wasmtime 引擎内运行。WASM 无法直接访问文件系统、网络、系统调用
2. **3 个受控 Host Function**。`host_read_file`、`host_write_file`、`host_llm_call`——任何对外交互都必须显式通过这三个函数，宿主层做访问控制和审计
3. **零持久化沙盒内状态**。每次 `paporot analyze` 都是从零加载 wasm，不存在跨执行状态泄露风险

---

## 最终输出：Dashboard

分析完的结果不是扔一堆 JSON 给你自己翻。Paporot 自动生成三份报告：

- `report.json`：完整的结构化数据，给 CI/CD 系统消费。包含每个 Skill 的输出 raw JSON、DAG 层级、风险评分
- `report.md`：开发者友好的分析报告，有模块列表、依赖关系、执行流分析、行为边界分类、风险因素和缓解建议
- `dashboard.html`：单文件自包含的 HTML Dashboard。翻到 Page 8 你看到的就是一个真实项目的分析结果——Family Inventory Warehouse。6 个 Skill 全 PASS（绿色），分析出 6 个模块、6 条执行流，风险等级 HIGH——因为缺少认证、只有客户端验证。Dashboard 用 Mermaid 渲染依赖图和流程图，暗色主题，可以直接嵌入项目文档或团队 Wiki。

---

## 怎么用

三步：

```bash
# 1. 编译 WASM core
cargo build -p paporot-core --target wasm32-wasip1 --release

# 2. 编译 native binary
cargo build --release

# 3. 初始化 + 分析
Paporot init
Paporot analyze
```

前置：Rust 1.96+、wasm32-wasip1 编译目标。LLM API Key 可选。

Paporot 有 16 个子命令，不只是 `analyze`。你可以单独跑任何一个 Skill 的对应命令：`Paporot trace import` 导入 Trace、`Paporot trajectory diff` 对比两次执行、`Paporot state build` 只做状态机构建、`Paporot coupling build` 只做耦合图——完全模块化。

---

## 总结

回到一开始的问题：**AI Agent 写了一堆代码，你除了看代码 diff，怎么知道它的行为变好还是变坏？**

Paporot 的答案是三层递进：

- **P0 告诉你"Agent 的行为结构变了"**——状态节点多了少了、转移路径改了
- **P1 告诉你"这种变化是改善还是退化"**——entropy 是升了还是降了、loop_ratio 是大了还是小了、stability 变好了还是变糟了
- **P2 告诉你"改了 A，B 和 C 会受影响吗"**——capability 之间有没有行为层面的耦合，动一个会不会牵动一片

这一切全自动，一行命令触发。跑在 WASM 沙盒里，安全边界只有 3 个受控 Host Function。既回答了"改了什么能力"，也回答了"行为变好还是变坏"。

用 Rust 构建，开源 MIT 协议。

**谢谢，有问题随时聊。**
