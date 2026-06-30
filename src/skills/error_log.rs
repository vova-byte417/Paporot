//! 错误日志与降级报告
//!
//! - 将所有 Skill 执行错误写入 `.Paporot/logs/` 目录
//! - 每条错误单独 JSON 文件，带时间戳
//! - 生成人类可读的错误摘要（Markdown）
//! - 降级：上游失败 → 下游跳过（已在 DAG 层实现）

use crate::skills::types::{SkillRunResult, SkillRunStatus};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ─── Error Log Entry ─────────────────────────────────────────────────

/// 单条错误日志
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorLogEntry {
    pub timestamp: String,
    pub skill_name: String,
    pub phase: String,
    pub error_code: String,
    pub detail: String,
    pub suggestion: Option<String>,
    pub duration_ms: u64,
}

/// 降级报告 —— 汇总哪些 Skill 被跳过及原因
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegradationReport {
    pub generated_at: String,
    pub total_skills: usize,
    pub ok_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub entries: Vec<DegradationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegradationEntry {
    pub skill_name: String,
    pub status: String,
    pub reason: String,
    pub depends_on: Vec<String>,
}

/// 日志记录的状态
#[derive(Debug, Clone)]
pub struct LogSummary {
    pub ok: usize,
    pub skipped: usize,
    pub failed: usize,
    pub log_files: Vec<PathBuf>,
}

// ─── Error Logger ──────────────────────────────────────────────────

pub struct ErrorLogger {
    logs_dir: PathBuf,
}

impl ErrorLogger {
    pub fn new(logs_dir: impl Into<PathBuf>) -> Self {
        let logs_dir = logs_dir.into();
        fs::create_dir_all(&logs_dir).ok();
        Self { logs_dir }
    }

    /// 将 Skill 执行结果写入错误日志
    ///
    /// 只记录 Failed / TimedOut 的 Skill
    pub fn log_results(&self, results: &[SkillRunResult]) -> LogSummary {
        let mut ok = 0;
        let mut skipped = 0;
        let mut failed = 0;
        let mut log_files = Vec::new();

        for result in results {
            match result.status {
                SkillRunStatus::Ok => ok += 1,
                SkillRunStatus::Skipped => skipped += 1,
                SkillRunStatus::Failed | SkillRunStatus::TimedOut => {
                    failed += 1;
                    if let Some(ref error) = result.error {
                        let entry = ErrorLogEntry {
                            timestamp: chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string(),
                            skill_name: result.skill_name.clone(),
                            phase: error.phase.clone(),
                            error_code: error.error_code.clone(),
                            detail: error.detail.clone(),
                            suggestion: error.suggestion.clone(),
                            duration_ms: result.duration_ms,
                        };

                        let filename = format!(
                            "error_{}_{}.json",
                            sanitize_filename(&result.skill_name),
                            chrono::Local::now().format("%Y%m%d_%H%M%S")
                        );
                        let path = self.logs_dir.join(&filename);

                        if let Ok(json) = serde_json::to_string_pretty(&entry) {
                            if fs::write(&path, &json).is_ok() {
                                log_files.push(path);
                            }
                        }
                    }
                }
            }
        }

        LogSummary {
            ok,
            skipped,
            failed,
            log_files,
        }
    }

    /// 生成降级报告（Markdown）
    pub fn generate_degradation_report(
        &self,
        results: &[SkillRunResult],
    ) -> DegradationReport {
        let total = results.len();
        let ok_count = results.iter().filter(|r| r.status == SkillRunStatus::Ok).count();
        let skipped_count = results.iter().filter(|r| r.status == SkillRunStatus::Skipped).count();
        let failed_count = results.iter()
            .filter(|r| r.status == SkillRunStatus::Failed || r.status == SkillRunStatus::TimedOut)
            .count();

        let mut entries = Vec::new();
        for result in results {
            let (reason, status_str) = match result.status {
                SkillRunStatus::Ok => continue,
                SkillRunStatus::Skipped => {
                    ("Upstream dependency failed".to_string(), "skipped".to_string())
                }
                SkillRunStatus::Failed | SkillRunStatus::TimedOut => {
                    let detail = result.error.as_ref()
                        .map(|e| e.detail.clone())
                        .unwrap_or_else(|| "Unknown error".to_string());
                    (detail, "failed".to_string())
                }
            };
            entries.push(DegradationEntry {
                skill_name: result.skill_name.clone(),
                status: status_str,
                reason,
                depends_on: vec![], // populated by caller if needed
            });
        }

        DegradationReport {
            generated_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            total_skills: total,
            ok_count,
            skipped_count,
            failed_count,
            entries,
        }
    }

    /// 写出降级报告（JSON）
    pub fn write_degradation_json(&self, report: &DegradationReport) -> std::io::Result<PathBuf> {
        let path = self.logs_dir.join("degradation_report.json");
        let json = serde_json::to_string_pretty(report)?;
        fs::write(&path, &json)?;
        Ok(path)
    }

    /// 写出降级报告（Markdown，人类可读）
    pub fn write_degradation_markdown(&self, report: &DegradationReport) -> std::io::Result<PathBuf> {
        let path = self.logs_dir.join("degradation_report.md");
        let mut md = String::new();

        md.push_str("# Degradation Report\n\n");
        md.push_str(&format!("**Generated**: {}\n\n", report.generated_at));
        md.push_str("## Summary\n\n");
        md.push_str(&format!("| Metric | Count |\n"));
        md.push_str(&format!("|--------|-------|\n"));
        md.push_str(&format!("| Total  | {} |\n", report.total_skills));
        md.push_str(&format!("| OK     | {} |\n", report.ok_count));
        md.push_str(&format!("| Skipped| {} |\n", report.skipped_count));
        md.push_str(&format!("| Failed | {} |\n\n", report.failed_count));

        if !report.entries.is_empty() {
            md.push_str("## Details\n\n");
            for entry in &report.entries {
                let icon = if entry.status == "failed" { "❌" } else { "⏭️" };
                md.push_str(&format!("### {} {}\n\n", icon, entry.skill_name));
                md.push_str(&format!("- **Status**: {}\n", entry.status));
                md.push_str(&format!("- **Reason**: {}\n", entry.reason));
                if !entry.depends_on.is_empty() {
                    md.push_str(&format!("- **Depends On**: {}\n", entry.depends_on.join(", ")));
                }
                md.push('\n');
            }
        }

        fs::write(&path, &md)?;
        Ok(path)
    }
}

fn sanitize_filename(name: &str) -> String {
    name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::types::SkillError;

    fn make_result(name: &str, status: SkillRunStatus, error: Option<SkillError>) -> SkillRunResult {
        SkillRunResult {
            skill_name: name.into(),
            status,
            duration_ms: 100,
            output_json: None,
            error,
        }
    }

    #[test]
    fn test_log_error_results() {
        let tmp = std::env::temp_dir().join("paporot_test_logs");
        let _ = fs::remove_dir_all(&tmp);
        let logger = ErrorLogger::new(&tmp);

        let results = vec![
            make_result("ok-skill", SkillRunStatus::Ok, None),
            make_result("skip-skill", SkillRunStatus::Skipped, None),
            make_result("fail-skill", SkillRunStatus::Failed, Some(SkillError {
                phase: "exec".into(),
                error_code: "WASM_TRAP".into(),
                detail: "out of bounds".into(),
                suggestion: Some("check input size".into()),
            })),
        ];

        let summary = logger.log_results(&results);
        assert_eq!(summary.ok, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.log_files.len(), 1);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_degradation_report() {
        let tmp = std::env::temp_dir().join("paporot_test_degradation");
        let _ = fs::remove_dir_all(&tmp);
        let logger = ErrorLogger::new(&tmp);

        let results = vec![
            make_result("repo", SkillRunStatus::Ok, None),
            make_result("module", SkillRunStatus::Skipped, None),
            make_result("dep", SkillRunStatus::Failed, Some(SkillError {
                phase: "compat_input".into(),
                error_code: "no_compat_path".into(),
                detail: "schema version mismatch".into(),
                suggestion: None,
            })),
        ];

        let report = logger.generate_degradation_report(&results);
        assert_eq!(report.total_skills, 3);
        assert_eq!(report.ok_count, 1);
        assert_eq!(report.skipped_count, 1);
        assert_eq!(report.failed_count, 1);
        assert_eq!(report.entries.len(), 2);

        let json_path = logger.write_degradation_json(&report).unwrap();
        assert!(json_path.exists());

        let md_path = logger.write_degradation_markdown(&report).unwrap();
        assert!(md_path.exists());
        let md = fs::read_to_string(&md_path).unwrap();
        assert!(md.contains("Degradation Report"));
        assert!(md.contains("schema version mismatch"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
