//! LLM Prompt Engineering 模板
//!
//! 对应 PRD 第 4 节中的所有 prompt 模板。
//! 使用 XML 分隔 + Chain-of-Thought + JSON 强制输出策略。

/// System Prompt —— 全局 Behavior Extractor
pub const SYSTEM_PROMPT_BEHAVIOR_EXTRACTOR: &str = r#"You are Paporot Behavior Extractor v1.1, a world-class software architect and requirements traceability expert.

Core Principles:
- Focus exclusively on observable, user-facing or architecturally significant **behaviors**, not code implementation details.
- A Capability must be meaningful to a Tech Lead / Product Owner.
- Be concise, precise, and conservative.
- Always flag potential breaking changes, security implications, and compatibility risks.
- Output **ONLY** valid JSON. No explanations, no markdown, no extra text.

Capability Guidelines:
- name: Short, action-oriented (max 80 chars)
- description: 1-2 clear sentences from user or system perspective
- status: "new" | "modified" | "deleted" | "unchanged"
- module: Main affected module/service (e.g. "auth", "payment")
- confidence: float between 0.0 and 1.0

Few-Shot Examples:

Example 1 - Adding authentication:
```json
{
  "version_id": "v41",
  "message": "添加 JWT 认证",
  "capabilities": [
    {
      "id": "cap_auth_001",
      "name": "JWT Token-based Authentication",
      "description": "用户登录后颁发 JWT，支持短时有效期和刷新",
      "status": "new",
      "module": "auth",
      "confidence": 0.95
    }
  ],
  "prd_coverage": {
    "percentage": 80.0,
    "total_items": 5,
    "covered_items": 4,
    "details": []
  }
}
```

Example 2 - Modifying payment flow:
```json
{
  "version_id": "v42",
  "message": "重构支付流程支持退款",
  "capabilities": [
    {
      "id": "cap_pay_001",
      "name": "Payment Refund Processing",
      "description": "支持用户发起退款申请，管理员审核后原路返还资金",
      "status": "new",
      "module": "payment",
      "confidence": 0.92
    },
    {
      "id": "cap_pay_002",
      "name": "Payment Status Tracking",
      "description": "支付状态从简单成功/失败扩展为 pending/processing/success/failed/refunded 五种状态",
      "status": "modified",
      "module": "payment",
      "confidence": 0.88
    }
  ],
  "prd_coverage": {
    "percentage": 100.0,
    "total_items": 2,
    "covered_items": 2,
    "details": []
  }
}
```
"#;

/// System Prompt —— Behavior Diff
pub const SYSTEM_PROMPT_DIFF: &str = r#"You are an expert technical reviewer. Produce a clear Behavior Diff between two snapshots. Use hierarchical Markdown."#;

/// System Prompt —— PRD Coverage
pub const SYSTEM_PROMPT_COVERAGE: &str = r#"You are a requirements traceability expert. Compute PRD coverage accurately. Output ONLY valid JSON."#;

/// System Prompt —— Regression + Risk
pub const SYSTEM_PROMPT_REGRESSION_RISK: &str = r#"You are a senior QA engineer and security auditor. Analyze behavior changes for regressions and risks. Output ONLY valid JSON."#;

/// 构建 Behavior Extraction User Prompt
pub fn build_extraction_prompt(
    git_diff: &str,
    full_context: Option<&str>,
    prev_snapshot_summary: Option<&str>,
    prd_content: Option<&str>,
) -> String {
    let context_block = full_context
        .map(|c| format!("\n<full_context>\n{c}\n</full_context>"))
        .unwrap_or_default();

    let prev_block = prev_snapshot_summary
        .map(|s| format!("\n<previous_snapshot_summary>\n{s}\n</previous_snapshot_summary>"))
        .unwrap_or_default();

    let prd_block = prd_content
        .map(|p| format!("\n<prd_reference>\n{p}\n</prd_reference>"))
        .unwrap_or_default();

    format!(
        r#"<task>
Analyze the code changes and produce a complete Behavior Snapshot.
</task>

<git_diff>
{git_diff}
</git_diff>
{context_block}
{prev_block}
{prd_block}

<instructions>
1. Extract all significant behavior changes as Capabilities.
2. Map behaviors to PRD items where possible.
3. Identify potential regressions and risks.
4. Think step by step internally, then output ONLY the JSON.
</instructions>

<output_format>
Return a single valid JSON object matching the BehaviorSnapshot schema exactly.
</output_format>"#
    )
}

/// 构建 Behavior Diff User Prompt
pub fn build_diff_prompt(snapshot_from: &str, snapshot_to: &str) -> String {
    format!(
        r#"<previous>
{snapshot_from}
</previous>

<current>
{snapshot_to}
</current>

<task>
Generate Behavior Diff in Markdown with sections: 新增能力、修改能力、删除能力、影响范围、风险与注意事项.
</task>"#
    )
}

/// 构建 PRD Coverage User Prompt
pub fn build_coverage_prompt(prd_items: &str, capabilities: &str) -> String {
    format!(
        r#"<prd_items>
{prd_items}
</prd_items>

<capabilities>
{capabilities}
</capabilities>

<task>
Compute coverage and output JSON with percentage and per-item mapping.
</task>"#
    )
}

/// 构建 Regression + Risk User Prompt
pub fn build_regression_risk_prompt(prev_snapshot: &str, current_snapshot: &str) -> String {
    format!(
        r#"<previous>
{prev_snapshot}
</previous>

<current>
{current_snapshot}
</current>

<task>
Compare previous and current behaviors. Focus on:
- Critical workflows (login, data access, etc.)
- Backward compatibility
- Security / Performance impact

Output regression status and risk assessment in JSON.
</task>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_extraction_prompt_minimal() {
        let prompt = build_extraction_prompt(
            "diff --git a/main.rs b/main.rs\n+fn hello() {}",
            None,
            None,
            None,
        );
        assert!(prompt.contains("<git_diff>"));
        assert!(prompt.contains("Analyze the code changes"));
    }

    #[test]
    fn test_build_extraction_prompt_full() {
        let prompt = build_extraction_prompt(
            "diff content",
            Some("fn main() {}"),
            Some("Previous: v1 had 5 capabilities"),
            Some("PRD-001: Login feature"),
        );
        assert!(prompt.contains("<full_context>"));
        assert!(prompt.contains("<previous_snapshot_summary>"));
        assert!(prompt.contains("<prd_reference>"));
    }

    #[test]
    fn test_build_diff_prompt() {
        let prompt = build_diff_prompt(r#"{"version_id":"v1"}"#, r#"{"version_id":"v2"}"#);
        assert!(prompt.contains("<previous>"));
        assert!(prompt.contains("<current>"));
        assert!(prompt.contains("新增能力"));
    }
}
