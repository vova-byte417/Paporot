//! Host Function FFI 绑定
//!
//! 这些函数由 native loader（wasmtime）以 `env` 模块提供。

#[link(wasm_import_module = "env")]
extern "C" {
    /// 读取文件内容
    /// path_ptr, path_len -> (data_ptr << 32) | data_len  (0 = 失败)
    pub fn host_read_file(path_ptr: *const u8, path_len: i32) -> i64;

    /// 写入文件内容
    /// path_ptr, path_len, data_ptr, data_len -> errno (0 = 成功)
    pub fn host_write_file(path_ptr: *const u8, path_len: i32, data_ptr: *const u8, data_len: i32) -> i32;

    /// LLM 调用
    /// prompt_ptr, prompt_len, schema_ptr, schema_len -> (resp_ptr << 32) | resp_len
    pub fn host_llm_call(
        prompt_ptr: *const u8, prompt_len: i32,
        schema_ptr: *const u8, schema_len: i32,
    ) -> i64;

    /// Contract 验证
    /// type_ptr, type_len, content_ptr, content_len -> (result_ptr << 32) | result_len
    pub fn host_verify_contract(
        type_ptr: *const u8, type_len: i32,
        content_ptr: *const u8, content_len: i32,
    ) -> i64;

    /// 收集 Evidence
    /// id_ptr, id_len, input_ptr, input_len, output_ptr, output_len, intermediate_ptr, intermediate_len -> errno
    pub fn host_capture_evidence(
        id_ptr: *const u8, id_len: i32,
        input_ptr: *const u8, input_len: i32,
        output_ptr: *const u8, output_len: i32,
        intermediate_ptr: *const u8, intermediate_len: i32,
    ) -> i32;

    /// 保存 Replay Case
    /// case_ptr, case_len -> errno
    pub fn host_save_replay_case(case_ptr: *const u8, case_len: i32) -> i32;

    /// 加载所有 Replay Case
    /// -> (cases_ptr << 32) | cases_len
    pub fn host_load_replay_cases() -> i64;
}

// ─── Wrappers ─────────────────────────────────────────────────────

fn unpack(packed: i64) -> (*const u8, usize) {
    if packed == 0 {
        return (std::ptr::null(), 0);
    }
    let ptr = (packed >> 32) as *const u8;
    let len = (packed & 0xFFFF_FFFF) as usize;
    (ptr, len)
}

/// 读取文件（文本）
pub fn read_file(path: &str) -> Option<String> {
    let bytes = read_file_bytes(path)?;
    String::from_utf8(bytes).ok()
}

/// 读取文件（二进制）
pub fn read_file_bytes(path: &str) -> Option<Vec<u8>> {
    let path_bytes = path.as_bytes();
    let packed = unsafe { host_read_file(path_bytes.as_ptr(), path_bytes.len() as i32) };
    let (ptr, len) = unpack(packed);
    if ptr.is_null() || len == 0 {
        return None;
    }
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
    Some(data.to_vec())
}

pub fn write_file(path: &str, content: &str) -> Result<(), i32> {
    let path_bytes = path.as_bytes();
    let data_bytes = content.as_bytes();
    let errno = unsafe {
        host_write_file(
            path_bytes.as_ptr(), path_bytes.len() as i32,
            data_bytes.as_ptr(), data_bytes.len() as i32,
        )
    };
    if errno == 0 {
        Ok(())
    } else {
        Err(errno)
    }
}

pub fn llm_call(prompt: &str, schema: &str) -> Option<String> {
    let prompt_bytes = prompt.as_bytes();
    let schema_bytes = schema.as_bytes();
    let packed = unsafe {
        host_llm_call(
            prompt_bytes.as_ptr(), prompt_bytes.len() as i32,
            schema_bytes.as_ptr(), schema_bytes.len() as i32,
        )
    };
    let (ptr, len) = unpack(packed);
    if ptr.is_null() || len == 0 {
        return None;
    }
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8(data.to_vec()).ok()
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
    let (ptr, len) = unpack(packed);
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
    let (ptr, len) = unpack(packed);
    if ptr.is_null() || len == 0 {
        return Some("[]".to_string());
    }
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8(data.to_vec()).ok()
}
