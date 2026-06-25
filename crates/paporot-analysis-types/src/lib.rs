//! Paporot Analysis Shared Types
//!
//! Shared type definitions for the Paporot analysis pipeline.
//! Used by both `paporot-core` (wasm32-wasip1) and the native binary (x86_64).

use serde::{Deserialize, Serialize};

// ─── Severity ──────────────────────────────────────────────────────

/// 严重程度（共享于核心类型和分析类型之间）
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    Low,
    Medium,
    High,
}

// ─── Language ──────────────────────────────────────────────────────

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

// ─── Diff Preprocessor Types ───────────────────────────────────────

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

// ─── L1 AST Output ─────────────────────────────────────────────────

/// 确定性分析产出的原始变更
#[derive(Serialize, Deserialize, Debug, Clone)]
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeSource {
    /// L1 确定性 AST 分析
    Ast,
    /// L2 规则引擎命中
    Rule,
    /// L3 LLM 推断
    Llm,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
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

// ─── L2 Rule Types ─────────────────────────────────────────────────

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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RuleMatch {
    pub rule_id: String,
    pub raw_change_id: String,
    pub matched_tags: Vec<String>,
    pub severity: Severity,
    pub category: RuleCategory,
    pub description: String,
}

// ─── L3 LLM Fragments ──────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LlmFragment {
    pub fragment_id: String,
    pub content: String,
    pub file_paths: Vec<String>,
    pub raw_json: Option<String>,
}

// ─── Aggregated Results ─────────────────────────────────────────────

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

// ─── Capability / Snapshot Core Types ──────────────────────────────

/// 最小可理解的行为单元
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Capability {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: CapabilityStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_modules: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<BehaviorContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preconditions: Vec<Condition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub postconditions: Vec<Condition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invariants: Vec<Condition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<CapabilityCategory>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<DependsOn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depended_by: Vec<DependedBy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evolved_from: Option<CapabilityRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_trace_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<String>,
    // ── v3: 溯源字段（从 RawChange 反填） ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_change_type: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggered_by_rules: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityStatus {
    New,
    Modified,
    Deleted,
    Unchanged,
}

impl Capability {
    pub fn status_name(&self) -> &str {
        match self.status {
            CapabilityStatus::New => "新增",
            CapabilityStatus::Modified => "修改",
            CapabilityStatus::Deleted => "删除",
            CapabilityStatus::Unchanged => "未变化",
        }
    }
}

// ─── BehaviorContract ─────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BehaviorContract {
    HttpEndpoint { method: String, path_template: String, auth_required: bool },
    Function { name: String, visibility: String, is_async: bool },
    DataSchema { kind: SchemaKind, derives: Vec<String> },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SchemaKind {
    Struct,
    Enum,
    TypeAlias,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Condition {
    pub kind: ConditionKind,
    pub expression: String,
    pub severity: Severity,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConditionKind {
    Precondition,
    Postcondition,
    Invariant,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityCategory {
    Functional,
    Security,
    Performance,
    Ux,
    Operational,
    DataIntegrity,
}

// ─── Dependency Graph Types ───────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CapabilityRef {
    pub capability_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_version: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DependsOn {
    pub target: CapabilityRef,
    pub relation: DependencyRelation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RelationSource>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DependedBy {
    pub source: CapabilityRef,
    pub relation: DependencyRelation,
    pub confidence: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyRelation {
    Calls,
    ConsumesEvent,
    ReadsData,
    WritesData,
    PostconditionDepends,
    SharesState,
    ImplementsOrComposes,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationSource {
    AstInferred,
    RuleInferred,
    LlmInferred,
    Manual,
}

// ─── BehaviorSnapshot ─────────────────────────────────────────────

fn default_schema_version() -> u32 {
    3
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BehaviorSnapshot {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub version_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    pub timestamp: String,
    pub message: String,
    pub capabilities: Vec<Capability>,
    pub prd_coverage: PrdCoverage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regression: Option<Regression>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<RiskAssessment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl BehaviorSnapshot {
    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BehaviorDiff {
    pub from_version: String,
    pub to_version: String,
    pub timestamp: String,
    pub added: Vec<Capability>,
    pub modified: Vec<Capability>,
    pub deleted: Vec<Capability>,
    pub unchanged: Vec<Capability>,
    pub impact_summary: String,
    pub risks_and_notes: Vec<String>,
}

// ─── PrdCoverage ─────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PrdCoverage {
    pub percentage: f32,
    pub total_items: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub covered_items: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<PrdCoverageDetail>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PrdCoverageDetail {
    pub prd_id: String,
    pub requirement: String,
    pub status: CoverageStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mapped_capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CoverageStatus {
    Pass,
    Partial,
    Fail,
    NotDetected,
}

// ─── Regression ──────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Regression {
    pub status: RegressionStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detected_regressions: Vec<RegressionItem>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RegressionStatus {
    Pass,
    Fail,
    Warning,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegressionItem {
    pub workflow: String,
    pub previous_status: String,
    pub current_status: String,
    pub description: String,
    pub severity: Severity,
}

// ─── RiskAssessment ──────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RiskAssessment {
    pub level: RiskLevel,
    pub score: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub factors: Vec<RiskFactor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mitigations: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RiskFactor {
    pub category: String,
    pub description: String,
    pub severity: Severity,
}

// ─── Evidence Types ────────────────────────────────────────────────

/// 一个 Capability 的完整推断证据。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Evidence {
    /// 关联的 Capability ID
    pub capability_id: String,
    /// 关联的 Snapshot version ID
    pub snapshot_version: String,
    /// L1 AST 证据
    pub l1: Vec<L1Evidence>,
    /// L2 规则匹配证据
    pub l2: Vec<L2Evidence>,
    /// L3 LLM 证据（None 表示未配置 L3 provider）
    pub l3: Option<L3Evidence>,
    /// 三层置信度评分
    pub confidence: EvidenceConfidence,
    /// 证据生成时间
    pub generated_at: String,
}

/// L1 AST 层面的符号提取证据。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct L1Evidence {
    /// 符号名称
    pub symbol: String,
    /// 所在文件
    pub file_path: String,
    /// 行号
    pub line: usize,
    /// 符号类型
    pub kind: SymbolKind,
    /// 可见性
    pub visibility: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Implementation,
    Module,
}

/// L2 规则匹配证据。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct L2Evidence {
    /// 匹配的规则 ID
    pub rule_id: String,
    /// 规则名称
    pub rule_name: String,
    /// 被匹配的 L1 符号
    pub matched_symbol: String,
    /// 触发的文件变更
    pub file_change: String,
    /// 匹配原因描述
    pub reason: String,
    /// 严重程度: critical / high / medium / low
    pub severity: String,
}

/// L3 LLM 推断证据。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct L3Evidence {
    /// LLM prompt 的 hash
    pub prompt_hash: String,
    /// LLM 输出的推断片段
    pub fragment: String,
    /// LLM 模型名称
    pub model: String,
    /// LLM 调用时间
    pub timestamp: String,
}

/// 三层独立置信度评分
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvidenceConfidence {
    /// L1 AST 证据可信度
    pub l1_score: f64,
    /// L2 规则匹配可信度
    pub l2_score: f64,
    /// L3 LLM 推断可信度（None 表示无 L3）
    pub l3_score: Option<f64>,
}

impl Default for EvidenceConfidence {
    fn default() -> Self {
        Self {
            l1_score: 0.0,
            l2_score: 0.0,
            l3_score: None,
        }
    }
}

/// Snapshot 创建时的轻量证据 hash
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvidenceHash {
    pub l1_hash: String,
    pub l2_hash: String,
    pub l3_hash: Option<String>,
}

// ─── v3 Loop Engineering Types ─────────────────────────────────────

/// 规则级抑制条目，持久化到 .Paporot/rules/suppressions.toml
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RuleSuppression {
    pub rule_id: String,
    /// 必填：文件路径 glob pattern，作为 scope 约束
    pub file_pattern: String,
    /// 可选：进一步限制变更类型
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_type: Option<String>,
    pub effect: SuppressionEffect,
    pub reason: String,
    pub created_by: String,
    pub created_at: String,
    /// 来源审查文件（如 review_v1.toml）
    pub source_review: String,
    pub hit_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_hit: Option<String>,
    pub status: SuppressionStatus,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SuppressionEffect {
    /// confidence → 0.2
    Suppress,
    /// 打 tag，不降 confidence
    Warn,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SuppressionStatus {
    Active,
    Stale,
    Revoked,
}

/// 反馈索引（Native → WASM 传输格式）
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FeedbackIndex {
    /// Layer 1: (symbol_name, file_path, change_type) → reason
    pub exact_reject_map: std::collections::HashMap<String, String>,
    /// Layer 2: rule suppression list
    pub rule_suppressions: Vec<RuleSuppression>,
    /// Layer 3: rejected file path prefixes for prefix matching
    pub rejected_prefixes: Vec<String>,
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Language Tests ─────────────────────────────────────────

    #[test]
    fn test_language_from_extension_known() {
        assert_eq!(Language::from_extension("rs"), Language::Rust);
        assert_eq!(Language::from_extension("ts"), Language::TypeScript);
        assert_eq!(Language::from_extension("tsx"), Language::TypeScript);
        assert_eq!(Language::from_extension("py"), Language::Python);
        assert_eq!(Language::from_extension("go"), Language::Go);
        assert_eq!(Language::from_extension("java"), Language::Java);
    }

    #[test]
    fn test_language_from_extension_unknown() {
        assert_eq!(Language::from_extension("toml"), Language::Unknown);
        assert_eq!(Language::from_extension(""), Language::Unknown);
    }

    #[test]
    fn test_language_from_filename() {
        assert_eq!(Language::from_filename("src/main.rs"), Language::Rust);
        assert_eq!(Language::from_filename("app/index.ts"), Language::TypeScript);
        assert_eq!(Language::from_filename("lib/util.py"), Language::Python);
        assert_eq!(Language::from_filename("unknown"), Language::Unknown);
    }

    // ─── ChangeType Tests ───────────────────────────────────────

    #[test]
    fn test_change_type_is_breaking() {
        assert!(ChangeType::FunctionRemoved.is_breaking());
        assert!(ChangeType::FunctionSignatureChanged.is_breaking());
        assert!(ChangeType::StructFieldRemoved.is_breaking());
        assert!(ChangeType::EnumVariantRemoved.is_breaking());
        assert!(ChangeType::TraitMethodChanged.is_breaking());
        assert!(ChangeType::HttpRouteRemoved.is_breaking());
        assert!(ChangeType::HttpRouteChanged.is_breaking());
        assert!(ChangeType::ConstantRemoved.is_breaking());
        assert!(ChangeType::ImportRemoved.is_breaking());

        assert!(!ChangeType::FunctionAdded.is_breaking());
        assert!(!ChangeType::StructAdded.is_breaking());
        assert!(!ChangeType::EnumAdded.is_breaking());
        assert!(!ChangeType::DocOnly.is_breaking());
        assert!(!ChangeType::UnknownChange.is_breaking());
    }

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

    // ─── RawChange Tests ────────────────────────────────────────

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

    // ─── RuleTrigger Tests ──────────────────────────────────────

    #[test]
    fn test_rule_trigger_composition() {
        let trigger = RuleTrigger::And(
            Box::new(RuleTrigger::SymbolMatches { pattern: "auth".into() }),
            Box::new(RuleTrigger::ChangeTypeIn(vec![ChangeType::FunctionAdded])),
        );
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

    // ─── Evidence Tests ─────────────────────────────────────────

    #[test]
    fn test_evidence_serde_roundtrip() {
        let evidence = Evidence {
            capability_id: "cap_001".into(),
            snapshot_version: "v1".into(),
            l1: vec![L1Evidence {
                symbol: "login".into(),
                file_path: "src/auth.rs".into(),
                line: 42,
                kind: SymbolKind::Function,
                visibility: "pub".into(),
            }],
            l2: vec![L2Evidence {
                rule_id: "r001".into(),
                rule_name: "auth_pattern".into(),
                matched_symbol: "login".into(),
                file_change: "src/auth.rs".into(),
                reason: "detects auth entry point".into(),
                severity: "high".into(),
            }],
            l3: Some(L3Evidence {
                prompt_hash: "abc123".into(),
                fragment: "email/password authentication".into(),
                model: "deepseek-chat".into(),
                timestamp: "2026-06-12T14:00:00Z".into(),
            }),
            confidence: EvidenceConfidence {
                l1_score: 0.85,
                l2_score: 0.72,
                l3_score: Some(0.90),
            },
            generated_at: "2026-06-12T14:00:00Z".into(),
        };

        let json = serde_json::to_string(&evidence).unwrap();
        let decoded: Evidence = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.capability_id, "cap_001");
        assert_eq!(decoded.l1.len(), 1);
        assert_eq!(decoded.l2.len(), 1);
        assert!(decoded.l3.is_some());
        assert_eq!(decoded.confidence.l1_score, 0.85);
    }

    #[test]
    fn test_evidence_without_l3() {
        let evidence = Evidence {
            capability_id: "cap_002".into(),
            snapshot_version: "v1".into(),
            l1: vec![],
            l2: vec![],
            l3: None,
            confidence: EvidenceConfidence::default(),
            generated_at: "2026-06-12T14:00:00Z".into(),
        };

        let json = serde_json::to_string(&evidence).unwrap();
        let decoded: Evidence = serde_json::from_str(&json).unwrap();
        assert!(decoded.l3.is_none());
        assert!(decoded.confidence.l3_score.is_none());
    }

    #[test]
    fn test_l1_evidence_symbol_kinds() {
        let kinds = vec![
            SymbolKind::Function,
            SymbolKind::Struct,
            SymbolKind::Enum,
            SymbolKind::Trait,
            SymbolKind::Implementation,
            SymbolKind::Module,
        ];

        for kind in kinds {
            let evidence = L1Evidence {
                symbol: "test".into(),
                file_path: "src/test.rs".into(),
                line: 1,
                kind: kind.clone(),
                visibility: "pub".into(),
            };
            let json = serde_json::to_string(&evidence).unwrap();
            let decoded: L1Evidence = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded.kind, kind);
        }
    }

    #[test]
    fn test_evidence_hash() {
        let hash = EvidenceHash {
            l1_hash: "abc123def456".into(),
            l2_hash: "789ghi".into(),
            l3_hash: None,
        };
        let json = serde_json::to_string(&hash).unwrap();
        let decoded: EvidenceHash = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.l1_hash, "abc123def456");
        assert!(decoded.l3_hash.is_none());
    }
}
