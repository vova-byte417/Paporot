//! Paporot — WASM Sandbox Loader (Native Entry Point)
//!
//! 极薄的 wasmtime loader。加载 paporot-core.wasm 并通过 3 个 host function
//! 向沙盒内的分析管线提供 read_file / write_file / llm_call 能力。
//! CLI 参数通过 WASI args 透传到 .wasm 的 main()。

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use wasmtime::*;
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtxBuilder};
use wasmtime_wasi::preview1::WasiP1Ctx;

use serde_json::json;

extern crate Paporot;

mod config;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // ── Native subcommands (no wasmtime needed) ──────────────────
    if args.len() >= 2 {
        match args[1].as_str() {
            "init" => return cmd_init(),
            "version" | "--version" | "-V" => {
                println!("paporot {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "eval" => return cmd_eval(&args[2..]),
            "task" => return cmd_task(&args[2..]),
            // "dashboard" removed — use `paporot analyze --full` instead
            "status" => return cmd_status(),
            "skill" => return cmd_skill(&args[2..]),
            "analyze" => return cmd_analyze(&args[2..]),
            _ => {} // fall through to WASM
        }
    }

    let paporot_dir = find_paporot_dir()?;

    // Resolve paporot-core.wasm in order:
    //   1. .Paporot/bin/ (project-local install)
    //   2. Next to the native binary (system install)
    //   3. crates/paporot-core/target/... (dev build)
    let wasm_path = {
        let a = paporot_dir.join("bin").join("paporot-core.wasm");
        let b = std::env::current_exe().ok()
            .and_then(|e| e.parent().map(|p| p.join("paporot-core.wasm")))
            .unwrap_or_default();
        let c = PathBuf::from("crates/paporot-core/target/wasm32-wasip1/release/paporot-core.wasm");

        if a.exists() { a }
        else if b.exists() { b }
        else if c.exists() { c }
        else {
            anyhow::bail!(
                "paporot-core.wasm not found.\nBuild: cargo build -p paporot-core --target wasm32-wasip1 --release"
            );
        }
    };

    // wasmtime
    let mut wasm_cfg = Config::default();
    wasm_cfg.wasm_memory64(false);
    wasm_cfg.wasm_multi_memory(false);
    let engine = Engine::new(&wasm_cfg)?;
    let module = Module::from_file(&engine, &wasm_path)?;

    // LLM config
    let llm_config = load_llm_config(&paporot_dir);

    // WASI context with args and pre-opened dir
    let mut wasi_builder = WasiCtxBuilder::new();
    for arg in &args {
        wasi_builder.arg(arg);
    }
    wasi_builder
        .preopened_dir(&paporot_dir, ".", DirPerms::all(), FilePerms::all())?
        .inherit_stdio();

    let wasi_ctx = wasi_builder.build_p1();
    let host = SandboxHost::new(wasi_ctx, llm_config, paporot_dir.clone());
    let mut store = Store::new(&engine, host);

    // ── Collect project source files into .Paporot/work/ ────────
    let project_root = paporot_dir.parent().unwrap_or(Path::new("."));
    collect_sources(project_root, &paporot_dir)?;

    let mut linker = Linker::new(&engine);
    wasmtime_wasi::preview1::add_to_linker_sync(&mut linker, |h: &mut SandboxHost| &mut h.wasi)?;
    register_host_functions(&mut linker)?;

    let instance = linker.instantiate(&mut store, &module)?;

    // Call _start (WASI entry) — proc_exit is a trap, handle it gracefully
    let start = instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .context("_start not found in paporot-core.wasm")?;
    let result = start.call(&mut store, ());

    match result {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            // WASI proc_exit is a trap with i32 exit code
            if let Some(exit_code) = e.downcast_ref::<wasmtime_wasi::I32Exit>() {
                std::process::exit(exit_code.0);
            }
            // Otherwise it's a real error
            eprintln!("Fatal error: {:?}", e);
            std::process::exit(1);
        }
    }
}

// ─── SandboxHost ─────────────────────────────────────────────────

struct SandboxHost {
    wasi: WasiP1Ctx,
    llm_config: Option<config::LlmConfig>,
    paporot_dir: PathBuf,
    /// Whether any signed skill is available (enables host_exec_command)
    has_signed_skill: bool,
}

impl SandboxHost {
    fn new(
        wasi: WasiP1Ctx,
        llm_config: Option<config::LlmConfig>,
        paporot_dir: PathBuf,
    ) -> Self {
        let has_signed_skill = check_any_skill_signed(&paporot_dir);
        Self { wasi, llm_config, paporot_dir, has_signed_skill }
    }
}

/// Check if any installed skill has a valid signature file
fn check_any_skill_signed(paporot_dir: &Path) -> bool {
    let skills_dir = paporot_dir.join("skills");
    if !skills_dir.exists() {
        return false;
    }
    for entry in skills_dir.read_dir().into_iter().flatten().flatten() {
        let sig_path = entry.path().join("signature");
        if sig_path.exists() && sig_path.is_file() {
            // Verify the signature matches
            if let (Ok(wasm_bytes), Ok(sig)) =
                (std::fs::read(entry.path().join("skill.wasm")),
                 std::fs::read_to_string(&sig_path))
            {
                use sha2::{Sha256, Digest};
                let secret = std::env::var("PAPOROT_SIGNING_SECRET")
                    .unwrap_or_else(|_| "paporot-default-signing-secret".to_string());
                let mut hasher = Sha256::new();
                hasher.update(secret.as_bytes());
                hasher.update(&wasm_bytes);
                let expected: String = hasher.finalize()
                    .iter().map(|b| format!("{:02x}", b)).collect();
                if sig.trim() == expected {
                    return true;
                }
            }
        }
    }
    false
}

fn load_llm_config(dir: &std::path::Path) -> Option<config::LlmConfig> {
    let path = dir.join("config.toml");
    let path_str = path.to_string_lossy().to_string();
    let cfg = config::Config::load_or_default(&path_str);
    if cfg.llm.api_key.is_empty() {
        // Try PAPOROT_API_KEY env var
        if let Ok(key) = std::env::var("PAPOROT_API_KEY") {
            let mut llm = cfg.llm;
            llm.api_key = key;
            return Some(llm);
        }
        None
    } else {
        Some(cfg.llm)
    }
}

fn find_paporot_dir() -> Result<PathBuf> {
    let mut current = std::env::current_dir()?;
    loop {
        let candidate = current.join(".Paporot");
        if candidate.exists() && candidate.is_dir() {
            return Ok(candidate);
        }
        if !current.pop() { break; }
    }
    let fallback = PathBuf::from(".Paporot");
    if !fallback.exists() {
        fs::create_dir_all(&fallback)?;
    }
    Ok(fallback)
}

/// `Paporot init` — initialize .Paporot/ in current directory
fn cmd_init() -> Result<()> {
    let base = std::env::current_dir()?;
    let paporot_dir = base.join(".Paporot");

    // 1. Create directory structure
    fs::create_dir_all(paporot_dir.join("skills"))?;
    fs::create_dir_all(paporot_dir.join("reports"))?;

    // 2. Create config.toml (if not exists)
    let config_path = paporot_dir.join("config.toml");
    if !config_path.exists() {
        let sample = config::Config::sample_toml();
        fs::write(&config_path, sample)?;
        println!("  created .Paporot/config.toml");
    }

    // 3. Copy skills from install dir (next to binary)
    let skill_src = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("skills")))
        .filter(|p| p.exists());

    if let Some(src) = skill_src {
        copy_dir_contents(&src, &paporot_dir.join("skills"))?;
    } else {
        let src2 = Path::new(".Paporot/skills");
        if src2.exists() {
            copy_dir_contents(src2, &paporot_dir.join("skills"))?;
        }
    }

    println!("Paporot initialized in {}", paporot_dir.display());
    println!("  skills/  — {} skill directories",
        fs::read_dir(paporot_dir.join("skills"))?.count());
    println!("  config.toml  — configure your API key here");
    println!("\nNext steps:");
    println!("  Paporot analyze");
    println!("  Paporot skill list");

    Ok(())
}

// ─── Native eval command ─────────────────────────────────────────

fn cmd_eval(args: &[String]) -> Result<()> {
    use ::Paporot::eval::*;

    let runner = EvalRunner::new(Path::new("."))?;

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("auto");

    match subcmd {
        "auto" => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                runner.eval_auto(None).await
            })?;
        }
        "compare" => {
            let mut task = None;
            let mut from = None;
            let mut to = None;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--task" => { i += 1; task = args.get(i).cloned(); }
                    "--from" => { i += 1; from = args.get(i).cloned(); }
                    "--to" => { i += 1; to = args.get(i).cloned(); }
                    _ => {}
                }
                i += 1;
            }
            let task_id = task.ok_or_else(|| anyhow::anyhow!("--task is required"))?;
            let cmp = compare_evals(
                runner.store(),
                &task_id,
                from.as_deref(),
                to.as_deref(),
            )?;
            print_compare(&cmp);
        }
        "trend" => {
            let mut task = None;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--task" => { i += 1; task = args.get(i).cloned(); }
                    _ => {}
                }
                i += 1;
            }
            let task_id = task.ok_or_else(|| anyhow::anyhow!("--task is required"))?;
            let history = trend_history(runner.store(), &task_id)?;
            print_trend(&history);
        }
        "regression" => {
            let result = check_regression(runner.store())?;
            println!("Paporot Eval Regression Check");
            println!("  Checked: {} tasks", result.checked_tasks);
            if result.regressions.is_empty() {
                println!("  No regressions detected.");
            } else {
                println!("  Regressions: {}", result.regressions.len());
                for r in &result.regressions {
                    println!("    [{}] {} : {} → {} — {}",
                        r.severity, r.task_id, r.from_outcome, r.to_outcome, r.description);
                }
            }
        }
        _ => {
            anyhow::bail!("Unknown eval subcommand '{}'. Try: auto, compare, trend, regression", subcmd);
        }
    }

    Ok(())
}

fn print_compare(cmp: &::Paporot::eval::types::EvalCompare) {
    println!("═══ Eval Compare: {} ═══", cmp.task_id);
    println!("  {} → {}", cmp.from.eval_id, cmp.to.eval_id);
    println!("  Trend: {}", cmp.trend);
    if !cmp.metrics.is_empty() {
        println!("\n  Metrics:");
        for m in &cmp.metrics {
            let dir = match m.direction {
                ::Paporot::eval::types::MetricDirection::Up => "↑",
                ::Paporot::eval::types::MetricDirection::Down => "↓",
                ::Paporot::eval::types::MetricDirection::Flat => "→",
            };
            println!("    {}: {:.2} → {:.2} ({:+.1}%) {}",
                m.label, m.from_value, m.to_value, m.change_pct, dir);
        }
    }
}

fn print_trend(history: &::Paporot::eval::types::EvalTrendHistory) {
    println!("═══ Trend: {} ═══", history.task_id);
    println!("  Description: {}", history.task_description);
    println!("  Trials: {}", history.trials.len());
    for point in &history.trials {
        println!("    #{} {}: {} (tools: {}, tokens: {})",
            point.trial_index, point.eval_id, point.outcome.label(),
            point.total_tool_calls.unwrap_or(0),
            point.total_tokens.unwrap_or(0),
        );
    }
}

// ─── Native task command ─────────────────────────────────────────

fn cmd_task(args: &[String]) -> Result<()> {
    use ::Paporot::eval::TaskManager;

    let tm = TaskManager::new(Path::new(".Paporot"))?;
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("list");

    match subcmd {
        "new" => {
            let mut desc = String::new();
            let mut criteria = Vec::new();
            let mut modules = Vec::new();
            let mut category = "Other".to_string();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--criteria" => { i += 1; if let Some(c) = args.get(i) { criteria.push(c.clone()); } }
                    "--module" => { i += 1; if let Some(m) = args.get(i) { modules.push(m.clone()); } }
                    "--category" => { i += 1; if let Some(c) = args.get(i) { category = c.clone(); } }
                    other => {
                        if !other.starts_with('-') { desc = other.to_string(); }
                    }
                }
                i += 1;
            }
            if desc.is_empty() {
                anyhow::bail!("Usage: paporot task new \"description\" --criteria \"...\"");
            }

            let cat = match category.as_str() {
                "BugFix" => ::Paporot::eval::types::TaskCategory::BugFix,
                "Feature" => ::Paporot::eval::types::TaskCategory::Feature,
                "Refactor" => ::Paporot::eval::types::TaskCategory::Refactor,
                "Test" => ::Paporot::eval::types::TaskCategory::Test,
                "Doc" => ::Paporot::eval::types::TaskCategory::Doc,
                other => ::Paporot::eval::types::TaskCategory::Other(other.into()),
            };

            let task = tm.create(&desc, cat, modules, criteria)?;
            println!("Task created: {}", task.id);
            println!("  Description: {}", task.description);
        }
        "list" => {
            let tasks = tm.list()?;
            println!("Tasks: {}", tasks.len());
            for task in &tasks {
                println!("  [{}] {} — {}", task.id, task.category, task.description);
            }
        }
        "show" => {
            let task_id = args.get(1).ok_or_else(|| anyhow::anyhow!("Usage: paporot task show <task_id>"))?;
            match tm.load(task_id)? {
                Some(task) => {
                    println!("Task: {}", task.id);
                    println!("  Description: {}", task.description);
                    println!("  Category: {}", task.category);
                    println!("  Modules: {:?}", task.modules);
                    println!("  Success Criteria: {:?}", task.success_criteria);
                }
                None => println!("Task '{}' not found", task_id),
            }
        }
        _ => {
            anyhow::bail!("Unknown task subcommand '{}'. Try: new, list, show", subcmd);
        }
    }

    Ok(())
}

// ─── Native status command ───────────────────────────────────────

fn cmd_status() -> Result<()> {
    use ::Paporot::eval::TaskManager;
    use ::Paporot::storage::cache::CacheManager;

    println!("paporot v{}", env!("CARGO_PKG_VERSION"));
    println!("Project: {}", std::env::current_dir()?.display());

    let pp = Path::new(".Paporot");
    if !pp.exists() {
        println!("  ⚠ .Paporot/ not initialized. Run 'paporot init' first.");
        return Ok(());
    }

    let tm = TaskManager::new(pp)?;
    match tm.list() {
        Ok(tasks) => println!("  Tasks: {}", tasks.len()),
        Err(_) => println!("  Tasks: 0"),
    }

    let cache = CacheManager::new(pp);
    match cache.read_code_change() {
        Ok(Some(_)) => println!("  Cache: code_change.json present"),
        Ok(None) => println!("  Cache: empty"),
        Err(_) => println!("  Cache: unavailable"),
    }

    Ok(())
}

// ─── Analyze command (full pipeline) ──────────────────────────────

fn cmd_analyze(args: &[String]) -> Result<()> {
    use ::Paporot::eval::*;

    let mut commit = None;
    let mut no_llm = false;
    let mut full = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--commit" => { i += 1; commit = args.get(i).cloned(); }
            "--no-llm" => { no_llm = true; }
            "--full" => { full = true; }
            "--change" => { full = false; }
            _ => {}
        }
        i += 1;
    }

    let mode_label = if full { "Full (变更叙事 + 能力全景)" } else { "Change (行为变更叙事)" };
    let total_steps = if full { 7 } else { 6 };

    let paporot_dir = find_paporot_dir()?;
    let project_root = std::env::current_dir()?;

    println!("╔══════════════════════════════════════════════╗");
    println!("║   Paporot v{} — Agent Behavior Audit     ║", env!("CARGO_PKG_VERSION"));
    println!("║   Mode: {:<34}║", mode_label);
    println!("╚══════════════════════════════════════════════╝");
    println!();

    // ── Step 1: Load config ──────────────────────────────────
    let cfg = config::Config::load_or_default(
        &paporot_dir.join("config.toml").to_string_lossy()
    );
    let llm_available = !cfg.llm.api_key.is_empty();
    if no_llm {
        println!("[1/{}] Config loaded (LLM disabled via --no-llm)", total_steps);
    } else if llm_available {
        println!("[1/{}] Config loaded (LLM: {} {})", total_steps, cfg.llm.model, cfg.llm.endpoint);
    } else {
        println!("[1/{}] Config loaded (no API key)", total_steps);
    }

    // ── Step 2: Run eval ────────────────────────────────────
    println!("[2/{}] Running mechanical evaluation...", total_steps);
    let runner = EvalRunner::new(&project_root)?;
    let rt = tokio::runtime::Runtime::new()?;
    let eval_result = rt.block_on(async { runner.eval_auto(commit.as_deref()).await })?;

    // ── Step 3: Behavior change narrative ────────────────────
    println!("[3/{}] Analyzing behavior changes...", total_steps);
    print_behavior_narrative(&eval_result);

    // ── Step 4: LLM Narrative ────────────────────────────────
    let narrative: Option<NarrativeData> = if !no_llm && llm_available {
        println!("[4/{}] Generating LLM narrative...", total_steps);
        match generate_narrative(&cfg, &eval_result) {
            Ok(n) => {
                println!("  ✓ Headline: \"{}\"", n.headline);
                println!("  ✓ {} module interpretations", n.module_interpretations.len());
                let _ = fs::create_dir_all(paporot_dir.join("cache"));
                if let Ok(json) = serde_json::to_string_pretty(&n) {
                    let _ = fs::write(paporot_dir.join("cache").join("narrative.json"), &json);
                }
                Some(n)
            }
            Err(e) => { println!("  ⚠ LLM narrative failed: {}", e); None }
        }
    } else {
        println!("[4/{}] LLM narrative skipped", total_steps);
        None
    };

    // ── Step 5: Compare & Regression ─────────────────────────
    println!("[5/{}] Cross-referencing with history...", total_steps);
    let store = runner.store();
    let task_id = &eval_result.task.id;
    let previous = store.list_trials(task_id).unwrap_or_default();
    let prev_count = previous.len().saturating_sub(1);
    if prev_count > 0 {
        match compare_evals(store, task_id, Some(previous[previous.len()-2].eval_id.as_str()), Some(eval_result.eval_id.as_str())) {
            Ok(cmp) => {
                println!("  Compared with trial #{}", previous.len()-1);
                let improved = cmp.metrics.iter().filter(|m| m.change_pct>0.0).count();
                let regressed = cmp.metrics.iter().filter(|m| m.change_pct<0.0).count();
                println!("  Metrics: {} improved, {} regressed", improved, regressed);
                if let Ok(json) = serde_json::to_string_pretty(&cmp) {
                    let _ = fs::write(paporot_dir.join("reports").join("compare.json"), &json);
                }
            }
            Err(e) => println!("  Compare skipped: {}", e),
        }
    } else {
        println!("  No previous trials (first evaluation)");
    }
    match check_regression(store) {
        Ok(reg) => {
            println!("  Regression check: {} issues", reg.regressions.len());
            if let Ok(json) = serde_json::to_string_pretty(&reg) {
                let _ = fs::write(paporot_dir.join("reports").join("regression.json"), &json);
            }
        }
        Err(e) => println!("  Regression check skipped: {}", e),
    }

    // ── Step 6: Generate reports ─────────────────────────────
    println!("[6/{}] Generating reports...", total_steps);
    fs::create_dir_all(paporot_dir.join("reports"))?;

    let eval_json_str = serde_json::to_string_pretty(&eval_result)?;
    fs::write(paporot_dir.join("reports").join("eval_result.json"), &eval_json_str)?;
    println!("  ✓ reports/eval_result.json");

    let summary = generate_summary_md_v2(&eval_result, narrative.as_ref(), prev_count);
    fs::write(paporot_dir.join("reports").join("summary.md"), &summary)?;
    println!("  ✓ reports/summary.md");

    // ── Optional Step 7: Git history scan for panorama ───────
    let panorama_json = if full {
        println!("[7/{}] Scanning git history for capability panorama...", total_steps);
        match scan_git_history(&project_root) {
            Ok(hist) => {
                println!("  ✓ Found {} historical modules", hist.len());
                let hist_str = serde_json::to_string(&hist).unwrap_or_default();
                let _ = fs::write(paporot_dir.join("cache").join("panorama.json"), &hist_str);
                Some(hist_str)
            }
            Err(e) => { println!("  ⚠ git history scan failed: {}", e); None }
        }
    } else { None };

    // ── Generate dashboard.html ──────────────────────────────
    let dependency_graph = extract_dependencies(&project_root, &eval_result.code_change);
    let dashboard_html = generate_dashboard_v2(
        &eval_result, narrative.as_ref(), prev_count, full,
        &dependency_graph, panorama_json.as_deref(),
    );
    fs::write(paporot_dir.join("reports").join("dashboard.html"), dashboard_html.as_bytes())?;
    println!("  ✓ reports/dashboard.html");

    println!();
    println!("═══ Analysis Complete ═══");
    let cc = &eval_result.code_change;
    if let Some(ref n) = narrative {
        println!("  📰  \"{}\"", n.headline);
    }
    println!("  📊  {} files · +{}/-{} lines · {} symbols changed",
        cc.files_changed.len(), cc.additions, cc.deletions,
        cc.symbols_added.len() + cc.symbols_removed.len());
    println!();
    println!("📊 View results:");
    println!("  reports/dashboard.html     — open in browser to view");

    // Try to open browser
    let report_path = paporot_dir.join("reports").join("dashboard.html");
    let _ = open::that(&report_path);

    Ok(())
}

/// Print behavior change narrative to terminal
fn print_behavior_narrative(eval: &::Paporot::eval::types::EvalResult) {
    let cc = &eval.code_change;

    let mut added_by_module: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    let mut removed_by_module: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();

    for s in &cc.symbols_added {
        let m = module_short(&s.file_path);
        added_by_module.entry(m).or_default().push(format!("  + {} ({})  — {}", s.name, s.kind, s.file_path));
    }
    for s in &cc.symbols_removed {
        let m = module_short(&s.file_path);
        removed_by_module.entry(m).or_default().push(format!("  - {} ({})  — {}", s.name, s.kind, s.file_path));
    }

    let all_mods: Vec<String> = added_by_module.keys()
        .chain(removed_by_module.keys()).cloned().collect::<std::collections::BTreeSet<_>>()
        .into_iter().collect();

    println!();
    println!("  ┌─ Behavior Change Narrative ────────────────────────────");
    println!("  │");
    println!("  │  Files:  {} changed (+{} / -{} lines)", cc.files_changed.len(), cc.additions, cc.deletions);
    println!("  │  Symbols: {} added, {} removed = {} total changes",
        cc.symbols_added.len(), cc.symbols_removed.len(),
        cc.symbols_added.len() + cc.symbols_removed.len());

    if !all_mods.is_empty() {
        println!("  │");
        println!("  │  ── Per-Module Breakdown ──");
        for m in &all_mods {
            let a_count = added_by_module.get(m).map(|v| v.len()).unwrap_or(0);
            let r_count = removed_by_module.get(m).map(|v| v.len()).unwrap_or(0);
            println!("  │  📦 {}  ({}+ / {}-)", m, a_count, r_count);
            if let Some(lines) = added_by_module.get(m) {
                for l in lines.iter().take(5) { println!("  │  {}", l); }
                if lines.len() > 5 { println!("  │    ... and {} more added", lines.len() - 5); }
            }
            if let Some(lines) = removed_by_module.get(m) {
                for l in lines.iter().take(5) { println!("  │  {}", l); }
                if lines.len() > 5 { println!("  │    ... and {} more removed", lines.len() - 5); }
            }
            println!("  │");
        }
    }

    // Impact spread
    if all_mods.len() > 1 {
        println!("  │  ── Impact Spread ──");
        for i in 0..all_mods.len().min(6) {
            for j in (i+1)..all_mods.len().min(6) {
                println!("  │  {} ──→ {}", all_mods[i], all_mods[j]);
            }
        }
    }

    // Module summary
    if !cc.modules.is_empty() {
        println!("  │");
        println!("  │  Modules affected: {}", cc.modules.join(", "));
    }
    println!("  └───────────────────────────────────────────────────────");
}

/// Short module name from file path
fn module_short(file_path: &str) -> String {
    if file_path.starts_with("src/") {
        if let Some(slash) = file_path[4..].find('/') {
            return format!("src/{}", &file_path[4..4+slash]);
        }
        return "src".into();
    }
    if let Some(slash) = file_path.find('/') {
        return file_path[..slash].to_string();
    }
    file_path.to_string()
}

// ─── Dependency & Git History Helpers ──────────────────────────────

/// Extract module dependency graph by parsing import/use/require from source files
fn extract_dependencies(
    project_root: &std::path::Path,
    cc: &::Paporot::eval::types::CodeChangeSummary,
) -> Vec<(String, String, f64)> {
    use std::collections::HashMap;
    let mut edges: Vec<(String, String, f64)> = Vec::new();
    let mut module_files: HashMap<String, Vec<String>> = HashMap::new();

    for f in &cc.files_changed {
        let m = module_short(f);
        module_files.entry(m).or_default().push(f.clone());
    }

    if module_files.len() < 2 {
        return edges;
    }

    let modules: Vec<String> = module_files.keys().cloned().collect();
    for f in &cc.files_changed {
        let content = std::fs::read_to_string(project_root.join(f)).unwrap_or_default();
        let imports = parse_imports(&content);
        let src_mod = module_short(f);
        for imp_mod_path in &imports {
            for tgt_mod in &modules {
                if imp_mod_path.starts_with(tgt_mod) || tgt_mod.contains(imp_mod_path.as_str()) {
                    if src_mod != *tgt_mod {
                        edges.push((src_mod.clone(), tgt_mod.clone(), 1.0));
                    }
                }
            }
        }
    }

    // Deduplicate and sum strengths
    let mut edge_map: HashMap<(String, String), f64> = HashMap::new();
    for (a, b, w) in edges {
        let key = if a < b { (a, b) } else { (b, a) };
        *edge_map.entry(key).or_default() += w;
    }
    edge_map.into_iter().map(|((a, b), w)| (a, b, w.min(10.0))).collect()
}

/// Parse import/use/require statements from source content
fn parse_imports(content: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        // Rust: use crate::module or use super::module
        if line.starts_with("use ") {
            let rest = line.strip_prefix("use ").unwrap_or("");
            let rest = rest.trim_end_matches(';');
            let parts: Vec<&str> = rest.split("::").collect();
            if parts.len() >= 2 && (parts[0] == "crate" || parts[0] == "self" || parts[0] == "super") {
                imports.push(parts[1..].join("/"));
            } else if !parts.is_empty() && !parts[0].contains('{') {
                imports.push(parts.join("/"));
            }
        }
        // Python: from X import Y / import X
        else if line.starts_with("from ") || line.starts_with("import ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let mod_name = if parts[0] == "from" { parts[1].to_string() } else { parts[1].to_string() };
                imports.push(mod_name.replace('.', "/"));
            }
        }
        // JS/TS: import ... from 'module' / require('module')
        else if line.starts_with("import ") {
            if let Some(from_part) = line.split(" from ").nth(1) {
                let cleaned = from_part.trim().trim_matches(|c| c == '\'' || c == '"' || c == ';');
                if cleaned.starts_with('.') || cleaned.starts_with('@') || !cleaned.contains("//") {
                    imports.push(cleaned.to_string());
                }
            }
        } else if line.contains("require(") {
            if let Some(start) = line.find("require(") {
                let after = &line[start+8..];
                if let Some(end) = after.find(')') {
                    let module = after[..end].trim().trim_matches(|c| c == '\'' || c == '"' || c == '`');
                    imports.push(module.to_string());
                }
            }
        }
    }
    imports
}

/// Scan git history for module activity (for panorama)
fn scan_git_history(project_root: &std::path::Path) -> Result<Vec<serde_json::Value>, String> {
    use std::process::Command;
    let output = Command::new("git")
        .args(["log", "--name-only", "--format=%ai", "-n", "100"])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("git command failed: {}", e))?;

    if !output.status.success() {
        return Err("git log failed (not a git repo?)".into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut module_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("20") { continue; }
        let m = module_short(line);
        *module_counts.entry(m).or_default() += 1;
    }

    let result: Vec<serde_json::Value> = module_counts.into_iter()
        .filter(|(_, c)| *c > 0)
        .map(|(name, count)| serde_json::json!({"module": name, "changeCount": count}))
        .collect();

    Ok(result)
}

/// Build file tree from changed files
fn build_file_tree(files: &[String]) -> serde_json::Value {
    use std::collections::BTreeMap;
    #[derive(serde::Serialize)]
    struct TreeNode {
        name: String,
        path: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        children: Vec<TreeNode>,
        #[serde(rename = "isFile")]
        is_file: bool,
    }

    fn insert_path(root: &mut BTreeMap<String, TreeNode>, parts: &[String], full_path: &str, idx: usize) {
        if idx >= parts.len() { return; }
        let name = &parts[idx];
        let path = parts[..=idx].join("/");
        let entry = root.entry(name.clone()).or_insert_with(|| TreeNode {
            name: name.clone(),
            path: path.clone(),
            children: vec![],
            is_file: false,
        });
        if idx == parts.len() - 1 {
            entry.is_file = true;
            entry.path = full_path.to_string();
        } else {
            let mut children_map = BTreeMap::new();
            for child in entry.children.drain(..) {
                children_map.insert(child.name.clone(), child);
            }
            insert_path(&mut children_map, parts, full_path, idx + 1);
            entry.children = children_map.into_values().collect();
        }
    }

    let mut root_map: BTreeMap<String, TreeNode> = BTreeMap::new();
    for f in files {
        let parts: Vec<String> = f.split('/').map(|s| s.to_string()).collect();
        insert_path(&mut root_map, &parts, f, 0);
    }
    serde_json::to_value(root_map.into_values().collect::<Vec<_>>()).unwrap_or_default()
}

// ─── Narrative Data ────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct NarrativeData {
    headline: String,
    subtitle: String,
    module_interpretations: Vec<ModuleNarrative>,
    overall_risk: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct ModuleNarrative {
    module: String,
    description: String,
    risk: String,
    symbols: Vec<String>,
}

fn generate_narrative(cfg: &config::Config, eval: &::Paporot::eval::types::EvalResult) -> Result<NarrativeData, String> {
    let cc = &eval.code_change;

    let mut mod_symbols: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for s in &cc.symbols_added {
        mod_symbols.entry(module_short(&s.file_path)).or_default()
            .push(format!("+ {} ({})", s.name, s.kind));
    }
    for s in &cc.symbols_removed {
        mod_symbols.entry(module_short(&s.file_path)).or_default()
            .push(format!("- {} ({})", s.name, s.kind));
    }

    let mods_str: Vec<String> = mod_symbols.iter().map(|(m, syms)| {
        format!("    \"{}\": {:?}", m, syms)
    }).collect();

    let prompt = format!(
        r#"You are analyzing code changes made by an AI coding agent. Based on the data below, write a narrative analysis in Chinese.

Task: {}
Files: {} files changed, +{}/-{} lines
Modules: {:?}

Symbol changes by module:
{}

Generate a JSON with these exact fields:
{{
  "headline": "A catchy Chinese headline for this change (max 20 chars, e.g., '认证模块架构升级')",
  "subtitle": "1-2 Chinese sentences explaining what the agent did and what modules are affected",
  "module_interpretations": [
    {{
      "module": "module_name",
      "description": "2-3 Chinese sentences explaining what changed in this module and why it matters",
      "risk": "low|medium|high",
      "symbols": ["symbol_name1", "symbol_name2"]
    }}
  ],
  "overall_risk": "low|medium|high"
}}

Respond ONLY with valid JSON. No markdown, no explanation outside JSON."#,
        eval.task.description,
        cc.files_changed.len(), cc.additions, cc.deletions,
        cc.modules,
        mods_str.join(",\n"),
    );

    let response = call_deepseek_api_sync(&cfg.llm, &prompt, "")?;
    let v: serde_json::Value = serde_json::from_str(&response)
        .map_err(|e| format!("JSON parse error: {}. Raw: {}", e, &response[..response.len().min(300)]))?;

    let headline = v["headline"].as_str().unwrap_or("代码变更分析").to_string();
    let subtitle = v["subtitle"].as_str().unwrap_or("Agent 对项目进行了代码修改").to_string();
    let overall_risk = v["overall_risk"].as_str().unwrap_or("medium").to_string();

    let module_interpretations: Vec<ModuleNarrative> = v["module_interpretations"]
        .as_array().map(|arr| arr.iter().map(|item| ModuleNarrative {
            module: item["module"].as_str().unwrap_or("unknown").to_string(),
            description: item["description"].as_str().unwrap_or("").to_string(),
            risk: item["risk"].as_str().unwrap_or("low").to_string(),
            symbols: item["symbols"].as_array()
                .map(|s| s.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default(),
        }).collect()).unwrap_or_default();

    Ok(NarrativeData { headline, subtitle, module_interpretations, overall_risk })
}

/// V2 markdown summary with behavior narrative
fn generate_summary_md_v2(
    eval: &::Paporot::eval::types::EvalResult,
    narrative: Option<&NarrativeData>,
    prev_count: usize,
) -> String {
    let cc = &eval.code_change;
    let mut md = String::new();

    if let Some(n) = narrative {
        md.push_str(&format!("# {}\n\n", n.headline));
        md.push_str(&format!("_{}_\n\n", n.subtitle));
    } else {
        md.push_str("# Paporot Behavior Audit Report\n\n");
    }

    md.push_str(&format!("**Task:** {}\n\n", eval.task.description));
    md.push_str(&format!("**Eval ID:** {}\n\n", eval.eval_id));
    md.push_str(&format!("**Date:** {}\n\n", eval.created_at));

    md.push_str("## Behavior Changes\n\n");
    md.push_str(&format!("- Files: {} changed (+{} / -{} lines)\n", cc.files_changed.len(), cc.additions, cc.deletions));
    md.push_str(&format!("- Symbols: {} added, {} removed\n\n", cc.symbols_added.len(), cc.symbols_removed.len()));

    if !cc.symbols_added.is_empty() {
        md.push_str("### Added\n\n");
        for s in &cc.symbols_added {
            md.push_str(&format!("- `{}` ({}) in `{}`\n", s.name, s.kind, s.file_path));
        }
        md.push_str("\n");
    }
    if !cc.symbols_removed.is_empty() {
        md.push_str("### Removed\n\n");
        for s in &cc.symbols_removed {
            md.push_str(&format!("- `{}` ({}) in `{}`\n", s.name, s.kind, s.file_path));
        }
        md.push_str("\n");
    }

    if let Some(n) = narrative {
        if !n.module_interpretations.is_empty() {
            md.push_str("## LLM Module Interpretations\n\n");
            for mi in &n.module_interpretations {
                md.push_str(&format!("### {} (Risk: {})\n\n", mi.module, mi.risk));
                md.push_str(&format!("{}\n\n", mi.description));
                if !mi.symbols.is_empty() {
                    md.push_str(&format!("Key symbols: {}\n\n", mi.symbols.join(", ")));
                }
            }
        }
    }

    md.push_str("## Assessment\n\n");
    md.push_str(&format!("- Outcome: {}\n", eval.outcome.label()));
    for g in &eval.grader_results {
        md.push_str(&format!("- {}: {}\n", g.name, if g.passed { "✓ PASS" } else { "✗ FAIL" }));
    }
    md.push_str(&format!("\n- Previous trials: {}\n", prev_count));

    md
}

/// V2 dashboard HTML entry — delegates to sub-builders
fn generate_dashboard_v2(
    eval: &::Paporot::eval::types::EvalResult,
    narrative: Option<&NarrativeData>,
    prev_count: usize,
    full: bool,
    dependency_graph: &[(String, String, f64)],
    panorama_json: Option<&str>,
) -> String {
    let cc = &eval.code_change;
    let headline = narrative.map(|n| n.headline.as_str()).unwrap_or("代码变更分析");
    let subtitle = narrative.map(|n| n.subtitle.as_str()).unwrap_or("Agent 对项目进行了代码修改");
    let overall_risk = narrative.map(|n| n.overall_risk.as_str()).unwrap_or("medium");
    let file_tree_json = build_file_tree(&cc.files_changed);

    let data = build_dashboard_data(eval, cc, narrative, headline, subtitle, overall_risk, prev_count, dependency_graph, &file_tree_json, full, panorama_json);
    let data_json = serde_json::to_string(&data).unwrap_or_default();
    let version = env!("CARGO_PKG_VERSION");

    build_dashboard_html(headline, &data_json, version)
}

/// Build the full JSON data block for the dashboard
fn build_dashboard_data(
    eval: &::Paporot::eval::types::EvalResult,
    cc: &::Paporot::eval::types::CodeChangeSummary,
    narrative: Option<&NarrativeData>,
    headline: &str,
    subtitle: &str,
    overall_risk: &str,
    prev_count: usize,
    dependency_graph: &[(String, String, f64)],
    file_tree_json: &serde_json::Value,
    full: bool,
    panorama_json: Option<&str>,
) -> serde_json::Value {
    let added: Vec<_> = cc.symbols_added.iter().map(|s| json!({"name":s.name,"kind":s.kind.to_string(),"file":s.file_path,"line":s.line_start})).collect();
    let removed: Vec<_> = cc.symbols_removed.iter().map(|s| json!({"name":s.name,"kind":s.kind.to_string(),"file":s.file_path,"line":s.line_start})).collect();
    let mod_edges: Vec<_> = dependency_graph.iter().map(|(s,t,w)| json!({"source":s,"target":t,"strength":w})).collect();

    let mut panorama_data = serde_json::json!({"nodes":[],"links":[]});
    if full {
        if let Some(pj) = panorama_json {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(pj) {
                // Build force graph nodes+links from history + current changes
                let hist_nodes: Vec<serde_json::Value> = v.as_array().unwrap_or(&vec![]).iter()
                    .map(|h| json!({"id": h["module"], "name": h["module"], "changeCount": h["changeCount"], "isChanged": false, "size": 12}))
                    .collect();
                panorama_data["nodes"] = serde_json::to_value(hist_nodes).unwrap_or_default();
                panorama_data["links"] = serde_json::to_value(mod_edges.clone()).unwrap_or_default();
            }
        }
    }

    serde_json::json!({
        "headline": headline,
        "subtitle": subtitle,
        "overallRisk": overall_risk,
        "symbols": {"added": added, "removed": removed},
        "modules": cc.modules,
        "files": cc.files_changed,
        "interpretations": narrative.map(|n| &n.module_interpretations),
        "filesChanged": cc.files_changed.len(),
        "additions": cc.additions,
        "deletions": cc.deletions,
        "prevTrials": prev_count,
        "graders": eval.grader_results.iter().map(|g| json!({"name":g.name,"passed":g.passed})).collect::<Vec<_>>(),
        "eval": {"eval_id":eval.eval_id,"task":{"description":eval.task.description,"id":eval.task.id},"outcome":eval.outcome.label(),"created_at":eval.created_at},
        "modEdges": mod_edges,
        "fileTree": file_tree_json,
        "panorama": panorama_data,
        "full": full,
        "symbolCount": cc.symbols_added.len() + cc.symbols_removed.len(),
    })
}

/// Assemble the complete HTML document
fn build_dashboard_html(headline: &str, data_json: &str, version: &str) -> String {
    let css = build_dashboard_css();
    let js = build_dashboard_js(data_json);
    let version_escaped = version.replace('"', "\\\"");

    format!(r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1.0">
<title>Paporot — {title}</title>
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600;700;800;900&display=swap" rel="stylesheet">
<script src="https://d3js.org/d3.v7.min.js"></script>
<style>
{css}
</style>
</head>
<body>
<header><div class="logo">Paporot<span>Dashboard</span></div>
<nav><button class="active" data-tab="narrative">变更叙事</button><button data-tab="panorama">能力全景</button><button data-tab="tasks">Tasks</button></nav>
<div class="status" id="st">v{ver}</div></header>
<div class="layout">
<aside>
<div><h3>Task 历史</h3><div id="s-tasks"><div style="font-size:12px;color:var(--text3);">当前分析</div></div></div>
<div><h3>模块索引</h3><div id="s-mods" style="display:flex;flex-wrap:wrap;gap:4px;"></div></div>
</aside>
<main>
<div class="page active" id="pg-narrative">
<div id="bb"></div>
<div id="sr" class="stats fade-up" style="animation-delay:.1s"></div>
<div class="sec fade-up" style="animation-delay:.2s"><h2>影响范围 · 三层瀑布扩散</h2><div class="wf-box" id="wf"><div class="tip" id="wf-tip"></div><div class="leg"><span><span class="dot" style="background:var(--brand)"></span>符号</span><span><span class="dot" style="background:var(--cyan)"></span>模块</span><span><span class="dot" style="background:var(--text3)"></span>下游</span></div></div></div>
<div class="sec fade-up" style="animation-delay:.4s"><h2>LLM 解读</h2><div id="ints"></div></div>
</div>
<div class="page" id="pg-panorama"><div class="wf-box" id="pn" style="height:calc(100vh - var(--header) - 48px);"><div class="tip" id="pn-tip"></div></div></div>
<div class="page" id="pg-tasks"><div style="padding:24px;"><h2 style="font-size:16px;font-weight:600;color:var(--text);margin-bottom:16px;">全部 Task</h2><table style="width:100%;border-collapse:collapse;font-size:14px;" id="tbl"></table></div></div>
</main>
</div>
<script>
{js}
</script>
</body>
</html>"##,
        title = headline,
        css = css,
        ver = version_escaped,
        js = js,
    )
}

/// Build CSS styles for the dashboard
fn build_dashboard_css() -> String {
    r##"*,*::before,*::after{margin:0;padding:0;box-sizing:border-box;}
:root{--bg:#111827;--bg2:#1F2937;--border:rgba(255,255,255,0.06);--text:#F9FAFB;--text2:#9CA3AF;--text3:#6B7280;--brand:#6366F1;--cyan:#06B6D4;--amber:#F59E0B;--red:#EF4444;--green:#22C55E;--sidebar:280px;--header:56px;}
body{font-family:'Inter',-apple-system,sans-serif;background:var(--bg);color:var(--text);overflow:hidden;height:100vh;}
header{height:var(--header);display:flex;align-items:center;padding:0 20px;border-bottom:1px solid var(--border);background:rgba(17,24,39,0.9);backdrop-filter:blur(12px);position:fixed;top:0;left:0;right:0;z-index:100;}
header .logo{font-size:18px;font-weight:700;color:var(--brand);letter-spacing:-0.5px;margin-right:32px;}
header .logo span{color:var(--text2);font-weight:400;margin-left:4px;font-size:13px;}
nav{display:flex;gap:4px;flex:1;}
nav button{padding:6px 20px;border-radius:8px;font-size:14px;font-weight:500;color:var(--text2);cursor:pointer;border:none;background:transparent;font-family:inherit;transition:all .2s;}
nav button:hover{color:var(--text);background:rgba(255,255,255,0.04);}
nav button.active{color:var(--text);background:rgba(99,102,241,0.12);}
header .status{font-size:12px;color:var(--text3);}
.layout{display:flex;height:calc(100vh - var(--header));margin-top:var(--header);}
aside{width:var(--sidebar);min-width:var(--sidebar);border-right:1px solid var(--border);overflow-y:auto;padding:16px;display:flex;flex-direction:column;gap:16px;background:rgba(17,24,39,0.6);}
aside h3{font-size:11px;font-weight:600;text-transform:uppercase;letter-spacing:.1em;color:var(--text3);margin-bottom:8px;}
aside .task-item{padding:8px 10px;border-radius:6px;cursor:pointer;transition:background .15s;display:flex;gap:8px;align-items:flex-start;}
aside .task-item:hover{background:var(--border);}
aside .task-dot{width:8px;height:8px;border-radius:50%;margin-top:4px;flex-shrink:0;background:var(--brand);}
aside .task-item .title{font-size:13px;color:var(--text);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;margin-top:2px;}
aside .mod-tag{font-size:11px;padding:2px 8px;border-radius:4px;cursor:pointer;background:rgba(255,255,255,0.04);color:var(--text2);display:inline-block;margin:2px;transition:all .15s;}
aside .mod-tag:hover{background:rgba(99,102,241,0.15);color:var(--brand);}
main{flex:1;overflow-y:auto;}
.page{display:none;}
.page.active{display:block;}
.billboard{text-align:center;padding:60px 40px 40px;background:linear-gradient(180deg,rgba(99,102,241,0.08) 0%,transparent 100%);border-bottom:1px solid var(--border);}
.billboard h1{font-size:36px;font-weight:800;color:var(--brand);letter-spacing:-1px;line-height:1.2;}
.billboard .sub{font-size:16px;color:var(--text2);margin-top:12px;max-width:640px;margin-left:auto;margin-right:auto;line-height:1.6;}
.billboard .tags{display:flex;gap:8px;justify-content:center;margin-top:16px;flex-wrap:wrap;}
.billboard .tags span{font-size:12px;padding:4px 12px;border-radius:6px;background:rgba(99,102,241,0.1);color:var(--brand);border:1px solid rgba(99,102,241,0.2);}
.billboard .tags .r-high{background:rgba(239,68,68,0.1);color:var(--red);border-color:rgba(239,68,68,0.2);}
.billboard .tags .r-low{background:rgba(34,197,94,0.1);color:var(--green);border-color:rgba(34,197,94,0.2);}
.billboard .tags .r-med{background:rgba(245,158,11,0.1);color:var(--amber);border-color:rgba(245,158,11,0.2);}
.stats{display:flex;gap:16px;padding:24px;max-width:900px;margin:0 auto;}
.stat{flex:1;background:rgba(30,41,59,0.6);border:1px solid var(--border);border-radius:12px;padding:20px;text-align:center;}
.stat .num{font-size:32px;font-weight:800;color:var(--brand);line-height:1;}
.stat .label{font-size:11px;color:var(--text3);margin-top:6px;text-transform:uppercase;letter-spacing:.1em;font-weight:600;}
.sec{padding:24px;}
.sec h2{font-size:14px;font-weight:600;color:var(--text2);margin-bottom:16px;text-transform:uppercase;letter-spacing:.08em;}
.wf-box{width:100%;height:520px;background:var(--bg2);border:1px solid var(--border);border-radius:12px;position:relative;overflow:hidden;}
.wf-box .tip{position:absolute;padding:10px 14px;pointer-events:none;opacity:0;background:rgba(30,41,59,0.95);border:1px solid var(--border);border-radius:8px;font-size:12px;color:var(--text);z-index:10;line-height:1.5;backdrop-filter:blur(8px);transition:opacity .15s;max-width:260px;}
.wf-box .leg{position:absolute;bottom:12px;right:16px;display:flex;gap:12px;font-size:11px;color:var(--text3);}
.wf-box .leg .dot{width:8px;height:8px;border-radius:50%;display:inline-block;margin-right:4px;}
.mcard{background:var(--bg2);border:1px solid var(--border);border-radius:10px;padding:20px;margin-bottom:12px;}
.mcard .mhead{display:flex;align-items:center;gap:12px;margin-bottom:12px;}
.mcard .mname{font-size:18px;font-weight:700;color:var(--text);}
.mcard .mrisk{font-size:11px;padding:3px 10px;border-radius:4px;font-weight:600;text-transform:uppercase;}
.r-low{background:rgba(34,197,94,0.12);color:var(--green);}
.r-medium{background:rgba(245,158,11,0.12);color:var(--amber);}
.r-high{background:rgba(239,68,68,0.12);color:var(--red);}
.mcard .mdesc{font-size:14px;color:var(--text2);line-height:1.7;}
.mcard .msyms{display:flex;gap:6px;margin-top:12px;flex-wrap:wrap;}
.sym-tag{font-size:11px;padding:3px 10px;border-radius:4px;background:rgba(99,102,241,0.08);color:var(--brand);font-family:'SF Mono','Fira Code',monospace;}
@keyframes pulse{0%,100%{opacity:0.5;}50%{opacity:1;}}
@keyframes fadeInUp{from{opacity:0;transform:translateY(20px);}to{opacity:1;transform:translateY(0);}}
@keyframes popIn{0%{opacity:0;transform:scale(0.3);}60%{transform:scale(1.15);}100%{opacity:1;transform:scale(1);}}
.fade-up{animation:fadeInUp .6s ease-out;}
.empty{text-align:center;padding:80px 20px;color:var(--text3);}
.empty .icon{font-size:48px;margin-bottom:16px;opacity:.3;}
.empty h3{font-size:18px;margin-bottom:8px;color:var(--text2);}
.empty code{font-size:13px;padding:4px 12px;background:rgba(99,102,241,0.1);border-radius:6px;color:var(--brand);font-family:'SF Mono','Fira Code',monospace;}
::-webkit-scrollbar{width:6px;}
::-webkit-scrollbar-track{background:transparent;}
::-webkit-scrollbar-thumb{background:rgba(255,255,255,0.06);border-radius:3px;}
::-webkit-scrollbar-thumb:hover{background:rgba(255,255,255,0.12);}"##.to_string()
}

/// Build all JavaScript logic for the dashboard
fn build_dashboard_js(data_json: &str) -> String {
    let escaped_json = data_json.replace('\\', "\\\\").replace('`', "\\`");
    format!(r##"const D={dj};const esc=s=>{{const d=document.createElement('div');d.textContent=s||'';return d.innerHTML;}};
let pg='narrative';
document.querySelectorAll('nav button').forEach(b=>b.addEventListener('click',()=>{{
document.querySelectorAll('nav button').forEach(x=>x.classList.remove('active'));b.classList.add('active');
pg=b.dataset.tab;document.querySelectorAll('.page').forEach(p=>p.classList.remove('active'));
document.getElementById('pg-'+pg).classList.add('active');
if(pg==='panorama')drawPN();if(pg==='tasks')renderTasks();
}}));
// Billboard + Stats
const rCls=D.overallRisk==='high'?'r-high':D.overallRisk==='low'?'r-low':'r-med';
document.getElementById('bb').innerHTML=`<div class="billboard"><h1>「${{esc(D.headline)}}」</h1><div class="sub">${{esc(D.subtitle)}}</div><div class="tags"><span>${{D.filesChanged}}个文件</span><span>+${{D.additions}}/-${{D.deletions}}</span>${{(D.modules||[]).slice(0,5).map(m=>`<span>${{esc(m)}}</span>`).join('')}}<span class="${{rCls}}">风险：${{D.overallRisk==='high'?'高':D.overallRisk==='low'?'低':'中'}}</span></div></div>`;
document.getElementById('sr').innerHTML=`<div class="stat"><div class="num">${{D.filesChanged}}</div><div class="label">Files Changed</div></div><div class="stat"><div class="num">+${{D.additions}}</div><div class="label">Lines Added</div></div><div class="stat"><div class="num">-${{D.deletions}}</div><div class="label">Lines Deleted</div></div><div class="stat"><div class="num">${{D.symbolCount||0}}</div><div class="label">Symbols Changed</div></div>`;
// LLM interpretations
const intEl=document.getElementById('ints');
const ints=D.interpretations||[];
if(ints.length===0){{intEl.innerHTML='<div class="empty"><div class="icon">&#129300;</div><h3>暂无LLM解读</h3><p>运行<code>paporot analyze</code>并配置API Key以生成AI解读</p></div>';}}else{{
intEl.innerHTML=ints.map(m=>`<div class="mcard"><div class="mhead"><div class="mname">&#x1F4E6; ${{esc(m.module)}}</div><div class="mrisk r-${{m.risk||'low'}}">${{m.risk||'low'}}</div></div><div class="mdesc">${{esc(m.description)}}</div>${{m.symbols&&m.symbols.length>0?`<div class="msyms">${{m.symbols.map(s=>`<span class="sym-tag">${{esc(s)}}</span>`).join('')}}</div>`:''}}</div>`).join('');
}}
// Sidebar
const sT=document.getElementById('s-tasks');
if(D.eval){{sT.innerHTML=`<div class="task-item"><div class="task-dot"></div><div><div class="title">${{esc(D.eval.task?D.eval.task.description:'Current')}}</div></div></div>`;}}
const sM=document.getElementById('s-mods');
const mods=D.modules||[];sM.innerHTML=mods.length>0?mods.map(m=>`<span class="mod-tag" data-m="${{esc(m)}}">${{esc(m)}}</span>`).join(''):'<span style="font-size:11px;color:var(--text3);">无</span>';
document.querySelectorAll('.mod-tag').forEach(t=>t.addEventListener('click',()=>{{document.querySelectorAll('nav button').forEach(b=>b.classList.remove('active'));const b2=document.querySelector('nav button[data-tab="panorama"]');if(b2)b2.classList.add('active');document.querySelectorAll('.page').forEach(p=>p.classList.remove('active'));document.getElementById('pg-panorama').classList.add('active');pg='panorama';drawPN(t.dataset.m);}}));

// ═══ Waterfall ═══
(function(){{
const c=document.getElementById('wf');const ex=c.querySelector('svg');if(ex)ex.remove();
const syms=[...(D.symbols.added||[]).map(s=>({{...s,action:'added'}})),...(D.symbols.removed||[]).map(s=>({{...s,action:'removed'}}))];
if(syms.length===0&&(D.modules||[]).length===0){{const svg=d3.select('#wf').append('svg').attr('width','100%').attr('height',520);svg.append('text').attr('x','50%').attr('y',260).attr('text-anchor','middle').attr('fill','#6B7280').attr('font-size','14px').text('暂无符号变更数据');return;}}
const W=c.clientWidth,H=520;const svg=d3.select('#wf').append('svg').attr('width',W).attr('height',H);const tip=d3.select('#wf-tip');
const edgeData=D.modEdges||[];
const zoom=d3.zoom().scaleExtent([0.5,3]).on('zoom',e=>{{gAll.attr('transform',e.transform);}});
svg.call(zoom).on('dblclick.zoom',null);
const gAll=svg.append('g').attr('transform',`translate(${{W/2}},${{H/2}})`);

// 3-layer layout
const nSyms=syms.length, nMods=(D.modules||[]).length;
const symNodes=syms.map((s,i)=>({{
id:'sym-'+i,name:s.name,kind:s.kind,action:s.action,file:s.file,line:s.line||0,
layer:0,r:Math.min(140,60+nSyms*6),angle:2*Math.PI*i/Math.max(nSyms,1)-Math.PI/2,
size:Math.max(6,Math.min(14,s.name.length*1.2)),x:0,y:0
}}));
const modNodes=(D.modules||[]).map((m,i)=>({{
id:'mod-'+m,name:m,layer:1,r:Math.min(250,160+nMods*10),angle:2*Math.PI*i/Math.max(nMods,1)-Math.PI/2,
size:Math.max(16,Math.min(80,mods.filter(x=>m===x).length*16+8)),x:0,y:0
}}));
const depNodes=[];const seen=new Set();
edgeData.forEach(e=>{{if(!D.modules.includes(e.source)&&!seen.has(e.source)){{seen.add(e.source);depNodes.push({{id:'dep-'+e.source,name:e.source,layer:2,r:320+depNodes.length*18,angle:2*Math.PI*depNodes.length/8-Math.PI/2,size:Math.max(14,Math.min(60,e.strength*6)),x:0,y:0}});}}
if(!D.modules.includes(e.target)&&!seen.has(e.target)){{seen.add(e.target);depNodes.push({{id:'dep-'+e.target,name:e.target,layer:2,r:320+depNodes.length*18,angle:2*Math.PI*depNodes.length/8-Math.PI/2,size:Math.max(14,Math.min(60,e.strength*6)),x:0,y:0}});}}}});
const allNodes=[...symNodes,...modNodes,...depNodes];
allNodes.forEach(n=>{{n.x=n.r*Math.cos(n.angle);n.y=n.r*Math.sin(n.angle);}});

// Layer animation: delay per layer
const layerDelay=[0,300,600];
const nodeTime=200;

// Links from symbols -> modules (Bézier)
const symToModLinks=[];
symNodes.forEach(s=>{{modNodes.forEach(m=>{{symToModLinks.push({{source:s,target:m}});}});}});
gAll.selectAll('.slink').data(symToModLinks).join('path').attr('d',d=>`M${{d.source.x}},${{d.source.y}}C${{d.source.x*0.7}},${{(d.source.y+d.target.y)/2}} ${{d.target.x*0.7}},${{(d.source.y+d.target.y)/2}} ${{d.target.x}},${{d.target.y}}`).attr('stroke','#475569').attr('stroke-width',0.4).attr('fill','none').attr('opacity',0).transition().delay((_,i)=>layerDelay[0]+i*30).attr('opacity',0.25).duration(nodeTime);

// Module-to-module edges (horizontal coupling)
modNodes.forEach((m,i)=>{{if(i<modNodes.length-1){{modNodes.slice(i+1).forEach(m2=>{{gAll.append('line').attr('x1',m.x).attr('y1',m.y).attr('x2',m2.x).attr('y2',m2.y).attr('stroke','#374151').attr('stroke-width',0.5).attr('opacity',0).transition().delay(layerDelay[1]+i*40).attr('opacity',0.2).duration(nodeTime);}});}}}});

// Module-to-dep downstream links
modNodes.forEach(m=>{{depNodes.forEach(d=>{{gAll.append('line').attr('x1',m.x).attr('y1',m.y).attr('x2',d.x).attr('y2',d.y).attr('stroke','#374151').attr('stroke-width',0.3).attr('stroke-dasharray','3,3').attr('opacity',0).transition().delay(layerDelay[2]+50).attr('opacity',0.15).duration(nodeTime);}});}});

// Symbol nodes (Layer 0)
symNodes.forEach((n,i)=>{{const g=gAll.append('g').attr('opacity',0).attr('transform',`translate(${{n.x}},${{n.y}})scale(0.3)`);
g.transition().delay(layerDelay[0]+i*50).duration(300).attr('opacity',1).attrTween('transform',()=>t=>`translate(${{n.x}},${{n.y}})scale(${{d3.interpolate(0.3,1)(t)}})`);
g.append('circle').attr('r',n.size).attr('fill',n.action==='removed'?'#EF4444':'#6366F1').attr('stroke','#111827').attr('stroke-width',2).style('animation','pulse 3s infinite');
g.append('text').attr('x',n.size+6).attr('y',3.5).text(n.name).attr('fill','#F9FAFB').attr('font-size','10px').attr('font-weight','600');
g.on('mouseenter',e=>{{tip.style('opacity','1').style('left',e.offsetX+14+'px').style('top',e.offsetY-10+'px').html(`<b>${{n.name}}</b><br/>${{n.kind}} · ${{n.action==='removed'?'已删除':'新增'}}<br/>${{n.file}}:${{n.line}}`);}});
g.on('mouseleave',()=>tip.style('opacity','0'));
}});

// Module nodes (Layer 1)
modNodes.forEach((n,i)=>{{const g=gAll.append('g').attr('opacity',0).attr('transform',`translate(${{n.x}},${{n.y}})scale(0.3)`);
g.transition().delay(layerDelay[1]+i*80).duration(300).attr('opacity',1).attrTween('transform',()=>t=>`translate(${{n.x}},${{n.y}})scale(${{d3.interpolate(0.3,1)(t)}})`);
g.append('rect').attr('x',-n.size/2).attr('y',-9).attr('width',n.size).attr('height',18).attr('rx',4).attr('fill','#06B6D4').attr('opacity',0.6);
g.append('text').attr('x',n.size/2+6).attr('y',3.5).text(n.name).attr('fill','#9CA3AF').attr('font-size','12px');
g.on('mouseenter',e=>{{tip.style('opacity','1').style('left',e.offsetX+14+'px').style('top',e.offsetY-10+'px').html(`<b>${{n.name}}</b><br/>受影响模块`);}});
g.on('mouseleave',()=>tip.style('opacity','0'));
}});

// Dep nodes (Layer 2)
depNodes.forEach((n,i)=>{{const g=gAll.append('g').attr('opacity',0).attr('transform',`translate(${{n.x}},${{n.y}})scale(0.3)`);
g.transition().delay(layerDelay[2]+i*100).duration(300).attr('opacity',1).attrTween('transform',()=>t=>`translate(${{n.x}},${{n.y}})scale(${{d3.interpolate(0.3,1)(t)}})`);
g.append('rect').attr('x',-n.size/2).attr('y',-7).attr('width',n.size).attr('height',14).attr('rx',3).attr('fill','#475569').attr('opacity',0.4);
g.append('text').attr('x',n.size/2+6).attr('y',3).text(n.name).attr('fill','#6B7280').attr('font-size','11px');
}});
}})();

// ═══ Panorama ═══
function drawPN(focusMod){{
const c=document.getElementById('pn');const ex=c.querySelector('svg');if(ex)ex.remove();
try{{
const svg=d3.select('#pn').append('svg').attr('viewBox','-400 -300 800 600').attr('preserveAspectRatio','xMidYMid meet').style('width','100%').style('height','100%');
const gAll=svg.append('g');
const zoom=d3.zoom().scaleExtent([0.3,4]).on('zoom',e=>{{gAll.attr('transform',e.transform);}});
svg.call(zoom).on('dblclick.zoom',null);
const tip=d3.select('#pn-tip');

const pn=D.panorama||{{}};const pnNodes=pn.nodes||[];const pnLinks=pn.links||[];
let nodes,links;
if(pnNodes.length===0){{
const syms=[...(D.symbols.added||[]),...(D.symbols.removed||[])];const mm={{}};
syms.forEach(s=>{{const m=s.file?s.file.split('/').slice(0,-1).join('/')||'root':'root';if(!mm[m])mm[m]={{mod:m,count:0,isChanged:true}};mm[m].count++;}});
nodes=Object.values(mm).map((m,i)=>({{id:m.mod,name:m.mod.split('/').pop()||m.mod,count:m.count,isChanged:true,size:16+m.count*3,x:0,y:0,vy:0,vx:0,index:i}}));
links=D.modEdges||[];
}}else{{
const curMods=new Set((D.modules||[]).map(m=>m.toLowerCase()));
nodes=pnNodes.map((n,i)=>({{id:n.id||n.module,name:n.name||n.module,count:n.changeCount||0,isChanged:curMods.has((n.id||n.module||'').toLowerCase()),size:14+(n.changeCount||0)*1.5,x:0,y:0,vy:0,vx:0,index:i}}));
links=pnLinks;
}}

if(nodes.length===0){{svg.append('text').attr('x',0).attr('y',0).attr('text-anchor','middle').attr('fill','#6B7280').attr('font-size','14px').text(D.full?'暂无全景数据':'运行 paporot analyze --full 查看完整能力全景');return;}}

const sim=d3.forceSimulation(nodes).force('link',d3.forceLink(links).id(d=>d.id).distance(80)).force('charge',d3.forceManyBody().strength(-150)).force('center',d3.forceCenter(0,0)).force('collision',d3.forceCollide(20)).alpha(1).alphaDecay(0.015);

const le=gAll.append('g').selectAll('line').data(links).join('line').attr('stroke','#374151').attr('stroke-width',d=>(d.strength||d.val||0.5)*2).attr('opacity',0.15);

const drag=d3.drag().on('start',(e,d)=>{{if(!e.active)sim.alphaTarget(0.3).restart();d.fx=d.x;d.fy=d.y;}}).on('drag',(e,d)=>{{d.fx=e.x;d.fy=e.y;}}).on('end',(e,d)=>{{if(!e.active)sim.alphaTarget(0);d.fx=null;d.fy=null;}});

const ng=gAll.append('g').selectAll('g').data(nodes).join('g').call(drag);
ng.attr('transform',d=>`translate(${{d.x}},${{d.y}})scale(0)`).transition().delay((d,i)=>i*80).duration(400).attrTween('transform',d=>t=>`translate(${{d.x}},${{d.y}})scale(${{d3.interpolate(0,1)(t)}})`);

ng.append('rect').attr('width',d=>d.size*2).attr('height',d=>d.size*2).attr('x',d=>-d.size).attr('y',d=>-d.size).attr('rx',4).attr('fill',d=>d.isChanged?'#6366F1':d.count>5?'#06B6D4':'#374151').attr('opacity',d=>d.isChanged?0.85:0.5).style('animation',d=>d.isChanged?'pulse 2.5s infinite':'none');

ng.append('text').text(d=>d.name).attr('y',d=>d.size+13).attr('text-anchor','middle').attr('fill','#9CA3AF').attr('font-size','10px').attr('opacity',0.8);

ng.on('mouseenter',(e,d)=>{{tip.style('opacity','1').style('left',e.offsetX+14+'px').style('top',e.offsetY-8+'px').html(`<b>${{d.name}}</b><br/>变更次数：${{d.count}}<br/>${{d.isChanged?'本次变更  ⚡':'历史模块'}}`);}});
ng.on('mouseleave',()=>tip.style('opacity','0'));

sim.on('tick',()=>{{le.attr('x1',d=>d.source.x).attr('y1',d=>d.source.y).attr('x2',d=>d.target.x).attr('y2',d=>d.target.y);ng.attr('transform',d=>`translate(${{d.x}},${{d.y}})`);}});

if(focusMod){{setTimeout(()=>{{const n=nodes.find(n=>n.id===focusMod||(n.id||'').includes(focusMod));if(n){{sim.alphaTarget(0.1).restart();n.fx=0;n.fy=0;setTimeout(()=>{{n.fx=null;n.fy=null;sim.alphaTarget(0);}},800);}}}},600);}}
}}catch(e){{c.innerHTML='<div style="color:#EF4444;padding:40px;text-align:center;font-size:14px;">Panorama error: '+e.message+'</div>';}}
}}

// ═══ Tasks ═══
function renderTasks(){{
const t=document.getElementById('tbl');
if(D.eval){{t.innerHTML=`<thead><tr style="border-bottom:1px solid var(--border);"><th style="text-align:left;padding:12px 16px;font-size:11px;color:var(--text3);text-transform:uppercase;">ID</th><th style="text-align:left;padding:12px 16px;font-size:11px;color:var(--text3);text-transform:uppercase;">描述</th><th style="text-align:left;padding:12px 16px;font-size:11px;color:var(--text3);text-transform:uppercase;">类别</th><th style="text-align:left;padding:12px 16px;font-size:11px;color:var(--text3);text-transform:uppercase;">模块</th></tr></thead><tbody><tr style="border-bottom:1px solid var(--border);"><td style="padding:12px 16px;font-family:monospace;font-size:12px;color:var(--text2);">${{(D.eval.eval_id||'').slice(0,12)}}...</td><td style="padding:12px 16px;font-weight:500;">${{esc(D.eval.task?.description||'')}}</td><td style="padding:12px 16px;">—</td><td style="padding:12px 16px;">${{(D.modules||[]).slice(0,3).join(', ')||'—'}}</td></tr></tbody>`;}}
else{{t.innerHTML='<tr><td colspan="4" style="padding:40px;text-align:center;color:var(--text3);">暂无 Task</td></tr>';}}
}}"##, dj = escaped_json)
}

/// Synchronous wrapper for DeepSeek API calls
fn call_deepseek_api_sync(llm: &config::LlmConfig, prompt: &str, _schema: &str) -> Result<String, String> {
    let api_key = if llm.api_key.is_empty() {
        std::env::var("PAPOROT_API_KEY").unwrap_or_default()
    } else {
        llm.api_key.clone()
    };

    if api_key.is_empty() {
        return Err("No API key configured".into());
    }

    let endpoint = if llm.endpoint.is_empty() {
        "https://api.deepseek.com/v1/chat/completions"
    } else {
        &llm.endpoint
    };

    let body = serde_json::json!({
        "model": llm.model,
        "messages": [
            {"role": "user", "content": prompt}
        ],
        "temperature": llm.temperature,
        "max_tokens": llm.max_tokens,
    });

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(120))
        .json(&body)
        .send()
        .map_err(|e| format!("HTTP error: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(format!("API error {}: {}", status, body));
    }

    let json: serde_json::Value = resp.json().map_err(|e| format!("Parse error: {}", e))?;
    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No content in response".into())
}

// ─── Skill sign command ───────────────────────────────────────────

fn cmd_skill(args: &[String]) -> Result<()> {
    if args.is_empty() {
        println!("Usage: paporot skill sign <skill_name>");
        println!("  sign    Sign a skill with SHA256 hash (for host_exec_command access)");
        return Ok(());
    }

    match args[0].as_str() {
        "sign" => {
            if args.len() < 2 {
                anyhow::bail!("Usage: paporot skill sign <skill_name>");
            }
            let skill_name = &args[1];
            let paporot_dir = find_paporot_dir()?;
            let wasm_path = paporot_dir.join("skills").join(skill_name).join("skill.wasm");
            let sig_path = paporot_dir.join("skills").join(skill_name).join("signature");

            if !wasm_path.exists() {
                anyhow::bail!("Skill '{}' not found at {}", skill_name, wasm_path.display());
            }

            use sha2::{Sha256, Digest};
            let wasm_bytes = fs::read(&wasm_path)?;
            let mut hasher = Sha256::new();
            hasher.update(&wasm_bytes);
            let hash_bytes = hasher.finalize();
            let hash = hash_bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>();

            // Simple approach: use a host secret from environment or default
            let secret = std::env::var("PAPOROT_SIGNING_SECRET")
                .unwrap_or_else(|_| "paporot-default-signing-secret".to_string());
            let mut keyed_hasher = Sha256::new();
            keyed_hasher.update(secret.as_bytes());
            keyed_hasher.update(&wasm_bytes);
            let sig_bytes = keyed_hasher.finalize();
            let signature = sig_bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>();

            fs::write(&sig_path, &signature)?;
            println!("Signed skill '{}'", skill_name);
            println!("  SHA256(hash): {}", hash);
            println!("  Signature:     {}", signature);
            println!("  Stored at:     {}", sig_path.display());
        }
        other => anyhow::bail!("Unknown skill subcommand '{}'. Try: sign", other),
    }

    Ok(())
}

/// Copy contents of `src` directory into `dst` (non-recursive dir copy)
fn copy_dir_contents(src: &Path, dst: &Path) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let name = entry.file_name();
        let dst_entry = dst.join(&name);
        if ty.is_dir() {
            fs::create_dir_all(&dst_entry)?;
            copy_dir_contents(&entry.path(), &dst_entry)?;
        } else {
            fs::copy(entry.path(), &dst_entry)?;
        }
    }
    Ok(())
}

/// Scan project root for source files and copy text files into .Paporot/work/sources/
fn collect_sources(project_root: &Path, paporot_dir: &Path) -> Result<()> {
    let work = paporot_dir.join("work").join("sources");
    fs::create_dir_all(&work)?;

    // Source file extensions we care about
    let text_exts: &[&str] = &["rs", "toml", "md", "json", "yaml", "yml", "html", "css", "js", "ts", "py", "go", "java", "c", "cpp", "h", "hpp"];
    // Files to skip
    let skip_dirs: &[&str] = &["target", ".git", "node_modules", ".Paporot", "__pycache__"];

    let mut file_list = Vec::new();
    let mut total_bytes = 0usize;
    let max_total = 128 * 1024; // 128 KB limit

    scan_dir(project_root, project_root, text_exts, skip_dirs, &mut file_list, &mut total_bytes, max_total);

    // Copy files
    for (rel_path, abs_path) in &file_list {
        if let Ok(content) = fs::read_to_string(abs_path) {
            let dst = work.join(rel_path);
            if let Some(parent) = dst.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&dst, &content);
        }
    }

    // Write manifest
    let manifest: Vec<serde_json::Value> = file_list.iter().map(|(rel, _)| {
        serde_json::json!({"path": rel})
    }).collect();
    let manifest_json = serde_json::to_string_pretty(&manifest).unwrap_or_default();
    let _ = fs::write(work.join("_manifest.json"), &manifest_json);

    eprintln!("[loader] Collected {} source files into {}", file_list.len(), work.display());
    Ok(())
}

fn scan_dir(
    base: &Path, dir: &Path,
    exts: &[&str], skip_dirs: &[&str],
    files: &mut Vec<(String, PathBuf)>,
    total_bytes: &mut usize, max_total: usize,
) {
    if *total_bytes >= max_total { return; }
    let entries = match fs::read_dir(dir) { Ok(e) => e, Err(_) => return };

    for entry in entries.flatten() {
        if *total_bytes >= max_total { return; }
        let fname = entry.file_name();
        let name = fname.to_string_lossy();
        if skip_dirs.contains(&name.as_ref()) { continue; }
        let ft = entry.file_type();
        if let Ok(true) = ft.as_ref().map(|t| t.is_dir()) {
            scan_dir(base, &entry.path(), exts, skip_dirs, files, total_bytes, max_total);
        } else if let Ok(true) = ft.as_ref().map(|t| t.is_file()) {
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                if exts.contains(&ext) {
                    let entry_path = entry.path();
                    let rel = entry_path.strip_prefix(base).unwrap_or(&entry_path);
                    let size = entry.metadata().map(|m| m.len() as usize).unwrap_or(0);
                    if *total_bytes + size <= max_total {
                        *total_bytes += size;
                        files.push((rel.to_string_lossy().to_string(), entry_path));
                    }
                }
            }
        }
    }
}

// ─── Host Functions ──────────────────────────────────────────────

/// Grow WASM memory if needed, then write data and return (ptr << 32) | len
fn pack_to_wasm(
    caller: &mut Caller<'_, SandboxHost>,
    data: &[u8],
) -> i64 {
    let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
        Some(m) => m, None => return 0,
    };
    let alloc_at = mem.data_size(&mut *caller); // save BEFORE potential grow
    let target = alloc_at + data.len();
    let page_size = 65536;
    let needed = (target + page_size - 1) / page_size;
    let cur_pages = (alloc_at + page_size - 1) / page_size;
    if needed > cur_pages {
        if mem.grow(&mut *caller, (needed - cur_pages) as u64).is_err() {
            return 0;
        }
    }
    let ptr = alloc_at as i32;
    let _ = mem.write(caller, alloc_at, data);
    ((ptr as i64) << 32) | (data.len() as i64)
}

fn register_host_functions(linker: &mut Linker<SandboxHost>) -> Result<()> {
    // host_read_file(path_ptr, path_len) -> (data_ptr << 32) | data_len
    linker.func_wrap("env", "host_read_file",
        |mut caller: Caller<'_, SandboxHost>, path_ptr: i32, path_len: i32| -> i64 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 0,
            };
            let mut buf = vec![0u8; path_len as usize];
            if mem.read(&caller, path_ptr as usize, &mut buf).is_err() { return 0; }
            let path = String::from_utf8_lossy(&buf).into_owned();

            let dir = caller.data().paporot_dir.clone();
            let dir_canonical = dir.canonicalize().ok();
            let resolved = dir.join(&path);
            let data = resolved.canonicalize().ok()
                .and_then(|c| {
                    match &dir_canonical {
                        Some(dc) if c.starts_with(dc) => fs::read(&c).ok(),
                        _ => None,
                    }
                });

            match data {
                Some(bytes) => pack_to_wasm(&mut caller, &bytes),
                None => 0,
            }
        },
    )?;

    // host_write_file(path_ptr, path_len, data_ptr, data_len) -> errno
    linker.func_wrap("env", "host_write_file",
        |mut caller: Caller<'_, SandboxHost>, path_ptr: i32, path_len: i32,
         data_ptr: i32, data_len: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 1,
            };
            let mut p = vec![0u8; path_len as usize];
            let mut d = vec![0u8; data_len as usize];
            if mem.read(&caller, path_ptr as usize, &mut p).is_err() { return 1; }
            if mem.read(&caller, data_ptr as usize, &mut d).is_err() { return 1; }
            let path = String::from_utf8_lossy(&p).into_owned();

            let dir = &caller.data().paporot_dir;
            let resolved = dir.join(&path);
            if let Some(parent) = resolved.parent() { let _ = fs::create_dir_all(parent); }
            match fs::write(&resolved, &d) {
                Ok(()) => 0,
                Err(e) => e.raw_os_error().unwrap_or(1),
            }
        },
    )?;

    // host_exec_command(cmd_ptr, cmd_len) -> (out_ptr << 32) | out_len
    // Only available when at least one skill is signed
    // Command must match whitelist
    linker.func_wrap("env", "host_exec_command",
        |mut caller: Caller<'_, SandboxHost>,
         cmd_ptr: i32, cmd_len: i32| -> i64 {
            if !caller.data().has_signed_skill {
                eprintln!("[sandbox] host_exec_command blocked: no signed skill");
                return 0;
            }

            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 0,
            };
            let mut buf = vec![0u8; cmd_len as usize];
            if mem.read(&caller, cmd_ptr as usize, &mut buf).is_err() { return 0; }
            let cmd = String::from_utf8_lossy(&buf).into_owned();

            // Command whitelist
            let allowed = [
                "cargo test", "cargo check", "cargo build",
                "cargo clippy", "cargo fmt --check",
                "pytest", "npm test", "npm run test",
                "go test", "make test", "make check",
                "rustfmt --check", "eslint",
            ];
            let is_allowed = allowed.iter().any(|prefix| cmd.starts_with(prefix));
            if !is_allowed {
                eprintln!("[sandbox] host_exec_command blocked: command not whitelisted: {}", cmd);
                return 0;
            }

            // Execute command with timeout
            let output = std::process::Command::new("sh")
                .arg("-c").arg(&cmd)
                .output();

            match output {
                Ok(out) => {
                    let combined = [out.stdout, out.stderr].concat();
                    pack_to_wasm(&mut caller, &combined)
                }
                Err(e) => {
                    let err = format!("exec error: {}", e);
                    pack_to_wasm(&mut caller, err.as_bytes())
                }
            }
        },
    )?;

    // host_llm_call(prompt_ptr, prompt_len, schema_ptr, schema_len) -> (resp_ptr << 32) | resp_len
    linker.func_wrap("env", "host_llm_call",
        |mut caller: Caller<'_, SandboxHost>,
         prompt_ptr: i32, prompt_len: i32,
         schema_ptr: i32, schema_len: i32| -> i64 {
            // Phase 1: read prompt and schema from WASM memory
            let (prompt, schema) = {
                let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m, None => return 0,
                };
                let mut p = vec![0u8; prompt_len as usize];
                let mut s = vec![0u8; schema_len as usize];
                if mem.read(&caller, prompt_ptr as usize, &mut p).is_err() { return 0; }
                if mem.read(&caller, schema_ptr as usize, &mut s).is_err() { return 0; }
                (
                    String::from_utf8_lossy(&p).into_owned(),
                    String::from_utf8_lossy(&s).into_owned(),
                )
            };
            // mem dropped here

            // Phase 2: call LLM (no WASM memory borrow held)
            let config = caller.data().llm_config.clone();
            let result = match config {
                Some(ref cfg) => call_deepseek_api(cfg, &prompt, &schema),
                None => {
                    eprintln!("[sandbox] No LLM config — returning stub");
                    Ok(r#"{"status":"ok","note":"LLM unavailable"}"#.to_string())
                }
            };

            // Phase 3: write result back to WASM memory
            match result {
                Ok(response) => pack_to_wasm(&mut caller, response.as_bytes()),
                Err(e) => {
                    eprintln!("[sandbox] LLM error: {}", e);
                    0
                }
            }
        },
    )?;

    Ok(())
}

/// 同步调用 DeepSeek API（使用 reqwest blocking）
fn call_deepseek_api(config: &config::LlmConfig, prompt: &str, schema: &str) -> Result<String, String> {
    let api_key = if config.api_key.is_empty() {
        std::env::var("PAPOROT_API_KEY").unwrap_or_default()
    } else {
        config.api_key.clone()
    };

    if api_key.is_empty() {
        return Err("No API key configured. Set PAPOROT_API_KEY or add [llm] to .Paporot/config.toml".into());
    }

    let endpoint = if config.endpoint.is_empty() {
        "https://api.deepseek.com/v1/chat/completions"
    } else {
        &config.endpoint
    };

    let full_prompt = format!(
        "{}\n\nRespond ONLY with valid JSON matching this schema (no markdown, no explanation):\n{}",
        prompt, schema
    );

    let body = serde_json::json!({
        "model": config.model,
        "messages": [
            {"role": "user", "content": full_prompt}
        ],
        "temperature": config.temperature,
        "max_tokens": config.max_tokens,
    });

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(120))
        .json(&body)
        .send()
        .map_err(|e| format!("HTTP error: {}", e))?;

    let status = resp.status();
    let raw_body = resp
        .text()
        .map_err(|e| format!("Read response error: {}", e))?;

    let json: serde_json::Value = serde_json::from_str(&raw_body)
        .map_err(|e| format!("Parse error (HTTP {}): {} — body preview: {}", status.as_u16(), e,
            if raw_body.len() > 200 { format!("{}...", &raw_body[..200]) } else { raw_body.clone() }))?;

    // Check for API-level error first
    if let Some(err) = json["error"]["message"].as_str() {
        return Err(format!("LLM API error: {}", err));
    }

    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| {
            eprintln!("[sandbox] LLM unexpected response: {}", json);
            "Missing content in LLM response".to_string()
        })?;

    // Strip markdown code fences and extract JSON
    let cleaned = extract_json_from_text(content);

    // If extraction found valid JSON, return cleaned version
    if serde_json::from_str::<serde_json::Value>(&cleaned).is_ok() {
        return Ok(cleaned);
    }

    // Try harder: find JSON object/array boundaries
    if let Some(start) = cleaned.find('{').or_else(|| cleaned.find('[')) {
        let end_char = if cleaned.as_bytes()[start] == b'{' { '}' } else { ']' };
        if let Some(end) = cleaned.rfind(end_char) {
            let extracted = cleaned[start..=end].to_string();
            if serde_json::from_str::<serde_json::Value>(&extracted).is_ok() {
                return Ok(extracted);
            }
        }
    }

    // Last resort: return the cleaned content as-is, let caller handle
    Ok(cleaned)
}

/// Extract JSON from text wrapped in markdown fences or other prose
fn extract_json_from_text(text: &str) -> String {
    let text = text.trim();
    // Remove markdown code fences: ```json or ``` at start, ``` at end
    let text = text.trim_start_matches("```json");
    let text = text.trim_start_matches("```");
    let text = text.trim();
    let text = text.trim_end_matches("```");
    text.trim().to_string()
}
