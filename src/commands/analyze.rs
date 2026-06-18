//! `paporot analyze` —— 运行完整 Skill 分析管线
//!
//! 加载 .Paporot/skills/ 下的所有兼容 Skill，
//! 通过 DAG 引擎编排执行，生成 JSON/Markdown/HTML 三种报告。

use crate::skills::SkillRuntime;
use std::collections::HashMap;
use std::path::PathBuf;

/// 运行分析
pub async fn run(
    paporot_dir: &str,
    api_key: Option<&str>,
    prd: Option<&str>,
    extra_input: Option<&str>,
) -> anyhow::Result<()> {
    let base = PathBuf::from(paporot_dir);
    let version = env!("CARGO_PKG_VERSION");

    // 1. 初始化 Runtime
    println!("Paporot Skill Analysis Pipeline");
    println!("  version: v{}", version);
    println!();

    let mut runtime = SkillRuntime::new(&base, version)?;

    // 2. 如果提供了 API Key，注入到 WASM Host
    if let Some(key) = api_key {
        if !key.is_empty() {
            runtime = runtime.with_llm(key, "deepseek-pro");
        }
    }

    // 3. 准备额外输入
    let mut extra_inputs: HashMap<String, Vec<u8>> = HashMap::new();

    // PRD 内容
    if let Some(prd_path) = prd {
        match std::fs::read_to_string(prd_path) {
            Ok(content) => {
                extra_inputs.insert("prd_content".into(), content.into_bytes());
                println!("  [input] Loaded PRD from {}", prd_path);
            }
            Err(e) => {
                eprintln!("  [warn] Failed to load PRD: {}", e);
            }
        }
    }

    // 额外的 key=value 输入
    if let Some(input_str) = extra_input {
        for pair in input_str.split(',') {
            let kv: Vec<&str> = pair.splitn(2, '=').collect();
            if kv.len() == 2 {
                extra_inputs.insert(kv[0].trim().to_string(), kv[1].trim().as_bytes().to_vec());
            }
        }
    }

    println!("  [input] {} extra inputs", extra_inputs.len());
    println!();

    // 4. 运行分析
    println!("Running analysis pipeline...");
    let summary = runtime.run_analysis(&extra_inputs).await?;

    // 5. 打印摘要
    println!();
    println!("═══════════════════════════════════════════════");
    println!("  Analysis Complete");
    println!("═══════════════════════════════════════════════");
    println!("  Total Skills : {}", summary.total_skills);
    println!("  OK           : {}", summary.ok);
    println!("  Skipped      : {}", summary.skipped);
    println!("  Failed       : {}", summary.failed);
    println!("  Duration     : {}ms", summary.total_duration_ms);

    let risk = if summary.failed > 0 {
        "HIGH"
    } else if summary.skipped > 0 {
        "MEDIUM"
    } else {
        "LOW"
    };
    println!("  Risk Level   : {}", risk);
    println!();
    println!("  {}", summary.high_level_summary);
    println!();
    println!("  Reports:");
    println!("    JSON : {}/reports/analysis_result.json", paporot_dir);
    println!("    MD   : {}/reports/architecture.md", paporot_dir);
    println!("    HTML : {}/reports/dashboard.html", paporot_dir);

    Ok(())
}
