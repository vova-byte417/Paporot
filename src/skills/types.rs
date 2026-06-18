//! Skill 系统核心类型定义
//!
//! 对应 skill.toml 的完整 Rust 表示，以及 Skill 执行结果类型。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ─── SkillManifest ──────────────────────────────────────────────────

/// 从 skill.toml 解析出的 Skill 元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub skill: SkillMeta,
    pub inputs: SkillInputs,
    pub outputs: SkillOutputs,
    #[serde(rename = "llm_calls", default)]
    pub llm_calls: Option<LlmBudget>,
    #[serde(default)]
    pub dependencies: SkillDeps,
    #[serde(default)]
    pub quality: QualityChecks,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    pub name: String,
    pub version: String,
    pub requires_paporot: String,
    pub description: String,
    pub timeout_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInputs {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
    #[serde(default)]
    pub schema_version: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillOutputs {
    pub schema: String,
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmBudget {
    pub max_calls: u32,
    pub preferred_model: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillDeps {
    #[serde(default)]
    pub uses_outputs_from: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QualityChecks {
    #[serde(default)]
    pub checks: Vec<String>,
}

// ─── 已安装的 Skill ─────────────────────────────────────────────────

/// 一个已安装并校验通过的 Skill
#[derive(Debug, Clone)]
pub struct InstalledSkill {
    pub manifest: SkillManifest,
    pub dir: PathBuf,       // skill 目录路径
    pub wasm_path: PathBuf, // skill.wasm 路径
}

// ─── Skill 执行结果 ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRunResult {
    pub skill_name: String,
    pub status: SkillRunStatus,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SkillError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillRunStatus {
    Ok,
    Skipped,
    TimedOut,
    Failed,
}

impl std::fmt::Display for SkillRunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillRunStatus::Ok => write!(f, "ok"),
            SkillRunStatus::Skipped => write!(f, "skipped"),
            SkillRunStatus::TimedOut => write!(f, "timeout"),
            SkillRunStatus::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillError {
    pub phase: String,
    pub error_code: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

// ─── 分析结果汇总 ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub total_skills: usize,
    pub ok: usize,
    pub skipped: usize,
    pub failed: usize,
    pub total_duration_ms: u64,
    pub high_level_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRegistryInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub requires_paporot: String,
    pub compatible: bool,
}
