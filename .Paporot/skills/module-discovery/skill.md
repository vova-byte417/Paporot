# Module Discovery

## 目标
发现系统中的业务模块和技术模块，聚类相关文件并推断每个模块的职责。输出模块目录供 Dependency Analysis 和 Architecture Doc Generator 消费。

## 输入
| 输入名 | 类型 | 说明 |
|--------|------|------|
| `repo_tree` | JSON | 仓库目录结构 |
| `ast_symbols` | String | AST 符号表（导出的 struct / trait / class / function） |
| `import_graph` | String | 跨文件 import 关系 |

## 输出 Schema
```json
{
  "modules": [
    {
      "name": "string",
      "responsibility": "string (max 2 sentences)",
      "files": ["string"],
      "category": "Service|API|Domain|Storage|Infrastructure|Utility",
      "public_symbols": ["string"],
      "file_count": 0
    }
  ],
  "module_count": 0,
  "file_count": 0
}
```

## 执行流程
1. 解析 `repo_tree`，按目录分组文件
2. 根据目录名预分类（src→Service, api→API, db→Storage 等）
3. 提取每个目录下的公开符号
4. 调用 LLM，提交目录分组 + AST 符号 + import 图，要求 LLM 为每个模块生成 2 句职责描述
5. 合并 LLM 结果到模块列表中
6. 输出结构化 JSON

## 依赖
- **上游**: repository-understanding（通过 cache 获取）
- **与上游的关系**: 在理解全局项目目标后，按模块细化

## LLM 调用
- **次数**: 1
- **模型**: deepseek-pro
- **用途**: 根据文件分组和符号信息，推断每个模块的业务职责

## 参考
- Domain-Driven Design - Bounded Context identification
- Package by Feature vs Package by Layer 组织原则
