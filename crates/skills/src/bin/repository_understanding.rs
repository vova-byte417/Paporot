/// Skill: Repository Understanding
///
/// Goal: 识别项目整体目标、技术栈、入口程序、核心业务能力
///
/// Inputs: repo_tree, repo_files, git_meta
/// Output: repository_understanding_output JSON

use paporot_skill_sdk::prelude::*;

#[no_mangle]
pub extern "C" fn paporot_skill_execute() -> i32 {
    // Step 1: Read inputs
    let repo_tree = match read_input("repo_tree") {
        Some(s) => s,
        None => {
            write_error("Missing required input: repo_tree");
            return 1;
        }
    };
    let repo_files = match read_input("repo_files") {
        Some(s) => s,
        None => {
            write_error("Missing required input: repo_files");
            return 1;
        }
    };
    let git_meta = read_input("git_meta").unwrap_or_default();

    // Step 2: Basic parsing of repo_tree
    let tree_json: Value = match serde_json::from_str(&repo_tree) {
        Ok(v) => v,
        Err(e) => {
            write_error(&format!("Failed to parse repo_tree: {}", e));
            return 1;
        }
    };

    // Step 3: Detect entrypoints from tree
    let files = tree_json.get("files").and_then(|f| f.as_array());
    let mut entrypoints: Vec<String> = Vec::new();
    if let Some(files) = files {
        for file in files {
            if let Some(path) = file.get("path").and_then(|p| p.as_str()) {
                if path.ends_with("main.rs")
                    || path.ends_with("main.go")
                    || path.ends_with("main.py")
                    || path.ends_with("index.ts")
                    || path.ends_with("app.ts")
                    || path == "src/bin"
                {
                    entrypoints.push(path.to_string());
                }
            }
        }
    }

    // Step 4: Use LLM to infer project purpose
    let prompt = format!(
        "Analyze this repository structure and infer its purpose.\n\
         Repository tree summary (key files):\n{}\n\n\
         Project metadata:\n{}\n\n\
         Git info:\n{}\n",
        repo_files,
        extract_metadata(&tree_json),
        git_meta
    );

    let schema = r#"{
        "type": "object",
        "properties": {
            "project_name": {"type": "string"},
            "purpose": {"type": "string", "description": "1-2 sentences explaining what this project does"},
            "languages": {"type": "array", "items": {"type": "string"}},
            "frameworks": {"type": "array", "items": {"type": "string"}},
            "architecture_style_candidates": {"type": "array", "items": {"type": "string"}, "description": "One or more of: modular_pipeline, layered, hexagonal, microservices, monolithic, cli_application"}
        },
        "required": ["project_name", "purpose", "languages", "frameworks"]
    }"#;

    let llm_result = llm_complete(&prompt, schema).unwrap_or_else(|| {
        json!({"project_name": "unknown", "purpose": "LLM unavailable", "languages": [], "frameworks": [], "architecture_style_candidates": []}).to_string()
    });

    // Step 5: Assemble output
    let mut output: Value = match serde_json::from_str(&llm_result) {
        Ok(v) => v,
        Err(_) => json!({"project_name": "unknown", "purpose": "LLM result unparseable", "languages": [], "frameworks": [], "architecture_style_candidates": []}),
    };

    // Add entrypoints and evidence
    if let Some(obj) = output.as_object_mut() {
        obj.insert("entrypoints".into(), json!(entrypoints));
        obj.insert("evidence".into(), json!([
            {"source_file": "repo_tree", "finding": format!("{} files scanned", tree_json.get("files").and_then(|f| f.as_array()).map(|a| a.len()).unwrap_or(0))}
        ]));
    }

    write_output(&output);
    0
}

fn extract_metadata(tree: &Value) -> String {
    let mut meta = String::new();
    if let Some(name) = tree.get("project_name").and_then(|v| v.as_str()) {
        meta.push_str(&format!("Project: {}\n", name));
    }
    if let Some(root) = tree.get("root").and_then(|v| v.as_str()) {
        meta.push_str(&format!("Root: {}\n", root));
    }
    meta
}

fn main() {}
