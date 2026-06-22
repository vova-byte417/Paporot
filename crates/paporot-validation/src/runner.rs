//! Benchmark Runner 核心循环
//!
//! Case YAML → load before/after 文件 → git diff --no-index → L1+L2 分析 → 提取 Actual
//! → ExactEvaluator → (失败) SemanticJudge → Verdict → CaseResult

use crate::dataset;
use crate::evaluator;
use crate::types::*;
use anyhow::{Context, Result};
use Paporot::analysis::l1_ast::AstAnalyzer;
use Paporot::analysis::l2_rules::RuleEngine;
use Paporot::analysis::preprocessor::DiffPreprocessor;
use Paporot::types::CapabilityCategory;
use Paporot::types::CapabilityStatus;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

/// 运行单个 Case
pub fn run_case(case: &Case, case_path: &Path) -> Result<CaseResult> {
    let start = Instant::now();

    // 1. Resolve fixture paths
    let before_path = dataset::resolve_fixture_path(case_path, &case.input.before);
    let after_path = dataset::resolve_fixture_path(case_path, &case.input.after);

    // 2. Generate diff via git diff --no-index
    let output = Command::new("git")
        .args(["diff", "--no-index", "--no-color"])
        .arg(&before_path)
        .arg(&after_path)
        .output()
        .with_context(|| format!(
            "Failed to run git diff between {} and {}",
            before_path.display(),
            after_path.display()
        ))?;

    let diff_text = String::from_utf8_lossy(&output.stdout).to_string();

    // git diff --no-index exits with code 1 when files differ (that's expected)
    // Only bail on other errors
    if diff_text.is_empty() {
        anyhow::bail!("Empty diff between {} and {}", before_path.display(), after_path.display());
    }

    // 3. L1: Parse diff, L2: AST + Rules
    let changes = DiffPreprocessor::parse(&diff_text);
    let raw_changes = AstAnalyzer::analyze(&changes)
        .context("L1 AST analysis failed")?;
    let rule_engine = RuleEngine::new();
    let rule_matches = rule_engine.evaluate(&raw_changes);

    // 4. Extract actual capabilities from analysis results
    let mut actual_caps = extract_capabilities(&raw_changes, &rule_matches);

    // 5. Build Actual & input for evaluator
    // Map paporot::analysis::types::CapabilityCategory → String
    let actual_capabilities: Vec<ActualCapability> = actual_caps
        .drain(..)
        .map(|(name, status, cats)| ActualCapability {
            name,
            status,
            categories: cats.into_iter().map(|c| format!("{:?}", c)).collect(),
        })
        .collect();

    let mut added_count = 0;
    let mut removed_count = 0;
    let mut modified_count = 0;
    let mut added_names = Vec::new();
    let mut removed_names = Vec::new();

    for ac in &actual_capabilities {
        match ac.status {
            CapabilityStatus::New => {
                added_count += 1;
                added_names.push(ac.name.clone());
            }
            CapabilityStatus::Deleted => {
                removed_count += 1;
                removed_names.push(ac.name.clone());
            }
            CapabilityStatus::Modified => {
                modified_count += 1;
            }
            CapabilityStatus::Unchanged => {}
        }
    }

    let actual = Actual {
        capabilities: actual_capabilities,
        diff_summary: Some(ActualDiffSummary {
            added_count,
            removed_count,
            modified_count,
            unchanged_count: 0,
            added_names,
            removed_names,
        }),
    };

    // 6. Evaluate
    let verdict = evaluator::evaluate(&case.expected, &actual)?;

    let duration_ms = start.elapsed().as_millis() as u64;

    // 7. Build summaries
    let expected_summary = format!("{} capability expects", case.expected.capabilities.len());
    let actual_summary = format!(
        "{} caps ({} new, {} removed, {} modified)",
        actual.capabilities.len(),
        actual.diff_summary.as_ref().map(|d| d.added_count).unwrap_or(0),
        actual.diff_summary.as_ref().map(|d| d.removed_count).unwrap_or(0),
        actual.diff_summary.as_ref().map(|d| d.modified_count).unwrap_or(0),
    );

    Ok(CaseResult {
        case_id: case.id.clone(),
        name: case.name.clone(),
        category: format!("{:?}", case.category).to_lowercase(),
        verdict,
        expected_summary,
        actual_summary,
        duration_ms,
    })
}

/// 运行一组 Case 并返回 SuiteResult
pub fn run_suite(name: &str, cases: &[Case], case_paths: &[(Case, String)]) -> Result<SuiteResult> {
    let start = Instant::now();
    let total = cases.len();
    let mut results = Vec::new();
    let mut pass = 0;
    let mut semantic_pass = 0;
    let mut fail = 0;

    for (case, case_path) in case_paths {
        let case_path = Path::new(case_path);
        match run_case(case, case_path) {
            Ok(result) => {
                match &result.verdict {
                    Verdict::Pass => pass += 1,
                    Verdict::SemanticPass { .. } => semantic_pass += 1,
                    Verdict::Fail { .. } => fail += 1,
                }
                results.push(result);
            }
            Err(e) => {
                fail += 1;
                results.push(CaseResult {
                    case_id: case.id.clone(),
                    name: case.name.clone(),
                    category: format!("{:?}", case.category).to_lowercase(),
                    verdict: Verdict::Fail {
                        reason: format!("{:?}", e),
                    },
                    expected_summary: "error".into(),
                    actual_summary: "error".into(),
                    duration_ms: 0,
                });
            }
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    let pass_rate = if total > 0 {
        (pass + semantic_pass) as f64 / total as f64 * 100.0
    } else {
        100.0
    };

    Ok(SuiteResult {
        suite_name: name.to_string(),
        total,
        pass,
        semantic_pass,
        fail,
        pass_rate,
        cases: results,
        duration_ms,
    })
}

/// 从 L1+L2 分析结果提取 capability 三元组
fn extract_capabilities(
    raw_changes: &[Paporot::analysis::types::RawChange],
    rule_matches: &[Paporot::analysis::types::RuleMatch],
) -> Vec<(String, CapabilityStatus, Vec<CapabilityCategory>)> {
    use Paporot::analysis::types::{ChangeType, RuleCategory};

    let mut caps = Vec::new();

    for change in raw_changes {
        let status = match change.change_type {
            ChangeType::FunctionAdded
            | ChangeType::StructAdded
            | ChangeType::StructFieldAdded
            | ChangeType::EnumAdded
            | ChangeType::EnumVariantAdded
            | ChangeType::TraitAdded
            | ChangeType::TraitMethodAdded
            | ChangeType::HttpRouteAdded
            | ChangeType::ImportAdded => CapabilityStatus::New,
            ChangeType::FunctionRemoved
            | ChangeType::StructFieldRemoved
            | ChangeType::EnumVariantRemoved
            | ChangeType::HttpRouteRemoved
            | ChangeType::ImportRemoved => CapabilityStatus::Deleted,
            ChangeType::FunctionSignatureChanged
            | ChangeType::StructFieldChanged
            | ChangeType::TraitMethodChanged
            | ChangeType::HttpRouteChanged => CapabilityStatus::Modified,
            _ => CapabilityStatus::Unchanged,
        };

        // Categories from rule matches (RuleMatch uses raw_change_id to link)
        let categories: Vec<CapabilityCategory> = rule_matches
            .iter()
            .filter(|m| m.raw_change_id == change.id)
            .filter_map(|m| match m.category {
                RuleCategory::Security => Some(CapabilityCategory::Security),
                RuleCategory::Breaking => Some(CapabilityCategory::Functional),
                RuleCategory::Performance => Some(CapabilityCategory::Performance),
                RuleCategory::Deprecation => Some(CapabilityCategory::Operational),
                RuleCategory::Domain => Some(CapabilityCategory::Functional),
            })
            .collect::<Vec<_>>();
        // Dedup
        let mut seen = std::collections::HashSet::new();
        let categories: Vec<_> = categories
            .into_iter()
            .filter(|c| seen.insert(format!("{:?}", c)))
            .collect();

        caps.push((change.symbol_name.clone(), status, categories));
    }

    caps
}
