//! Paporot — WASM Sandbox Loader (Native Entry Point)
//!
//! 极薄的 wasmtime loader。加载 paporot-core.wasm 并通过 3 个 host function
//! 向沙盒内的分析管线提供 read_file / write_file / llm_call 能力。
//! CLI 参数通过 WASI args 透传到 .wasm 的 main()。

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use wasmtime::*;
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtxBuilder};
use wasmtime_wasi::preview1::WasiP1Ctx;

mod config;
mod verification;

/// Native command handler type
type CmdFn = fn(&[String], &Path) -> Result<()>;

/// v3: 构建 feedback index 并写入 .Paporot/work/feedback_index.json
fn build_and_write_feedback_index(paporot_dir: &Path) -> Result<()> {
    use paporot::feedback_loop::feedback_loader;
    use paporot::storage::SnapshotStorage;

    let snapshots_dir = paporot_dir.join("snapshots");
    if !snapshots_dir.exists() {
        eprintln!("[feedback] No snapshots directory, skipping");
        return Ok(());
    }

    let store = SnapshotStorage::new(&snapshots_dir);
    let snapshot = match store.load_latest() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[feedback] No snapshot to load: {:?}", e);
            return Ok(());
        }
    };

    eprintln!("[feedback] Loaded snapshot {}", snapshot.version_id);
    let (index, result) = feedback_loader::build_feedback_index(paporot_dir, &snapshot)?;
    feedback_loader::write_feedback_index(paporot_dir, &index)?;

    if result.rejected_count > 0 || result.suppression_count > 0 {
        eprintln!(
            "Feedback index: {} reviews ({} rejected), {} rule suppressions, {} file prefixes",
            result.total_reviews, result.rejected_count,
            result.suppression_count, result.prefix_count,
        );
    }

    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let paporot_dir = find_paporot_dir()?;

    // ── Native command registry ──
    let native_commands: HashMap<&str, CmdFn> = HashMap::from([
        ("init", cmd_init as CmdFn),
        ("feedback", cmd_feedback as CmdFn),
        ("trace", cmd_native_stub as CmdFn),
        ("trajectory", cmd_native_stub as CmdFn),
    ]);

    if args.len() >= 2 {
        if let Some(handler) = native_commands.get(args[1].as_str()) {
            let rest: Vec<String> = args[2..].to_vec();
            return handler(&rest, &paporot_dir);
        }
    }

    // ── WASM fallback (analyze, skill, snapshot, diff, coverage, ...) ──

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
    collect_git_diff(project_root, &paporot_dir)?;

    // ── v3: Build feedback index for analyze ──
    if args.len() >= 2 && args[1] == "analyze" {
        if let Err(e) = build_and_write_feedback_index(&paporot_dir) {
            eprintln!("Warning: Failed to build feedback index: {}", e);
        }
    }

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
        Ok(()) => {
            // ── Post-processing: inject dashboard JSON into Vue HTML template ──
            inject_dashboard_html(&paporot_dir);
            std::process::exit(0);
        }
        Err(e) => {
            // WASI proc_exit is a trap with i32 exit code
            if let Some(exit_code) = e.downcast_ref::<wasmtime_wasi::I32Exit>() {
                inject_dashboard_html(&paporot_dir);
                std::process::exit(exit_code.0);
            }
            // Otherwise it's a real error
            eprintln!("Fatal error: {:?}", e);
            std::process::exit(1);
        }
    }
}

/// Reads dashboard_data.json written by the WASM pipeline and injects it
/// into the pre-built Vue 3 SPA template to produce dashboard.html.
fn inject_dashboard_html(paporot_dir: &PathBuf) {
    let dash_data_path = paporot_dir.join("reports").join("dashboard_data.json");
    let dash_html_path = paporot_dir.join("reports").join("dashboard.html");

    // Read the dashboard_data.json
    let json_str = match std::fs::read_to_string(&dash_data_path) {
        Ok(s) => s,
        Err(_) => return, // No data to inject
    };

    // Validate that it's valid JSON
    if serde_json::from_str::<serde_json::Value>(&json_str).is_err() {
        eprintln!("Warning: dashboard_data.json is not valid JSON, skipping HTML generation");
        return;
    }

    // Embed Vue 3 runtime + HTML template at compile time (no CDN dependency)
    let vue3_js = include_str!("../paporot-dashboard/vue3.prod.js");
    let template = include_str!("../paporot-dashboard/dashboard_template.html");

    // Inject Vue 3 inline
    let html = template.replace(
        "/* VUE3_INLINE */",
        vue3_js,
    );

    // Inject JSON data into the template
    let html = html.replace(
        "window.__PAPOROT_DATA__ = undefined;",
        &format!("window.__PAPOROT_DATA__ = {};", json_str),
    );

    match std::fs::write(&dash_html_path, &html) {
        Ok(_) => eprintln!("[native] dashboard.html generated ({:.1}KB)", html.len() as f64 / 1024.0),
        Err(e) => eprintln!("Warning: Failed to write dashboard.html: {}", e),
    }
}

// ─── SandboxHost ─────────────────────────────────────────────────

struct SandboxHost {
    wasi: WasiP1Ctx,
    llm_config: Option<config::LlmConfig>,
    paporot_dir: PathBuf,
    evidence_buffer: verification::evidence::EvidenceBuffer,
}

impl SandboxHost {
    fn new(
        wasi: WasiP1Ctx,
        llm_config: Option<config::LlmConfig>,
        paporot_dir: PathBuf,
    ) -> Self {
        Self {
            wasi,
            llm_config,
            paporot_dir,
            evidence_buffer: verification::evidence::EvidenceBuffer::new(),
        }
    }
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
fn cmd_init(_args: &[String], _paporot_dir: &Path) -> Result<()> {
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

/// Native command stub — placeholder until subcommand is fully implemented
fn cmd_native_stub(args: &[String], _paporot_dir: &Path) -> Result<()> {
    let cmd = if args.is_empty() { "unknown" } else { &args[0] };
    println!("Native command '{}' — not yet implemented (Phase 4)", cmd);
    Ok(())
}

/// `Paporot feedback` — human-in-the-loop review via TOML
fn cmd_feedback(args: &[String], paporot_dir: &Path) -> Result<()> {
    use paporot::commands::feedback;
    use paporot::storage::SnapshotStorage;
    use paporot::types::*;

    let reviews_dir = paporot_dir.join("reviews");
    let snapshots_dir = paporot_dir.join("snapshots");

    if args.is_empty() || args[0] == "help" {
        println!("Paporot Feedback — Human Review System");
        println!();
        println!("Subcommands:");
        println!("  feedback generate <version>  Generate review_<version>.toml");
        println!("  feedback apply <toml_path>   Apply TOML review back to snapshot");
        println!("  feedback stats               Show review statistics");
        return Ok(());
    }

    match args[0].as_str() {
        "generate" => {
            let version = if args.len() > 1 { args[1].as_str() } else {
                anyhow::bail!("Usage: Paporot feedback generate <version_id>");
            };
            let store = SnapshotStorage::new(&snapshots_dir);
            let snapshot = store.load_by_version(version)?;
            feedback::generate_review_toml(&snapshot, &reviews_dir)?;
        }
        "apply" => {
            let toml_path = if args.len() > 1 { &args[1] } else {
                anyhow::bail!("Usage: Paporot feedback apply <toml_file>");
            };
            let store = SnapshotStorage::new(&snapshots_dir);
            let mut snapshot = store.load_latest()?;
            let review_store = feedback::apply_review_toml(
                &mut snapshot,
                std::path::Path::new(toml_path),
                &std::env::var("USER").unwrap_or_else(|_| "unknown".into()),
            )?;
            store.save(&snapshot)?;
            review_store.save(&reviews_dir.join("reviews.json"))?;
            println!("\n  Snapshot {} updated with {} reviews.",
                snapshot.version_id, review_store.stats.total_reviews);
        }
        "stats" => {
            let review_path = reviews_dir.join("reviews.json");
            let fb = FeedbackStore::load_or_new(&review_path)?;
            let total = fb.stats.total_reviews;
            println!("Paporot Feedback Statistics");
            println!("===========================");
            println!("  Total reviews  : {}", total);
            if total > 0 {
                println!("  Approved       : {} ({:.1}%)", fb.stats.approved,
                    100.0 * fb.stats.approved as f32 / total as f32);
                println!("  Rejected       : {} ({:.1}%)", fb.stats.rejected,
                    100.0 * fb.stats.rejected as f32 / total as f32);
                println!("  Corrected      : {} ({:.1}%)", fb.stats.corrected,
                    100.0 * fb.stats.corrected as f32 / total as f32);
                println!("  Flagged        : {} ({:.1}%)", fb.stats.flagged,
                    100.0 * fb.stats.flagged as f32 / total as f32);
            }
        }
        _ => {
            anyhow::bail!("Unknown feedback subcommand: {}. Try: generate, apply, stats", args[0]);
        }
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

/// Collect git diff from project history into .Paporot/work/diff.txt
///
/// Falls back from broader ranges to narrower to ensure meaningful diff.
fn collect_git_diff(project_root: &Path, paporot_dir: &Path) -> Result<()> {
    let work = paporot_dir.join("work");
    fs::create_dir_all(&work)?;

    let ranges = ["HEAD~10..HEAD", "HEAD~5..HEAD", "HEAD~1..HEAD", "HEAD"];
    let diff = ranges.iter()
        .find_map(|r| collect_single_diff(project_root, r))
        .unwrap_or_default();

    fs::write(work.join("diff.txt"), &diff)?;
    if diff.is_empty() {
        eprintln!("[loader] Git diff: no diff generated (empty repo or no git?)");
    } else {
        eprintln!("[loader] Git diff collected: {} bytes", diff.len());
    }
    Ok(())
}

fn collect_single_diff(project_root: &Path, range: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["diff", range, "-U3", "--", "."])
        .current_dir(project_root)
        .output()
        .ok()?;

    if output.status.success() && !output.stdout.is_empty() {
        let diff = String::from_utf8_lossy(&output.stdout).to_string();
        eprintln!("[loader] git diff {}: {} bytes", range, diff.len());
        Some(diff)
    } else {
        None
    }
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

    // host_verify_contract(type_ptr, type_len, content_ptr, content_len) -> packed_result
    linker.func_wrap("env", "host_verify_contract",
        |mut caller: Caller<'_, SandboxHost>,
         type_ptr: i32, type_len: i32,
         content_ptr: i32, content_len: i32| -> i64 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 0,
            };
            let mut t = vec![0u8; type_len as usize];
            let mut c = vec![0u8; content_len as usize];
            if mem.read(&caller, type_ptr as usize, &mut t).is_err() { return 0; }
            if mem.read(&caller, content_ptr as usize, &mut c).is_err() { return 0; }
            let artifact_type = String::from_utf8_lossy(&t).into_owned();
            let artifact_content = String::from_utf8_lossy(&c).into_owned();

            let dir = &caller.data().paporot_dir;
            let contract_path = dir.join("contracts").join(format!("{}.contract.yaml", artifact_type));
            let contract_yaml = std::fs::read_to_string(&contract_path).unwrap_or_default();

            let result = verification::contract::verify_artifact(
                &artifact_type, &artifact_type, &artifact_content, &contract_yaml,
            );
            match result {
                Ok(verification_result) => {
                    let json = serde_json::to_string(&verification_result).unwrap_or_default();
                    pack_to_wasm(&mut caller, json.as_bytes())
                }
                Err(e) => {
                    let err_json = format!(r#"{{"status":"ERROR","error":"{}"}}"#, e);
                    pack_to_wasm(&mut caller, err_json.as_bytes())
                }
            }
        },
    )?;

    // host_capture_evidence(id_ptr, id_len, input_ptr, input_len, output_ptr, output_len, intermediate_ptr, intermediate_len) -> errno
    linker.func_wrap("env", "host_capture_evidence",
        |mut caller: Caller<'_, SandboxHost>,
         id_ptr: i32, id_len: i32,
         input_ptr: i32, input_len: i32,
         output_ptr: i32, output_len: i32,
         intermediate_ptr: i32, intermediate_len: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 1,
            };
            let mut id_buf = vec![0u8; id_len as usize];
            let mut in_buf = vec![0u8; input_len as usize];
            let mut out_buf = vec![0u8; output_len as usize];
            let mut int_buf = vec![0u8; intermediate_len as usize];
            if mem.read(&caller, id_ptr as usize, &mut id_buf).is_err() { return 1; }
            if mem.read(&caller, input_ptr as usize, &mut in_buf).is_err() { return 1; }
            if mem.read(&caller, output_ptr as usize, &mut out_buf).is_err() { return 1; }
            if mem.read(&caller, intermediate_ptr as usize, &mut int_buf).is_err() { return 1; }

            let host = caller.data_mut();
            host.evidence_buffer.capture(
                &String::from_utf8_lossy(&id_buf),
                &String::from_utf8_lossy(&in_buf),
                &String::from_utf8_lossy(&out_buf),
                &String::from_utf8_lossy(&int_buf),
            );
            0
        },
    )?;

    // host_save_replay_case(case_ptr, case_len) -> errno
    linker.func_wrap("env", "host_save_replay_case",
        |mut caller: Caller<'_, SandboxHost>,
         case_ptr: i32, case_len: i32| -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m, None => return 1,
            };
            let mut buf = vec![0u8; case_len as usize];
            if mem.read(&caller, case_ptr as usize, &mut buf).is_err() { return 1; }
            let case_json = String::from_utf8_lossy(&buf).into_owned();
            let case: verification::types::ReplayCase = match serde_json::from_str(&case_json) {
                Ok(c) => c,
                Err(_) => return 1,
            };
            let dir = caller.data().paporot_dir.clone();
            match verification::evidence::save_replay_case(&case, &dir) {
                Ok(()) => 0,
                Err(_) => 1,
            }
        },
    )?;

    // host_load_replay_cases() -> packed_json
    linker.func_wrap("env", "host_load_replay_cases",
        |mut caller: Caller<'_, SandboxHost>| -> i64 {
            let dir = caller.data().paporot_dir.clone();
            let cases = verification::evidence::load_replay_cases(&dir);
            let json = serde_json::to_string(&cases).unwrap_or_else(|_| "[]".into());
            pack_to_wasm(&mut caller, json.as_bytes())
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
