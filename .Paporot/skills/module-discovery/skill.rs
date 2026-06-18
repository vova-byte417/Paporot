/// Skill: Module Discovery
///
/// Goal: 发现系统中的业务模块和技术模块，聚类相关文件并推断职责
///
/// Inputs: repo_tree, ast_symbols, import_graph
/// Output: module_discovery_output JSON

use paporot_skill_sdk::prelude::*;

#[no_mangle]
pub extern "C" fn paporot_skill_execute() -> i32 {
    let repo_tree = match read_input("repo_tree") {
        Some(s) => s,
        None => { write_error("Missing repo_tree"); return 1; }
    };
    let ast_symbols = read_input("ast_symbols").unwrap_or_default();
    let import_graph = read_input("import_graph").unwrap_or_default();

    let tree: Value = serde_json::from_str(&repo_tree).unwrap_or(json!({}));

    // Group files by directory
    let mut modules: Vec<Value> = Vec::new();
    let files = tree.get("files").and_then(|f| f.as_array());

    if let Some(files) = files {
        let mut dir_groups: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for file in files {
            if let Some(path) = file.get("path").and_then(|p| p.as_str()) {
                let parts: Vec<&str> = path.split('/').collect();
                let dir = if parts.len() > 1 { parts[0].to_string() } else { "root".to_string() };
                dir_groups.entry(dir).or_default().push(path.to_string());
            }
        }

        for (dir, files) in &dir_groups {
            let category = classify_dir(dir);
            modules.push(json!({
                "name": dir,
                "responsibility": format!("Module {}", dir),
                "files": files,
                "category": category,
                "public_symbols": [],
                "file_count": files.len()
            }));
        }
    }

    // Use LLM to refine module responsibilities
    let prompt = format!(
        "Given these file groupings by directory, infer the responsibility of each module. \
         Respond with an array of {{name, responsibility}} where responsibility is max 2 sentences.\n\n\
         AST symbols:\n{}\n\nImport graph:\n{}\n\nFile groups:\n{:?}",
        &ast_symbols[..ast_symbols.len().min(2000)],
        &import_graph[..import_graph.len().min(2000)],
        modules
    );

    let schema = r#"{
        "type": "object",
        "properties": {
            "modules": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "responsibility": {"type": "string"}
                    }
                }
            }
        }
    }"#;

    let llm_result = llm_complete(&prompt, schema);

    // Merge LLM results
    if let Some(ref result) = llm_result {
        if let Ok(refined) = serde_json::from_str::<Value>(result) {
            if let Some(refined_modules) = refined.get("modules").and_then(|m| m.as_array()) {
                for rm in refined_modules {
                    let name = rm.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    for m in &mut modules {
                        if m.get("name").and_then(|n| n.as_str()) == Some(name) {
                            if let Some(resp) = rm.get("responsibility").and_then(|r| r.as_str()) {
                                if let Some(obj) = m.as_object_mut() {
                                    obj.insert("responsibility".into(), json!(resp));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let output = json!({
        "modules": modules,
        "module_count": modules.len(),
        "file_count": files.map(|f| f.len()).unwrap_or(0)
    });

    write_output(&output);
    0
}

fn classify_dir(dir: &str) -> &str {
    match dir {
        "src" | "lib" | "main" => "Service",
        "api" | "routes" | "handlers" => "API",
        "domain" | "models" | "entities" => "Domain",
        "db" | "storage" | "repository" => "Storage",
        "infra" | "infrastructure" | "config" => "Infrastructure",
        "utils" | "helpers" | "common" => "Utility",
        _ => "Service",
    }
}

fn main() {}
