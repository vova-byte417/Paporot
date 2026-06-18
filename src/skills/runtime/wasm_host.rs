//! WASM Host —— 通过 wasmtime 加载并执行 skill.wasm
//!
//! 采用"共享内存预注入"模型：
//! 1. 实例化 WASM 后，将所有 inputs 写入 WASM 线性内存
//! 2. 记录每个 key 的 (ptr, len)
//! 3. paporot_read_input(key) 直接返回预计算偏移

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use wasmtime::*;

use super::super::types::{SkillError, SkillRunResult, SkillRunStatus};
use super::host_bridge::{LlmBridge, SkillCache};
use crate::config::LlmConfig;

// ─── 执行上下文（通过 Store 传递） ──────────────────────────────────

/// WASM 执行期间的可变状态
#[derive(Default)]
struct ExecutionState {
    /// key → (ptr, len) WASM 线性内存偏移
    input_table: HashMap<String, (i32, i32)>,
    output_buf: Vec<u8>,
    error_buf: Vec<u8>,
    llm_bridge: Option<LlmBridge>,
    cache: SkillCache,
}

// ─── WASM Host ──────────────────────────────────────────────────────

pub struct WasmHost {
    engine: Engine,
    llm_config: Option<LlmConfig>,
}

impl WasmHost {
    pub fn new() -> Result<Self> {
        let mut config = Config::default();
        config.wasm_memory64(false);
        config.wasm_multi_memory(false);

        let engine = Engine::new(&config)
            .context("Failed to create wasmtime engine")?;

        Ok(Self {
            engine,
            llm_config: None,
        })
    }

    /// 设置 LLM 配置（可选，有则启用 llm_complete）
    pub fn with_llm(mut self, config: LlmConfig) -> Self {
        self.llm_config = Some(config);
        self
    }

    pub fn validate(&self, wasm_path: &Path) -> Result<()> {
        if !wasm_path.exists() {
            anyhow::bail!("WASM file not found: {:?}", wasm_path);
        }
        let wasm_bytes = std::fs::read(wasm_path)
            .with_context(|| format!("Failed to read WASM: {:?}", wasm_path))?;

        Module::from_binary(&self.engine, &wasm_bytes)
            .context("Invalid WASM module")?;

        Ok(())
    }

    /// 执行 Skill
    pub fn execute(
        &self,
        skill_name: &str,
        wasm_path: &Path,
        _timeout_secs: u32,
        input_data: &HashMap<String, Vec<u8>>,
    ) -> SkillRunResult {
        let start = std::time::Instant::now();

        let result = self.execute_internal(wasm_path, input_data);
        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(state) => {
                let output_str = String::from_utf8_lossy(&state.output_buf).to_string();
                if !state.error_buf.is_empty() {
                    let err_str = String::from_utf8_lossy(&state.error_buf).to_string();
                    SkillRunResult {
                        skill_name: skill_name.to_string(),
                        status: SkillRunStatus::Failed,
                        duration_ms,
                        output_json: None,
                        error: Some(SkillError {
                            phase: "wasm_execute".into(),
                            error_code: "skill_returned_error".into(),
                            detail: err_str,
                            suggestion: Some("Check skill.wasm logic".into()),
                        }),
                    }
                } else {
                    SkillRunResult {
                        skill_name: skill_name.to_string(),
                        status: SkillRunStatus::Ok,
                        duration_ms,
                        output_json: Some(output_str),
                        error: None,
                    }
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                let (status, error_code, suggestion) = if err_str.contains("timeout") {
                    (SkillRunStatus::TimedOut, "timeout", "Increase timeout_secs in skill.toml")
                } else if err_str.contains("memory") || err_str.contains("out of bounds") {
                    (SkillRunStatus::Failed, "memory_oob", "Check skill memory usage")
                } else if err_str.contains("trap") || err_str.contains("unreachable") {
                    (SkillRunStatus::Failed, "wasm_trap", "Check skill.wasm integrity")
                } else if err_str.contains("missing required export") {
                    (SkillRunStatus::Failed, "wasm_missing_export", "Skill must export paporot_skill_execute")
                } else {
                    (SkillRunStatus::Failed, "wasm_error", "Check wasmtime logs")
                };

                SkillRunResult {
                    skill_name: skill_name.to_string(),
                    status,
                    duration_ms,
                    output_json: None,
                    error: Some(SkillError {
                        phase: "wasm_execute".into(),
                        error_code: error_code.into(),
                        detail: err_str,
                        suggestion: Some(suggestion.into()),
                    }),
                }
            }
        }
    }

    fn execute_internal(
        &self,
        wasm_path: &Path,
        input_data: &HashMap<String, Vec<u8>>,
    ) -> Result<ExecutionState> {
        let wasm_bytes =
            std::fs::read(wasm_path).context("Failed to read WASM module")?;

        let module =
            Module::from_binary(&self.engine, &wasm_bytes).context("Invalid WASM")?;

        let mut linker = Linker::new(&self.engine);

        // 注册 host functions（使用 Mutex<ExecutionState> 传递可变状态）
        let enable_llm = self.llm_config.is_some();
        register_host_functions(&mut linker, enable_llm)?;

        let state = Mutex::new(ExecutionState::default());
        if let Some(ref llm_config) = self.llm_config {
            state.lock().unwrap().llm_bridge = Some(LlmBridge::new(llm_config.clone()));
        }
        let mut store = Store::new(&self.engine, state);

        // 实例化
        let instance = linker
            .instantiate(&mut store, &module)
            .context("Failed to instantiate WASM module")?;

        // ── 预注入 inputs 到 WASM 线性内存 ──
        let memory = instance
            .get_memory(&mut store, "memory")
            .context("WASM module missing memory export")?;

        // 先写入所有 input 数据到线性内存末尾
        let mut input_table: HashMap<String, (i32, i32)> = HashMap::new();
        let mut offset = memory.data_size(&store);

        for (key, data) in input_data {
            // 对齐到 8 字节
            let aligned = (offset + 7) & !7;
            offset = aligned;

            memory
                .write(&mut store, offset, data)
                .with_context(|| format!("Failed to write input '{}' to WASM memory", key))?;

            input_table.insert(key.clone(), (offset as i32, data.len() as i32));
            offset += data.len();
        }

        // 将预计算偏移表写入 ExecutionState
        {
            let mut s = store.data_mut().lock().unwrap();
            s.input_table = input_table;
        }

        // ── 调用 paporot_skill_execute() ──
        let execute_fn = instance
            .get_typed_func::<(), i32>(&mut store, "paporot_skill_execute")
            .context("WASM module missing required export: paporot_skill_execute")?;

        let exit_code = execute_fn
            .call(&mut store, ())
            .context("WASM execution trapped")?;

        let state = store.into_data().into_inner().unwrap();

        if exit_code != 0 {
            let err = String::from_utf8_lossy(&state.error_buf).to_string();
            if !err.is_empty() {
                anyhow::bail!("Skill exited with code {}: {}", exit_code, err);
            } else {
                anyhow::bail!("Skill exited with code {}", exit_code);
            }
        }

        Ok(state)
    }
}

// ─── Host Function 注册 ─────────────────────────────────────────────

fn register_host_functions(linker: &mut Linker<Mutex<ExecutionState>>, enable_llm: bool) -> Result<()> {
    register_read_input(linker)?;
    register_output_write(linker)?;
    register_error_write(linker)?;
    register_log(linker)?;
    register_cache_put(linker)?;
    register_cache_get(linker)?;

    if enable_llm {
        register_llm_complete(linker)?;
    } else {
        // 注册一个返回错误的 stub
        register_llm_stub(linker)?;
    }

    Ok(())
}

/// paporot_read_input(key_ptr, key_len) -> i64
///
/// 从预注入表返回 (ptr << 32) | len
/// ptr 和 len 指向 WASM 线性内存中已写好的数据
fn register_read_input(linker: &mut Linker<Mutex<ExecutionState>>) -> Result<()> {
    linker
        .func_wrap(
            "env",
            "paporot_read_input",
            |mut caller: Caller<'_, Mutex<ExecutionState>>, key_ptr: i32, key_len: i32| -> i64 {
                let mem = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return 0,
                };

                let mut key_bytes = vec![0u8; key_len as usize];
                if mem.read(&caller, key_ptr as usize, &mut key_bytes).is_err() {
                    return 0;
                }
                let key = String::from_utf8_lossy(&key_bytes).to_string();

                let state = caller.data().lock().unwrap();
                if let Some((ptr, len)) = state.input_table.get(&key) {
                    ((*ptr as i64) << 32) | (*len as i64)
                } else {
                    0
                }
            },
        )
        .context("Failed to register paporot_read_input")?;

    Ok(())
}

/// paporot_output_write(ptr, len)
///
/// Skill 调用此函数将输出数据追加到 output_buf
fn register_output_write(linker: &mut Linker<Mutex<ExecutionState>>) -> Result<()> {
    linker
        .func_wrap(
            "env",
            "paporot_output_write",
            |mut caller: Caller<'_, Mutex<ExecutionState>>, ptr: i32, len: i32| {
                let mem = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return,
                };

                let mut data = vec![0u8; len as usize];
                if mem.read(&caller, ptr as usize, &mut data).is_err() {
                    return;
                }

                let mut state = caller.data().lock().unwrap();
                state.output_buf.extend_from_slice(&data);
            },
        )
        .context("Failed to register paporot_output_write")?;

    Ok(())
}

/// paporot_error_write(ptr, len)
///
/// Skill 调用此函数写入错误信息
fn register_error_write(linker: &mut Linker<Mutex<ExecutionState>>) -> Result<()> {
    linker
        .func_wrap(
            "env",
            "paporot_error_write",
            |mut caller: Caller<'_, Mutex<ExecutionState>>, ptr: i32, len: i32| {
                let mem = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return,
                };

                let mut data = vec![0u8; len as usize];
                if mem.read(&caller, ptr as usize, &mut data).is_err() {
                    return;
                }

                let mut state = caller.data().lock().unwrap();
                state.error_buf.extend_from_slice(&data);
            },
        )
        .context("Failed to register paporot_error_write")?;

    Ok(())
}

/// paporot_log(level, msg_ptr, msg_len)
fn register_log(linker: &mut Linker<Mutex<ExecutionState>>) -> Result<()> {
    linker
        .func_wrap(
            "env",
            "paporot_log",
            |mut caller: Caller<'_, Mutex<ExecutionState>>, level: i32, msg_ptr: i32, msg_len: i32| {
                let mem = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return,
                };

                let mut msg_bytes = vec![0u8; msg_len as usize];
                if mem.read(&caller, msg_ptr as usize, &mut msg_bytes).is_err() {
                    return;
                }
                let msg = String::from_utf8_lossy(&msg_bytes);
                let level_str = match level {
                    0 => "DEBUG",
                    1 => "INFO",
                    2 => "WARN",
                    3 => "ERROR",
                    _ => "UNKNOWN",
                };
                eprintln!("  [skill:{}] {}", level_str, msg);
            },
        )
        .context("Failed to register paporot_log")?;

    Ok(())
}

/// paporot_llm_complete(prompt_ptr, prompt_len, schema_ptr, schema_len) -> i64
///
/// 同步调用 LLM，返回打包的 (response_ptr << 32) | response_len
fn register_llm_complete(linker: &mut Linker<Mutex<ExecutionState>>) -> Result<()> {
    linker
        .func_wrap(
            "env",
            "paporot_llm_complete",
            |mut caller: Caller<'_, Mutex<ExecutionState>>,
             prompt_ptr: i32, prompt_len: i32,
             schema_ptr: i32, schema_len: i32|
             -> i64 {
                let mem = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return encode_error("memory not found"),
                };

                // 读取 prompt
                let mut prompt_bytes = vec![0u8; prompt_len as usize];
                if mem.read(&caller, prompt_ptr as usize, &mut prompt_bytes).is_err() {
                    return encode_error("failed to read prompt");
                }
                let prompt = String::from_utf8_lossy(&prompt_bytes).to_string();

                // 读取 schema
                let mut schema_bytes = vec![0u8; schema_len as usize];
                if mem.read(&caller, schema_ptr as usize, &mut schema_bytes).is_err() {
                    return encode_error("failed to read schema");
                }
                let schema = String::from_utf8_lossy(&schema_bytes).to_string();

                // 调用 LLM（通过 ExecutionState 中的 LlmBridge）
                let mut state = caller.data().lock().unwrap();
                let result = if let Some(ref mut bridge) = state.llm_bridge {
                    bridge.complete_sync(&prompt, &schema)
                } else {
                    r#"{"error": "LLM not configured"}"#.to_string()
                };
                drop(state);

                // 将结果写回 WASM 内存
                let result_bytes = result.as_bytes();
                let data_ptr = mem.data_size(&caller) as i32;
                if mem.write(&mut caller, data_ptr as usize, result_bytes).is_err() {
                    return encode_error("failed to write llm result");
                }

                ((data_ptr as i64) << 32) | (result_bytes.len() as i64)
            },
        )
        .context("Failed to register paporot_llm_complete")?;

    Ok(())
}

/// llm_complete stub —— 当没有配置 LLM 时返回错误
fn register_llm_stub(linker: &mut Linker<Mutex<ExecutionState>>) -> Result<()> {
    linker
        .func_wrap(
            "env",
            "paporot_llm_complete",
            |mut caller: Caller<'_, Mutex<ExecutionState>>,
             _prompt_ptr: i32, _prompt_len: i32,
             _schema_ptr: i32, _schema_len: i32|
             -> i64 {
                let err_msg = r#"{"error": "LLM not configured. Set api_key in .Paporot/config.toml"}"#;
                let bytes = err_msg.as_bytes();
                let mem = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return 0,
                };
                let data_ptr = mem.data_size(&caller) as i32;
                let _ = mem.write(&mut caller, data_ptr as usize, bytes);
                ((data_ptr as i64) << 32) | (bytes.len() as i64)
            },
        )
        .context("Failed to register llm stub")?;

    Ok(())
}

/// paporot_cache_put(key_ptr, key_len, val_ptr, val_len)
fn register_cache_put(linker: &mut Linker<Mutex<ExecutionState>>) -> Result<()> {
    linker
        .func_wrap(
            "env",
            "paporot_cache_put",
            |mut caller: Caller<'_, Mutex<ExecutionState>>,
             key_ptr: i32, key_len: i32,
             val_ptr: i32, val_len: i32| {
                let mem = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return,
                };

                let mut key_bytes = vec![0u8; key_len as usize];
                if mem.read(&caller, key_ptr as usize, &mut key_bytes).is_err() {
                    return;
                }
                let key = String::from_utf8_lossy(&key_bytes).to_string();

                let mut val_bytes = vec![0u8; val_len as usize];
                if mem.read(&caller, val_ptr as usize, &mut val_bytes).is_err() {
                    return;
                }

                let mut state = caller.data().lock().unwrap();
                state.cache.put(&key, val_bytes);
            },
        )
        .context("Failed to register paporot_cache_put")?;

    Ok(())
}

/// paporot_cache_get(key_ptr, key_len) -> i64
fn register_cache_get(linker: &mut Linker<Mutex<ExecutionState>>) -> Result<()> {
    linker
        .func_wrap(
            "env",
            "paporot_cache_get",
            |mut caller: Caller<'_, Mutex<ExecutionState>>,
             key_ptr: i32, key_len: i32|
             -> i64 {
                let mem = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return 0,
                };

                let mut key_bytes = vec![0u8; key_len as usize];
                if mem.read(&caller, key_ptr as usize, &mut key_bytes).is_err() {
                    return 0;
                }
                let key = String::from_utf8_lossy(&key_bytes).to_string();

                // Clone data before releasing lock
                let data: Option<Vec<u8>> = {
                    let state = caller.data().lock().unwrap();
                    state.cache.get(&key).cloned()
                };

                if let Some(data) = data {
                    let data_ptr = mem.data_size(&caller) as i32;
                    let data_len = data.len() as i32;
                    if mem.write(&mut caller, data_ptr as usize, &data).is_err() {
                        return 0;
                    }
                    ((data_ptr as i64) << 32) | (data_len as i64)
                } else {
                    0
                }
            },
        )
        .context("Failed to register paporot_cache_get")?;

    Ok(())
}

/// 将错误信息编码为返回值的辅助函数
fn encode_error(msg: &str) -> i64 {
    let bytes = msg.as_bytes();
    // 使用负数 ptr 表示错误
    -(bytes.len() as i64)
}

// ─── 测试 ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_host_creation() {
        let host = WasmHost::new();
        assert!(host.is_ok());
    }

    #[test]
    fn test_validate_missing_file() {
        let host = WasmHost::new().unwrap();
        let result = host.validate(Path::new("nonexistent.wasm"));
        assert!(result.is_err());
    }
}
