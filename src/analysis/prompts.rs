//! Minimal prompts stub for analysis L3 LLM bridge
//!
//! v0.4.0: Prompts were moved from the deleted src/prompts.rs
//! into the analysis module to keep it self-contained.

/// System prompt for behavior extraction from git diff
pub const SYSTEM_PROMPT_BEHAVIOR_EXTRACTOR: &str = r#"You are a software behavior analyst. Given a git diff, identify the high-level capabilities (behaviors) that the code changes implement.

Focus on:
1. What user-facing or developer-facing behaviors are introduced/changed/removed
2. Security-sensitive changes
3. Cross-module dependency changes

Respond ONLY with valid JSON."#;

/// Build an extraction prompt for the LLM to analyze diff content
pub fn build_extraction_prompt(
    diff: &str,
    _prd_content: Option<&str>,
    _existing_caps: Option<&str>,
    _context: Option<&str>,
) -> String {
    format!(
        r#"Analyze the following git diff and extract all capabilities (behaviors) that are being introduced, modified, or removed.

DIFF:
```
{}
```

Return a JSON object matching this schema:
{{
  "capabilities": [
    {{
      "id": "unique capability identifier",
      "name": "human-readable capability name",
      "description": "what behavior this capability provides",
      "category": "Security|Business|Infra|UX|API|Data",
      "status": "added|modified|removed",
      "confidence": 0.0-1.0
    }}
  ]
}}"#,
        diff
    )
}
