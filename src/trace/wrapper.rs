//! Wrapper 实时捕获模式 + SDK API
//!
//! # 子进程模式
//!
//! `paporot trace record -- agent-cli "fix this bug"`
//! Paporot 启动 Agent 作为子进程，捕获 stdout 并解析为 BehaviorTrace。
//!
//! # SDK API
//!
//! 通过 `TraceRecorder` 在代码中手动记录每次 tool 调用，
//! 适用于有精确控制需求的场景。

use crate::trace::adapter;
use crate::trace::error::TraceError;
use crate::trace::storage::TraceStorage;
use crate::trace::types::{BehaviorTrace, Observation, TokenUsage, ToolCall, TraceSource};

/// Wrapper 配置。
#[derive(Debug, Clone)]
pub struct WrapperConfig {
    /// Agent CLI 命令（如 ["claude", "fix this bug"]）
    pub agent_command: Vec<String>,
    /// 输出格式: "deepseek" | "claude-code" | "openai" | "auto"
    pub output_format: String,
    /// 可选的要关联的 Capability ID
    pub capability_id: Option<String>,
    /// 标签
    pub tags: Vec<String>,
}

impl Default for WrapperConfig {
    fn default() -> Self {
        Self {
            agent_command: vec!["echo".into(), r#"{"id":"test","choices":[{"message":{"role":"assistant","content":"ok"}}]}"#.into()],
            output_format: "auto".into(),
            capability_id: None,
            tags: Vec::new(),
        }
    }
}

/// 子进程 wrapper：启动 Agent，捕获 stdout，解析为 BehaviorTrace。
///
/// # 流程
///
/// 1. 解析 command
/// 2. spawn 子进程，捕获 stdout
/// 3. 收集所有输出行
/// 4. 用适配器解析并保存
pub fn run_wrapper(
    storage: &TraceStorage,
    config: &WrapperConfig,
) -> Result<BehaviorTrace, TraceError> {
    use std::process::Command;

    if config.agent_command.is_empty() {
        return Err(TraceError::Io {
            message: "Agent command is empty".into(),
        });
    }

    let program = &config.agent_command[0];
    let mut cmd = Command::new(program);
    if config.agent_command.len() > 1 {
        cmd.args(&config.agent_command[1..]);
    }

    let output = cmd.output().map_err(|e| TraceError::Io {
        message: format!("Failed to run agent command '{}': {}", program, e),
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if stdout.trim().is_empty() {
        return Err(TraceError::Io {
            message: format!(
                "Agent '{}' produced no stdout. Stderr: {}",
                program, stderr
            ),
        });
    }

    // 选择适配器
    let adapter: Box<dyn adapter::TraceAdapter> =
        if config.output_format == "auto" {
            adapter::auto_detect(&stdout).ok_or_else(|| TraceError::ParseError {
                message: format!(
                    "Cannot auto-detect output format for '{}'. Specify --format",
                    program
                ),
                adapter: "auto".into(),
            })?
        } else {
            adapter::find_adapter(&config.output_format).ok_or_else(|| {
                TraceError::ParseError {
                    message: format!("Unknown format: {}", config.output_format),
                    adapter: config.output_format.clone(),
                }
            })?
        };

    let mut traces = adapter.parse(&stdout, "stdout_capture")?;

    if let Some(mut trace) = traces.pop() {
        // 标记为 captured 来源
        trace.source = TraceSource::Captured {
            agent_name: program.to_string(),
        };
        if let Some(ref cap_id) = config.capability_id {
            trace.capability_ids.push(cap_id.clone());
        }
        trace.tags = config.tags.clone();

        storage.save(&trace)?;
        Ok(trace)
    } else {
        Err(TraceError::ParseError {
            message: "No trace parsed from agent stdout".into(),
            adapter: adapter.name().into(),
        })
    }
}

// ─── SDK API: TraceRecorder ────────────────────────────────────

/// SDK 风格 API：手动记录 tool 调用前后。
///
/// 适用于有精确控制需求的用户，在 Agent 代码中显式调用。
///
/// # 示例
///
/// ```ignore
/// let storage = TraceStorage::new(".Paporot");
/// storage.init()?;
///
/// let mut recorder = TraceRecorder::start("sess-001", "fix the bug");
/// let call_id = recorder.record_tool_call("grep", json!({"pattern": "login"}));
/// recorder.record_observation(&call_id, "found in src/auth.rs");
/// recorder.finish(&storage, "Fixed!")?;
/// ```
pub struct TraceRecorder {
    trace: BehaviorTrace,
}

impl TraceRecorder {
    /// 开始记录一次 Agent 执行。
    pub fn start(session_id: &str, prompt: &str) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            trace: BehaviorTrace {
                id: String::new(),
                session_id: session_id.to_string(),
                prompt: prompt.to_string(),
                tool_calls: Vec::new(),
                observations: Vec::new(),
                final_output: String::new(),
                token_usage: TokenUsage::default(),
                started_at: now,
                finished_at: String::new(),
                source: TraceSource::Captured {
                    agent_name: "sdk".into(),
                },
                tags: Vec::new(),
                capability_ids: Vec::new(),
                deleted: false,
            },
        }
    }

    /// 记录一次 tool 调用。
    ///
    /// 返回生成的 call_id，用于后续 `record_observation` 关联。
    pub fn record_tool_call(
        &mut self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> String {
        let idx = self.trace.tool_calls.len() + 1;
        let call_id = format!("call_{}_{:03}", self.trace.session_id, idx);
        self.trace.tool_calls.push(ToolCall {
            id: call_id.clone(),
            tool_name: tool_name.to_string(),
            args,
            timestamp: chrono::Utc::now().to_rfc3339(),
            duration_ms: 0,
            result_id: None,
        });
        call_id
    }

    /// 记录 tool 调用的返回结果。
    pub fn record_observation(&mut self, call_id: &str, content: &str) {
        let idx = self.trace.observations.len() + 1;
        let obs_id = format!("obs_{}_{:03}", self.trace.session_id, idx);
        self.trace.observations.push(Observation {
            id: obs_id,
            tool_call_id: call_id.to_string(),
            content: content.to_string(),
            truncated: false,
            truncated_at_bytes: None,
        });
    }

    /// 结束记录、更新 token 用量并持久化。
    pub fn finish(
        mut self,
        storage: &TraceStorage,
        final_output: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Result<BehaviorTrace, TraceError> {
        self.trace.finished_at = chrono::Utc::now().to_rfc3339();
        self.trace.final_output = final_output.to_string();
        self.trace.token_usage = TokenUsage {
            input_tokens,
            output_tokens,
            cache_read_tokens: None,
            cache_write_tokens: None,
        };

        storage.save(&self.trace)?;
        Ok(self.trace)
    }

    /// 获取当前 trace 的只读引用（调试用）。
    pub fn current_trace(&self) -> &BehaviorTrace {
        &self.trace
    }

    /// 获取已记录的 tool 调用数。
    pub fn tool_call_count(&self) -> usize {
        self.trace.tool_calls.len()
    }

    /// 设置标签。
    pub fn add_tag(&mut self, tag: &str) {
        self.trace.tags.push(tag.to_string());
    }

    /// 关联 Capability。
    pub fn link_capability(&mut self, cap_id: &str) {
        if !self.trace.capability_ids.contains(&cap_id.to_string()) {
            self.trace.capability_ids.push(cap_id.to_string());
        }
    }
}

// ─── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::types::TraceFilter;
    use tempfile::TempDir;

    fn setup_storage() -> (TraceStorage, TempDir) {
        let tmp = TempDir::new().unwrap();
        let storage = TraceStorage::new(tmp.path().join(".Paporot"));
        storage.init().unwrap();
        (storage, tmp)
    }

    /// 测试：子进程模式运行简单命令。
    /// 使用 echo 模拟 Agent 输出 DeepSeek 格式 JSON。
    #[test]
    fn test_run_wrapper_with_echo_json() {
        let (storage, _tmp) = setup_storage();

        let config = WrapperConfig {
            agent_command: vec![
                "echo".into(),
                r#"{"id":"chatcmpl-001","choices":[{"message":{"role":"assistant","content":"ok"}}]}"#.into(),
            ],
            output_format: "deepseek".into(),
            capability_id: Some("cap_001".into()),
            tags: vec!["wrapper-test".into()],
        };

        let trace = run_wrapper(&storage, &config).unwrap();

        assert_eq!(trace.final_output, "ok");
        assert!(trace.capability_ids.contains(&"cap_001".to_string()));
        assert!(trace.tags.contains(&"wrapper-test".to_string()));

        // 验证 trace 已存储
        let list = storage.list(&TraceFilter::default()).unwrap();
        assert_eq!(list.len(), 1);
    }

    /// 测试：子进程模式 auto-detect。
    #[test]
    fn test_run_wrapper_auto_detect() {
        let (storage, _tmp) = setup_storage();

        let config = WrapperConfig {
            agent_command: vec![
                "echo".into(),
                r#"{"id":"chatcmpl-002","choices":[{"message":{"role":"assistant","content":"done"}}]}"#.into(),
            ],
            output_format: "auto".into(),
            ..Default::default()
        };

        let trace = run_wrapper(&storage, &config).unwrap();
        assert_eq!(trace.final_output, "done");
    }

    /// 测试：子进程模式不存在的命令。
    #[test]
    fn test_run_wrapper_command_not_found() {
        let (storage, _tmp) = setup_storage();

        let config = WrapperConfig {
            agent_command: vec!["nonexistent_command_xyz".into()],
            ..Default::default()
        };

        let result = run_wrapper(&storage, &config);
        assert!(result.is_err(), "Should fail for nonexistent command");
    }

    /// 测试：子进程模式空命令。
    #[test]
    fn test_run_wrapper_empty_command() {
        let (storage, _tmp) = setup_storage();

        let config = WrapperConfig {
            agent_command: vec![],
            ..Default::default()
        };

        let result = run_wrapper(&storage, &config);
        assert!(result.is_err(), "Should fail for empty command");
    }

    // ─── SDK API 测试 ─────────────────────────────────────────

    /// 测试：TraceRecorder.start → record_tool_call → finish 完整流程。
    #[test]
    fn test_trace_recorder_lifecycle() {
        let (storage, _tmp) = setup_storage();

        let mut recorder = TraceRecorder::start("sess-test-001", "fix the login bug");

        assert_eq!(recorder.tool_call_count(), 0);
        assert_eq!(recorder.current_trace().prompt, "fix the login bug");

        let call_id = recorder.record_tool_call("grep", serde_json::json!({"pattern": "login"}));
        assert!(!call_id.is_empty());
        assert!(call_id.starts_with("call_sess-test-001_"));
        assert_eq!(recorder.tool_call_count(), 1);

        recorder.record_observation(&call_id, "found in src/auth.rs:42");
        assert_eq!(recorder.current_trace().observations.len(), 1);
        assert_eq!(
            recorder.current_trace().observations[0].tool_call_id,
            call_id
        );

        let call_id2 = recorder.record_tool_call("read", serde_json::json!({"file_path": "src/auth.rs"}));
        recorder.record_observation(&call_id2, "fn login(email, password) { ... }");

        assert_eq!(recorder.tool_call_count(), 2);

        let trace = recorder.finish(&storage, "bug fixed", 100, 50).unwrap();

        assert_eq!(trace.final_output, "bug fixed");
        assert_eq!(trace.token_usage.input_tokens, 100);
        assert_eq!(trace.token_usage.output_tokens, 50);
        assert_eq!(trace.tool_calls.len(), 2);
        assert_eq!(trace.observations.len(), 2);
        assert!(!trace.started_at.is_empty());
        assert!(!trace.finished_at.is_empty());

        // 验证持久化
        let list = storage.list(&TraceFilter::default()).unwrap();
        assert_eq!(list.len(), 1);
    }

    /// 测试：TraceRecorder 标签和 Capability 关联。
    #[test]
    fn test_trace_recorder_tags_and_caps() {
        let (storage, _tmp) = setup_storage();

        let mut recorder = TraceRecorder::start("sess-tags-001", "test");
        recorder.add_tag("production");
        recorder.add_tag("security");
        recorder.link_capability("cap_auth");
        recorder.link_capability("cap_auth"); // 去重

        let trace = recorder.finish(&storage, "done", 10, 5).unwrap();

        assert_eq!(trace.tags.len(), 2);
        assert!(trace.tags.contains(&"production".to_string()));
        assert!(trace.tags.contains(&"security".to_string()));
        assert_eq!(trace.capability_ids.len(), 1); // 去重
        assert!(trace.capability_ids.contains(&"cap_auth".to_string()));
    }

    /// 测试：TraceRecorder.source 为 Captured。
    #[test]
    fn test_trace_recorder_source_is_captured() {
        let (storage, _tmp) = setup_storage();

        let recorder = TraceRecorder::start("sess-src-001", "test");
        let trace = recorder.finish(&storage, "ok", 5, 5).unwrap();

        match &trace.source {
            TraceSource::Captured { agent_name } => {
                assert_eq!(agent_name, "sdk");
            }
            _ => panic!("Source should be Captured, got {:?}", trace.source),
        }
    }

    /// 测试：WrapperConfig 默认值。
    #[test]
    fn test_wrapper_config_default() {
        let config = WrapperConfig::default();
        assert_eq!(config.output_format, "auto");
        assert!(config.capability_id.is_none());
        assert!(config.tags.is_empty());
        assert!(!config.agent_command.is_empty());
    }
}
