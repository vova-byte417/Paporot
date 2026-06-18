//! Paporot — WASM Sandbox Loader (Native Entry Point)
//!
//! 极薄的 wasmtime loader。加载 paporot-core.wasm 并通过 3 个 host function
//! 向沙盒内的分析管线提供 read_file / write_file / llm_call 能力。
//! CLI 参数通过 WASI args 透传到 .wasm 的 main()。

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use wasmtime::*;
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtxBuilder};
use wasmtime_wasi::preview1::WasiP1Ctx;

mod config;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let paporot_dir = find_paporot_dir()?;
    let wasm_path = paporot_dir.join("bin").join("paporot-core.wasm");

    let wasm_path = if wasm_path.exists() {
        wasm_path
    } else {
        let alt = PathBuf::from("crates/paporot-core/target/wasm32-wasip1/release/paporot-core.wasm");
        if alt.exists() { alt } else {
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
    let host = SandboxHost::new(wasi_ctx, llm_config, paporot_dir);
    let mut store = Store::new(&engine, host);

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
}

impl SandboxHost {
    fn new(
        wasi: WasiP1Ctx,
        llm_config: Option<config::LlmConfig>,
        paporot_dir: PathBuf,
    ) -> Self {
        Self { wasi, llm_config, paporot_dir }
    }
}

fn load_llm_config(dir: &std::path::Path) -> Option<config::LlmConfig> {
    let path = dir.join("config.toml");
    path.exists().then(|| ())
        .and_then(|_| fs::read_to_string(&path).ok())
        .and_then(|s| toml::from_str::<config::Config>(&s).ok())
        .map(|c| c.llm)
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
    Ok(PathBuf::from(".Paporot"))
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
        .json(&body)
        .send()
        .map_err(|e| format!("HTTP error: {}", e))?;

    let json: serde_json::Value = resp
        .json()
        .map_err(|e| format!("Parse error: {}", e))?;

    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("Missing content in LLM response")?;

    // Strip markdown code fences if present
    let cleaned = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    Ok(cleaned.to_string())
}
