# Architecture Document Generator

## 目标
聚合所有上游 Skill 的输出，生成最终的架构文档。这是 Skill Pipeline 的最后一环，负责汇总和呈现分析结果。

## 输入
| 输入名 | 来源 | 说明 |
|--------|------|------|
| `skill_output__repository-understanding` | repository-understanding | 项目概览 |
| `skill_output__module-discovery` | module-discovery | 模块目录 |
| `skill_output__dependency-analysis` | dependency-analysis | 依赖图 |
| `skill_output__runtime-flow-analysis` | runtime-flow-analysis | 执行流 |
| `skill_output__behavior-boundary-discovery` | behavior-boundary-discovery | 行为边界 |

## 输出 Schema
```json
{
  "generated_files": [
    ".paporot/reports/architecture.md",
    ".paporot/reports/behavior.md",
    ".paporot/reports/data/analysis_result.json"
  ],
  "sections_status": {
    "project_overview": "ok|skipped",
    "module_catalog": "ok|skipped",
    "dependency_graph": "ok|skipped",
    "runtime_flows": "ok|skipped",
    "behavioral_components": "ok|skipped",
    "coverage": "skipped"
  },
  "summary": "string",
  "high_level_summary": "string (2-3 paragraphs)",
  "sections": [
    {
      "id": "string",
      "title": "string",
      "status": "ok|skipped",
      "data": "string | null"
    }
  ]
}
```

## 执行流程
1. 读取所有上游 Skill 输出（通过 input 机制自动获取）
2. 为每个上游 Skill 创建对应 section（带 OK/Skipped 标记）
3. 统计完成/跳过的 section 数量
4. 调用 LLM，提交所有上游分析结果，要求 LLM 生成 2-3 段高层架构总结
5. 生成最终的 sections_status 和 summary
6. 输出结构化 JSON（供 Dashboard 渲染）

## 依赖
- **上游**: repository-understanding, module-discovery, dependency-analysis, runtime-flow-analysis, behavior-boundary-discovery
- **与上游的关系**: 聚合所有上游结果，无新分析

## LLM 调用
- **次数**: 1
- **模型**: deepseek-pro
- **用途**: 综合所有分析结果，生成高层架构总结

## 参考
- arc42 - Template for architecture documentation
- C4 Model - Context, Container, Component, Code
- Google SRE - Architecture Review process
