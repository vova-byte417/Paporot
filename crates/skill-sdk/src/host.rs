//! Host Function FFI 绑定
//!
//! 这些函数由 Paporot Runtime (wasmtime) 以 `env` 模块提供。

#[link(wasm_import_module = "env")]
extern "C" {
    /// 读取预注入的输入数据
    /// 返回打包的 (ptr << 32) | len
    pub fn paporot_read_input(key_ptr: *const u8, key_len: i32) -> i64;

    /// 调用 LLM (DeepSeek)
    /// prompt: 用户输入的 prompt
    /// schema: 输出的 JSON Schema
    /// 返回打包的 (response_ptr << 32) | response_len
    pub fn paporot_llm_complete(
        prompt_ptr: *const u8, prompt_len: i32,
        schema_ptr: *const u8, schema_len: i32,
    ) -> i64;

    /// 写入输出数据
    pub fn paporot_output_write(ptr: *const u8, len: i32);

    /// 写入错误信息
    pub fn paporot_error_write(ptr: *const u8, len: i32);

    /// 缓存操作
    pub fn paporot_cache_put(key_ptr: *const u8, key_len: i32, val_ptr: *const u8, val_len: i32);
    pub fn paporot_cache_get(key_ptr: *const u8, key_len: i32) -> i64;

    /// 日志
    pub fn paporot_log(level: i32, msg_ptr: *const u8, msg_len: i32);
}

/// Unpack (ptr << 32) | len
fn unpack_result(packed: i64) -> (*const u8, usize) {
    if packed == 0 {
        return (std::ptr::null(), 0);
    }
    let ptr = (packed >> 32) as *const u8;
    let len = (packed & 0xFFFF_FFFF) as usize;
    (ptr, len)
}

/// 从 WASM 线性内存中读取字符串
pub fn read_input(key: &str) -> Option<String> {
    let key_bytes = key.as_bytes();
    let packed = unsafe { paporot_read_input(key_bytes.as_ptr(), key_bytes.len() as i32) };
    let (ptr, len) = unpack_result(packed);
    if ptr.is_null() || len == 0 {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8(slice.to_vec()).ok()
}

/// 调用 LLM
pub fn llm_complete(prompt: &str, output_schema: &str) -> Option<String> {
    let prompt_bytes = prompt.as_bytes();
    let schema_bytes = output_schema.as_bytes();
    let packed = unsafe {
        paporot_llm_complete(
            prompt_bytes.as_ptr(),
            prompt_bytes.len() as i32,
            schema_bytes.as_ptr(),
            schema_bytes.len() as i32,
        )
    };
    let (ptr, len) = unpack_result(packed);
    if ptr.is_null() || len == 0 {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8(slice.to_vec()).ok()
}

/// 写入 JSON 输出
pub fn write_output(json: &serde_json::Value) {
    let s = json.to_string();
    let bytes = s.as_bytes();
    unsafe { paporot_output_write(bytes.as_ptr(), bytes.len() as i32) };
}

/// 写入错误
pub fn write_error(msg: &str) {
    let bytes = msg.as_bytes();
    unsafe { paporot_error_write(bytes.as_ptr(), bytes.len() as i32) };
}

/// 缓存操作
pub fn cache_put(key: &str, value: &[u8]) {
    let k = key.as_bytes();
    unsafe { paporot_cache_put(k.as_ptr(), k.len() as i32, value.as_ptr(), value.len() as i32) };
}

pub fn cache_get(key: &str) -> Option<Vec<u8>> {
    let k = key.as_bytes();
    let packed = unsafe { paporot_cache_get(k.as_ptr(), k.len() as i32) };
    let (ptr, len) = unpack_result(packed);
    if ptr.is_null() || len == 0 {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    Some(slice.to_vec())
}

/// 日志
pub fn skill_log(level: i32, msg: &str) {
    let bytes = msg.as_bytes();
    unsafe { paporot_log(level, bytes.as_ptr(), bytes.len() as i32) };
}

// ─── Verification Host Functions ─────────────────────────────────

#[link(wasm_import_module = "env")]
extern "C" {
    /// Contract 验证
    /// type_ptr, type_len, content_ptr, content_len -> (result_ptr << 32) | result_len
    fn host_verify_contract(
        type_ptr: *const u8, type_len: i32,
        content_ptr: *const u8, content_len: i32,
    ) -> i64;

    /// 收集 Evidence
    /// id_ptr, id_len, input_ptr, input_len, output_ptr, output_len, intermediate_ptr, intermediate_len -> errno
    fn host_capture_evidence(
        id_ptr: *const u8, id_len: i32,
        input_ptr: *const u8, input_len: i32,
        output_ptr: *const u8, output_len: i32,
        intermediate_ptr: *const u8, intermediate_len: i32,
    ) -> i32;

    /// 保存 Replay Case
    /// case_ptr, case_len -> errno
    fn host_save_replay_case(case_ptr: *const u8, case_len: i32) -> i32;

    /// 加载所有 Replay Case
    /// -> (cases_ptr << 32) | cases_len
    fn host_load_replay_cases() -> i64;
}

/// 调用 Contract Engine 验证 artifact
pub fn verify_contract(artifact_type: &str, artifact_content: &str) -> Option<String> {
    let type_bytes = artifact_type.as_bytes();
    let content_bytes = artifact_content.as_bytes();
    let packed = unsafe {
        host_verify_contract(
            type_bytes.as_ptr(), type_bytes.len() as i32,
            content_bytes.as_ptr(), content_bytes.len() as i32,
        )
    };
    let (ptr, len) = unpack_result(packed);
    if ptr.is_null() || len == 0 {
        return None;
    }
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8(data.to_vec()).ok()
}

/// 收集 evidence（内存中，沙盒生命周期内）
pub fn capture_evidence(artifact_id: &str, input: &str, output: &str, intermediate: &str) -> i32 {
    let id = artifact_id.as_bytes();
    let inp = input.as_bytes();
    let out = output.as_bytes();
    let int = intermediate.as_bytes();
    unsafe {
        host_capture_evidence(
            id.as_ptr(), id.len() as i32,
            inp.as_ptr(), inp.len() as i32,
            out.as_ptr(), out.len() as i32,
            int.as_ptr(), int.len() as i32,
        )
    }
}

/// 保存 replay case（FAIL 时持久化）
pub fn save_replay_case(case_json: &str) -> i32 {
    let bytes = case_json.as_bytes();
    unsafe { host_save_replay_case(bytes.as_ptr(), bytes.len() as i32) }
}

/// 加载所有 replay case
pub fn load_replay_cases() -> Option<String> {
    let packed = unsafe { host_load_replay_cases() };
    let (ptr, len) = unpack_result(packed);
    if ptr.is_null() || len == 0 {
        return Some("[]".to_string());
    }
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8(data.to_vec()).ok()
}
