/// Skill: Architecture Document Generator
///
/// Goal: 聚合所有上游 Skill 输出，生成架构文档
///
/// Inputs: 上游 Skill 的输出（通过 cache 获取）
/// Output: architecture_doc_output JSON

use paporot_skill_sdk::prelude::*;

#[no_mangle]
pub extern "C" fn paporot_skill_execute() -> i32 {
    // Read upstream outputs
    let repo_understanding = read_input("skill_output__repository-understanding");
    let module_discovery = read_input("skill_output__module-discovery");
    let dependency_analysis = read_input("skill_output__dependency-analysis");
    let flow_analysis = read_input("skill_output__runtime-flow-analysis");
    let behavior_boundary = read_input("skill_output__behavior-boundary-discovery");

    let mut sections: Vec<Value> = Vec::new();

    // Section 1: Project Overview
    sections.push(json!({
        "id": "project_overview",
        "title": "Project Overview",
        "status": if repo_understanding.is_some() { "ok" } else { "skipped" },
        "data": repo_understanding.clone()
    }));

    // Section 2: Module Catalog
    sections.push(json!({
        "id": "module_catalog",
        "title": "Module Catalog",
        "status": if module_discovery.is_some() { "ok" } else { "skipped" },
        "data": module_discovery.clone()
    }));

    // Section 3: Dependency Graph
    sections.push(json!({
        "id": "dependency_graph",
        "title": "Dependency Graph",
        "status": if dependency_analysis.is_some() { "ok" } else { "skipped" },
        "data": dependency_analysis.clone()
    }));

    // Section 4: Runtime Flows
    sections.push(json!({
        "id": "runtime_flows",
        "title": "Runtime Flows",
        "status": if flow_analysis.is_some() { "ok" } else { "skipped" },
        "data": flow_analysis.clone()
    }));

    // Section 5: Behavioral Components
    sections.push(json!({
        "id": "behavioral_components",
        "title": "Behavioral Components",
        "status": if behavior_boundary.is_some() { "ok" } else { "skipped" },
        "data": behavior_boundary.clone()
    }));

    // Generate summary
    let ok_count = sections.iter().filter(|s| s["status"] == "ok").count();
    let skipped_count = sections.len() - ok_count;

    let summary = if skipped_count == 0 {
        "All analysis sections completed successfully.".to_string()
    } else {
        format!("{} sections OK, {} sections skipped due to upstream errors.", ok_count, skipped_count)
    };

    // Use LLM to generate high-level analysis
    let prompt = format!(
        "You are analyzing a software project. Based on these analysis results, \
         write a brief (2-3 paragraph) high-level analysis summarizing the \
         architecture, key findings, and any risks or recommendations.\n\n\
         Project overview: {}\n\
         Modules: {}\n\
         Dependencies: {}\n\
         Flows: {}\n\
         Behavior boundary: {}",
        repo_understanding.as_deref().unwrap_or("N/A"),
        module_discovery.as_deref().unwrap_or("N/A"),
        dependency_analysis.as_deref().unwrap_or("N/A"),
        flow_analysis.as_deref().unwrap_or("N/A"),
        behavior_boundary.as_deref().unwrap_or("N/A")
    );

    let schema = r#"{"type": "object", "properties": {"high_level_summary": {"type": "string"}}}"#;
    let high_level_summary = llm_complete(&prompt, schema)
        .map(|s| {
            serde_json::from_str::<Value>(&s)
                .ok()
                .and_then(|v| v.get("high_level_summary")
                    .and_then(|h| h.as_str())
                    .map(|s| s.to_string()))
        })
        .flatten()
        .unwrap_or_else(|| "Analysis completed.".to_string());

    let output = json!({
        "generated_files": [
            ".paporot/reports/architecture.md",
            ".paporot/reports/behavior.md",
            ".paporot/reports/data/analysis_result.json"
        ],
        "sections_status": {
            "project_overview": if repo_understanding.is_some() { "ok" } else { "skipped" },
            "module_catalog": if module_discovery.is_some() { "ok" } else { "skipped" },
            "dependency_graph": if dependency_analysis.is_some() { "ok" } else { "skipped" },
            "runtime_flows": if flow_analysis.is_some() { "ok" } else { "skipped" },
            "behavioral_components": if behavior_boundary.is_some() { "ok" } else { "skipped" },
            "coverage": "skipped"
        },
        "summary": summary,
        "high_level_summary": high_level_summary,
        "sections": sections
    });

    write_output(&output);
    0
}

fn main() {}
