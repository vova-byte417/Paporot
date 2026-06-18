# Dependency Analysis

## 目标
构建模块级依赖图，计算扇入/扇出耦合指标，检测循环依赖和架构违规。结果供 Architecture Doc Generator 和 Dashboard 消费。

## 输入
| 输入名 | 类型 | 说明 |
|--------|------|------|
| `import_graph` | String | 模块间 import 关系（`A -> B` 每行一条） |
| `symbol_references` | String | 跨模块符号引用 |
| `call_graph` | String | 函数调用图 |

## 输出 Schema
```json
{
  "dependencies": [
    {"from": "string", "to": "string", "type": "import"}
  ],
  "cycles": [
    {
      "modules": ["string"],
      "length": 0
    }
  ],
  "high_coupling_modules": [
    {
      "name": "string",
      "fan_in": 0,
      "fan_out": 0,
      "risk": "low|medium|high"
    }
  ],
  "architecture_violations": [],
  "mermaid": "string (Mermaid graph TD)",
  "total_dependencies": 0
}
```

## 执行流程
1. 解析 `import_graph`，构建有向边列表
2. 计算每个模块的 fan-in（被依赖次数）和 fan-out（依赖次数）
3. DFS 检测循环依赖（记录环路径和长度）
4. 标记高耦合模块（fan-in + fan-out > 10 为中风险，> 20 为高风险）
5. 生成 Mermaid 依赖图
6. 输出结构化 JSON

## 依赖
- **上游**: module-discovery（通过 cache 获取模块列表）
- **与上游的关系**: 在已识别的模块基础上，分析模块间的依赖方向和强度

## LLM 调用
- **次数**: 0（纯算法计算，不需要 LLM）
- **原因**: 图算法（DFS 环检测、扇入扇出统计）由 Rust 代码直接完成

## 参考
- John Lakos - Large-Scale C++ Software Design（层级依赖原则）
- Acyclic Dependencies Principle (ADP)
- Stable Dependencies Principle (SDP)
