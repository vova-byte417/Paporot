/// Skill: LLM Rubric Grader
///
/// Goal: LLM-based rubric grading of Agent behavior quality
///
/// Inputs: code_change_cache, eval_context, grader_results
/// Output: llm_rubric_output JSON
///
/// v0.4.0: New skill replacing manual eval review

use paporot_skill_sdk::prelude::*;

fn main() {}

#[no_mangle]
pub extern "C" fn paporot_skill_execute() -> i32 {
    // Step 1: Read inputs
    let code_change_cache = match read_input("code_change_cache") {
        Some(s) => s,
        None => {
            write_error("Missing required input: code_change_cache");
            return 1;
        }
    };
    let _eval_context = read_input("eval_context").unwrap_or_default();
    let grader_results_str = match read_input("grader_results") {
        Some(s) => s,
        None => {
            write_error("Missing required input: grader_results");
            return 1;
        }
    };

    // Step 2: Parse inputs
    let cc: Value = match serde_json::from_str(&code_change_cache) {
        Ok(v) => v,
        Err(e) => {
            write_error(&format!("Failed to parse code_change_cache: {}", e));
            return 1;
        }
    };
    let graders: Vec<Value> = match serde_json::from_str(&grader_results_str) {
        Ok(v) => v,
        Err(e) => {
            write_error(&format!("Failed to parse grader_results: {}", e));
            return 1;
        }
    };

    // Step 3: Summary of grader results for LLM prompt
    let grader_summary: Vec<String> = graders.iter().map(|g| {
        format!(
            "{}: {} (passed: {})",
            g.get("name").and_then(|v| v.as_str()).unwrap_or("unknown"),
            if g.get("passed").and_then(|v| v.as_bool()).unwrap_or(false) { "PASS" } else { "FAIL" },
            g.get("passed").and_then(|v| v.as_bool()).unwrap_or(false)
        )
    }).collect();

    let files_changed = cc.get("files_changed")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let additions = cc.get("additions").and_then(|v| v.as_u64()).unwrap_or(0);
    let deletions = cc.get("deletions").and_then(|v| v.as_u64()).unwrap_or(0);

    // Step 4: Build LLM prompt
    let prompt = format!(
        r#"You are an AI code review judge. Evaluate the quality of the following code change using a 0-10 rubric.

## Code Change
- Files changed: {}
- Lines added: +{}
- Lines deleted: -{}

## Grader Results
{}

## Rubric Dimensions
1. 正确性 (Correctness): Does the change achieve its apparent goal? Are there bugs?
2. 可维护性 (Maintainability): Is the code clean, well-structured, and easy to understand?
3. 安全性 (Security): Are there any security concerns introduced?
4. 工具使用效率 (Tool Efficiency): Was the change minimal and focused?
5. 最小变更原则 (Minimal Change): Could fewer lines achieve the same result?

For each dimension, assign a score 0-10 with a brief justification."#,
        files_changed, additions, deletions,
        grader_summary.join("\n")
    );

    let schema = r#"{
        "type": "object",
        "properties": {
            "overall_score": {"type": "number", "minimum": 0, "maximum": 10},
            "dimensions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "score": {"type": "number", "minimum": 0, "maximum": 10},
                        "justification": {"type": "string"}
                    },
                    "required": ["name", "score", "justification"]
                }
            },
            "summary": {"type": "string"}
        },
        "required": ["overall_score", "dimensions", "summary"]
    }"#;

    // Step 5: Call LLM
    let result = match llm_complete(&prompt, schema) {
        Some(s) => s,
        None => {
            write_error("LLM call failed or returned no result");
            let fallback = serde_json::json!({
                "overall_score": null,
                "dimensions": [],
                "summary": "LLM rubric unavailable",
                "error": "LLM call returned None"
            });
            write_output(&fallback);
            return 0;
        }
    };

    // Step 6: Validate response is valid JSON
    let output: Value = match serde_json::from_str(&result) {
        Ok(v) => v,
        Err(_) => {
            serde_json::json!({
                "overall_score": null,
                "dimensions": [],
                "summary": "Invalid LLM response",
                "raw": result
            })
        }
    };

    // Step 7: Write output
    write_output(&output);

    0
}
