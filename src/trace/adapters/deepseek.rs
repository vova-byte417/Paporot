//! DeepSeek Trace Adapter
//!
//! 将 DeepSeek API response / run log 转换为 Paporot BehaviorTrace。
//!
//! # 支持格式
//!
//! 1. JSONL 格式: 每行一个 DeepSeek Chat Completion response 对象
//! 2. Run Log 格式: DeepSeek Platform 导出的 run 日志（JSON 对象，包含 turns 数组）

use crate::trace::adapter::TraceAdapter;
use crate::trace::error::TraceError;
use crate::trace::types::{BehaviorTrace, Observation, TokenUsage, ToolCall, TraceSource};

use super::deepseek_types::*;

/// DeepSeek 适配器
pub struct DeepSeekAdapter;

impl DeepSeekAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl TraceAdapter for DeepSeekAdapter {
    fn name(&self) -> &str {
        "deepseek"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn can_handle(&self, raw: &str) -> bool {
        let head = &raw[..raw.len().min(4096)];

        // JSONL 格式: 首行包含 "choices" + "id"
        if let Some(first_line) = head.lines().next() {
            if first_line.contains("\"choices\"") && first_line.contains("\"id\"") {
                return true;
            }
        }

        // Run Log 格式: 包含 "run_id" + "turns"
        if head.contains("\"run_id\"") && head.contains("\"turns\"") {
            return true;
        }

        false
    }

    fn parse(&self, raw: &str, file_path: &str) -> Result<Vec<BehaviorTrace>, TraceError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        // Run Log 格式优先检测: 包含 "run_id" + "turns"（完整 JSON 对象）
        if trimmed.contains("\"run_id\"") && trimmed.contains("\"turns\"") {
            return self.parse_run_log(trimmed, file_path);
        }

        // JSONL 格式: 首行包含 "choices" + "id"
        if let Some(first_line) = trimmed.lines().next() {
            if first_line.contains("\"choices\"") && first_line.contains("\"id\"") {
                return self.parse_jsonl(trimmed, file_path);
            }
        }

        Err(TraceError::ParseError {
            message: "Unrecognized DeepSeek format".into(),
            adapter: self.name().into(),
        })
    }

    fn description(&self) -> &str {
        "Parses DeepSeek API Chat Completion responses and Platform Run Logs"
    }
}

// ─── inventory 注册 ──────────────────────────────────────────

inventory::submit! {
    crate::trace::adapter_registry::AdapterEntry {
        name: "deepseek",
        factory: || Box::new(DeepSeekAdapter::new()),
    }
}

// ─── 私有方法 ─────────────────────────────────────────────────────

impl DeepSeekAdapter {
    fn parse_jsonl(
        &self,
        raw: &str,
        file_path: &str,
    ) -> Result<Vec<BehaviorTrace>, TraceError> {
        let mut traces = Vec::new();
        let mut skipped = 0u32;
        let mut skip_reasons = Vec::new();

        for (line_no, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<DeepSeekResponse>(line) {
                Ok(resp) => {
                    traces.push(self.response_to_trace(&resp, file_path));
                }
                Err(e) => {
                    skipped += 1;
                    skip_reasons.push(format!("Line {}: {}", line_no + 1, e));
                }
            }
        }

        if traces.is_empty() && skipped > 0 {
            return Err(TraceError::ParseError {
                message: format!(
                    "Failed to parse all {} lines. Reasons: {}",
                    skipped,
                    skip_reasons.join("; ")
                ),
                adapter: self.name().into(),
            });
        }

        if skipped > 0 {
            return Err(TraceError::PartialImport {
                imported: traces.len(),
                skipped: skipped as usize,
                reasons: skip_reasons,
            });
        }

        Ok(traces)
    }

    fn parse_run_log(
        &self,
        raw: &str,
        file_path: &str,
    ) -> Result<Vec<BehaviorTrace>, TraceError> {
        let log: DeepSeekRunLog =
            serde_json::from_str(raw).map_err(|e| TraceError::ParseError {
                message: format!("Failed to parse DeepSeek Run Log: {}", e),
                adapter: self.name().into(),
            })?;

        let traces: Vec<BehaviorTrace> = log
            .turns
            .iter()
            .map(|turn| {
                let mut trace = self.response_to_trace(&turn.response, file_path);
                trace.session_id = log.run_id.clone();
                if let Some(ref prompt) = turn.prompt {
                    trace.prompt = prompt.clone();
                }
                if let Some(ref ts) = turn.timestamp {
                    trace.started_at = ts.clone();
                    trace.finished_at = ts.clone();
                }
                trace
            })
            .collect();

        Ok(traces)
    }

    fn response_to_trace(&self, resp: &DeepSeekResponse, file_path: &str) -> BehaviorTrace {
        let mut final_output_parts = Vec::new();
        let mut tool_calls = Vec::new();
        let mut observations = Vec::new();
        let mut call_idx = 0u32;
        let session_id = resp.id.clone();

        for choice in &resp.choices {
            let msg = &choice.message;

            if let Some(ref content) = msg.content {
                if !content.is_empty() {
                    final_output_parts.push(content.clone());
                }
            }

            if let Some(ref d_tool_calls) = msg.tool_calls {
                for d_call in d_tool_calls {
                    call_idx += 1;

                    let args: serde_json::Value =
                        serde_json::from_str(&d_call.function.arguments).unwrap_or_else(
                            |_| serde_json::Value::String(d_call.function.arguments.clone()),
                        );

                    let call_id = format!("call_deepseek_{}_{:03}", session_id, call_idx);
                    let obs_id = format!("obs_deepseek_{}_{:03}", session_id, call_idx);

                    tool_calls.push(ToolCall {
                        id: call_id.clone(),
                        tool_name: d_call.function.name.clone(),
                        args,
                        timestamp: String::new(),
                        duration_ms: 0,
                        result_id: Some(obs_id.clone()),
                    });

                    observations.push(Observation {
                        id: obs_id,
                        tool_call_id: call_id,
                        content: String::new(),
                        truncated: false,
                        truncated_at_bytes: None,
                    });
                }
            }
        }

        let final_output = final_output_parts.join("\n");
        let timestamp = format_timestamp(resp.created);

        BehaviorTrace {
            id: String::new(),
            session_id,
            prompt: String::new(),
            tool_calls,
            observations,
            final_output,
            token_usage: TokenUsage {
                input_tokens: resp
                    .usage
                    .as_ref()
                    .map(|u| u.prompt_tokens)
                    .unwrap_or(0),
                output_tokens: resp
                    .usage
                    .as_ref()
                    .map(|u| u.completion_tokens)
                    .unwrap_or(0),
                cache_read_tokens: None,
                cache_write_tokens: None,
            },
            started_at: timestamp.clone(),
            finished_at: timestamp,
            source: TraceSource::Imported {
                adapter: self.name().into(),
                adapter_version: self.version().into(),
                file_path: file_path.to_string(),
            },
            tags: Vec::new(),
            capability_ids: Vec::new(),
            deleted: false,
        }
    }
}

// ─── 辅助函数 ──────────────────────────────────────────────────

fn format_timestamp(unix_secs: Option<u64>) -> String {
    match unix_secs {
        Some(secs) => {
            use chrono::TimeZone;
            chrono::Utc
                .timestamp_opt(secs as i64, 0)
                .single()
                .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or_else(|| format!("epoch:{}", secs))
        }
        None => "unknown".to_string(),
    }
}

// ─── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_handle_jsonl_format() {
        let adapter = DeepSeekAdapter::new();
        let sample = r#"{"id":"chatcmpl-123","choices":[{"message":{"role":"assistant","content":"Hello"}}]}"#;
        assert!(adapter.can_handle(sample));
    }

    #[test]
    fn test_can_handle_jsonl_with_tool_calls() {
        let adapter = DeepSeekAdapter::new();
        let sample = r#"{"id":"chatcmpl-456","choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"grep","arguments":"{\"pattern\": \"login\"}"}}]}}],"usage":{"prompt_tokens":120,"completion_tokens":45}},"created":1718000000}"#;
        assert!(adapter.can_handle(sample));
    }

    #[test]
    fn test_can_handle_run_log_format() {
        let adapter = DeepSeekAdapter::new();
        let sample = r#"{"run_id":"run-001","turns":[]}"#;
        assert!(adapter.can_handle(sample));
    }

    #[test]
    fn test_can_handle_unknown_format() {
        let adapter = DeepSeekAdapter::new();
        assert!(!adapter.can_handle("just some random text"));
        assert!(!adapter.can_handle(""));
    }

    #[test]
    fn test_parse_empty() {
        let adapter = DeepSeekAdapter::new();
        let result = adapter.parse("", "test.jsonl").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_single_jsonl() {
        let adapter = DeepSeekAdapter::new();
        let sample = r#"{"id":"chatcmpl-001","choices":[{"message":{"role":"assistant","content":"Hello!","tool_calls":[{"id":"call_1","type":"function","function":{"name":"grep","arguments":"{\"pattern\":\"login\"}"}}]}}],"usage":{"prompt_tokens":120,"completion_tokens":45,"total_tokens":165},"created":1718000000}"#;
        let result = adapter.parse(sample, "test.jsonl").unwrap();
        assert_eq!(result.len(), 1);

        let trace = &result[0];
        assert_eq!(trace.session_id, "chatcmpl-001");
        assert_eq!(trace.tool_calls.len(), 1);
        assert_eq!(trace.tool_calls[0].tool_name, "grep");
        assert_eq!(trace.token_usage.input_tokens, 120);
        assert_eq!(trace.token_usage.output_tokens, 45);
    }

    #[test]
    fn test_parse_jsonl_multiple_lines() {
        let adapter = DeepSeekAdapter::new();
        let sample = r#"{"id":"a","choices":[{"message":{"role":"assistant","content":"Hello"}}]}
{"id":"b","choices":[{"message":{"role":"assistant","content":"World"}}]}"#;
        let result = adapter.parse(sample, "test.jsonl").unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_parse_jsonl_partial_failure() {
        let adapter = DeepSeekAdapter::new();
        let sample = r#"{"id":"a","choices":[{"message":{"role":"assistant","content":"OK"}}]}
not valid json
{"id":"b","choices":[{"message":{"role":"assistant","content":"OK2"}}]}"#;
        let result = adapter.parse(sample, "test.jsonl");
        match result {
            Err(TraceError::PartialImport { imported, skipped, .. }) => {
                assert_eq!(imported, 2);
                assert_eq!(skipped, 1);
            }
            _ => panic!("Expected PartialImport error"),
        }
    }

    #[test]
    fn test_parse_run_log() {
        let adapter = DeepSeekAdapter::new();
        let sample = r#"{"run_id":"run-001","turns":[{"index":1,"prompt":"fix bug","response":{"id":"chat-a","choices":[{"message":{"role":"assistant","content":"done"}}]},"timestamp":"2026-06-12T14:00:00Z"}]}"#;
        let result = adapter.parse(sample, "test.jsonl").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].session_id, "run-001");
        assert_eq!(result[0].prompt, "fix bug");
        assert_eq!(result[0].started_at, "2026-06-12T14:00:00Z");
    }

    #[test]
    fn test_parse_unknown_format_error() {
        let adapter = DeepSeekAdapter::new();
        let result = adapter.parse("not a valid format", "test.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_format_timestamp() {
        let ts = format_timestamp(Some(1718000000));
        assert!(ts.contains("2024") || ts.contains("epoch:"));
    }
}
