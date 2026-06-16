//! Claude Code Trace Adapter
//!
//! 将 Claude Code session 日志 / API response 转换为 Paporot BehaviorTrace。
//!
//! # 支持格式
//!
//! 1. Session Log: JSON 对象包含 "messages" 数组（每个 message 有 type/tool_use）
//! 2. JSONL 格式: 每行一个 Claude API response 对象

use crate::trace::adapter::TraceAdapter;
use crate::trace::error::TraceError;
use crate::trace::types::{BehaviorTrace, Observation, TokenUsage, ToolCall, TraceSource};

use super::claude_types::*;

/// Claude Code 适配器
pub struct ClaudeAdapter;

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl TraceAdapter for ClaudeAdapter {
    fn name(&self) -> &str {
        "claude-code"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn can_handle(&self, raw: &str) -> bool {
        let head = &raw[..raw.len().min(4096)];
        // Session Log 格式: 包含 "messages" + "type":"assistant" / "tool_use"
        if head.contains("\"messages\"") {
            return head.contains("\"type\"");
        }
        // JSONL 格式: 首行是 Claude API response
        if let Some(first_line) = head.lines().next() {
            return first_line.contains("\"type\":\"assistant\"")
                && (first_line.contains("\"tool_use\"") || first_line.contains("\"content\""));
        }
        false
    }

    fn parse(
        &self,
        raw: &str,
        file_path: &str,
    ) -> Result<Vec<BehaviorTrace>, TraceError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        // Session Log 格式: 包含 "messages"
        if trimmed.starts_with('{') && trimmed.contains("\"messages\"") {
            return self.parse_session_log(trimmed, file_path);
        }

        // JSONL 格式: 每行一条 message
        if let Some(first_line) = trimmed.lines().next() {
            if first_line.contains("\"type\":\"assistant\"") {
                return self.parse_jsonl(trimmed, file_path);
            }
        }

        Err(TraceError::ParseError {
            message: "Unrecognized Claude format".into(),
            adapter: self.name().into(),
        })
    }

    fn description(&self) -> &str {
        "Parses Claude Code session logs and API responses into Paporot BehaviorTraces"
    }
}

impl ClaudeAdapter {
    /// 解析 Session Log 格式。
    fn parse_session_log(
        &self,
        raw: &str,
        file_path: &str,
    ) -> Result<Vec<BehaviorTrace>, TraceError> {
        let log: ClaudeSessionLog =
            serde_json::from_str(raw).map_err(|e| TraceError::ParseError {
                message: format!("Failed to parse Claude session log: {}", e),
                adapter: self.name().into(),
            })?;

        let mut tool_calls = Vec::new();
        let mut observations = Vec::new();
        let mut final_output_parts = Vec::new();
        let mut prompt = String::new();
        let mut call_idx = 0u32;
        let session_id = format!("claude-session-{}", uuid::Uuid::new_v4());

        for msg in &log.messages {
            match msg.msg_type.as_str() {
                "user" => {
                    if let Some(ref text) = msg.text {
                        if prompt.is_empty() {
                            prompt = text.clone();
                        }
                    }
                }
                "assistant" => {
                    if let Some(ref blocks) = msg.content {
                        for block in blocks {
                            match block.block_type.as_str() {
                                "text" => {
                                    if let Some(ref text) = block.text {
                                        final_output_parts.push(text.clone());
                                    }
                                }
                                "tool_use" => {
                                    call_idx += 1;
                                    let call_id = format!("call_{}_{:03}", session_id, call_idx);
                                    let obs_id = format!("obs_{}_{:03}", session_id, call_idx);

                                    tool_calls.push(ToolCall {
                                        id: call_id.clone(),
                                        tool_name: block.name.clone().unwrap_or_default(),
                                        args: block.input.clone().unwrap_or(serde_json::Value::Null),
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
                                _ => {}
                            }
                        }
                    }
                }
                "tool_result" => {
                    // tool_result 通常跟随 tool_use，这里简单记录
                }
                _ => {}
            }
        }

        let trace = BehaviorTrace {
            id: String::new(),
            session_id,
            prompt,
            tool_calls,
            observations,
            final_output: final_output_parts.join("\n"),
            token_usage: TokenUsage::default(),
            started_at: chrono::Utc::now().to_rfc3339(),
            finished_at: chrono::Utc::now().to_rfc3339(),
            source: TraceSource::Imported {
                adapter: self.name().into(),
                adapter_version: self.version().into(),
                file_path: file_path.to_string(),
            },
            tags: Vec::new(),
            capability_ids: Vec::new(),
            deleted: false,
        };

        Ok(vec![trace])
    }

    /// 解析 JSONL 格式。
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
            match serde_json::from_str::<ClaudeApiResponse>(line) {
                Ok(resp) => {
                    if resp.msg_type == "assistant" {
                        let mut tool_calls = Vec::new();
                        let mut observations = Vec::new();
                        let mut final_output = String::new();
                        let session_id = resp
                            .id
                            .as_ref()
                            .cloned()
                            .unwrap_or_else(|| format!("claude-msg-{}", line_no));
                        let mut call_idx = 0u32;

                        if let Some(ref blocks) = resp.content {
                            for block in blocks {
                                match block.block_type.as_str() {
                                    "text" => {
                                        if let Some(ref text) = block.text {
                                            if final_output.is_empty() {
                                                final_output = text.clone();
                                            } else {
                                                final_output.push_str("\n");
                                                final_output.push_str(text);
                                            }
                                        }
                                    }
                                    "tool_use" => {
                                        call_idx += 1;
                                        let call_id =
                                            format!("call_{}_{:03}", session_id, call_idx);
                                        let obs_id =
                                            format!("obs_{}_{:03}", session_id, call_idx);

                                        tool_calls.push(ToolCall {
                                            id: call_id.clone(),
                                            tool_name: block.name.clone().unwrap_or_default(),
                                            args: block
                                                .input
                                                .clone()
                                                .unwrap_or(serde_json::Value::Null),
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
                                    _ => {}
                                }
                            }
                        }

                        traces.push(BehaviorTrace {
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
                                    .map(|u| u.input_tokens)
                                    .unwrap_or(0),
                                output_tokens: resp
                                    .usage
                                    .as_ref()
                                    .map(|u| u.output_tokens)
                                    .unwrap_or(0),
                                cache_read_tokens: None,
                                cache_write_tokens: None,
                            },
                            started_at: chrono::Utc::now().to_rfc3339(),
                            finished_at: chrono::Utc::now().to_rfc3339(),
                            source: TraceSource::Imported {
                                adapter: self.name().into(),
                                adapter_version: self.version().into(),
                                file_path: file_path.to_string(),
                            },
                            tags: Vec::new(),
                            capability_ids: Vec::new(),
                            deleted: false,
                        });
                    }
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
                    "Failed to parse all {} lines: {}",
                    skipped,
                    skip_reasons.join("; ")
                ),
                adapter: self.name().into(),
            });
        }

        Ok(traces)
    }
}

// ─── inventory 注册 ────────────────────────────────────────────

inventory::submit! {
    crate::trace::adapter_registry::AdapterEntry {
        name: "claude-code",
        factory: || Box::new(ClaudeAdapter::new()),
    }
}

// ─── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_handle_session_log() {
        let adapter = ClaudeAdapter::new();
        let sample =
            r#"{"messages":[{"type":"user","text":"fix bug"},{"type":"assistant","content":[{"type":"tool_use","id":"t1","name":"read"}]}]}"#;
        assert!(
            adapter.can_handle(sample),
            "Should recognize Claude session log format"
        );
    }

    #[test]
    fn test_can_handle_jsonl_assistant_message() {
        let adapter = ClaudeAdapter::new();
        let sample = r#"{"type":"assistant","content":[{"type":"text","text":"done"}]}"#;
        assert!(
            adapter.can_handle(sample),
            "Should recognize Claude JSONL assistant message"
        );
    }

    #[test]
    fn test_can_handle_unknown_format() {
        let adapter = ClaudeAdapter::new();
        assert!(
            !adapter.can_handle("random text"),
            "Should reject unknown format"
        );
    }

    #[test]
    fn test_can_handle_empty() {
        let adapter = ClaudeAdapter::new();
        assert!(!adapter.can_handle(""), "Should reject empty input");
    }

    #[test]
    fn test_parse_session_log() {
        let adapter = ClaudeAdapter::new();
        let sample = r#"{"messages":[
            {"type":"user","text":"fix the login bug"},
            {"type":"assistant","content":[
                {"type":"tool_use","id":"t1","name":"read","input":{"file_path":"src/auth.rs","limit":50}},
                {"type":"tool_use","id":"t2","name":"grep","input":{"pattern":"login"}}
            ]}
        ]}"#;

        let result = adapter.parse(sample, "test.json").unwrap();
        assert_eq!(result.len(), 1, "Should produce one trace from session log");
        let trace = &result[0];
        assert_eq!(trace.prompt, "fix the login bug");
        assert_eq!(trace.tool_calls.len(), 2, "Should have 2 tool calls");
        assert_eq!(trace.tool_calls[0].tool_name, "read");
        assert_eq!(trace.tool_calls[1].tool_name, "grep");
    }

    #[test]
    fn test_parse_jsonl_format() {
        let adapter = ClaudeAdapter::new();
        let sample = r#"{"id":"msg_001","type":"assistant","content":[{"type":"text","text":"Hello!"}],"usage":{"input_tokens":10,"output_tokens":5}}"#;

        let result = adapter.parse(sample, "test.jsonl").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].final_output, "Hello!");
        assert_eq!(result[0].token_usage.input_tokens, 10);
        assert_eq!(result[0].token_usage.output_tokens, 5);
    }

    #[test]
    fn test_parse_jsonl_with_tool_use() {
        let adapter = ClaudeAdapter::new();
        let sample = r#"{"id":"msg_002","type":"assistant","content":[{"type":"tool_use","id":"call_1","name":"write","input":{"file_path":"test.rs","content":"fn main() {}"}}],"usage":{"input_tokens":50,"output_tokens":20}}"#;

        let result = adapter.parse(sample, "test.jsonl").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].tool_calls.len(), 1);
        assert_eq!(result[0].tool_calls[0].tool_name, "write");
    }

    #[test]
    fn test_parse_empty() {
        let adapter = ClaudeAdapter::new();
        let result = adapter.parse("", "test.json").unwrap();
        assert!(result.is_empty(), "Empty input should produce empty result");
    }

    #[test]
    fn test_parse_session_log_multiple_messages() {
        let adapter = ClaudeAdapter::new();
        let sample = r#"{"messages":[
            {"type":"user","text":"fix bug"},
            {"type":"assistant","content":[{"type":"tool_use","id":"t1","name":"read","input":{}}]},
            {"type":"assistant","content":[{"type":"text","text":"I found the issue. Let me fix it."}]}
        ]}"#;

        let result = adapter.parse(sample, "test.json").unwrap();
        assert_eq!(result.len(), 1);
        let trace = &result[0];
        assert!(trace.final_output.contains("I found the issue"));
    }
}
