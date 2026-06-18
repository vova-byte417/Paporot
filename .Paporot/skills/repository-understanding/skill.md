# Repository Understanding

## 目标
识别项目的整体目标、技术栈、入口程序、核心业务能力。作为 Skill Pipeline 的第一环，为下游所有分析提供全局上下文。

## 输入
| 输入名 | 类型 | 说明 |
|--------|------|------|
| `repo_tree` | JSON | 仓库目录树结构（文件路径、类型、大小） |
| `repo_files` | String | 关键文件内容摘要（Cargo.toml / package.json / go.mod 等） |
| `git_meta` | String | Git 元数据（分支、最近 commit、贡献者等） |

## 输出 Schema
```json
{
  "project_name": "string",
  "purpose": "string (1-2 sentences)",
  "languages": ["string"],
  "frameworks": ["string"],
  "architecture_style_candidates": ["modular_pipeline|layered|hexagonal|microservices|monolithic|cli_application"],
  "entrypoints": ["string"],
  "evidence": [
    {
      "source_file": "string",
      "finding": "string"
    }
  ]
}
```

## 执行流程
1. 解析 `repo_tree` JSON，提取目录/文件清单
2. 从 `repo_files` 中读取构建配置文件，推断语言和框架
3. 扫描通用入口文件模式（main.rs / main.go / index.ts 等）
4. 调用 LLM，将上述结构信息 + git_meta 提交，要求 LLM 推断项目用途、候选架构风格
5. 汇总为结构化 JSON 输出

## 依赖
无（第一环，无上游依赖）

## LLM 调用
- **次数**: 1
- **模型**: deepseek-pro
- **用途**: 基于仓库结构和元数据，推断项目目的与技术栈

## 参考
- Martin Fowler - Architecture Description
- ISO/IEC 42010 - Systems and software engineering — Architecture description
