//! 分析层内部类型定义
//!
//! L1 AST → L2 规则 → L3 LLM 各阶段共用的数据结构。

use serde::{Deserialize, Serialize};

// ─── 语言枚举 ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    Rust,
    TypeScript,
    JavaScript,
    Python,
    Go,
    Java,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "rs" => Language::Rust,
            "ts" | "tsx" => Language::TypeScript,
            "js" | "jsx" | "mjs" => Language::JavaScript,
            "py" => Language::Python,
            "go" => Language::Go,
            "java" => Language::Java,
            _ => Language::Unknown,
        }
    }

    pub fn from_filename(path: &str) -> Self {
        let ext = std::path::Path::new(path)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        Self::from_extension(&ext)
    }
}

// ─── Diff 预处理类型 ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub language: Language,
    pub kind: ChangeKind,
    pub hunks: Vec<Hunk>,
    pub old_content: Option<String>,
    pub new_content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Deleted,
    Modified,
    Renamed { from: String, to: String },
}

#[derive(Debug, Clone)]
pub struct Hunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    Context(String),
    Addition(String),
    Deletion(String),
}

#[derive(Debug, Clone)]
pub struct DiffSummary {
    pub files_changed: usize,
    pub additions: usize,
    pub deletions: usize,
    pub by_language: Vec<(Language, usize)>,
}

// ─── L1 AST 产出 ───────────────────────────────────────────────────────

/// 确定性分析产出的原始变更
#[derive(Debug, Clone)]
pub struct RawChange {
    pub id: String,
    pub source: ChangeSource,
    pub change_type: ChangeType,
    pub file_path: String,
    pub language: Language,
    pub line_start: usize,
    pub line_end: usize,
    pub symbol_name: String,
    pub old_signature: Option<String>,
    pub new_signature: Option<String>,
    pub confidence: f32,
    pub module: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeSource {
    /// L1 确定性 AST 分析
    Ast,
    /// L2 规则引擎命中
    Rule,
    /// L3 LLM 推断
    Llm,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChangeType {
    // 函数级
    FunctionAdded,
    FunctionRemoved,
    FunctionSignatureChanged,
    // 结构体/类
    StructAdded,
    StructFieldAdded,
    StructFieldChanged,
    StructFieldRemoved,
    // 枚举
    EnumAdded,
    EnumVariantAdded,
    EnumVariantRemoved,
    // 接口/trait
    TraitAdded,
    TraitMethodAdded,
    TraitMethodChanged,
    // HTTP
    HttpRouteAdded,
    HttpRouteChanged,
    HttpRouteRemoved,
    // 依赖
    ImportAdded,
    ImportRemoved,
    // 配置/常量
    ConstantAdded,
    ConstantChanged,
    ConstantRemoved,
    // 错误
    ErrorVariantAdded,
    ErrorVariantRemoved,
    // 其他
    ConfigFileChanged,
    DependencyVersionChanged,
    DocOnly,
    UnknownChange,
}

impl ChangeType {
    pub fn label(&self) -> &str {
        match self {
            ChangeType::FunctionAdded => "函数新增",
            ChangeType::FunctionRemoved => "函数删除",
            ChangeType::FunctionSignatureChanged => "函数签名变更",
            ChangeType::StructAdded => "结构体新增",
            ChangeType::StructFieldAdded => "结构体字段新增",
            ChangeType::StructFieldChanged => "结构体字段变更",
            ChangeType::StructFieldRemoved => "结构体字段删除",
            ChangeType::EnumAdded => "枚举新增",
            ChangeType::EnumVariantAdded => "枚举变体新增",
            ChangeType::EnumVariantRemoved => "枚举变体删除",
            ChangeType::TraitAdded => "Trait 新增",
            ChangeType::TraitMethodAdded => "Trait 方法新增",
            ChangeType::TraitMethodChanged => "Trait 方法变更",
            ChangeType::HttpRouteAdded => "HTTP 路由新增",
            ChangeType::HttpRouteChanged => "HTTP 路由变更",
            ChangeType::HttpRouteRemoved => "HTTP 路由删除",
            ChangeType::ImportAdded => "依赖导入新增",
            ChangeType::ImportRemoved => "依赖导入删除",
            ChangeType::ConstantAdded => "常量新增",
            ChangeType::ConstantChanged => "常量变更",
            ChangeType::ConstantRemoved => "常量删除",
            ChangeType::ErrorVariantAdded => "错误类型新增",
            ChangeType::ErrorVariantRemoved => "错误类型删除",
            ChangeType::ConfigFileChanged => "配置文件变更",
            ChangeType::DependencyVersionChanged => "依赖版本变更",
            ChangeType::DocOnly => "仅文档变更",
            ChangeType::UnknownChange => "未分类变更",
        }
    }

    /// 是否属于破坏性变更类型
    pub fn is_breaking(&self) -> bool {
        matches!(
            self,
            ChangeType::FunctionRemoved
                | ChangeType::FunctionSignatureChanged
                | ChangeType::StructFieldRemoved
                | ChangeType::EnumVariantRemoved
                | ChangeType::TraitMethodChanged
                | ChangeType::HttpRouteRemoved
                | ChangeType::HttpRouteChanged
                | ChangeType::ConstantRemoved
                | ChangeType::ImportRemoved
        )
    }
}

// ─── L2 规则类型 ───────────────────────────────────────────────────────

use crate::types::Severity;

#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub category: RuleCategory,
    pub severity: Severity,
    pub trigger: RuleTrigger,
    pub tags: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleCategory {
    Security,
    Breaking,
    Performance,
    Deprecation,
    Domain,
}

#[derive(Debug, Clone)]
pub enum RuleTrigger {
    SymbolMatches { pattern: String },
    ChangeTypeIn(Vec<ChangeType>),
    FilePathMatches { pattern: String },
    ContentContains { pattern: String },
    And(Box<RuleTrigger>, Box<RuleTrigger>),
    Or(Box<RuleTrigger>, Box<RuleTrigger>),
    Not(Box<RuleTrigger>),
}

#[derive(Debug, Clone)]
pub struct RuleMatch {
    pub rule_id: String,
    pub raw_change_id: String,
    pub matched_tags: Vec<String>,
    pub severity: Severity,
    pub category: RuleCategory,
    pub description: String,
}

// ─── L3 LLM 分片 ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LlmFragment {
    pub fragment_id: String,
    pub content: String,
    pub file_paths: Vec<String>,
    pub raw_json: Option<String>,
}

// ─── 聚合结果 ──────────────────────────────────────────────────────────

/// L1+L2+L3 合并后的中间产出
#[derive(Debug, Clone)]
pub struct MergedCapability {
    pub name: String,
    pub description: String,
    pub module: Option<String>,
    pub confidence: f32,
    pub source: ChangeSource,
    pub raw_changes: Vec<RawChange>,
    pub rule_matches: Vec<RuleMatch>,
    pub tags: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Language 接口测试 ──────────────────────────────────────────

    /// 测试项: Language::from_extension 标准扩展名
    /// 输入: "rs"/"ts"/"py"/"go"/"java"/"unknown_ext"
    /// 预期: 正确映射
    #[test]
    fn test_language_from_extension_known() {
        assert_eq!(Language::from_extension("rs"), Language::Rust);
        assert_eq!(Language::from_extension("ts"), Language::TypeScript);
        assert_eq!(Language::from_extension("tsx"), Language::TypeScript);
        assert_eq!(Language::from_extension("py"), Language::Python);
        assert_eq!(Language::from_extension("go"), Language::Go);
        assert_eq!(Language::from_extension("java"), Language::Java);
    }

    /// 测试项: Language::from_extension 未知扩展名
    /// 输入: "toml"
    /// 预期: Language::Unknown
    #[test]
    fn test_language_from_extension_unknown() {
        assert_eq!(Language::from_extension("toml"), Language::Unknown);
        assert_eq!(Language::from_extension(""), Language::Unknown);
    }

    /// 测试项: Language::from_filename 从路径推断
    /// 输入: "src/main.rs"/"app/index.ts"/"lib/util.py"
    /// 预期: 正确推断
    #[test]
    fn test_language_from_filename() {
        assert_eq!(Language::from_filename("src/main.rs"), Language::Rust);
        assert_eq!(Language::from_filename("app/index.ts"), Language::TypeScript);
        assert_eq!(Language::from_filename("lib/util.py"), Language::Python);
        assert_eq!(Language::from_filename("unknown"), Language::Unknown);
    }

    // ─── ChangeType 接口测试 ────────────────────────────────────────

    /// 测试项: ChangeType::is_breaking 破坏性变更识别
    /// 输入: 各类 ChangeType 变体
    /// 预期: 9 种破坏性（含 Removed/Changed 变体），其余非破坏性
    #[test]
    fn test_change_type_is_breaking() {
        // 破坏性:
        assert!(ChangeType::FunctionRemoved.is_breaking());
        assert!(ChangeType::FunctionSignatureChanged.is_breaking());
        assert!(ChangeType::StructFieldRemoved.is_breaking());
        assert!(ChangeType::EnumVariantRemoved.is_breaking());
        assert!(ChangeType::TraitMethodChanged.is_breaking());
        assert!(ChangeType::HttpRouteRemoved.is_breaking());
        assert!(ChangeType::HttpRouteChanged.is_breaking());
        assert!(ChangeType::ConstantRemoved.is_breaking());
        assert!(ChangeType::ImportRemoved.is_breaking());

        // 非破坏性:
        assert!(!ChangeType::FunctionAdded.is_breaking());
        assert!(!ChangeType::StructAdded.is_breaking());
        assert!(!ChangeType::EnumAdded.is_breaking());
        assert!(!ChangeType::DocOnly.is_breaking());
        assert!(!ChangeType::UnknownChange.is_breaking());
    }

    /// 测试项: ChangeType::label 中文标签
    /// 输入: 每个变体
    /// 预期: 返回非空中文字符串
    #[test]
    fn test_change_type_label_not_empty() {
        use ChangeType::*;
        let all = [
            FunctionAdded, FunctionRemoved, FunctionSignatureChanged,
            StructAdded, StructFieldAdded, StructFieldChanged, StructFieldRemoved,
            EnumAdded, EnumVariantAdded, EnumVariantRemoved,
            TraitAdded, TraitMethodAdded, TraitMethodChanged,
            HttpRouteAdded, HttpRouteChanged, HttpRouteRemoved,
            ImportAdded, ImportRemoved,
            ConstantAdded, ConstantChanged, ConstantRemoved,
            ErrorVariantAdded, ErrorVariantRemoved,
            ConfigFileChanged, DependencyVersionChanged, DocOnly, UnknownChange,
        ];
        for ct in &all {
            assert!(!ct.label().is_empty(), "{:?}.label() 不应为空", ct);
        }
    }

    // ─── RawChange 构造测试 ─────────────────────────────────────────

    /// 测试项: RawChange 构造和字段访问
    /// 输入: 手动构造的 RawChange
    /// 预期: 所有字段可访问，值正确
    #[test]
    fn test_raw_change_construction() {
        let rc = RawChange {
            id: "rc1".into(),
            source: ChangeSource::Ast,
            change_type: ChangeType::FunctionAdded,
            file_path: "src/lib.rs".into(),
            language: Language::Rust,
            line_start: 10, line_end: 12,
            symbol_name: "hello".into(),
            old_signature: None,
            new_signature: Some("fn hello()".into()),
            confidence: 0.95,
            module: Some("lib".into()),
            tags: vec!["public".into()],
        };
        assert_eq!(rc.id, "rc1");
        assert_eq!(rc.symbol_name, "hello");
        assert_eq!(rc.confidence, 0.95);
        assert_eq!(rc.language, Language::Rust);
    }

    // ─── RuleTrigger 枚举构造测试 ───────────────────────────────────

    /// 测试项: RuleTrigger 组合逻辑构造
    /// 输入: SymbolMatches + ChangeTypeIn 的 And 组合
    /// 预期: 构造不崩溃，类型正确
    #[test]
    fn test_rule_trigger_composition() {
        let trigger = RuleTrigger::And(
            Box::new(RuleTrigger::SymbolMatches { pattern: "auth".into() }),
            Box::new(RuleTrigger::ChangeTypeIn(vec![ChangeType::FunctionAdded])),
        );
        // 验证 variant 匹配
        match &trigger {
            RuleTrigger::And(left, right) => {
                match left.as_ref() {
                    RuleTrigger::SymbolMatches { pattern } => assert_eq!(pattern, "auth"),
                    _ => panic!("左分支应为 SymbolMatches"),
                }
                match right.as_ref() {
                    RuleTrigger::ChangeTypeIn(types) => assert!(types.contains(&ChangeType::FunctionAdded)),
                    _ => panic!("右分支应为 ChangeTypeIn"),
                }
            }
            _ => panic!("应为 And"),
        }
    }

    /// 测试项: RuleTrigger Not 组合
    /// 输入: Not(SymbolMatches("test"))
    /// 预期: 结构正确
    #[test]
    fn test_rule_trigger_not() {
        let trigger = RuleTrigger::Not(
            Box::new(RuleTrigger::FilePathMatches { pattern: "_test.rs".into() }),
        );
        match &trigger {
            RuleTrigger::Not(inner) => match inner.as_ref() {
                RuleTrigger::FilePathMatches { pattern } => assert_eq!(pattern, "_test.rs"),
                _ => panic!("内层应为 FilePathMatches"),
            },
            _ => panic!("应为 Not"),
        }
    }
}
