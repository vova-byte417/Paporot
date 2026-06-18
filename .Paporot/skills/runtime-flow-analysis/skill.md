# Runtime Flow Analysis

## 目标
发现端到端业务执行路径（从入口到出口），标注每阶段职责（输入/校验/业务逻辑/持久化/输出），识别系统 I/O 副作用。输出供 Behavior Boundary Discovery 和 Architecture Doc Generator 消费。

## 输入
| 输入名 | 类型 | 说明 |
|--------|------|------|
| `ast` | String | AST 结构信息（可选，辅助理解） |
| `call_graph` | String | 函数调用图（`caller -> callee`） |
| `entry_points` | String | 入口函数列表（每行一个） |

## 输出 Schema
```json
{
  "flows": [
    {
      "name": "string (human-readable description)",
      "trigger": "CLI|HTTP|MQ|Timer|Event",
      "entry_point": "string",
      "path": ["string"],
      "phases": {
        "input": ["string"],
        "validation": ["string"],
        "business_logic": ["string"],
        "persistence": ["string"],
        "output": ["string"]
      },
      "side_effect": "string"
    }
  ],
  "mermaid": "string (Mermaid flowchart TD)",
  "flow_count": 0
}
```

## 执行流程
1. 解析 `entry_points`，获取所有入口函数
2. 对每个入口，从 `call_graph` 中 DFS 追踪调用链（最大深度 20，防死循环）
3. 根据函数名关键词将调用链节点分类到 5 个 phase：
   - `input`: read/parse/input
   - `validation`: valid/check/verify
   - `business_logic`: 默认分类
   - `persistence`: save/write/store/db
   - `output`: output/print/render/display
4. 调用 LLM，提交路径信息，要求为每条 flow 生成人类可读的描述名
5. 生成 Mermaid 流程图
6. 输出结构化 JSON

## 依赖
- **上游**: dependency-analysis（通过 cache 获取依赖上下文）
- **与上游的关系**: 在模块依赖图的基础上，追踪代码执行路径

## LLM 调用
- **次数**: 1
- **模型**: deepseek-pro
- **用途**: 为每条发现的执行路径生成简洁的描述名称

## 参考
- Use Case driven approach (Ivar Jacobson)
- Event Storming - identifying commands, events, and aggregates
- Sequence Diagram / Activity Diagram for flow visualization
