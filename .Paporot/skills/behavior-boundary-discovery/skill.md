# Behavior Boundary Discovery

## 目标
发现影响可观测行为（用户可见、API 契约、数据格式）的组件边界，区分行为核心模块与支撑/内部模块。通过 Git diff 分析变更，分类为行为变更 vs 非行为变更，并评估风险等级。

## 输入
| 输入名 | 类型 | 说明 |
|--------|------|------|
| `ast` | String | AST 结构（可选） |
| `git_diff` | String | 最新 commit 的 unified diff |
| `call_graph` | String | 函数调用图 |

## 输出 Schema
```json
{
  "behavioral_modules": ["string"],
  "non_behavioral_modules": ["string"],
  "behavioral_functions": [
    {
      "name": "string",
      "module": "string",
      "output_type": "unknown|api_response|cli_output|data_persistence|log_entry"
    }
  ],
  "non_behavioral_functions": [
    {
      "name": "string",
      "module": "string",
      "reason": "logging|metrics|tracing|cache|formatting|internal"
    }
  ],
  "changed_boundaries": [
    {
      "function": "string",
      "change_type": "modified|added|deleted",
      "user_visible": true,
      "risk": "low|medium|high"
    }
  ],
  "boundary_summary": "string",
  "risk_level": "low|medium|high"
}
```

## 执行流程
1. 解析 `git_diff`，从 `@@` 标记中提取变更的函数名
2. 按函数名初步分类：
   - 非行为：log/metric/trace/format/cache/debug
   - 行为：其他
3. 调用 LLM，提交变更函数列表 + 部分调用图，要求 LLM 二次确认分类并评估风险
4. 合并 LLM 分类结果
5. 生成变更边界 + 风险摘要
6. 输出结构化 JSON

## 依赖
- **上游**: runtime-flow-analysis（通过 cache 获取 flow 上下文）
- **与上游的关系**: 在理解完整执行路径后，定位行为变更发生的位置

## LLM 调用
- **次数**: 1
- **模型**: deepseek-pro
- **用途**: 二次确认行为/非行为分类，评估整体风险等级

## 参考
- Michael Nygard - Release It!（稳定性和反脆弱性模式）
- Behavioral vs Structural change classification
- Change Risk Analysis - Facebook Sapienz approach
