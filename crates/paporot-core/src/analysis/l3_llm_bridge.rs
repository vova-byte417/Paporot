//! L3 LLM Bridge：通过 host_llm_call 调用外部 LLM
//!
//! 在 paporot-core WASM 沙盒内，LLM 调用全部走 host function，
//! 不直接发起 HTTP。L3 仅处理 L1+L2 无法确定的部分。

use crate::host;
use crate::types::*;

// ─── Prompt Constants ────────────────────────────────────────────

/// L3 行为提取 System Prompt
const SYSTEM_PROMPT: &str = "\
You are a software behavior analyst. Your job is to extract human-readable
behavioral capabilities from code diffs that could not be fully parsed by
deterministic AST analysis.

For each capability you identify, output a JSON object with:
- id: a unique identifier like \"cap_xxx_001\"
- name: a short action-oriented name (max 80 chars)
- description: 1-2 sentences explaining what the code does from a user/system perspective
- module: the primary module/service affected
- confidence: your confidence score 0.0-1.0
- evidence: list of file paths and line numbers that support this capability

Return ONLY a valid JSON object with a \"capabilities\" array. No markdown, no explanation.";

/// 构建 L3 提取 prompt
pub fn build_prompt(
    residual_diff: &str,
    low_confidence_changes: &[RawChange],
) -> String {
    let mut prompt = String::new();

    if !low_confidence_changes.is_empty() {
        prompt.push_str("## Low-Confidence Changes (L1 detected but unsure)\n\n");
        for (i, rc) in low_confidence_changes.iter().enumerate() {
            prompt.push_str(&format!("{}. {} in {} (confidence: {:.2})\n",
                i + 1, rc.symbol_name, rc.file_path, rc.confidence));
        }
        prompt.push_str("\n");
    }

    if !residual_diff.is_empty() {
        prompt.push_str("## Residual Diff (not covered by L1/L2)\n\n");
        // Truncate to avoid excessive token usage, respecting UTF-8 boundaries
        let limit = 8000usize;
        if residual_diff.len() > limit {
            let mut safe_limit = limit;
            while safe_limit > 0 && !residual_diff.is_char_boundary(safe_limit) {
                safe_limit -= 1;
            }
            prompt.push_str(&residual_diff[..safe_limit]);
            prompt.push_str("\n... (truncated)\n");
        } else {
            prompt.push_str(residual_diff);
        }
        prompt.push_str("\n");
    }

    prompt.push_str("\nExtract capabilities from the above in valid JSON format.");
    prompt
}

// ─── L3 Bridge ───────────────────────────────────────────────────

/// L3 LLM 增强桥接器（WASM 内同步调用 host_llm_call）
pub struct LlmBridge;

impl LlmBridge {
    /// 对低置信度变更 + 残留 diff 调用 LLM 补充语义描述
    ///
    /// * `low_confidence` - L1 获取到的低置信度（<0.5）变更
    /// * `residual_diff` - L1+L2 未覆盖的残留 diff 片段
    pub fn enhance(
        low_confidence: &[RawChange],
        residual_diff: &str,
    ) -> Vec<LlmFragment> {
        if low_confidence.is_empty() && residual_diff.trim().is_empty() {
            return vec![];
        }

        let user_prompt = build_prompt(residual_diff, low_confidence);

        let response = match host::llm_call(SYSTEM_PROMPT, &user_prompt) {
            Some(text) => text,
            None => {
                // LLM 不可用，返回带降级标记的空片段
                return vec![];
            }
        };

        vec![LlmFragment {
            fragment_id: format!("llm_l3_001"),
            content: response,
            file_paths: low_confidence.iter().map(|r| r.file_path.clone()).collect(),
            raw_json: None,
        }]
    }

    /// 将 L3 输出合并为 Capability 列表
    pub fn merge_fragments(fragments: &[LlmFragment]) -> Vec<Capability> {
        let mut capabilities = Vec::new();

        for fragment in fragments {
            if let Ok(snapshot) = serde_json::from_str::<BehaviorSnapshot>(&fragment.content) {
                capabilities.extend(snapshot.capabilities);
            }
        }

        capabilities
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_prompt_with_changes() {
        let changes = vec![RawChange {
            id: "rc1".into(),
            symbol_name: "mystery_fn".into(),
            file_path: "src/lib.rs".into(),
            line: 42,
            confidence: 0.3,
            language: Language::Rust,
            visibility: "pub".into(),
            change_type: ChangeType::Added,
            tags: vec![],
        }];

        let prompt = build_prompt("", &changes);
        assert!(prompt.contains("mystery_fn"));
        assert!(prompt.contains("Low-Confidence"));
        assert!(prompt.contains("0.30"));
    }

    #[test]
    fn test_build_prompt_with_residual() {
        let prompt = build_prompt("+fn foo() {}", &[]);
        assert!(prompt.contains("Residual Diff"));
        assert!(prompt.contains("+fn foo()"));
    }

    #[test]
    fn test_build_prompt_truncates_large_diff() {
        let big = "a".repeat(10000);
        let prompt = build_prompt(&big, &[]);
        assert!(prompt.len() <= 8500); // system prompt + user prompt reasonable
        assert!(prompt.contains("(truncated)"));
    }

    #[test]
    fn test_enhance_empty_inputs() {
        let result = LlmBridge::enhance(&[], "");
        assert!(result.is_empty());
    }

    #[test]
    fn test_merge_fragments_empty() {
        let caps = LlmBridge::merge_fragments(&[]);
        assert!(caps.is_empty());
    }

    #[test]
    fn test_merge_fragments_non_json() {
        let fragments = vec![LlmFragment {
            fragment_id: "f1".into(),
            content: "plain text".into(),
            file_paths: vec![],
            raw_json: None,
        }];
        let caps = LlmBridge::merge_fragments(&fragments);
        assert!(caps.is_empty());
    }

    #[test]
    fn test_merge_fragments_valid_snapshot() {
        let snap_json = serde_json::json!({
            "schema_version": 3,
            "version_id": "v1",
            "timestamp": "2026-01-01T00:00:00Z",
            "message": "llm output",
            "capabilities": [{
                "id": "cap_001",
                "name": "LLM Cap",
                "description": "from LLM",
                "status": "new",
            }],
            "prd_coverage": { "percentage": 0.0, "total_items": 0 }
        });
        let fragments = vec![LlmFragment {
            fragment_id: "f1".into(),
            content: snap_json.to_string(),
            file_paths: vec![],
            raw_json: None,
        }];
        let caps = LlmBridge::merge_fragments(&fragments);
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].name, "LLM Cap");
    }
}
