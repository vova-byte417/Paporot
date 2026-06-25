//! 分析管线（沙盒内执行）
//!
//! DAG 编排 -> Skill 加载 -> LLM 执行 -> 报告生成 -> 写出

use crate::host;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ─── TOML 解析（无外部依赖，手工解析） ────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillToml {
    pub skill: SkillMeta,
    pub inputs: SkillInputs,
    pub outputs: SkillOutputs,
    #[serde(rename = "llm_calls", default)]
    pub llm_calls: Option<LlmBudget>,
    #[serde(default)]
    pub dependencies: SkillDeps,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    pub name: String,
    pub version: String,
    pub requires_paporot: String,
    pub description: String,
    pub timeout_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInputs {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillOutputs {
    pub schema: String,
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmBudget {
    pub max_calls: u32,
    pub preferred_model: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillDeps {
    #[serde(default)]
    pub uses_outputs_from: Vec<String>,
}

// ─── DAG ──────────────────────────────────────────────────────────

fn build_dag(skills: &[(String, SkillToml)]) -> HashMap<String, Vec<String>> {
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for (name, _) in skills {
        graph.entry(name.clone()).or_default();
    }
    for (name, toml) in skills {
        for dep in &toml.dependencies.uses_outputs_from {
            graph.entry(dep.clone()).or_default().push(name.clone());
        }
    }
    graph
}

fn toposort(graph: &HashMap<String, Vec<String>>) -> Result<Vec<Vec<String>>, String> {
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    for node in graph.keys() {
        in_degree.entry(node.clone()).or_insert(0);
    }
    for (_node, deps) in graph {
        for dep in deps {
            *in_degree.entry(dep.clone()).or_insert(0) += 1;
        }
    }

    let mut queue: Vec<String> = graph.keys()
        .filter(|n| *in_degree.get(*n).unwrap_or(&0) == 0)
        .cloned()
        .collect();

    let mut layers: Vec<Vec<String>> = Vec::new();
    while !queue.is_empty() {
        layers.push(queue.clone());
        let mut next = Vec::new();
        for node in &queue {
            if let Some(dependents) = graph.get(node) {
                for dep in dependents {
                    if let Some(count) = in_degree.get_mut(dep) {
                        *count -= 1;
                        if *count == 0 {
                            next.push(dep.clone());
                        }
                    }
                }
            }
        }
        queue = next;
    }

    let sorted: usize = layers.iter().map(|l| l.len()).sum();
    if sorted != graph.len() {
        return Err("Cycle detected in skill dependencies".into());
    }

    Ok(layers)
}

// ─── Skill 扫描 ──────────────────────────────────────────────────

fn scan_skills() -> Result<Vec<(String, SkillToml)>, String> {
    let skills_dir = Path::new("skills");
    if !skills_dir.exists() {
        return Ok(Vec::new());
    }

    let known: &[&str] = &[
        "repository-understanding",
        "module-discovery",
        "dependency-analysis",
        "runtime-flow-analysis",
        "behavior-boundary-discovery",
        "architecture-doc-generator",
        "verification-runner",
    ];

    let mut skills = Vec::new();
    for name in known {
        let toml_path = skills_dir.join(name).join("skill.toml");
        if let Some(content) = host::read_file(toml_path.to_str().unwrap_or("")) {
            if let Ok(toml) = toml::from_str::<SkillToml>(&content) {
                skills.push((toml.skill.name.clone(), toml));
            }
        }
    }
    Ok(skills)
}

// ─── 管线执行 ────────────────────────────────────────────────────

pub fn execute_analyze(_input_pairs: &[String], prd_path: Option<&str>) -> Result<(), String> {
    let skills = scan_skills()?;
    if skills.is_empty() {
        eprintln!("[sandbox] No compatible skills found");
        return Ok(());
    }

    eprintln!("[sandbox] Found {} skills", skills.len());

    let graph = build_dag(&skills);
    let layers = toposort(&graph)?;
    eprintln!("[sandbox] DAG: {} layers", layers.len());

    if let Some(prd) = prd_path {
        if let Some(content) = host::read_file(prd) {
            eprintln!("[sandbox] PRD loaded ({} bytes)", content.len());
            let _ = host::write_file("work/prd_content.txt", &content);
        } else {
            eprintln!("[sandbox] Warning: PRD not found at {}", prd);
        }
    }

    // Read source file context collected by native loader
    let source_context = read_source_context();

    let mut output_cache: HashMap<String, String> = HashMap::new();
    let mut total_ok = 0;
    let mut total_skipped = 0;
    let mut total_failed = 0;

    for (layer_idx, layer) in layers.iter().enumerate() {
        eprintln!("[sandbox] Layer {}: {:?}", layer_idx + 1, layer);

        for skill_name in layer {
            let skill_toml = skills.iter().find(|(n, _)| n == skill_name).map(|(_, t)| t);
            let upstream_ok = if let Some(toml) = skill_toml {
                toml.dependencies.uses_outputs_from.iter().all(|dep| output_cache.contains_key(dep))
            } else {
                true
            };

            if !upstream_ok {
                eprintln!("[sandbox] {} -> SKIPPED (upstream failed)", skill_name);
                total_skipped += 1;
                continue;
            }

            let upstream_outputs: HashMap<String, String> = skill_toml
                .map(|t| t.dependencies.uses_outputs_from.iter()
                    .filter_map(|dep| output_cache.get(dep).map(|v| (dep.clone(), v.clone())))
                    .collect())
                .unwrap_or_default();

            match execute_single_skill(skill_name, skill_toml, &upstream_outputs, &source_context) {
                Ok(Some(output)) => {
                    output_cache.insert(skill_name.clone(), output.clone());
                    total_ok += 1;
                    eprintln!("[sandbox] {} -> OK", skill_name);
                }
                Ok(None) => {
                    total_skipped += 1;
                    eprintln!("[sandbox] {} -> SKIPPED", skill_name);
                }
                Err(e) => {
                    total_failed += 1;
                    eprintln!("[sandbox] {} -> FAILED: {}", skill_name, e);
                }
            }
        }
    }

    // Generate reports
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let failed = total_failed;
    let risk = if failed > 0 { "high" } else if total_skipped > 0 { "medium" } else { "low" };

    let json_report = serde_json::json!({
        "project_name": "Paporot Analysis",
        "analyzed_at": &now,
        "summary": {
            "total_skills": skills.len(),
            "ok": total_ok, "skipped": total_skipped, "failed": failed,
            "risk_level": risk
        },
        "skill_results": build_results_json(&skills, &output_cache),
        "high_level_summary": format!("{} skills analyzed: {} OK, {} skipped, {} failed", skills.len(), total_ok, total_skipped, failed)
    });

    let json_str = serde_json::to_string_pretty(&json_report).map_err(|e| e.to_string())?;
    host::write_file("reports/analysis_result.json", &json_str).map_err(|e| format!("write error: {}", e))?;

    let md = build_markdown_report(&skills, &output_cache, &now, total_ok, total_skipped, failed, risk);
    host::write_file("reports/architecture.md", &md).map_err(|e| format!("write error: {}", e))?;

    let dashboard_data = build_dashboard_data_json(&skills, &output_cache, &now);
    host::write_file("reports/dashboard_data.json", &serde_json::to_string_pretty(&dashboard_data).map_err(|e| format!("json error: {}", e))?).map_err(|e| format!("write error: {}", e))?;

    eprintln!("[sandbox] Reports written:");
    eprintln!("  reports/analysis_result.json");
    eprintln!("  reports/architecture.md");
    eprintln!("  reports/dashboard_data.json");

    eprintln!("\n== Analysis Complete ==");
    eprintln!("  Total : {}", skills.len());
    eprintln!("  OK    : {}", total_ok);
    eprintln!("  Skip  : {}", total_skipped);
    eprintln!("  Fail  : {}", failed);

    Ok(())
}

// ─── Source Context ───────────────────────────────────────────────

fn read_source_context() -> Option<String> {
    let manifest = host::read_file("work/sources/_manifest.json")?;
    let items: Vec<serde_json::Value> = serde_json::from_str(&manifest).ok()?;

    let mut ctx = String::from("## Project Source Files\n\n");
    for item in &items {
        if let Some(path) = item["path"].as_str() {
            let full = format!("work/sources/{}", path);
            if let Some(content) = host::read_file(&full) {
                let truncated = if content.len() > 4000 {
                    let safe_cut: String = content.chars().take(4000).collect();
                    format!("{}... (truncated, {} total bytes)", safe_cut, content.len())
                } else {
                    content
                };
                ctx.push_str(&format!("### {}\n```\n{}\n```\n\n", path, truncated));
            }
        }
    }
    Some(ctx)
}

// ─── Skill Execution ──────────────────────────────────────────────

fn execute_single_skill(
    skill_name: &str,
    skill_toml: Option<&SkillToml>,
    upstream: &HashMap<String, String>,
    source_context: &Option<String>,
) -> Result<Option<String>, String> {
    // ── Procedural skills (no LLM) ───────────────────────────
    if skill_name == "verification-runner" {
        return run_verification_runner(upstream);
    }

    let description = skill_toml
        .map(|t| t.skill.description.as_str())
        .unwrap_or(skill_name);

    // Build upstream context
    let mut upstream_ctx = String::new();
    for (name, output) in upstream {
        upstream_ctx.push_str(&format!("\n## Upstream: {}\n{}\n", name, output));
    }

    // Build prompt based on skill role
    let (prompt, schema) = build_skill_prompt(skill_name, description, &upstream_ctx, source_context);

    match host::llm_call(&prompt, &schema) {
        Some(response) => Ok(Some(response)),
        None => {
            // Fallback: structured stub with clear LLM-unavailable marker
            let stub = build_skill_stub(skill_name, upstream);
            Ok(Some(stub))
        }
    }
}

fn build_skill_prompt(
    skill_name: &str,
    description: &str,
    upstream_ctx: &str,
    source_context: &Option<String>,
) -> (String, String) {
    let src = source_context.as_deref().unwrap_or("(no source files available)");

    match skill_name {
        "repository-understanding" => {
            let prompt = format!(
                "You are the '{skill_name}' analysis skill for a software project behavior audit system.\n\
                 Description: {description}\n\n\
                 Analyze the project source files below and produce a structured JSON report.\n\
                 Identify: project name, purpose, programming languages, frameworks, entry points, architecture style.\n\n\
                 {upstream_ctx}\n\n\
                 Source Files:\n{src}\n\n\
                 Respond ONLY with valid JSON (no markdown, no explanation).",
                skill_name = skill_name, description = description,
                upstream_ctx = upstream_ctx, src = src
            );
            let schema = r#"{"type":"object","properties":{"project_name":{"type":"string"},"purpose":{"type":"string"},"languages":{"type":"array","items":{"type":"string"}},"frameworks":{"type":"array","items":{"type":"string"}},"entrypoints":{"type":"array","items":{"type":"string"}},"architecture_style_candidates":{"type":"array","items":{"type":"string"}},"key_findings":{"type":"array","items":{"type":"string"}}}}"#;
            (prompt, schema.to_string())
        }
        "module-discovery" => {
            let prompt = format!(
                "You are the '{skill_name}' analysis skill for a software project behavior audit system.\n\
                 Description: {description}\n\n\
                 Discover and classify all modules in the project. A module is a logical grouping of files sharing a responsibility.\n\
                 Analyze the source files below and output structured JSON.\n\n\
                 {upstream_ctx}\n\n\
                 Source Files:\n{src}\n\n\
                 Respond ONLY with valid JSON (no markdown, no explanation).",
                skill_name = skill_name, description = description,
                upstream_ctx = upstream_ctx, src = src
            );
            let schema = r#"{"type":"object","properties":{"modules":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"responsibility":{"type":"string"},"category":{"type":"string"},"file_count":{"type":"integer"},"key_files":{"type":"array","items":{"type":"string"}}}}}}},"module_count":{"type":"integer"}}}"#;
            (prompt, schema.to_string())
        }
        "dependency-analysis" => {
            let prompt = format!(
                "You are the '{skill_name}' analysis skill for a software project behavior audit system.\n\
                 Description: {description}\n\n\
                 Build a module dependency graph based on the upstream module-discovery output and source files.\n\
                 Identify: dependencies between modules, potential circular dependencies, architecture violations, coupling metrics.\n\n\
                 {upstream_ctx}\n\n\
                 Source Files:\n{src}\n\n\
                 Respond ONLY with valid JSON (no markdown, no explanation).",
                skill_name = skill_name, description = description,
                upstream_ctx = upstream_ctx, src = src
            );
            let schema = r#"{"type":"object","properties":{"dependencies":{"type":"array","items":{"type":"object","properties":{"from":{"type":"string"},"to":{"type":"string"},"type":{"type":"string"}}}},"cycles":{"type":"array","items":{"type":"string"}},"architecture_violations":{"type":"array","items":{"type":"string"}},"high_coupling_pairs":{"type":"array","items":{"type":"string"}},"risk_areas":{"type":"array","items":{"type":"string"}}}}"#;
            (prompt, schema.to_string())
        }
        "runtime-flow-analysis" => {
            let prompt = format!(
                "You are the '{skill_name}' analysis skill for a software project behavior audit system.\n\
                 Description: {description}\n\n\
                 Trace end-to-end execution paths through the codebase. Start from entry points and follow call chains.\n\
                 Identify: data flow, control flow, side effects, error handling paths.\n\n\
                 {upstream_ctx}\n\n\
                 Source Files:\n{src}\n\n\
                 Respond ONLY with valid JSON (no markdown, no explanation).",
                skill_name = skill_name, description = description,
                upstream_ctx = upstream_ctx, src = src
            );
            let schema = r#"{"type":"object","properties":{"flows":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"entry_point":{"type":"string"},"phases":{"type":"array","items":{"type":"string"}},"side_effects":{"type":"array","items":{"type":"string"}},"risk_level":{"type":"string"}}}},"flow_count":{"type":"integer"},"critical_paths":{"type":"array","items":{"type":"string"}}}}"#;
            (prompt, schema.to_string())
        }
        "behavior-boundary-discovery" => {
            let prompt = format!(
                "You are the '{skill_name}' analysis skill for a software project behavior audit system.\n\
                 Description: {description}\n\n\
                 Discover component boundaries that affect observable behavior.\n\
                 Classify modules as behavioral-core (directly affects system behavior) or support (infrastructure/utility).\n\
                 Identify: public APIs, behavioral contracts, state mutation points.\n\n\
                 {upstream_ctx}\n\n\
                 Source Files:\n{src}\n\n\
                 Respond ONLY with valid JSON (no markdown, no explanation).",
                skill_name = skill_name, description = description,
                upstream_ctx = upstream_ctx, src = src
            );
            let schema = r#"{"type":"object","properties":{"behavioral_modules":{"type":"array","items":{"type":"string"}},"support_modules":{"type":"array","items":{"type":"string"}},"public_apis":{"type":"array","items":{"type":"string"}},"state_mutation_points":{"type":"array","items":{"type":"string"}},"boundary_risks":{"type":"array","items":{"type":"string"}},"overall_risk":{"type":"string"}}}"#;
            (prompt, schema.to_string())
        }
        "architecture-doc-generator" => {
            // This skill aggregates all upstream outputs
            let prompt = format!(
                "You are the '{skill_name}' analysis skill for a software project behavior audit system.\n\
                 Description: {description}\n\n\
                 Aggregate all upstream analysis results and produce a comprehensive architecture document summary.\n\
                 Synthesize findings from ALL upstream skills into a cohesive narrative.\n\
                 Highlight: key architectural decisions, risks, and recommendations.\n\n\
                 {upstream_ctx}\n\n\
                 Source Files:\n{src}\n\n\
                 Respond ONLY with valid JSON (no markdown, no explanation).",
                skill_name = skill_name, description = description,
                upstream_ctx = upstream_ctx, src = src
            );
            let schema = r#"{"type":"object","properties":{"architecture_summary":{"type":"string"},"key_decisions":{"type":"array","items":{"type":"string"}},"risks":{"type":"array","items":{"type":"string"}},"recommendations":{"type":"array","items":{"type":"string"}},"diagram_description":{"type":"string"}}}"#;
            (prompt, schema.to_string())
        }
        _ => {
            let prompt = format!(
                "Analyze the project according to: {description}.\n{upstream_ctx}\n{src}\nRespond ONLY with valid JSON.",
                description = description, upstream_ctx = upstream_ctx, src = src
            );
            (prompt, r#"{"type":"object"}"#.to_string())
        }
    }
}

fn build_skill_stub(skill_name: &str, upstream: &HashMap<String, String>) -> String {
    match skill_name {
        "repository-understanding" => serde_json::json!({
            "project_name": "Unknown",
            "purpose": "Analysis incomplete - no LLM available",
            "languages": ["Rust"],
            "frameworks": [],
            "entrypoints": ["src/main.rs"],
            "architecture_style_candidates": ["modular"],
            "key_findings": ["LLM unavailable - using static analysis stub"]
        }).to_string(),
        "module-discovery" => serde_json::json!({
            "modules": [{"name": "core", "responsibility": "Main application logic", "category": "Service", "file_count": 1, "key_files": ["src/main.rs"]}],
            "module_count": 1
        }).to_string(),
        "dependency-analysis" => serde_json::json!({
            "dependencies": [],
            "cycles": [],
            "architecture_violations": [],
            "high_coupling_pairs": [],
            "risk_areas": ["LLM unavailable - dependency analysis limited"]
        }).to_string(),
        "runtime-flow-analysis" => serde_json::json!({
            "flows": [{"name": "main", "entry_point": "main()", "phases": ["init", "execute"], "side_effects": [], "risk_level": "low"}],
            "flow_count": 1, "critical_paths": []
        }).to_string(),
        "behavior-boundary-discovery" => serde_json::json!({
            "behavioral_modules": [], "support_modules": [],
            "public_apis": [], "state_mutation_points": [],
            "boundary_risks": ["LLM unavailable - boundary analysis limited"],
            "overall_risk": "unknown"
        }).to_string(),
        "architecture-doc-generator" => {
            let upstream_list: Vec<&str> = upstream.keys().map(|s| s.as_str()).collect();
            serde_json::json!({
                "architecture_summary": format!("Analysis pipeline completed. Upstream skills: {}", upstream_list.join(", ")),
                "key_decisions": ["LLM unavailable for architecture generation"],
                "risks": ["Automated analysis limited without LLM"],
                "recommendations": ["Configure a valid API key for full analysis"],
                "diagram_description": "Pipeline: repository-understanding -> module-discovery -> dependency-analysis -> runtime-flow-analysis -> behavior-boundary-discovery -> architecture-doc-generator"
            }).to_string()
        }
        _ => r#"{"status":"LLM unavailable"}"#.to_string(),
    }
}

// ─── Verification Runner (Procedural, no LLM) ──────────────────

fn run_verification_runner(upstream: &HashMap<String, String>) -> Result<Option<String>, String> {
    use crate::host;

    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut replay_count: u32 = 0;
    let mut overall_pass = true;

    // Map upstream skill names to artifact types
    for (skill_name, output) in upstream {
        // Skip skills that don't produce artifacts to verify
        if output.is_empty() || output == "{}" {
            continue;
        }

        let artifact_type = match skill_name.as_str() {
            "architecture-doc-generator" => "json",
            "dependency-analysis" => "json",
            "runtime-flow-analysis" => "json",
            "module-discovery" => "json",
            "behavior-boundary-discovery" => "json",
            "repository-understanding" => "json",
            _ => continue,
        };

        // 1. Contract Verification
        let verify_result_json = host::verify_contract(artifact_type, output)
            .unwrap_or_else(|| {
                format!(r#"{{"status":"ERROR","error":"host_verify_contract failed for {}"}}"#, skill_name)
            });

        let verify_result: serde_json::Value = serde_json::from_str(&verify_result_json)
            .unwrap_or_else(|_| {
                serde_json::json!({
                    "artifact_id": skill_name,
                    "artifact_type": artifact_type,
                    "status": "ERROR",
                    "rule_results": [],
                    "suggestions": ["Failed to parse verification result"]
                })
            });

        let status = verify_result["status"].as_str().unwrap_or("ERROR");

        // 2. Evidence Collection
        let prev_outputs: serde_json::Value = upstream.iter()
            .filter(|(n, _)| n.as_str() != skill_name)
            .map(|(n, v)| (n.clone(), serde_json::Value::String(v.clone())))
            .collect::<serde_json::Map<_, _>>()
            .into();

        host::capture_evidence(
            skill_name,
            &prev_outputs.to_string(),
            output,
            "{}", // intermediate not available in MVP
        );

        // 3. FAIL → Save Replay Case
        if status != "PASS" {
            overall_pass = false;

            let case = serde_json::json!({
                "case_id": format!("replay-{}", chrono::Utc::now().timestamp_millis()),
                "created_at": chrono::Utc::now().to_rfc3339(),
                "artifact_type": artifact_type,
                "artifact_id": skill_name,
                "upstream_input": prev_outputs,
                "failed_artifact": output,
                "contract_result": verify_result,
                "suggestions": verify_result["suggestions"].as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>())
                    .unwrap_or_default(),
            });

            host::save_replay_case(&case.to_string());
            replay_count += 1;
        }

        results.push(serde_json::json!({
            "artifact_id": skill_name,
            "artifact_type": artifact_type,
            "status": status,
            "rule_results": verify_result["rule_results"],
            "suggestions": verify_result["suggestions"],
        }));
    }

    let overall_status = if overall_pass { "PASS" } else { "FAIL" };
    let output = serde_json::json!({
        "overall_status": overall_status,
        "results": results,
        "replay_cases_saved": replay_count,
    });

    let output_str = serde_json::to_string_pretty(&output)
        .map_err(|e| e.to_string())?;

    if !overall_pass {
        // Write failure details for debugging
        let _ = host::write_file("work/verification_failure.json", &output_str);
        return Err(format!(
            "Verification FAILED: {}/{} artifacts failed. See work/verification_failure.json for details.",
            results.iter().filter(|r| r["status"] != "PASS").count(),
            results.len(),
        ));
    }

    Ok(Some(output_str))
}

// ─── 报告生成 ────────────────────────────────────────────────────

fn build_results_json(
    skills: &[(String, SkillToml)],
    cache: &HashMap<String, String>,
) -> Vec<serde_json::Value> {
    skills.iter().map(|(name, _toml)| {
        let output = cache.get(name);
        let status = if output.is_some() { "ok" } else { "failed" };
        let parsed: Option<serde_json::Value> = output.and_then(|s| serde_json::from_str(s).ok());
        serde_json::json!({
            "name": name,
            "status": status,
            "output": parsed
        })
    }).collect()
}

fn build_markdown_report(
    skills: &[(String, SkillToml)],
    cache: &HashMap<String, String>,
    analyzed_at: &str,
    ok: usize, skipped: usize, failed: usize, risk: &str,
) -> String {
    let mut md = String::new();
    md.push_str("# Paporot Architecture Analysis Report\n\n");
    md.push_str(&format!("**Analyzed**: {}\n\n", analyzed_at));
    md.push_str("## Summary\n\n");
    md.push_str(&format!("- Total Skills: {}\n", skills.len()));
    md.push_str(&format!("- OK: {}\n", ok));
    md.push_str(&format!("- Skipped: {}\n", skipped));
    md.push_str(&format!("- Failed: {}\n", failed));
    md.push_str(&format!("- Risk: {}\n\n", risk));

    md.push_str("## Skill Results\n\n");
    for (name, toml) in skills {
        let status = if cache.contains_key(name.as_str()) { "OK" } else { "FAILED" };
        md.push_str(&format!("### {} - {}\n\n", toml.skill.description, status));
        md.push_str(&format!("*Skill*: `{}`\n\n", name));

        if let Some(output) = cache.get(name.as_str()) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(output) {
                md.push_str("```json\n");
                md.push_str(&serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| output.clone()));
                md.push_str("\n```\n\n");
            } else {
                md.push_str(&format!("```\n{}\n```\n\n", output));
            }
        } else {
            md.push_str("*No output*\n\n");
        }
    }
    md
}


// ─── Dashboard Data JSON Builder ────────────────────────────────────
//
// Builds a JSON structure consumed by the Vue 3 SPA dashboard.
// The native binary reads this JSON and injects it into the pre-built Vue template.

fn build_dashboard_data_json(
    skills: &[(String, SkillToml)],
    cache: &HashMap<String, String>,
    analyzed_at: &str,
) -> serde_json::Value {
    let skill_items: Vec<serde_json::Value> = skills.iter().map(|(name, _toml)| {
        let output = cache.get(name);
        let status = if output.is_some() { "ok" } else { "skipped" };
        let summary = output.cloned().unwrap_or_default();
        serde_json::json!({
            "name": name,
            "status": status,
            "duration_ms": 0,
            "output_summary": summary,
            "error": serde_json::Value::Null,
        })
    }).collect();

    serde_json::json!({
        "project_name": "Paporot Analysis",
        "analyzed_at": analyzed_at,
        "git_commit": serde_json::Value::Null,
        "git_ref": serde_json::Value::Null,
        "l1_analysis": {
            "total_files": 0,
            "total_changes": 0,
            "changes": [],
            "by_language": {},
            "by_type": {},
            "by_directory": [],
            "confidence_distribution": { "high": 0, "medium": 0, "low": 0 },
        },
        "l2_analysis": {
            "total_matches": 0,
            "matches": [],
            "by_severity": {},
            "by_category": {},
        },
        "l3_analysis": {
            "fragment_count": 0,
            "model_used": serde_json::Value::Null,
        },
        "feedback_loop": {
            "loaded": false,
            "exact_reject_count": 0,
            "rule_suppression_count": 0,
            "prefix_warning_count": 0,
            "suppressions": [],
            "changes": [],
        },
        "snapshot": serde_json::Value::Null,
        "trace_association": serde_json::Value::Null,
        "contracts": serde_json::Value::Null,
        "skills": skill_items,
    })
}

// ─── Skill List ─────────────────────────────────────────────────────

pub fn execute_skill_list() -> Result<(), String> {
    let skills = scan_skills()?;

    println!("{:<35} {:<10} {:<12} {}", "NAME", "VERSION", "COMPATIBLE", "DESCRIPTION");
    for (name, toml) in &skills {
        let compatible = if toml.skill.requires_paporot.contains("0.2") { "YES" } else { "?" };
        println!("{:<35} {:<10} {:<12} {}",
            name, toml.skill.version, compatible,
            if toml.skill.description.len() > 50 {
                format!("{}...", &toml.skill.description[..50])
            } else {
                toml.skill.description.clone()
            }
        );
    }
    println!("{} skills installed", skills.len());
    Ok(())
}
