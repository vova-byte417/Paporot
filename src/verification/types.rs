use serde::{Deserialize, Serialize};

/// Output from a single Contract verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub artifact_id: String,
    pub artifact_type: String,
    pub status: String, // "PASS" | "FAIL"
    pub rule_results: Vec<RuleResult>,
    pub suggestions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleResult {
    pub rule: String,
    pub pass: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// YAML contract definition.
#[derive(Debug, Clone, Deserialize)]
pub struct ContractConfig {
    pub artifact_type: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub severity: String,
    pub rules: ContractRules,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContractRules {
    #[serde(default)]
    pub syntax: serde_yaml::Value,
    #[serde(default)]
    pub structure: serde_yaml::Value,
}

/// A single evidence record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub artifact_id: String,
    pub input: String,
    pub output: String,
    pub intermediate: String,
}

/// A saved replay case (persisted on FAIL).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayCase {
    pub case_id: String,
    pub created_at: String,
    pub artifact_type: String,
    pub artifact_id: String,
    pub upstream_input: serde_json::Value,
    pub failed_artifact: String,
    pub contract_result: VerificationResult,
    pub suggestions: Vec<String>,
}
