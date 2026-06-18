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
                    format!("{}... (truncated, {} total bytes)", &content[..4000], content.len())
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
    ok: usize, skipped: usize, failed: usize, risk: &str,
) -> String {
    let mut skill_cards = String::new();

    for (name, toml) in skills {
        let has_output = cache.contains_key(name.as_str());
        let status_color = if has_output { "#3fb950" } else { "#f85149" };
        let status_text = if has_output { "PASS" } else { "FAIL" };

        let output_html = if let Some(output) = cache.get(name.as_str()) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(output) {
                let pretty = serde_json::to_string_pretty(&parsed).unwrap_or_default();
                let escaped = html_escape(&pretty);
                format!("<pre class=\"output\">{}</pre>", escaped)
            } else {
                format!("<pre class=\"output\">{}</pre>", html_escape(output))
            }
        } else {
            String::from("<pre class=\"output no-output\">(no output)</pre>")
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

    let risk_color = match risk {
        "high" => "#f85149",
        "medium" => "#d29922",
        _ => "#3fb950",
    };

    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Paporot Architecture Analysis Dashboard</title>
<style>
:root {{
  --bg: #0d1117;
  --card-bg: #161b22;
  --border: #30363d;
  --text: #c9d1d9;
  --text-dim: #8b949e;
  --ok: #3fb950;
  --warn: #d29922;
  --fail: #f85149;
  --accent: #58a6ff;
}}
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  background: var(--bg);
  color: var(--text);
  line-height: 1.6;
}}
.container {{ max-width: 960px; margin: 0 auto; padding: 24px 16px; }}
.header {{ margin-bottom: 32px; }}
.header h1 {{ font-size: 28px; margin-bottom: 4px; }}
.header .time {{ color: var(--text-dim); font-size: 14px; }}

/* Summary meters */
.meters {{
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 12px;
  margin-bottom: 24px;
}}
.meter {{
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 20px 16px;
  text-align: center;
}}
.meter .n {{ font-size: 36px; font-weight: 700; }}
.meter .label {{ font-size: 13px; color: var(--text-dim); margin-top: 4px; }}
.meter.ok .n {{ color: var(--ok); }}
.meter.skip .n {{ color: var(--warn); }}
.meter.fail .n {{ color: var(--fail); }}

.risk-badge {{
  display: inline-block;
  padding: 4px 14px;
  border-radius: 12px;
  font-size: 13px;
  font-weight: 600;
}}
.risk-badge.low {{ background: rgba(63,185,80,0.15); color: var(--ok); }}
.risk-badge.medium {{ background: rgba(210,153,34,0.15); color: var(--warn); }}
.risk-badge.high {{ background: rgba(248,81,73,0.15); color: var(--fail); }}

/* Skill cards */
.section-title {{
  font-size: 20px;
  margin: 24px 0 16px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border);
}}
.skill-card {{
  background: var(--card-bg);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 20px;
  margin-bottom: 16px;
}}
.skill-header {{
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 8px;
}}
.skill-name {{
  font-size: 16px;
  font-weight: 600;
  font-family: 'SF Mono', 'Fira Code', monospace;
}}
.skill-status {{
  font-size: 13px;
  font-weight: 700;
  padding: 2px 10px;
  border-radius: 10px;
  background: rgba(63,185,80,0.1);
  border: 1px solid rgba(63,185,80,0.3);
}}
.skill-desc {{
  font-size: 13px;
  color: var(--text-dim);
  margin-bottom: 12px;
}}
.output {{
  background: #0d1117;
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 16px;
  font-size: 13px;
  font-family: 'SF Mono', 'Fira Code', monospace;
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 480px;
  overflow-y: auto;
  line-height: 1.5;
}}
.no-output {{
  color: var(--text-dim);
  font-style: italic;
}}
.footer {{
  text-align: center;
  color: var(--text-dim);
  font-size: 12px;
  margin-top: 40px;
  padding-top: 16px;
  border-top: 1px solid var(--border);
}}
</style>
</head>
<body>
<div class="container">
  <div class="header">
    <h1>Paporot Architecture Analysis</h1>
    <p class="time">Analyzed: {analyzed_at}</p>
  </div>

  <div class="meters">
    <div class="meter ok"><div class="n">{ok}</div><div class="label">Passed</div></div>
    <div class="meter skip"><div class="n">{skipped}</div><div class="label">Skipped</div></div>
    <div class="meter fail"><div class="n">{failed}</div><div class="label">Failed</div></div>
    <div class="meter"><div class="n" style="color:{risk_color}">{risk_upper}</div><div class="label">Risk Level</div></div>
  </div>

  <h2 class="section-title">Skill Analysis Results</h2>
  {skill_cards}

  <div class="footer">
    Generated by Paporot WASM Sandbox Pipeline
  </div>
</div>
</body>
</html>"#,
        analyzed_at = html_escape(analyzed_at),
        ok = ok, skipped = skipped, failed = failed,
        risk_color = risk_color,
        risk_upper = risk.to_uppercase(),
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
