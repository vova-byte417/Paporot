/// Skill: Behavior Boundary Discovery
///
/// Goal: 发现影响可观测行为的组件边界，区分行为核心与支撑模块
///
/// Inputs: ast, git_diff, call_graph
/// Output: behavior_boundary_discovery_output JSON

use paporot_skill_sdk::prelude::*;

#[no_mangle]
pub extern "C" fn paporot_skill_execute() -> i32 {
    let _ast = read_input("ast").unwrap_or_default();
    let git_diff = read_input("git_diff").unwrap_or_default();
    let call_graph = read_input("call_graph").unwrap_or_default();

    // Detect changed functions from git diff
    let changed: Vec<String> = git_diff
        .lines()
        .filter(|l| l.starts_with("@@"))
        .filter_map(|l| {
            // Extract function name from @@ ... @@ <function_name>
            let parts: Vec<&str> = l.split_whitespace().collect();
            parts.last().map(|s| s.trim().to_string())
        })
        .collect();

    // Classify as behavioral / non-behavioral
    let mut behavioral: Vec<Value> = Vec::new();
    let mut non_behavioral: Vec<Value> = Vec::new();
    let mut changed_boundaries: Vec<Value> = Vec::new();

    for func in &changed {
        let is_behavioral = !func.contains("log")
            && !func.contains("metric")
            && !func.contains("trace")
            && !func.contains("format")
            && !func.contains("cache")
            && !func.contains("debug");

        if is_behavioral {
            behavioral.push(json!({
                "name": func,
                "module": extract_module(func),
                "output_type": "unknown"
            }));
            changed_boundaries.push(json!({
                "function": func,
                "change_type": "modified",
                "user_visible": true,
                "risk": "medium"
            }));
        } else {
            non_behavioral.push(json!({
                "name": func,
                "module": extract_module(func),
                "reason": infer_non_behavioral_reason(func)
            }));
        }
    }

    // Use LLM to refine classification
    let prompt = format!(
        "These functions were changed in the latest commit. Classify each as behavioral \
         (user-visible behavior changes) or non-behavioral (internal changes only).\n\n\
         Changed functions:\n{:#?}\n\n\
         Call graph (partial):\n{}",
        changed,
        &call_graph[..call_graph.len().min(3000)]
    );

    let schema = r#"{
        "type": "object",
        "properties": {
            "behavioral_modules": {"type": "array", "items": {"type": "string"}},
            "non_behavioral_modules": {"type": "array", "items": {"type": "string"}},
            "boundary_summary": {"type": "string"},
            "risk_assessment": {"type": "string", "enum": ["low", "medium", "high"]}
        }
    }"#;

    let llm_result = llm_complete(&prompt, schema);

    let mut behavioral_modules: Vec<String> = behavioral.iter()
        .filter_map(|v| v.get("module").and_then(|m| m.as_str()).map(|s| s.to_string()))
        .collect();
    let mut non_behavioral_modules: Vec<String> = non_behavioral.iter()
        .filter_map(|v| v.get("module").and_then(|m| m.as_str()).map(|s| s.to_string()))
        .collect();

    let mut boundary_summary = format!(
        "{} behavioral changes, {} non-behavioral changes",
        behavioral.len(), non_behavioral.len()
    );
    let mut risk_level = "medium".to_string();

    if let Some(ref result) = llm_result {
        if let Ok(v) = serde_json::from_str::<Value>(result) {
            if let Some(m) = v.get("behavioral_modules").and_then(|m| m.as_array()) {
                for item in m {
                    if let Some(s) = item.as_str() {
                        if !behavioral_modules.contains(&s.to_string()) {
                            behavioral_modules.push(s.to_string());
                        }
                    }
                }
            }
            if let Some(m) = v.get("non_behavioral_modules").and_then(|m| m.as_array()) {
                for item in m {
                    if let Some(s) = item.as_str() {
                        if !non_behavioral_modules.contains(&s.to_string()) {
                            non_behavioral_modules.push(s.to_string());
                        }
                    }
                }
            }
            if let Some(s) = v.get("boundary_summary").and_then(|s| s.as_str()) {
                boundary_summary = s.to_string();
            }
            if let Some(r) = v.get("risk_assessment").and_then(|r| r.as_str()) {
                risk_level = r.to_string();
            }
        }
    }

    let output = json!({
        "behavioral_modules": behavioral_modules,
        "non_behavioral_modules": non_behavioral_modules,
        "behavioral_functions": behavioral,
        "non_behavioral_functions": non_behavioral,
        "changed_boundaries": changed_boundaries,
        "boundary_summary": boundary_summary,
        "risk_level": risk_level
    });

    write_output(&output);
    0
}

fn extract_module(func: &str) -> String {
    func.split('.').next().unwrap_or(func).to_string()
}

fn infer_non_behavioral_reason(func: &str) -> &str {
    if func.contains("log") { "logging" }
    else if func.contains("metric") { "metrics" }
    else if func.contains("trace") { "tracing" }
    else if func.contains("cache") { "cache" }
    else if func.contains("format") { "formatting" }
    else { "internal" }
}

fn main() {}
