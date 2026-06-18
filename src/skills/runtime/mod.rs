//! Skill Runtime
//!
//! 统一的 Skill 执行入口。负责：
//! - 加载 Skill（Registry）
//! - 构建 DAG（DagEngine）
//! - 执行 Skill（WasmHost）
//! - Schema 兼容（SchemaCompat）
//! - 结果收集（AnalysisResult）

pub mod dag;
pub mod host_bridge;
pub mod wasm_host;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};

use super::registry::SkillRegistry;
use super::schema_compat::SchemaCompat;
use super::types::*;
use crate::config::LlmConfig;
use crate::report::ReportGenerator;

use dag::{build_dag, topological_layers};
use wasm_host::WasmHost;

/// Skill Runtime —— 面向 CLI 的统一入口
pub struct SkillRuntime {
    pub registry: SkillRegistry,
    pub schema_compat: SchemaCompat,
    pub wasm_host: WasmHost,
    pub skills_dir: PathBuf,
    pub reports_dir: PathBuf,
    pub logs_dir: PathBuf,
}

impl SkillRuntime {
    /// 创建新的 Runtime
    pub fn new(base_dir: impl AsRef<Path>, paporot_version: &str) -> Result<Self> {
        let base = base_dir.as_ref();
        let skills_dir = base.join("skills");
        let reports_dir = base.join("reports");
        let logs_dir = base.join("logs");

        Ok(Self {
            registry: SkillRegistry::new(&skills_dir, paporot_version),
            schema_compat: SchemaCompat::new(),
            wasm_host: WasmHost::new()?,
            skills_dir,
            reports_dir,
            logs_dir,
        })
    }

    /// 注入 LLM 配置（api_key + model）
    pub fn with_llm(mut self, api_key: &str, model: &str) -> Self {
        self.wasm_host = self.wasm_host.with_llm(LlmConfig {
            endpoint: "https://api.deepseek.com/v1/chat/completions".to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            temperature: 0.3,
            max_tokens: 4096,
            max_retries: 3,
            timeout_secs: 120,
        });
        self
    }

    /// 运行完整分析管线
    ///
    /// `extra_inputs`: 额外输入（如 PRD 内容）
    pub async fn run_analysis(
        &self,
        extra_inputs: &HashMap<String, Vec<u8>>,
    ) -> Result<AnalysisSummary> {
        // 1. 加载兼容的 Skill
        let skills = self.registry.load_compatible()?;

        if skills.is_empty() {
            anyhow::bail!("No compatible skills found in {:?}", self.skills_dir);
        }

        // 2. 构建 DAG
        let graph = build_dag(&skills)
            .context("Failed to build DAG")?;

        // 3. 拓扑排序
        let layers = topological_layers(&graph)
            .context("Failed to sort DAG layers")?;

        println!(
            "  [runtime] DAG layers: {} ({} skills)",
            layers.len(),
            skills.len()
        );

        // 4. 逐层执行
        let mut results: Vec<SkillRunResult> = Vec::new();
        let mut output_cache: HashMap<String, String> = HashMap::new();

        for (layer_idx, layer) in layers.iter().enumerate() {
            println!("  [runtime] Layer {}: {:?}", layer_idx + 1, layer);

            // 当前层内可并行（MVP 简化：先串行，后续可并行）
            for skill_name in layer {
                let node = match graph.nodes.get(skill_name) {
                    Some(n) => n,
                    None => continue,
                };

                // 检查上游依赖是否都成功
                let mut upstream_failed = false;
                for dep in &node.deps {
                    let dep_result = results.iter().find(|r| &r.skill_name == dep);
                    match dep_result {
                        Some(r) if r.status != SkillRunStatus::Ok => {
                            upstream_failed = true;
                            break;
                        }
                        None => {
                            upstream_failed = true;
                            break;
                        }
                        _ => {}
                    }
                }

                if upstream_failed {
                    results.push(SkillRunResult {
                        skill_name: skill_name.clone(),
                        status: SkillRunStatus::Skipped,
                        duration_ms: 0,
                        output_json: None,
                        error: Some(SkillError {
                            phase: "dag_skip".into(),
                            error_code: "upstream_skip".into(),
                            detail: "Skipped because upstream Skill failed".into(),
                            suggestion: Some("Fix upstream Skill errors first".into()),
                        }),
                    });
                    continue;
                }

                // 准备输入数据
                let mut input_data: HashMap<String, Vec<u8>> = HashMap::new();

                // 注入上游 Skill 的输出
                for dep in &node.deps {
                    if let Some(cached) = output_cache.get(dep) {
                        input_data.insert(format!("skill_output__{}", dep), cached.as_bytes().to_vec());
                    }
                }

                // 注入额外输入
                for (k, v) in extra_inputs {
                    input_data.insert(k.clone(), v.clone());
                }

                // Schema Compat 检查
                let incompat = self.schema_compat.check_all(
                    &node.manifest.skill.name,
                    &node.manifest.inputs.schema_version,
                );
                if !incompat.is_empty() {
                    for inc in &incompat {
                        eprintln!(
                            "  [compat] {}: {} (Core v{} ≠ Skill v{})",
                            inc.skill_name, inc.input_name, inc.core_provides, inc.skill_requires
                        );
                    }
                    results.push(SkillRunResult {
                        skill_name: skill_name.clone(),
                        status: SkillRunStatus::Skipped,
                        duration_ms: 0,
                        output_json: None,
                        error: Some(SkillError {
                            phase: "compat_input".into(),
                            error_code: "no_compat_path".into(),
                            detail: format!("Schema version mismatch: {:?}", incompat),
                            suggestion: Some("Update skill or Core to match schema versions".into()),
                        }),
                    });
                    continue;
                }

                // 执行 Skill
                let timeout = node.manifest.skill.timeout_secs;
                let result = self.wasm_host.execute(
                    skill_name,
                    &node.wasm_path,
                    timeout,
                    &input_data,
                );

                // 缓存成功结果
                if result.status == SkillRunStatus::Ok {
                    if let Some(ref json) = result.output_json {
                        output_cache.insert(skill_name.clone(), json.clone());
                    }
                }

                results.push(result);
            }
        }

        // 5. 生成汇总
        let total = results.len();
        let ok = results.iter().filter(|r| r.status == SkillRunStatus::Ok).count();
        let skipped = results.iter().filter(|r| r.status == SkillRunStatus::Skipped).count();
        let failed = results
            .iter()
            .filter(|r| r.status == SkillRunStatus::Failed || r.status == SkillRunStatus::TimedOut)
            .count();
        let total_ms = results.iter().map(|r| r.duration_ms).sum();

        let _risk = if failed > 0 {
            "high".to_string()
        } else if skipped > 0 {
            "medium".to_string()
        } else {
            "low".to_string()
        };

        let summary = AnalysisSummary {
            total_skills: total,
            ok,
            skipped,
            failed,
            total_duration_ms: total_ms,
            high_level_summary: format!(
                "{} skills executed: {} OK, {} skipped, {} failed ({}ms)",
                total, ok, skipped, failed, total_ms
            ),
        };

        // 5.1 错误日志写入
        let error_logger = crate::skills::ErrorLogger::new(&self.logs_dir);
        let log_summary = error_logger.log_results(&results);
        if log_summary.failed > 0 {
            eprintln!(
                "  [error_log] {} errors written to {}",
                log_summary.failed,
                self.logs_dir.display()
            );
        }

        // 5.2 降级报告
        let deg_report = error_logger.generate_degradation_report(&results);
        if let Ok(path) = error_logger.write_degradation_json(&deg_report) {
            println!("  [degradation] Report: {}", path.display());
        }

        // 6. 生成报告
        let report_gen = ReportGenerator::new(&self.reports_dir);
        let consolidated = report_gen.build_consolidated_report(
            &summary,
            &results,
            &layers.iter().map(|l| l.clone()).collect::<Vec<_>>(),
            None,
            None,
        );
        match report_gen.write_all(&consolidated) {
            Ok((json_path, md_path, html_path)) => {
                println!("  [runtime] Reports generated:");
                println!("    JSON: {}", json_path.display());
                println!("    MD:   {}", md_path.display());
                println!("    HTML: {}", html_path.display());
            }
            Err(e) => {
                eprintln!("  [runtime] Failed to generate reports: {}", e);
            }
        }

        Ok(summary)
    }
}
