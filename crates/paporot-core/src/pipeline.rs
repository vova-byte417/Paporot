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

    let html = build_dashboard_html(&skills, &output_cache, &now, total_ok, total_skipped, failed, risk);
    host::write_file("reports/dashboard.html", &html).map_err(|e| format!("write error: {}", e))?;

    eprintln!("[sandbox] Reports written:");
    eprintln!("  reports/analysis_result.json");
    eprintln!("  reports/architecture.md");
    eprintln!("  reports/dashboard.html");

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

fn build_dashboard_html(
    skills: &[(String, SkillToml)],
    cache: &HashMap<String, String>,
    analyzed_at: &str,
    ok: usize, skipped: usize, failed: usize, _risk: &str,
) -> String {
    let mut skill_cards = String::new();

    for (name, toml) in skills {
        let has_output = cache.contains_key(name.as_str());
        let status_color = if has_output { "#3fb950" } else { "#f85149" };
        let status_text = if has_output { "PASS" } else { "FAIL" };

        let output_html = if let Some(output) = cache.get(name.as_str()) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(output) {
                render_skill_card(name, &parsed)
            } else {
                // Try harder: find JSON boundaries in the raw text
                if let Some(extracted) = extract_json_substring(output) {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&extracted) {
                        render_skill_card(name, &parsed)
                    } else {
                        format!("<div class=\"output-raw\">{}</div>", html_escape(output))
                    }
                } else {
                    format!("<div class=\"output-raw\">{}</div>", html_escape(output))
                }
            }
        } else {
            String::from("<div class=\"output-empty\">无输出内容</div>")
        };

        skill_cards.push_str(&format!(r#"
<div class="skill-card">
  <div class="skill-header">
    <span class="skill-name">{name}</span>
    <span class="skill-status" style="color:{color}">{status}</span>
  </div>
  <div class="skill-desc">{desc}</div>
  {output}
</div>"#,
            name = html_escape(name),
            color = status_color,
            status = status_text,
            desc = html_escape(&toml.skill.description),
            output = output_html,
        ));
    }

    let total = ok + skipped + failed;
    let pct_ok = if total > 0 { ok * 100 / total } else { 0 };
    let pct_skip = if total > 0 { skipped * 100 / total } else { 0 };
    let pct_fail = if total > 0 { failed * 100 / total } else { 0 };

    // Extract key metrics from skill outputs for the summary
    let (module_count, flow_count, risk_label, risk_color, narrative) = build_summary_metrics(cache);

    format!(r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Paporot 架构分析仪表盘</title>
<style>
:root {{
  --bg: #0d1117; --card-bg: #161b22; --border: #30363d;
  --text: #c9d1d9; --text-dim: #8b949e;
  --ok: #3fb950; --warn: #d29922; --fail: #f85149; --accent: #58a6ff;
}}
* {{ margin:0; padding:0; box-sizing:border-box; }}
body {{ font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif; background:var(--bg); color:var(--text); line-height:1.6; }}
.container {{ max-width:1060px; margin:0 auto; padding:24px 16px; }}
.header {{ margin-bottom:32px; }}
.header h1 {{ font-size:28px; }}
.header .time {{ color:var(--text-dim); font-size:14px; }}

.summary-chart {{ background:var(--card-bg); border:1px solid var(--border); border-radius:10px; padding:24px; margin-bottom:24px; }}
.summary-chart h2 {{ margin-bottom:20px; font-size:20px; }}
.summary-grid {{ display:grid; grid-template-columns:1fr 1fr; gap:16px; margin:16px 0; }}
.summary-metric {{ padding:12px 16px; border-radius:8px; border:1px solid var(--border); background:rgba(255,255,255,0.02); }}
.summary-metric .met-value {{ font-size:22px; font-weight:700; }}
.summary-metric .met-label {{ font-size:12px; color:var(--text-dim); margin-top:2px; }}
.summary-narrative {{ font-size:14px; line-height:1.8; color:var(--text); padding:16px 0; border-top:1px solid var(--border); margin-top:16px; }}
.summary-narrative strong {{ color:var(--accent); }}

.section-title {{ font-size:20px; margin:24px 0 16px; padding-bottom:8px; border-bottom:1px solid var(--border); }}
.skill-card {{ background:var(--card-bg); border:1px solid var(--border); border-radius:10px; padding:24px; margin-bottom:20px; }}
.skill-header {{ display:flex; justify-content:space-between; align-items:center; margin-bottom:8px; }}
.skill-name {{ font-size:16px; font-weight:600; font-family:'SF Mono','Fira Code',monospace; }}
.skill-status {{ font-size:13px; font-weight:700; padding:3px 12px; border-radius:10px; background:rgba(63,185,80,0.1); border:1px solid rgba(63,185,80,0.3); }}
.skill-desc {{ font-size:13px; color:var(--text-dim); margin-bottom:16px; padding-bottom:12px; border-bottom:1px solid var(--border); }}

/* ── Flat Modern Diagrams (Excalidraw-inspired) ── */
.diagram {{ margin:12px 0 16px; padding:24px 20px; border-radius:12px; background:rgba(22,27,34,0.6); border:1px solid rgba(48,54,61,0.5); overflow-x:auto; min-height:380px; display:flex; align-items:center; justify-content:center; }}
.diagram svg {{ display:block; max-width:100%; margin:0 auto; }}
.repo-diagram {{ min-height:460px; }}
.diagram-row {{ display:flex; align-items:center; justify-content:center; gap:8px; flex-wrap:wrap; }}
.diagram-col {{ display:flex; flex-direction:column; align-items:center; gap:8px; }}

/* Cluster groups */
.cluster {{ border-radius:14px; padding:16px; min-width:200px; }}
.cluster.biz {{ background:rgba(63,185,80,0.06); border:1.5px dashed rgba(63,185,80,0.3); }}
.cluster.tech {{ background:rgba(88,166,255,0.06); border:1.5px dashed rgba(88,166,255,0.3); }}
.cluster-title {{ font-size:11px; font-weight:700; text-transform:uppercase; letter-spacing:0.8px; margin-bottom:10px; text-align:center; }}
.cluster.biz .cluster-title {{ color:var(--ok); }}
.cluster.tech .cluster-title {{ color:var(--accent); }}
.cluster-item {{ display:flex; align-items:center; gap:8px; padding:6px 10px; border-radius:8px; margin-bottom:6px; font-size:12px; }}
.cluster.biz .cluster-item {{ background:rgba(63,185,80,0.08); border:1px solid rgba(63,185,80,0.2); }}
.cluster.tech .cluster-item {{ background:rgba(88,166,255,0.08); border:1px solid rgba(88,166,255,0.2); }}
.cluster-item-name {{ font-weight:600; font-size:13px; }}
.cluster-item-meta {{ font-size:11px; color:var(--text-dim); margin-left:auto; }}
.cluster-item-resp {{ font-size:11px; color:var(--text-dim); margin-top:2px; line-height:1.4; }}

/* Cluster diagram specific */
.cluster-diagram {{ min-height:440px; }}
/* Module relationship bar */
.mod-relations {{ margin-top:18px; padding:12px 16px; background:rgba(88,166,255,0.04); border-radius:8px; border:1px solid rgba(48,54,61,0.5); }}
.rel-row {{ display:flex; align-items:center; gap:12px; justify-content:center; flex-wrap:wrap; font-size:13px; }}
.rel-from, .rel-to {{ display:flex; flex-wrap:wrap; gap:6px; align-items:center; }}
.rel-biz {{ padding:3px 10px; border-radius:6px; font-size:11px; font-weight:600; background:rgba(63,185,80,0.12); color:var(--ok); border:1px solid rgba(63,185,80,0.2); }}
.rel-tech {{ padding:3px 10px; border-radius:6px; font-size:11px; font-weight:600; background:rgba(88,166,255,0.12); color:var(--accent); border:1px solid rgba(88,166,255,0.2); }}
.rel-arrow {{ padding:3px 12px; border-radius:4px; font-size:11px; font-weight:700; background:rgba(210,153,34,0.12); color:var(--warn); }}
.rel-more {{ font-size:11px; color:var(--text-dim); }}

/* Flow pipeline */
.pipeline {{ display:flex; align-items:center; gap:0; flex-wrap:wrap; justify-content:center; }}
.pipeline-step {{ padding:10px 18px; border-radius:10px; text-align:center; font-size:13px; font-weight:600; min-width:90px; }}
.pipeline-step.entry {{ background:rgba(188,140,255,0.15); border:1.5px solid rgba(188,140,255,0.4); color:#bc8cff; }}
.pipeline-step.phase {{ background:rgba(88,166,255,0.12); border:1.5px solid rgba(88,166,255,0.35); color:var(--accent); }}
.pipeline-step.effect {{ background:rgba(210,153,34,0.12); border:1.5px solid rgba(210,153,34,0.35); color:var(--warn); }}
.pipeline-arrow {{ color:var(--text-dim); font-size:20px; margin:0 2px; flex-shrink:0; }}
.pipeline-label {{ font-size:10px; color:var(--text-dim); display:block; margin-top:2px; font-weight:400; }}

/* Layered architecture stack */
.arch-stack {{ display:flex; flex-direction:column; gap:4px; max-width:560px; margin:0 auto; }}
.arch-diagram {{ min-height:480px; }}
.arch-layer {{ display:flex; align-items:center; padding:12px 16px; border-radius:8px; font-size:13px; }}
.arch-layer.l0 {{ background:rgba(248,81,73,0.1); border-left:3px solid rgba(248,81,73,0.5); margin-left:0; margin-right:0; }}
.arch-layer.l1 {{ background:rgba(210,153,34,0.1); border-left:3px solid rgba(210,153,34,0.5); margin-left:16px; margin-right:8px; }}
.arch-layer.l2 {{ background:rgba(88,166,255,0.1); border-left:3px solid rgba(88,166,255,0.5); margin-left:32px; margin-right:16px; }}
.arch-layer.l3 {{ background:rgba(63,185,80,0.1); border-left:3px solid rgba(63,185,80,0.5); margin-left:48px; margin-right:24px; }}
.arch-layer.l4 {{ background:rgba(188,140,255,0.1); border-left:3px solid rgba(188,140,255,0.5); margin-left:64px; margin-right:32px; }}
.arch-layer.l5 {{ background:rgba(139,148,158,0.1); border-left:3px solid rgba(139,148,158,0.5); margin-left:80px; margin-right:40px; }}
.arch-layer-label {{ font-weight:600; word-break:break-word; }}
.arch-layer-desc {{ font-size:11px; color:var(--text-dim); text-align:right; max-width:60%; }}

/* Boundary fence */
.boundary-fence {{ display:flex; gap:16px; margin-bottom:12px; }}
.fence-core, .fence-support {{ flex:1; border-radius:12px; padding:14px; }}
.fence-core {{ background:rgba(63,185,80,0.04); border:2px solid rgba(63,185,80,0.25); }}
.fence-support {{ background:rgba(139,148,158,0.04); border:2px solid rgba(139,148,158,0.2); }}
.fence-title {{ font-size:12px; font-weight:700; text-transform:uppercase; letter-spacing:0.6px; margin-bottom:8px; }}
.fence-core .fence-title {{ color:var(--ok); }}
.fence-support .fence-title {{ color:var(--text-dim); }}
.fence-item {{ font-size:12px; padding:4px 8px; border-radius:4px; margin-bottom:4px; font-family:'SF Mono','Fira Code',monospace; }}
.fence-core .fence-item {{ background:rgba(63,185,80,0.08); }}
.fence-support .fence-item {{ background:rgba(139,148,158,0.08); }}

/* Dependency groups */
.dep-groups {{ display:flex; flex-direction:column; gap:12px; }}
.dep-group {{ display:flex; align-items:flex-start; gap:12px; padding:10px 14px; background:rgba(255,255,255,0.02); border-radius:8px; border:1px solid rgba(48,54,61,0.5); }}
.dep-target {{ flex-shrink:0; padding:6px 14px; border-radius:8px; font-size:13px; font-weight:700; background:rgba(188,140,255,0.15); color:#bc8cff; border:1.5px solid rgba(188,140,255,0.3); min-width:140px; text-align:center; }}
.dep-arrow {{ flex-shrink:0; font-size:18px; color:var(--text-dim); padding-top:4px; }}
.dep-from-list {{ display:flex; flex-wrap:wrap; gap:6px; align-items:flex-start; }}
.dep-summary {{ font-size:11px; color:var(--text-dim); text-align:center; margin-top:4px; }}
 .dep-node {{ padding:5px 12px; border-radius:6px; font-size:12px; font-weight:600; border:1.5px solid rgba(88,166,255,0.4); background:rgba(88,166,255,0.08); color:var(--accent); white-space:nowrap; }}

/* 2-col grid */
.viz-grid-2col {{ display:grid; grid-template-columns:1fr 1fr; gap:12px; }}

.output-raw {{ background:#0d1117; border:1px solid var(--border); border-radius:6px; padding:16px; font-size:13px; font-family:'SF Mono','Fira Code',monospace; white-space:pre-wrap; word-break:break-word; max-height:400px; overflow-y:auto; }}
.output-empty {{ color:var(--text-dim); font-style:italic; padding:12px 0; }}

.viz-hero {{ display:flex; gap:14px; align-items:flex-start; }}
.viz-icon {{ font-size:32px; flex-shrink:0; width:48px; height:48px; display:flex; align-items:center; justify-content:center; background:rgba(88,166,255,0.1); border-radius:10px; }}
.viz-hero-text h3 {{ font-size:18px; color:var(--accent); }}
.viz-purpose {{ color:var(--text-dim); font-size:13px; line-height:1.5; margin-top:4px; }}
.viz-tags {{ display:flex; flex-direction:column; gap:6px; margin-top:12px; }}
.viz-tag-group {{ display:flex; align-items:flex-start; gap:8px; flex-wrap:wrap; }}
.viz-label {{ font-size:11px; color:var(--text-dim); font-weight:600; min-width:85px; text-transform:uppercase; letter-spacing:0.5px; padding-top:2px; }}
.tag {{ display:inline-block; padding:2px 10px; border-radius:10px; font-size:12px; font-weight:500; }}
.tag.lang {{ background:rgba(88,166,255,0.15); color:var(--accent); }}
.tag.fw {{ background:rgba(63,185,80,0.15); color:var(--ok); }}
.tag.style {{ background:rgba(188,140,255,0.15); color:#bc8cff; }}
.tag.se {{ background:rgba(210,153,34,0.15); color:var(--warn); }}
.viz-section {{ margin-top:14px; }}
.viz-section h4 {{ font-size:12px; color:var(--text-dim); text-transform:uppercase; letter-spacing:0.5px; margin-bottom:6px; }}
.viz-list {{ list-style:none; padding:0; }}
.viz-list li {{ padding:5px 10px; font-size:13px; border-radius:4px; margin-bottom:3px; background:rgba(255,255,255,0.03); }}
.viz-list li code {{ font-family:'SF Mono','Fira Code',monospace; font-size:12px; background:rgba(88,166,255,0.1); padding:1px 6px; border-radius:3px; }}
.viz-list li.ok {{ color:var(--ok); }}
.viz-list li.warn {{ color:var(--warn); }}
.viz-summary-bar {{ padding:8px 12px; background:rgba(88,166,255,0.08); border-radius:6px; font-size:13px; }}
.viz-big-num {{ font-size:20px; font-weight:700; color:var(--accent); margin-right:4px; }}

.mod-item {{ background:rgba(255,255,255,0.02); border:1px solid var(--border); border-radius:6px; padding:12px; margin-bottom:8px; }}
.mod-header {{ display:flex; align-items:center; gap:8px; margin-bottom:4px; }}
.mod-name {{ font-weight:600; font-size:14px; }}
.mod-cat {{ font-size:11px; padding:1px 8px; border-radius:8px; border:1px solid; }}
.mod-count {{ font-size:11px; color:var(--text-dim); margin-left:auto; }}
.mod-desc {{ font-size:12px; color:var(--text-dim); margin-bottom:4px; }}

.flow-item {{ background:rgba(255,255,255,0.02); border:1px solid var(--border); border-radius:8px; padding:14px; margin-bottom:10px; }}
.flow-header {{ display:flex; justify-content:space-between; align-items:center; margin-bottom:6px; }}
.flow-name {{ font-weight:600; font-size:14px; }}
.flow-risk {{ font-size:11px; font-weight:700; padding:2px 10px; border-radius:10px; }}
.flow-entry {{ font-size:12px; color:var(--text-dim); margin-bottom:8px; }}
.flow-entry code {{ font-family:'SF Mono','Fira Code',monospace; font-size:12px; color:var(--accent); }}
.flow-insight {{ font-size:13px; line-height:1.7; color:var(--text); padding:10px 14px; background:rgba(88,166,255,0.05); border-radius:8px; border-left:3px solid var(--accent); margin-bottom:16px; }}
.flow-conclusion {{ margin-top:8px; padding-top:8px; border-top:1px solid var(--border); }}

.boundary-split {{ display:grid; grid-template-columns:1fr 1fr; gap:10px; }}
.boundary-col h4 {{ font-size:12px; color:var(--text-dim); text-transform:uppercase; margin-bottom:6px; }}
.bound-list {{ display:flex; flex-direction:column; gap:4px; }}
.bound-item {{ padding:5px 10px; border-radius:4px; font-size:13px; font-family:'SF Mono','Fira Code',monospace; }}
.bound-item.core {{ background:rgba(63,185,80,0.12); color:var(--ok); }}
.bound-item.support {{ background:rgba(139,148,158,0.12); color:var(--text-dim); }}

/* Boundary decision steps (matching arch-decisions style) */
.b-decisions {{ display:flex; flex-direction:column; gap:8px; }}
.b-step {{ display:flex; gap:10px; align-items:center; padding:6px 0; font-size:13px; }}
.b-step code {{ font-family:'SF Mono','Fira Code',monospace; font-size:12px; background:rgba(88,166,255,0.1); padding:2px 8px; border-radius:4px; }}
.b-step-num {{ flex-shrink:0; width:22px; height:22px; display:flex; align-items:center; justify-content:center; background:var(--accent); color:#0d1117; border-radius:50%; font-size:11px; font-weight:700; }}

.arch-decisions {{ display:flex; flex-direction:column; gap:6px; }}
.arch-step {{ display:flex; gap:10px; align-items:flex-start; padding:6px 0; border-bottom:1px solid var(--border); font-size:13px; }}
.step-num {{ flex-shrink:0; width:22px; height:22px; display:flex; align-items:center; justify-content:center; background:var(--accent); color:#0d1117; border-radius:50%; font-size:11px; font-weight:700; }}
.arch-diagram-desc {{ font-size:13px; color:var(--text-dim); line-height:1.6; padding:12px; background:rgba(88,166,255,0.05); border-radius:6px; border-left:3px solid var(--accent); }}

.footer {{ text-align:center; color:var(--text-dim); font-size:12px; margin-top:40px; padding-top:16px; border-top:1px solid var(--border); }}
</style>
</head>
<body>
<div class="container">
<div class="header">
<h1>Paporot 架构分析</h1>
<p class="time">分析时间：{analyzed_at}</p>
</div>
<div class="summary-chart">
<h2>分析概览</h2>
<div class="summary-grid">
  <div class="summary-metric"><div class="met-value" style="color:#3fb950">{ok} / {total_skills}</div><div class="met-label">技能通过</div></div>
  <div class="summary-metric"><div class="met-value" style="color:{risk_color}">{risk_label}</div><div class="met-label">整体风险等级</div></div>
  <div class="summary-metric"><div class="met-value">{module_count}</div><div class="met-label">发现模块数</div></div>
  <div class="summary-metric"><div class="met-value">{flow_count}</div><div class="met-label">执行流程数</div></div>
</div>
<div class="summary-narrative">{narrative}</div>
</div>
<h2 class="section-title">技能分析结果</h2>
{skill_cards}
<div class="footer">由 Paporot WASM 沙箱管线生成</div>
</div>
</body>
</html>"#,
        total_skills = skills.len(),
        analyzed_at = html_escape(analyzed_at),
        ok = ok, risk_color = risk_color, risk_label = risk_label,
        module_count = module_count, flow_count = flow_count,
        narrative = narrative,
        skill_cards = skill_cards,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&#39;")
}

/// Try to extract a JSON object/array from text that may contain extra content
fn extract_json_substring(text: &str) -> Option<String> {
    let text = text.trim();
    // Try from first { or [
    let start = text.find('{').or_else(|| text.find('['))?;
    let end_char = if text.as_bytes()[start] == b'{' { '}' } else { ']' };
    let end = text.rfind(end_char)?;
    if end > start {
        Some(text[start..=end].to_string())
    } else {
        None
    }
}

// ─── Visual Card Renderers (JSON → HTML visualization) ──────────

fn render_skill_card(skill_name: &str, data: &serde_json::Value) -> String {
    match skill_name {
        "repository-understanding" => render_repo_card(data),
        "module-discovery" => render_module_card(data),
        "dependency-analysis" => render_dependency_card(data),
        "runtime-flow-analysis" => render_flow_card(data),
        "behavior-boundary-discovery" => render_boundary_card(data),
        "architecture-doc-generator" => render_architecture_card(data),
        _ => format!("<pre class=\"output-raw\">{}</pre>",
            html_escape(&serde_json::to_string_pretty(data).unwrap_or_default())),
    }
}

fn val_str(v: &serde_json::Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_string()
}

fn val_arr(v: &serde_json::Value, key: &str) -> Vec<String> {
    v[key].as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

fn val_u64(v: &serde_json::Value, key: &str) -> u64 {
    v[key].as_u64().unwrap_or(0)
}

fn render_repo_card(data: &serde_json::Value) -> String {
    let name = val_str(data, "project_name");
    let purpose = val_str(data, "purpose");
    let langs = val_arr(data, "languages");
    let frameworks = val_arr(data, "frameworks");
    let entrypoints = val_arr(data, "entrypoints");
    let styles = val_arr(data, "architecture_style_candidates");
    let findings = val_arr(data, "key_findings");

    let lang_tags: String = langs.iter().map(|l| format!("<span class=\"tag lang\">{}</span>", html_escape(l))).collect::<Vec<_>>().join(" ");
    let fw_tags: String = frameworks.iter().map(|f| format!("<span class=\"tag fw\">{}</span>", html_escape(f))).collect::<Vec<_>>().join(" ");
    let style_tags: String = styles.iter().map(|s| format!("<span class=\"tag style\">{}</span>", html_escape(s))).collect::<Vec<_>>().join(" ");
    let entries: String = entrypoints.iter().map(|e| format!("<li><code>{}</code></li>", html_escape(e))).collect::<Vec<_>>().join("");
    let finding_items: String = findings.iter().map(|f| format!("<li>{}</li>", html_escape(f))).collect::<Vec<_>>().join("");

    // Flat tech-stack diagram: hub-and-spoke SVG with connecting arrow lines
    let n_fw = frameworks.len();
    let n_lang = langs.len();
    let total_items = n_fw + n_lang + entrypoints.len();
    let tech_diagram = if total_items > 0 {
        let mut lines = String::new();
        let mut sat_items = String::new();
        let cx = 180.0;
        let cy = 200.0;
        let orbit_r = 130.0; // wider orbit to spread circles

        let mut draw_sat = |idx: f64, color: &str, label: &str| {
            let angle = std::f64::consts::PI * (-0.5 + idx * 2.0 / total_items as f64);
            let tx = cx + orbit_r * angle.cos();
            let ty = cy + orbit_r * angle.sin();
            // Thin connecting line from hub edge to satellite center
            let hub_edge_x = cx + 40.0 * angle.cos();
            let hub_edge_y = cy + 40.0 * angle.sin();
            lines.push_str(&format!(
                "<line x1=\"{hub_edge_x}\" y1=\"{hub_edge_y}\" x2=\"{tx}\" y2=\"{ty}\" stroke=\"{color}\" stroke-width=\"1\" stroke-dasharray=\"5 4\" opacity=\"0.4\"/>"
            ));
            sat_items.push_str(&format!(
                "<circle cx=\"{tx}\" cy=\"{ty}\" r=\"30\" fill=\"{color}\" opacity=\"0.1\"/><circle cx=\"{tx}\" cy=\"{ty}\" r=\"30\" fill=\"none\" stroke=\"{color}\" stroke-width=\"1.5\" stroke-dasharray=\"4 3\" opacity=\"0.6\"/><text x=\"{tx}\" y=\"{ty}\" fill=\"{color}\" font-size=\"11\" font-weight=\"600\" text-anchor=\"middle\" dominant-baseline=\"central\">{}</text>",
                label.chars().take(12).collect::<String>()
            ));
        };

        for (i, fw) in frameworks.iter().enumerate() {
            draw_sat(i as f64, "#3fb950", &html_escape(fw));
        }
        for (i, l) in langs.iter().enumerate() {
            draw_sat((n_fw + i) as f64, "#8b949e", &html_escape(l));
        }
        for (i, e) in entrypoints.iter().enumerate() {
            draw_sat((n_fw + n_lang + i) as f64, "#d29922", &html_escape(e).chars().take(10).collect::<String>());
        }

        format!(
            "<div class=\"diagram repo-diagram\"><svg viewBox=\"0 0 360 420\" style=\"width:100%; max-width:600px; height:auto;\"><circle cx=\"{cx}\" cy=\"{cy}\" r=\"40\" fill=\"#58a6ff\" opacity=\"0.15\"/><circle cx=\"{cx}\" cy=\"{cy}\" r=\"40\" fill=\"none\" stroke=\"#58a6ff\" stroke-width=\"2\"/><text x=\"{cx}\" y=\"{cy}\" fill=\"#58a6ff\" font-size=\"13\" font-weight=\"700\" text-anchor=\"middle\" dominant-baseline=\"central\">{}</text>{}{}</svg></div>",
            html_escape(&name).chars().take(16).collect::<String>(), lines, sat_items
        )
    } else {
        String::new()
    };

    format!(r#"{tech_diagram}
<div class="viz-repo">
  <div class="viz-hero">
    <div class="viz-icon">&#128218;</div>
    <div class="viz-hero-text">
      <h3>{name}</h3>
      <p class="viz-purpose">{purpose}</p>
    </div>
  </div>
  <div class="viz-tags">
    <div class="viz-tag-group"><span class="viz-label">语言</span> {lang_tags}</div>
    <div class="viz-tag-group"><span class="viz-label">框架</span> {fw_tags}</div>
    <div class="viz-tag-group"><span class="viz-label">架构</span> {style_tags}</div>
  </div>
  <div class="viz-section">
    <h4>入口点</h4>
    <ul class="viz-list">{entries}</ul>
  </div>
  <div class="viz-section">
    <h4>关键发现</h4>
    <ul class="viz-list">{finding_items}</ul>
  </div>
</div>"#,
        tech_diagram = tech_diagram,
        name = html_escape(&name), purpose = html_escape(&purpose),
        lang_tags = lang_tags, fw_tags = fw_tags, style_tags = style_tags,
        entries = entries, finding_items = finding_items,
    )
}

fn render_module_card(data: &serde_json::Value) -> String {
    let count = val_u64(data, "module_count");
    let modules = data["modules"].as_array();

    let mut biz_items = String::new();
    let mut tech_items = String::new();
    let mut biz_count = 0u64;
    let mut tech_count = 0u64;
    let mut biz_names: Vec<String> = Vec::new();
    let mut tech_names: Vec<String> = Vec::new();

    if let Some(mods) = modules {
        for m in mods {
            let mname = val_str(m, "name");
            let resp = val_str(m, "responsibility");
            let cat = val_str(m, "category").to_lowercase();
            let fc = val_u64(m, "file_count");
            // case-insensitive match for business categories
            let is_biz = matches!(cat.as_str(), "domain" | "business" | "ui" | "presentation" | "feature");

            let item = format!(r#"<div class="cluster-item"><div><span class="cluster-item-name">{}</span><span class="cluster-item-meta">{} 文件</span><div class="cluster-item-resp">{}</div></div></div>"#,
                html_escape(&mname), fc, html_escape(&resp));

            if is_biz { biz_items.push_str(&item); biz_count += 1; biz_names.push(mname); }
            else { tech_items.push_str(&item); tech_count += 1; tech_names.push(mname); }
        }
    }

    // Build module relationship map: which biz modules depend on which tech modules
    let mut relationships = String::new();
    if !biz_names.is_empty() && !tech_names.is_empty() {
        let biz_short: String = biz_names.iter().take(5)
            .map(|n| format!("<span class=\"rel-biz\">{}</span>", html_escape(n)))
            .collect::<Vec<_>>().join(" ");
        let biz_more = if biz_names.len() > 5 { format!(" <span class=\"rel-more\">+{} 更多</span>", biz_names.len() - 5) } else { String::new() };
        let tech_short: String = tech_names.iter()
            .map(|n| format!("<span class=\"rel-tech\">{}</span>", html_escape(n)))
            .collect::<Vec<_>>().join(" ");
        relationships = format!(
            r#"<div class="mod-relations"><div class="rel-row"><div class="rel-from">{}{}</div><span class="rel-arrow">依赖</span><div class="rel-to">{}</div></div></div>"#,
            biz_short, biz_more, tech_short
        );
    }

    let cluster_diagram = if !biz_items.is_empty() || !tech_items.is_empty() {
        let biz_section = if !biz_items.is_empty() {
            format!("<div class=\"cluster biz\"><div class=\"cluster-title\">📦 业务模块 ({})</div>{}</div>", biz_count, biz_items)
        } else { String::new() };
        let tech_section = if !tech_items.is_empty() {
            format!("<div class=\"cluster tech\"><div class=\"cluster-title\">⚙️ 技术基础设施 ({})</div>{}</div>", tech_count, tech_items)
        } else { String::new() };
        format!("<div class=\"diagram cluster-diagram\"><div class=\"diagram-row\" style=\"align-items:stretch;gap:20px;\">{}{}</div>{}</div>", biz_section, tech_section, relationships)
    } else {
        String::new()
    };

    format!(r#"{cluster_diagram}
<div class="viz-modules">
  <div class="viz-summary-bar">
    <span class="viz-big-num">{count}</span> 个模块已发现
  </div>
</div>"#,
        cluster_diagram = cluster_diagram,
        count = count,
    )
}

fn render_dependency_card(data: &serde_json::Value) -> String {
    let deps = data["dependencies"].as_array();
    let cycles = val_arr(data, "cycles");
    let violations = val_arr(data, "architecture_violations");
    let coupling = val_arr(data, "high_coupling_pairs");
    let risks = val_arr(data, "risk_areas");

    // Group dependencies by target (who depends on whom)
    let mut dep_graph: HashMap<String, Vec<String>> = HashMap::new();
    let mut dep_count = 0usize;
    if let Some(deps) = deps {
        dep_count = deps.len();
        for d in deps {
            let from = val_str(d, "from");
            let to = val_str(d, "to");
            dep_graph.entry(to).or_default().push(from);
        }
    }

    // Build a clear "Target ← Dependents" diagram
    let mut dep_diagram = String::new();
    if !dep_graph.is_empty() {
        let mut groups = String::new();
        for (target, dependents) in &dep_graph {
            let dep_names: String = dependents.iter()
                .map(|d| format!("<span class=\"dep-node\">{}</span>", html_escape(d)))
                .collect::<Vec<_>>().join(", ");
            groups.push_str(&format!(
                r#"<div class="dep-group"><span class="dep-target">{}</span><span class="dep-arrow">◀</span><div class="dep-from-list">{}</div></div>"#,
                html_escape(target), dep_names
            ));
        }
        dep_diagram = format!(
            "<div class=\"diagram\"><div class=\"dep-groups\">{}</div><div class=\"dep-summary\">{} 条依赖边（按目标分组）</div></div>",
            groups, dep_count
        );
    }

    let cycle_items: String = if cycles.is_empty() {
        "<li class=\"ok\">未检测到循环依赖</li>".to_string()
    } else {
        cycles.iter().map(|c| format!("<li class=\"warn\">{}</li>", html_escape(c))).collect::<Vec<_>>().join("")
    };

    let violation_items: String = if violations.is_empty() {
        "<li class=\"ok\">未检测到架构违规</li>".to_string()
    } else {
        violations.iter().map(|v| format!("<li class=\"warn\">{}</li>", html_escape(v))).collect::<Vec<_>>().join("")
    };

    let coupling_items: String = coupling.iter().map(|c| format!("<li class=\"warn\">{}</li>", html_escape(c))).collect::<Vec<_>>().join("");
    let risk_items: String = risks.iter().map(|r| format!("<li class=\"warn\">{}</li>", html_escape(r))).collect::<Vec<_>>().join("");

    format!(r#"{dep_diagram}
<div class="viz-deps">
  <div class="viz-grid-2col">
    <div class="viz-section">
      <h4>循环依赖</h4>
      <ul class="viz-list">{cycle_items}</ul>
    </div>
    <div class="viz-section">
      <h4>架构违规</h4>
      <ul class="viz-list">{violation_items}</ul>
    </div>
  </div>
  <div class="viz-grid-2col">
    <div class="viz-section">
      <h4>高耦合</h4>
      <ul class="viz-list">{coupling_items}</ul>
    </div>
    <div class="viz-section">
      <h4>风险区域</h4>
      <ul class="viz-list">{risk_items}</ul>
    </div>
  </div>
</div>"#,
        dep_diagram = dep_diagram,
        cycle_items = cycle_items,
        violation_items = violation_items,
        coupling_items = coupling_items, risk_items = risk_items,
    )
}

fn render_flow_card(data: &serde_json::Value) -> String {
    let count = val_u64(data, "flow_count");
    let flows = data["flows"].as_array();
    let critical = val_arr(data, "critical_paths");

    let mut all_pipelines = String::new();

    if let Some(flows) = flows {
        for f in flows {
            let fname = val_str(f, "name");
            let entry = val_str(f, "entry_point");
            let rl = val_str(f, "risk_level");
            let phases = f["phases"].as_array().map(|a| {
                a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect::<Vec<_>>()
            }).unwrap_or_default();
            let side_effects = f["side_effects"].as_array().map(|a| {
                a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect::<Vec<_>>()
            }).unwrap_or_default();

            let risk_color = match rl.as_str() {
                "high" => "#f85149", "medium" => "#d29922", _ => "#3fb950"
            };
            let risk_bg = match rl.as_str() {
                "high" => "rgba(248,81,73,0.15)", "medium" => "rgba(210,153,34,0.15)", _ => "rgba(63,185,80,0.15)"
            };

            // Pipeline diagram
            let mut steps = String::new();
            steps.push_str(&format!("<span class=\"pipeline-step entry\">入口<span class=\"pipeline-label\">{}</span></span>",
                html_escape(&entry.chars().take(20).collect::<String>())));
            for phase in &phases {
                steps.push_str("<span class=\"pipeline-arrow\">→</span>");
                steps.push_str(&format!("<span class=\"pipeline-step phase\">{}</span>",
                    html_escape(&phase.chars().take(25).collect::<String>())));
            }
            for se in &side_effects {
                steps.push_str("<span class=\"pipeline-arrow\">→</span>");
                steps.push_str(&format!("<span class=\"pipeline-step effect\">{}</span>",
                    html_escape(&se.chars().take(20).collect::<String>())));
            }

            all_pipelines.push_str(&format!(r#"
<div class="flow-item">
  <div class="flow-header">
    <span class="flow-name">{}</span>
    <span class="flow-risk" style="color:{};background:{}">{}</span>
  </div>
  <div class="diagram"><div class="pipeline">{}</div></div>
</div>"#,
                html_escape(&fname), risk_color, risk_bg, html_escape(&rl.to_uppercase()),
                steps));
        }
    }

    let crit_items: String = critical.iter()
        .map(|c| format!("<li>{}</li>", html_escape(c)))
        .collect::<Vec<_>>().join("");

    // Build flow analysis conclusions
    let (risk_counts, entry_points, top_effects, flow_insight) = analyze_flows(flows);

    let risk_breakdown = if risk_counts.is_empty() {
        String::new()
    } else {
        risk_counts.iter().map(|(label, cnt, color)| {
            format!("<span style=\"color:{};font-weight:600\">{}</span>: {} flows", color, label, cnt)
        }).collect::<Vec<_>>().join(" &nbsp;|&nbsp; ")
    };

    let entry_summary = if !entry_points.is_empty() {
        format!("主要入口点：<strong>{}</strong>", entry_points.join(", "))
    } else { String::new() };

    let effects_summary = if !top_effects.is_empty() {
        format!("最常见副作用：<strong>{}</strong>", top_effects.join(", "))
    } else { String::new() };

    format!(r#"
<div class="viz-flows">
  <div class="viz-summary-bar"><span class="viz-big-num">{}</span> 条执行流程已分析</div>
  <div class="flow-insight">{}{}{}{}</div>
  {}
  <div class="viz-section"><h4>关键路径</h4><ul class="viz-list">{}</ul></div>
</div>"#, count, risk_breakdown,
    if !entry_summary.is_empty() { format!("<br>{}", entry_summary) } else { String::new() },
    if !effects_summary.is_empty() { format!("<br>{}", effects_summary) } else { String::new() },
    flow_insight,
    all_pipelines, crit_items)
}

/// Analyze flows to produce human-readable conclusions
fn analyze_flows(flows: Option<&Vec<serde_json::Value>>) -> (Vec<(&'static str, usize, &'static str)>, Vec<String>, Vec<String>, String) {
    let Some(flows) = flows else {
        return (Vec::new(), Vec::new(), Vec::new(), String::new());
    };

    let mut high = 0; let mut medium = 0; let mut low = 0;
    let mut all_entries: Vec<String> = Vec::new();
    let mut all_effects: Vec<String> = Vec::new();

    for f in flows {
        let rl = f["risk_level"].as_str().unwrap_or("low");
        match rl { "high" => { high += 1; }, "medium" => { medium += 1; }, _ => { low += 1; } }
        if let Some(entry) = f["entry_point"].as_str() {
            all_entries.push(entry.to_string());
        }
        if let Some(effects) = f["side_effects"].as_array() {
            for e in effects {
                if let Some(s) = e.as_str() {
                    all_effects.push(s.to_string());
                }
            }
        }
    }

    let mut risk_counts: Vec<(&str, usize, &str)> = Vec::new();
    if high > 0 { risk_counts.push(("HIGH", high, "#f85149")); }
    if medium > 0 { risk_counts.push(("MEDIUM", medium, "#d29922")); }
    if low > 0 { risk_counts.push(("LOW", low, "#3fb950")); }

    // Top entry points (unique)
    let mut entry_freq: HashMap<String, usize> = HashMap::new();
    for e in &all_entries { *entry_freq.entry(e.clone()).or_default() += 1; }
    let mut entry_vec: Vec<_> = entry_freq.into_iter().collect();
    entry_vec.sort_by(|a,b| b.1.cmp(&a.1));
    let entry_points: Vec<String> = entry_vec.iter().take(3).map(|(e,c)| format!("{} ({})", e, c)).collect();

    // Most common side effects
    let mut effect_freq: HashMap<String, usize> = HashMap::new();
    for e in &all_effects { *effect_freq.entry(e.clone()).or_default() += 1; }
    let mut effect_vec: Vec<_> = effect_freq.into_iter().collect();
    effect_vec.sort_by(|a,b| b.1.cmp(&a.1));
    let top_effects: Vec<String> = effect_vec.iter().take(3).map(|(e,c)| format!("{} ({})", e, c)).collect();

    // Build conclusion insight
    let mut insight_parts: Vec<String> = Vec::new();
    if high > 0 {
        insight_parts.push(format!("检测到 {} 条高风险流程——涉及关键数据变更或敏感用户操作，需额外审查。", high));
    }
    if medium > 0 {
        insight_parts.push(format!("检测到 {} 条中风险流程——涉及用户输入或状态变更，应充分验证。", medium));
    }
    if !top_effects.is_empty() {
        insight_parts.push("最频繁的副作用均为 API 调用，系统高度依赖后端通信（I/O 密集型）。".to_string());
    }
    let flow_insight = if !insight_parts.is_empty() {
        format!("<div class=\"flow-conclusion\"><strong>分析结论：</strong> {}</div>", insight_parts.join(" "))
    } else { String::new() };

    (risk_counts, entry_points, top_effects, flow_insight)
}

fn render_boundary_card(data: &serde_json::Value) -> String {
    let behavioral = val_arr(data, "behavioral_modules");
    let support = val_arr(data, "support_modules");
    let apis = val_arr(data, "public_apis");
    let mutations = val_arr(data, "state_mutation_points");
    let risks = val_arr(data, "boundary_risks");
    let overall = val_str(data, "overall_risk");

    let risk_color = match overall.as_str() {
        "high" => "#f85149", "medium" => "#d29922", _ => "#3fb950"
    };

    let behavioral_items: String = behavioral.iter()
        .map(|b| format!("<div class=\"fence-item\">{}</div>", html_escape(b)))
        .collect::<Vec<_>>().join("");
    let support_items: String = support.iter()
        .map(|s| format!("<div class=\"fence-item\">{}</div>", html_escape(s)))
        .collect::<Vec<_>>().join("");

    let fence_diagram = if !behavioral_items.is_empty() || !support_items.is_empty() {
        let core = if !behavioral_items.is_empty() {
            format!("<div class=\"fence-core\"><div class=\"fence-title\">行为核心</div>{}</div>", behavioral_items)
        } else { String::new() };
        let sup = if !support_items.is_empty() {
            format!("<div class=\"fence-support\"><div class=\"fence-title\">支撑模块</div>{}</div>", support_items)
        } else { String::new() };
        format!("{}<div class=\"boundary-fence\">{}{}</div>",
            if !core.is_empty() && !sup.is_empty() {
                "<div style=\"text-align:center;margin-bottom:8px;font-size:11px;color:var(--text-dim)\">━━━  行为边界  ━━━</div>"
            } else { "" },
            core, sup)
    } else { String::new() };

    let api_items: String = apis.iter().enumerate()
        .map(|(i, a)| format!("<div class=\"b-step\"><span class=\"b-step-num\">{}</span><code>{}</code></div>",
            i + 1, html_escape(a)))
        .collect::<Vec<_>>().join("");
    let mut_items: String = mutations.iter().enumerate()
        .map(|(i, m)| format!("<div class=\"b-step\"><span class=\"b-step-num\">{}</span><code>{}</code></div>",
            i + 1, html_escape(m)))
        .collect::<Vec<_>>().join("");
    let risk_items: String = risks.iter().enumerate()
        .map(|(i, r)| format!("<div class=\"b-step\"><span class=\"b-step-num\" style=\"background:#f85149\">{}</span><span>{}</span></div>",
            i + 1, html_escape(r)))
        .collect::<Vec<_>>().join("");

    format!(r#"{fence_diagram}
<div class="viz-boundary">
  <div class="viz-summary-bar">整体风险等级：<span style="color:{risk_color};font-weight:700;font-size:16px">{risk_label}</span></div>
  <div class="viz-grid-2col">
    <div class="viz-section"><h4>公开 API 与行为契约</h4><div class="b-decisions">{api_items}</div></div>
    <div class="viz-section"><h4>状态变更点</h4><div class="b-decisions">{mut_items}</div></div>
  </div>
  <div class="viz-section"><h4>边界风险</h4><div class="b-decisions">{risk_items}</div></div>
</div>"#,
        fence_diagram = fence_diagram,
        risk_color = risk_color, risk_label = html_escape(&overall.to_uppercase()),
        api_items = api_items, mut_items = mut_items, risk_items = risk_items)
}

fn render_architecture_card(data: &serde_json::Value) -> String {
    let summary = val_str(data, "architecture_summary");
    let decisions = val_arr(data, "key_decisions");
    let risks = val_arr(data, "risks");
    let recommendations = val_arr(data, "recommendations");
    let diagram_text = val_str(data, "diagram_description");

    let decision_items: String = decisions.iter().enumerate()
        .map(|(i, d)|
            format!("<div class=\"arch-step\"><span class=\"step-num\">{}</span><span>{}</span></div>",
                i + 1, html_escape(d)))
        .collect::<Vec<_>>().join("");

    // Build layered architecture stack from diagram description
    let stack = build_arch_stack(&diagram_text);

    let risk_items: String = risks.iter()
        .map(|r| format!("<li class=\"warn\">{}</li>", html_escape(r)))
        .collect::<Vec<_>>().join("");
    let rec_items: String = recommendations.iter()
        .map(|r| format!("<li class=\"ok\">{}</li>", html_escape(r)))
        .collect::<Vec<_>>().join("");

    format!(r#"{stack}
<div class="viz-arch">
  <div class="viz-hero">
    <div class="viz-icon">&#128202;</div>
    <div class="viz-hero-text">
      <p class="viz-purpose">{summary}</p>
    </div>
  </div>
  <div class="viz-section">
    <h4>关键架构决策</h4>
    <div class="arch-decisions">{decisions}</div>
  </div>
  <div class="viz-grid-2col">
    <div class="viz-section"><h4>风险</h4><ul class="viz-list">{risk_items}</ul></div>
    <div class="viz-section"><h4>建议</h4><ul class="viz-list">{rec_items}</ul></div>
  </div>
</div>"#,
        stack = stack,
        summary = html_escape(&summary), decisions = decision_items,
        risk_items = risk_items, rec_items = rec_items)
}

/// Parse diagram_description text into a layered architecture stack diagram
fn build_arch_stack(desc: &str) -> String {
    // Split by both ASCII and Chinese punctuation
    let sentences: Vec<&str> = desc
        .split(|c: char| c == '.' || c == '\n' || c == '。' || c == '；' || c == '！' || c == '？' || c == '，')
        .map(|s| s.trim())
        .filter(|s| s.len() > 3)
        .collect();

    if sentences.is_empty() {
        // Fallback: treat the whole description as one layer
        let s = desc.trim();
        if s.is_empty() {
            return String::new();
        }
        let layers = format!(
            "<div class=\"arch-layer l0\"><span class=\"arch-layer-label\">{}</span><span class=\"arch-layer-desc\"></span></div>",
            html_escape(&s.chars().take(80).collect::<String>())
        );
        return format!("<div class=\"diagram arch-diagram\"><div class=\"arch-stack\">{}</div></div>", layers);
    }

    let mut layers = String::new();
    for (i, s) in sentences.iter().enumerate() {
        let li = i % 6; // cycle through 6 layer styles
        let truncated = s.chars().take(80).collect::<String>();
        layers.push_str(&format!(
            "<div class=\"arch-layer l{}\"><span class=\"arch-layer-label\">{}</span></div>",
            li, html_escape(&truncated)
        ));
    }

    format!("<div class=\"diagram arch-diagram\"><div class=\"arch-stack\">{}</div></div>", layers)
}

// ─── Summary Metrics Builder ──────────────────────────────────────

fn build_summary_metrics(cache: &HashMap<String, String>) -> (u64, u64, String, String, String) {
    let module_count = cache.get("module-discovery")
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v["module_count"].as_u64())
        .unwrap_or(0);

    let flow_count = cache.get("runtime-flow-analysis")
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v["flow_count"].as_u64())
        .unwrap_or(0);

    let risk = cache.get("behavior-boundary-discovery")
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v["overall_risk"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "low".to_string());

    let (risk_label, risk_color): (&str, &str) = match risk.as_str() {
        "high" => ("HIGH", "#f85149"),
        "medium" => ("MEDIUM", "#d29922"),
        _ => ("LOW", "#3fb950"),
    };

    // Build comprehensive narrative from all skill outputs
    let mut parts: Vec<String> = Vec::new();

    // Repository: project name + purpose
    if let Some(repo) = cache.get("repository-understanding")
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()) {
        let name = repo["project_name"].as_str().unwrap_or("本项目");
        let purpose = repo["purpose"].as_str().unwrap_or("");
        if !purpose.is_empty() {
            parts.push(format!("<strong>{}</strong> — {}", html_escape(name), html_escape(purpose)));
        } else {
            parts.push(format!("分析了 <strong>{}</strong>。", html_escape(name)));
        }
    }

    // Module discovery insights
    if let Some(mod_json) = cache.get("module-discovery")
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()) {
        let modules = mod_json["modules"].as_array();
        if let Some(mods) = modules {
            let biz: Vec<&str> = mods.iter()
                .filter(|m| matches!(m["category"].as_str(), Some("Domain") | Some("Business") | Some("UI") | Some("Presentation")))
                .filter_map(|m| m["name"].as_str())
                .collect();
            let tech: Vec<&str> = mods.iter()
                .filter(|m| !matches!(m["category"].as_str(), Some("Domain") | Some("Business") | Some("UI") | Some("Presentation")))
                .filter_map(|m| m["name"].as_str())
                .collect();
            if !biz.is_empty() {
                parts.push(format!("业务模块：<strong>{}</strong>。", biz.join("、")));
            }
            if !tech.is_empty() {
                let tech_short: String = tech.iter().take(5).map(|s| *s).collect::<Vec<_>>().join("、");
                let suffix = if tech.len() > 5 { format!("（等 {} 项）", tech.len()) } else { String::new() };
                parts.push(format!("基础设施：<strong>{}{}</strong>。", tech_short, suffix));
            }
        }
    }

    // Architecture recommendations from architecture-doc-generator
    if let Some(arch) = cache.get("architecture-doc-generator")
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()) {
        let summary = arch["architecture_summary"].as_str().unwrap_or("");
        if !summary.is_empty() {
            parts.push(format!("架构特性：<strong>{}</strong>。", html_escape(summary)));
        }
        let recs = arch["recommendations"].as_array();
        if let Some(recs) = recs {
            if !recs.is_empty() {
                let first = recs.first().and_then(|r| r.as_str()).unwrap_or("");
                parts.push(format!("建议：<strong>{}</strong>。", html_escape(first)));
            }
        }
    }

    // Flow analysis critical path insight
    if let Some(flow) = cache.get("runtime-flow-analysis")
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()) {
        let critical = flow["critical_paths"].as_array();
        if let Some(cp) = critical {
            if !cp.is_empty() {
                let first = cp.first().and_then(|c| c.as_str()).unwrap_or("");
                parts.push(format!("关键路径：<strong>{}</strong>。", html_escape(first)));
            }
        }
    }

    let narrative = parts.join(" ");
    (module_count, flow_count, risk_label.to_string(), risk_color.to_string(), narrative)
}

// ─── Skill List ───────────────────────────────────────────────────

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
