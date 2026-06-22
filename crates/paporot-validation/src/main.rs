//! paporot-benchmark — Golden Dataset Benchmark Runner
//!
//! Usage:
//!   cargo run -p paporot-validation -- [OPTIONS]
//!
//! Options:
//!   --datasets <dir>    Path to Golden Dataset YAML files (default: validation/datasets)
//!   --reports <dir>     Output directory for reports (default: validation/reports)
//!   --category <cat>    Filter by category: capability|diff|regression
//!   --id <id>           Run specific case by ID
//!   --title <title>     Report title

use anyhow::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let datasets = parse_flag(&args, "--datasets", "validation/datasets");
    let reports = parse_flag(&args, "--reports", "validation/reports");
    let category = parse_flag_opt(&args, "--category");
    let case_id = parse_flag_opt(&args, "--id");
    let title = parse_flag_opt(&args, "--title");

    let datasets_path = PathBuf::from(&datasets);
    let reports_path = PathBuf::from(&reports);

    if !datasets_path.exists() {
        anyhow::bail!("Datasets directory not found: {}. Run from project root.", datasets);
    }

    std::fs::create_dir_all(&reports_path)?;

    println!("Paporot Benchmark Runner");
    println!("  datasets : {}", datasets);
    println!("  reports  : {}", reports);
    if let Some(ref cat) = category {
        println!("  category : {}", cat);
    }
    if let Some(ref id) = case_id {
        println!("  case id  : {}", id);
    }
    println!();

    // 1. 加载所有 Case
    let mut cases = paporot_validation::dataset::load_all(&datasets_path)?;
    println!("Loaded {} cases", cases.len());

    // 2. 过滤
    if let Some(ref cat) = category {
        let cat_enum = match cat.as_str() {
            "capability" => paporot_validation::types::CaseCategory::Capability,
            "diff" => paporot_validation::types::CaseCategory::Diff,
            "regression" => paporot_validation::types::CaseCategory::Regression,
            _ => anyhow::bail!("Unknown category: {}. Use capability|diff|regression", cat),
        };
        cases = paporot_validation::dataset::filter_by_category(cases, cat_enum);
        println!("Filtered to {} cases (category: {})", cases.len(), cat);
    }

    if let Some(ref id) = case_id {
        cases = paporot_validation::dataset::filter_by_id(cases, id);
        println!("Filtered to {} cases (id: {})", cases.len(), id);
    }

    if cases.is_empty() {
        println!("No cases to run. Check your filters.");
        return Ok(());
    }

    // 3. 构建 (Case, case_path) 对
    let case_pairs = build_case_pairs(&cases, &datasets_path);

    // 4. 按 category 分组运行
    use paporot_validation::types::CaseCategory;
    let mut suite_results = Vec::new();

    for cat in [CaseCategory::Capability, CaseCategory::Diff, CaseCategory::Regression] {
        let cat_pairs: Vec<_> = case_pairs
            .iter()
            .filter(|(c, _)| c.category == cat)
            .collect();
        if cat_pairs.is_empty() {
            continue;
        }
        let cat_pairs_owned: Vec<_> = cat_pairs.iter().map(|(c, p)| ((*c).clone(), (*p).clone())).collect();
        let cases_refs: Vec<_> = cat_pairs_owned.iter().map(|(c, _)| c.clone()).collect();
        let suite_name = format!("{:?}", cat).to_lowercase();
        println!("Running {} suite ({} cases)...", suite_name, cat_pairs.len());

        match paporot_validation::runner::run_suite(&suite_name, &cases_refs, &cat_pairs_owned) {
            Ok(result) => {
                println!(
                    "  {}: {} pass, {} semantic, {} fail ({:.1}% in {}ms)",
                    suite_name,
                    result.pass,
                    result.semantic_pass,
                    result.fail,
                    result.pass_rate,
                    result.duration_ms,
                );
                suite_results.push(result);
            }
            Err(e) => {
                eprintln!("  {} suite failed: {}", suite_name, e);
            }
        }
    }

    // 5. 生成报告
    let report_title = title.unwrap_or_else(|| "Paporot Benchmark".to_string());
    let json = paporot_validation::report::json_summary(&suite_results);
    let html = paporot_validation::report::html_report(&report_title, &suite_results);

    let date_str = chrono::Local::now().format("%Y-%m-%d").to_string();
    let json_path = reports_path.join("benchmark_result.json");
    let html_path = reports_path.join(format!("benchmark_{}.html", date_str));

    std::fs::write(&json_path, &json)?;
    std::fs::write(&html_path, &html)?;

    println!();
    println!("Reports written:");
    println!("  JSON : {}", json_path.display());
    println!("  HTML : {}", html_path.display());

    // 6. 总结
    let total_pass: usize = suite_results.iter().map(|r| r.pass + r.semantic_pass).sum();
    let total_fail: usize = suite_results.iter().map(|r| r.fail).sum();
    let total: usize = total_pass + total_fail;
    let rate = if total > 0 {
        total_pass as f64 / total as f64 * 100.0
    } else {
        100.0
    };

    println!();
    println!("═══════════════════════════════════════════════");
    println!("  Benchmark Complete — {:.1}% Pass Rate", rate);
    println!("  Total: {} cases, {} Pass, {} Semantic, {} Fail",
        total,
        suite_results.iter().map(|r| r.pass).sum::<usize>(),
        suite_results.iter().map(|r| r.semantic_pass).sum::<usize>(),
        total_fail,
    );
    println!("═══════════════════════════════════════════════");

    if total_fail > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn parse_flag(args: &[String], flag: &str, default: &str) -> String {
    if let Some(idx) = args.iter().position(|a| a == flag) {
        args.get(idx + 1).cloned().unwrap_or_else(|| default.to_string())
    } else {
        default.to_string()
    }
}

fn parse_flag_opt(args: &[String], flag: &str) -> Option<String> {
    if let Some(idx) = args.iter().position(|a| a == flag) {
        args.get(idx + 1).cloned()
    } else {
        None
    }
}

fn build_case_pairs(
    cases: &[paporot_validation::types::Case],
    datasets: &std::path::Path,
) -> Vec<(paporot_validation::types::Case, String)> {
    cases
        .iter()
        .map(|case| {
            let yaml_path = find_case_yaml(datasets, &case.id);
            (case.clone(), yaml_path)
        })
        .collect()
}

fn find_case_yaml(datasets: &std::path::Path, case_id: &str) -> String {
    for entry in walkdir::WalkDir::new(datasets)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().and_then(|s| s.to_str()) == Some("yaml")
        })
    {
        if let Ok(content) = std::fs::read_to_string(entry.path()) {
            if content.contains(&format!("id: {}", case_id)) {
                return entry.path().to_string_lossy().to_string();
            }
        }
    }
    datasets.join("capability").to_string_lossy().to_string()
}
