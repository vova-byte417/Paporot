/// Skill: Runtime Flow Analysis  
///
/// Goal: 发现端到端业务执行路径，标注每阶段职责
///
/// Inputs: ast, call_graph, entry_points
/// Output: runtime_flow_analysis_output JSON

use paporot_skill_sdk::prelude::*;

#[no_mangle]
pub extern "C" fn paporot_skill_execute() -> i32 {
    let _ast = read_input("ast").unwrap_or_default();
    let call_graph = match read_input("call_graph") {
        Some(s) => s,
        None => { write_error("Missing call_graph"); return 1; }
    };
    let entry_points = read_input("entry_points").unwrap_or_default();

    // Parse entry points
    let entries: Vec<String> = entry_points.lines().filter(|l| !l.is_empty()).map(|l| l.to_string()).collect();

    let mut flows: Vec<Value> = Vec::new();
    let mut mermaid_lines: Vec<String> = vec!["flowchart TD".to_string()];

    for entry in &entries {
        let entry_name = entry.trim();

        // Simple traversal simulation: follow call graph from entry
        let path = trace_path(entry_name, &call_graph);

        let phases = classify_phases(&path);
        let side_effect = phases.get("output")
            .and_then(|o| o.as_array())
            .and_then(|a| a.first())
            .cloned()
            .map(|v| v.as_str().unwrap_or("unknown").to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let flow_id = format!("flow_{}", flows.len());
        mermaid_lines.push(format!("  Start_{} --> {}", flow_id, entry_name.replace('.', "_")));

        let mut prev = entry_name.replace('.', "_");
        for (i, node) in path.iter().enumerate() {
            let node_id = format!("{}_{}", node.replace('.', "_"), i);
            mermaid_lines.push(format!("  {} --> {}", prev, node_id));
            prev = node_id;
        }
        mermaid_lines.push(format!("  {} --> End_{}", prev, flow_id));

        flows.push(json!({
            "name": format!("Flow from {}", entry_name),
            "trigger": infer_trigger(entry_name),
            "entry_point": entry_name,
            "path": path,
            "phases": phases,
            "side_effect": side_effect
        }));
    }

    // Use LLM to refine flow descriptions
    if !flows.is_empty() {
        let prompt = format!(
            "Given these code flow paths, provide human-readable summaries:\n{:#?}\n\n\
             For each flow, provide a short description of what it does.",
            flows
        );
        let schema = r#"{"type": "object", "properties": {"descriptions": {"type": "array", "items": {"type": "string"}}}}"#;
        if let Some(result) = llm_complete(&prompt, schema) {
            if let Ok(v) = serde_json::from_str::<Value>(&result) {
                if let Some(descs) = v.get("descriptions").and_then(|d| d.as_array()) {
                    for (i, desc) in descs.iter().enumerate() {
                        if i < flows.len() {
                            if let Some(s) = desc.as_str() {
                                if let Some(obj) = flows[i].as_object_mut() {
                                    obj.insert("name".into(), json!(s));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let output = json!({
        "flows": flows,
        "mermaid": mermaid_lines.join("\n"),
        "flow_count": flows.len()
    });

    write_output(&output);
    0
}

fn trace_path(entry: &str, call_graph: &str) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = entry.to_string();
    let mut visited = std::collections::HashSet::new();
    visited.insert(current.clone());

    for _ in 0..20 {
        // max depth
        path.push(current.clone());
        let mut found = false;
        for line in call_graph.lines() {
            let parts: Vec<&str> = line.splitn(2, "->").collect();
            if parts.len() == 2 && parts[0].trim() == current {
                let next = parts[1].trim().to_string();
                if !visited.contains(&next) {
                    visited.insert(next.clone());
                    current = next;
                    found = true;
                    break;
                }
            }
        }
        if !found {
            break;
        }
    }
    path
}

fn classify_phases(path: &[String]) -> Value {
    let mut phases = json!({
        "input": [],
        "validation": [],
        "business_logic": [],
        "persistence": [],
        "output": []
    });

    for node in path {
        let node_lower = node.to_lowercase();
        let phase = if node_lower.contains("read") || node_lower.contains("parse") || node_lower.contains("input") {
            "input"
        } else if node_lower.contains("valid") || node_lower.contains("check") || node_lower.contains("verify") {
            "validation"
        } else if node_lower.contains("save") || node_lower.contains("write") || node_lower.contains("store") || node_lower.contains("db") {
            "persistence"
        } else if node_lower.contains("output") || node_lower.contains("print") || node_lower.contains("render") || node_lower.contains("display") {
            "output"
        } else {
            "business_logic"
        };

        if let Some(arr) = phases[phase].as_array_mut() {
            arr.push(json!(node));
        }
    }
    phases
}

fn infer_trigger(entry: &str) -> &str {
    if entry.contains("main") || entry.contains("cli") {
        "CLI"
    } else if entry.contains("handler") || entry.contains("route") || entry.contains("controller") {
        "HTTP"
    } else if entry.contains("consumer") || entry.contains("worker") {
        "MQ"
    } else {
        "CLI"
    }
}

fn main() {}
