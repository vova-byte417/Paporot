//! 报告生成器
//!
//! 将 Skill 执行管线的分析与执行结果转换为：
//! - JSON 机器报告
//! - Markdown 人类阅读报告
//! - Dashboard HTML 互动页面

use crate::skills::types::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};

use super::dashboard::render_dashboard_html;

/// 聚合后的分析结果 —— 供报告消费
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidatedReport {
    /// 项目名
    pub project_name: String,
    /// 分析时间戳
    pub analyzed_at: String,
    /// 总体摘要
    pub summary: AnalysisReportSummary,
    /// 每个 Skill 的结果
    pub skill_results: Vec<SkillReportItem>,
    /// DAG 执行层描述
    pub dag_layers: Vec<DagLayerDesc>,
    /// Mermaid 依赖图
    pub mermaid_deps: Option<String>,
    /// Mermaid 流程图
    pub mermaid_flows: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisReportSummary {
    pub total_skills: usize,
    pub ok: usize,
    pub skipped: usize,
    pub failed: usize,
    pub duration_secs: f64,
    pub risk_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillReportItem {
    pub name: String,
    pub status: String,
    pub duration_ms: u64,
    pub output_summary: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagLayerDesc {
    pub layer_index: usize,
    pub skills: Vec<String>,
    pub parallel: bool,
}

// ─── Report Generator ─────────────────────────────────────────────────

pub struct ReportGenerator {
    reports_dir: PathBuf,
}

impl ReportGenerator {
    pub fn new(reports_dir: impl AsRef<Path>) -> Self {
        let reports_dir = reports_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&reports_dir).ok();
        Self { reports_dir }
    }

    /// 从 Skill 执行结果 + Summary 构建统一报告
    pub fn build_consolidated_report(
        &self,
        summary: &AnalysisSummary,
        skill_results: &[SkillRunResult],
        dag_layers: &[Vec<String>],
        mermaid_deps: Option<&str>,
        mermaid_flows: Option<&str>,
    ) -> ConsolidatedReport {
        let items: Vec<SkillReportItem> = skill_results
            .iter()
            .map(|r| SkillReportItem {
                name: r.skill_name.clone(),
                status: r.status.to_string(),
                duration_ms: r.duration_ms,
                output_summary: r.output_json.as_ref().map(|j| {
                    // 截取前 200 字符作为摘要
                    if j.len() > 200 {
                        format!("{}...", &j[..200])
                    } else {
                        j.clone()
                    }
                }),
                error: r.error.as_ref().map(|e| e.detail.clone()),
            })
            .collect();

        let layers: Vec<DagLayerDesc> = dag_layers
            .iter()
            .enumerate()
            .map(|(i, layer)| DagLayerDesc {
                layer_index: i,
                skills: layer.clone(),
                parallel: layer.len() > 1,
            })
            .collect();

        ConsolidatedReport {
            project_name: "Paporot Analysis".to_string(),
            analyzed_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            summary: AnalysisReportSummary {
                total_skills: summary.total_skills,
                ok: summary.ok,
                skipped: summary.skipped,
                failed: summary.failed,
                duration_secs: summary.total_duration_ms as f64 / 1000.0,
                risk_level: if summary.failed > 0 { "high".into() } else if summary.skipped > 0 { "medium".into() } else { "low".into() },
            },
            skill_results: items,
            dag_layers: layers,
            mermaid_deps: mermaid_deps.map(|s| s.to_string()),
            mermaid_flows: mermaid_flows.map(|s| s.to_string()),
        }
    }

    /// 写出 JSON 报告
    pub fn write_json_report(&self, report: &ConsolidatedReport) -> Result<PathBuf> {
        let path = self.reports_dir.join("analysis_result.json");
        let json = serde_json::to_string_pretty(report)
            .context("Failed to serialize JSON report")?;
        fs::write(&path, json)?;
        Ok(path)
    }

    /// 写出 Markdown 报告
    pub fn write_markdown_report(&self, report: &ConsolidatedReport) -> Result<PathBuf> {
        let path = self.reports_dir.join("architecture.md");
        let md = self.render_markdown(report);
        fs::write(&path, &md)?;
        Ok(path)
    }

    /// 写出 Dashboard HTML
    pub fn write_dashboard_html(&self, report: &ConsolidatedReport) -> Result<PathBuf> {
        let path = self.reports_dir.join("dashboard.html");
        let html = render_dashboard_html(report);
        fs::write(&path, &html)?;
        Ok(path)
    }

    /// 写出所有三种报告
    pub fn write_all(
        &self,
        report: &ConsolidatedReport,
    ) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let json = self.write_json_report(report)?;
        let md = self.write_markdown_report(report)?;
        let html = self.write_dashboard_html(report)?;
        Ok((json, md, html))
    }

    // ─── Markdown 渲染 ─────────────────────────────────────────────

    fn render_markdown(&self, report: &ConsolidatedReport) -> String {
        let mut md = String::new();

        md.push_str(&format!("# Architecture Analysis Report\n\n"));
        md.push_str(&format!("**Project**: {}\n\n", report.project_name));
        md.push_str(&format!("**Analyzed At**: {}\n\n", report.analyzed_at));
        md.push_str("---\n\n");

        // Summary
        md.push_str("## Summary\n\n");
        md.push_str(&format!("| Metric | Value |\n"));
        md.push_str(&format!("|--------|-------|\n"));
        md.push_str(&format!("| Total Skills | {} |\n", report.summary.total_skills));
        md.push_str(&format!("| OK | {} |\n", report.summary.ok));
        md.push_str(&format!("| Skipped | {} |\n", report.summary.skipped));
        md.push_str(&format!("| Failed | {} |\n", report.summary.failed));
        md.push_str(&format!("| Duration | {:.2}s |\n", report.summary.duration_secs));
        md.push_str(&format!("| Risk Level | {} |\n\n", report.summary.risk_level));

        // DAG Execution
        md.push_str("## Execution Plan (DAG)\n\n");
        for layer in &report.dag_layers {
            md.push_str(&format!("### Layer {} {}\n", layer.layer_index + 1, if layer.parallel { "(parallel)" } else { "" }));
            for skill in &layer.skills {
                md.push_str(&format!("- {}\n", skill));
            }
            md.push('\n');
        }

        // Per-skill details
        md.push_str("## Skill Details\n\n");
        for item in &report.skill_results {
            let icon = match item.status.as_str() {
                "ok" => "✅",
                "skipped" => "⏭️",
                "failed" => "❌",
                _ => "⚠️",
            };
            md.push_str(&format!("### {} {}\n\n", icon, item.name));
            md.push_str(&format!("- **Status**: {}\n", item.status));
            md.push_str(&format!("- **Duration**: {}ms\n", item.duration_ms));
            if let Some(ref summary) = item.output_summary {
                md.push_str(&format!("- **Output**: `{}`\n", summary));
            }
            if let Some(ref err) = item.error {
                md.push_str(&format!("- **Error**: {}\n", err));
            }
            md.push('\n');
        }

        // Mermaid diagrams
        if let Some(ref deps) = report.mermaid_deps {
            md.push_str("## Dependency Graph\n\n");
            md.push_str("```mermaid\n");
            md.push_str(deps);
            md.push_str("\n```\n\n");
        }

        if let Some(ref flows) = report.mermaid_flows {
            md.push_str("## Runtime Flows\n\n");
            md.push_str("```mermaid\n");
            md.push_str(flows);
            md.push_str("\n```\n\n");
        }

        md
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_rendering() {
        let report = ConsolidatedReport {
            project_name: "test".into(),
            analyzed_at: "2026-01-01".into(),
            summary: AnalysisReportSummary {
                total_skills: 2,
                ok: 1,
                skipped: 0,
                failed: 1,
                duration_secs: 1.5,
                risk_level: "high".into(),
            },
            skill_results: vec![
                SkillReportItem {
                    name: "repo-understanding".into(),
                    status: "ok".into(),
                    duration_ms: 100,
                    output_summary: Some(r#"{"project_name":"test"}"#.into()),
                    error: None,
                },
                SkillReportItem {
                    name: "module-discovery".into(),
                    status: "failed".into(),
                    duration_ms: 50,
                    output_summary: None,
                    error: Some("missing input".into()),
                },
            ],
            dag_layers: vec![
                DagLayerDesc { layer_index: 0, skills: vec!["repo-understanding".into()], parallel: false },
                DagLayerDesc { layer_index: 1, skills: vec!["module-discovery".into()], parallel: false },
            ],
            mermaid_deps: Some("graph TD\n  A --> B".into()),
            mermaid_flows: None,
        };

        let gen = ReportGenerator::new("/tmp/paporot_test");
        let md = gen.render_markdown(&report);
        assert!(md.contains("Architecture Analysis Report"));
        assert!(md.contains("repo-understanding"));
        assert!(md.contains("graph TD"));
    }

    #[test]
    fn test_json_report_roundtrip() {
        let report = ConsolidatedReport {
            project_name: "test".into(),
            analyzed_at: "2026-01-01".into(),
            summary: AnalysisReportSummary {
                total_skills: 0, ok: 0, skipped: 0, failed: 0,
                duration_secs: 0.0, risk_level: "low".into(),
            },
            skill_results: vec![],
            dag_layers: vec![],
            mermaid_deps: None,
            mermaid_flows: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let _parsed: ConsolidatedReport = serde_json::from_str(&json).unwrap();
    }
}
