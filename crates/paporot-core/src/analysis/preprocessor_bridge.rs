//! 预处理器输出桥接
//!
//! 将 L1/L2 分析结果写入 `.Paporot/work/preprocessor_output.json`，
//! 供后续 Skill Pipeline 通过 host_read_file 读取。
//!
//! 输出格式：
//! ```json
//! {
//!   "version": "1.0",
//!   "l1_changes": [...],
//!   "l2_matches": [...],
//!   "l3_llm_fragments": [...],
//!   "summary": { ... }
//! }
//! ```

use serde::{Deserialize, Serialize};
use crate::host;
use crate::types::*;

/// 预处理器完整输出
#[derive(Serialize, Deserialize, Debug)]
pub struct PreprocessorOutput {
    pub version: String,
    pub l1_changes: Vec<RawChange>,
    pub l2_matches: Vec<RuleMatch>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub l3_llm_fragments: Vec<LlmFragment>,
    pub summary: PreprocessorSummary,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PreprocessorSummary {
    pub total_files: usize,
    pub l1_total_changes: usize,
    pub l2_total_matches: usize,
    pub l3_fragment_count: usize,
    pub languages_detected: Vec<String>,
}

/// 输出路径（相对于 .Paporot/）
pub const OUTPUT_PATH: &str = "work/preprocessor_output.json";

impl PreprocessorOutput {
    pub fn new(
        l1_changes: Vec<RawChange>,
        l2_matches: Vec<RuleMatch>,
        l3_fragments: Vec<LlmFragment>,
    ) -> Self {
        let languages: std::collections::BTreeSet<String> = l1_changes
            .iter()
            .map(|c| format!("{:?}", c.language))
            .collect();

        let summary = PreprocessorSummary {
            total_files: l1_changes.iter().map(|c| &c.file_path).collect::<std::collections::HashSet<_>>().len(),
            l1_total_changes: l1_changes.len(),
            l2_total_matches: l2_matches.len(),
            l3_fragment_count: l3_fragments.len(),
            languages_detected: languages.into_iter().collect(),
        };

        Self {
            version: "1.0".into(),
            l1_changes,
            l2_matches,
            l3_llm_fragments: l3_fragments,
            summary,
        }
    }

    /// 写入 .Paporot/work/preprocessor_output.json
    pub fn write_to_work(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize preprocessor output: {}", e))?;
        match host::write_file(OUTPUT_PATH, &json) {
            Ok(()) => Ok(()),
            Err(e) => Err(format!("Failed to write preprocessor output: {}", e)),
        }
    }

    /// 从 .Paporot/work/preprocessor_output.json 读取
    pub fn read_from_work() -> Option<Self> {
        let json = host::read_file(OUTPUT_PATH)?;
        serde_json::from_str(&json).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_roundtrip() {
        let changes = vec![RawChange {
            id: "rc1".into(),
            symbol_name: "hello".into(),
            file_path: "src/lib.rs".into(),
            line: 1,
            confidence: 0.9,
            language: Language::Rust,
            visibility: "pub".into(),
            change_type: ChangeType::Added,
            tags: vec![],
        }];

        let matches = vec![RuleMatch {
            rule_id: "r001".into(),
            rule_name: "Public API".into(),
            change_id: "rc1".into(),
            confidence: 0.95,
            category: RuleCategory::Breaking,
            evidence: Vec::new(),
            severity: Severity::High,
            matched_conditions: Vec::new(),
        }];

        let output = PreprocessorOutput::new(changes, matches, vec![]);
        assert_eq!(output.summary.l1_total_changes, 1);
        assert_eq!(output.summary.l2_total_matches, 1);
        assert_eq!(output.summary.languages_detected, vec!["Rust"]);
    }
}
