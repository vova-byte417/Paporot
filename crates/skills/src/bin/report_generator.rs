/// Skill: Report Generator
///
/// Goal: Aggregates all grader results and generates a comprehensive evaluation report
///
/// Inputs: code_change_cache, eval_context, all_grader_results, llm_rubric_result
/// Output: report_output JSON
///
/// v0.4.0: New skill generating consolidated reports

use paporot_skill_sdk::prelude::*;

fn main() {}

#[no_mangle]
pub extern "C" fn paporot_skill_execute() -> i32 {
    // Step 1: Read inputs
    let code_change_cache = read_input("code_change_cache").unwrap_or_default();
    let grader_results_str = read_input("all_grader_results").unwrap_or_default();
    let llm_rubric_str = read_input("llm_rubric_result").unwrap_or_else(|| "{}".to_string());

    // Step 2: Parse inputs
    let cc: Value = serde_json::from_str(&code_change_cache).unwrap_or_default();
    let graders: Vec<Value> = serde_json::from_str(&grader_results_str).unwrap_or_default();
    let rubric: Value = serde_json::from_str(&llm_rubric_str).unwrap_or_default();

    // Step 3: Compute overall outcome
    let total_graders = graders.len() as u32;
    let passed_graders = graders.iter()
        .filter(|g| g.get("passed").and_then(|v| v.as_bool()).unwrap_or(false))
        .count() as u32;

    let outcome = if total_graders == 0 {
        "N/A"
    } else if passed_graders == total_graders {
        "PASS"
    } else if passed_graders > 0 {
        "PARTIAL"
    } else {
        "FAIL"
    };

    // Step 4: Extract metrics
    let additions = cc.get("additions").and_then(|v| v.as_u64()).unwrap_or(0);
    let deletions = cc.get("deletions").and_then(|v| v.as_u64()).unwrap_or(0);
    let files_changed = cc.get("files_changed")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    // Step 5: Generate summary via LLM
    let prompt = format!(
        r#"Generate a concise summary (max 500 chars) of this code evaluation:

Files: {}, +{}/-{} lines
Outcome: {}
Test passed: {}
Lint passed: {}
Build passed: {}

Rubric score: {}/10

Write ONE paragraph describing the change quality."#,
        files_changed, additions, deletions, outcome,
        grader_passed(&graders, "test"),
        grader_passed(&graders, "lint"),
        grader_passed(&graders, "build"),
        rubric.get("overall_score").and_then(|v| v.as_f64()).unwrap_or(0.0)
    );

    let schema = r#"{
        "type": "object",
        "properties": {
            "summary": {"type": "string", "maxLength": 500}
        },
        "required": ["summary"]
    }"#;

    let llm_summary = match llm_complete(&prompt, schema) {
        Some(s) => {
            let v: Value = serde_json::from_str(&s).unwrap_or_default();
            v.get("summary").and_then(|v| v.as_str()).unwrap_or(&s).to_string()
        }
        None => format!(
            "Evaluation: {} out of {} graders passed. {} files changed (+{}/-{} lines).",
            outcome, total_graders, files_changed, additions, deletions
        ),
    };

    // Step 6: Assemble report
    let report = serde_json::json!({
        "outcome": outcome,
        "summary": truncate(&llm_summary, 500),
        "grader_details": graders,
        "llm_rubric": rubric,
        "metrics": {
            "total_files": files_changed,
            "total_additions": additions,
            "total_deletions": deletions,
            "build_passed": grader_passed(&graders, "build"),
            "tests_passed": grader_passed(&graders, "test"),
            "lint_passed": grader_passed(&graders, "lint"),
        },
        "recommendations": generate_recommendations(outcome, passed_graders, total_graders)
    });

    // Step 7: Write output
    write_output(&report);

    0
}

fn grader_passed(graders: &[Value], name: &str) -> bool {
    graders.iter().any(|g| {
        g.get("name").and_then(|v| v.as_str()).unwrap_or("") == name
            && g.get("passed").and_then(|v| v.as_bool()).unwrap_or(false)
    })
}

fn generate_recommendations(outcome: &str, passed: u32, total: u32) -> Vec<String> {
    let mut recs = Vec::new();
    match outcome {
        "FAIL" => {
            recs.push("All graders failed. Review the code for syntax errors or breaking changes.".into());
            recs.push("Run 'paporot eval auto' again after fixing build/test/lint failures.".into());
        }
        "PARTIAL" => {
            recs.push(format!("{}/{} graders passed. Focus on the failing graders.", passed, total));
            recs.push("Review grader details for specific error messages.".into());
        }
        _ => {
            recs.push("All automated checks passed. Review LLM rubric scores for quality insights.".into());
        }
    }
    recs
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let idx = s.char_indices().take(max_chars).last().map(|(i, _)| i).unwrap_or(s.len());
        s[..idx].to_string()
    }
}
