//! 核心数据类型定义
//!
//! Case, Expected, Actual, Verdict, CaseResult 等。
//! Expected 一个 struct 包含三类字段，Runner 按 category 选择校验。
//! ExpectedCapability.categories 为空时不校验 categories。

use Paporot::types::CapabilityStatus;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ─── Case 定义 ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Case {
    pub id: String,
    pub name: String,
    pub category: CaseCategory,
    pub description: String,
    pub input: CaseInput,
    pub expected: Expected,
    #[serde(default)]
    pub metadata: CaseMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CaseCategory {
    Capability,
    Diff,
    Regression,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseInput {
    #[serde(rename = "type", default = "default_input_type")]
    pub input_type: InputType,
    pub before: PathBuf,
    pub after: PathBuf,
}

fn default_input_type() -> InputType {
    InputType::Files
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum InputType {
    Files,
    #[allow(dead_code)]
    CommitRange,
}

// ─── Expected ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expected {
    #[serde(default)]
    pub capabilities: Vec<ExpectedCapability>,
    #[serde(default)]
    pub diff: Option<ExpectedDiff>,
    #[serde(default)]
    pub regression: Option<ExpectedRegression>,
    #[serde(default)]
    pub risk_level: Option<RiskLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedCapability {
    pub name: String,
    pub status: CapabilityStatus,
    #[serde(default)]
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedDiff {
    #[serde(default)]
    pub added_count: Option<usize>,
    #[serde(default)]
    pub removed_count: Option<usize>,
    #[serde(default)]
    pub modified_count: Option<usize>,
    #[serde(default)]
    pub added_names: Vec<String>,
    #[serde(default)]
    pub removed_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedRegression {
    pub degraded: bool,
    pub verdict: String,            // "Pass" | "Degraded" | "Watch"
    #[serde(default)]
    pub tool_call_delta_max: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

// ─── Actual ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Actual {
    pub capabilities: Vec<ActualCapability>,
    pub diff_summary: Option<ActualDiffSummary>,
}

#[derive(Debug, Clone)]
pub struct ActualCapability {
    pub name: String,
    pub status: CapabilityStatus,
    pub categories: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ActualDiffSummary {
    pub added_count: usize,
    pub removed_count: usize,
    pub modified_count: usize,
    pub unchanged_count: usize,
    pub added_names: Vec<String>,
    pub removed_names: Vec<String>,
}

// ─── Verdict ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Verdict {
    Pass,
    SemanticPass { confidence: f64, reason: String },
    Fail { reason: String },
}

impl Verdict {
    pub fn is_success(&self) -> bool {
        matches!(self, Verdict::Pass | Verdict::SemanticPass { .. })
    }
}

// ─── CaseResult ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct CaseResult {
    pub case_id: String,
    pub name: String,
    pub category: String,
    pub verdict: Verdict,
    pub expected_summary: String,
    pub actual_summary: String,
    pub duration_ms: u64,
}

// ─── CaseMetadata ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CaseMetadata {
    #[serde(default)]
    pub difficulty: Difficulty,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub human_verified: bool,
    #[serde(default)]
    pub auto_generated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    #[default]
    Medium,
    Easy,
    Hard,
}

// ─── Suite 汇总 ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct SuiteResult {
    pub suite_name: String,
    pub total: usize,
    pub pass: usize,
    pub semantic_pass: usize,
    pub fail: usize,
    pub pass_rate: f64,
    pub cases: Vec<CaseResult>,
    pub duration_ms: u64,
}
