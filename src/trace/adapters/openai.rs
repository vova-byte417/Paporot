//! OpenAI Trace Adapter
//!
//! 将 OpenAI Chat Completion response 转换为 Paporot BehaviorTrace。
//!
//! # 支持格式
//!
//! 1. JSONL 格式: 每行一个 OpenAI Chat Completion response 对象

use crate::trace::adapter::TraceAdapter;
use crate::trace::error::TraceError;
use crate::trace::types::{BehaviorTrace, Observation, TokenUsage, ToolCall, TraceSource};

use super::openai_types::*;

/// OpenAI 适配器
pub struct OpenAiAdapter;

impl OpenAiAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl TraceAdapter for OpenAiAdapter {
    fn name(&self) -> &str {
        "openai"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn can_handle(&self, raw: &str) -> bool {
        let head = &raw[..raw.len().min(4096)];
        if let Some(first_line) = head.lines().next() {
            first_line.contains("\"object\":\"chat.completion\"")
                && first_line.contains("\"choices\"")
        } else {
            false
        }
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

        let mut traces = Vec::new();
        let mut skipped = 0u32;
        let mut skip_reasons = Vec::new();

        for (line_no, line) in trimmed.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<OpenAiResponse>(line) {
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
                    "Failed to parse all {} lines: {}",
                    skipped,
                    skip_reasons.join("; ")
                ),
                adapter: self.name().into(),
            });
        }

        Ok(traces)
    }

    fn description(&self) -> &str {
        "Parses OpenAI Chat Completion responses into Paporot BehaviorTraces"
    }
}

impl OpenAiAdapter {
    fn response_to_trace(
        &self,
        resp: &OpenAiResponse,
        file_path: &str,
    ) -> BehaviorTrace {
        let session_id = resp.id.clone();
        let mut tool_calls = Vec::new();
        let mut observations = Vec::new();
        let mut final_output_parts = Vec::new();
        let mut call_idx = 0u32;

        for choice in &resp.choices {
            let msg = &choice.message;

            if let Some(ref content) = msg.content {
                if !content.is_empty() {
                    final_output_parts.push(content.clone());
                }
            }

            if let Some(ref oa_tool_calls) = msg.tool_calls {
                for oa_call in oa_tool_calls {
                    call_idx += 1;

                    let args: serde_json::Value =
                        serde_json::from_str(&oa_call.function.arguments).unwrap_or_else(|_| {
                            serde_json::Value::String(oa_call.function.arguments.clone())
                        });

                    let call_id = format!("call_{}_{:03}", session_id, call_idx);
                    let obs_id = format!("obs_{}_{:03}", session_id, call_idx);

                    tool_calls.push(ToolCall {
                        id: call_id.clone(),
                        tool_name: oa_call.function.name.clone(),
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

        let timestamp = if let Some(ts) = resp.created {
            chrono::DateTime::from_timestamp(ts as i64, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| chrono::Utc::now().to_rfc3339())
        } else {
            chrono::Utc::now().to_rfc3339()
        };

        BehaviorTrace {
            id: String::new(),
            session_id,
            prompt: String::new(),
            tool_calls,
            observations,
            final_output: final_output_parts.join("\n"),
            token_usage: TokenUsage {
                input_tokens: resp.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
                output_tokens: resp.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
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

// ─── inventory 注册 ────────────────────────────────────────────

inventory::submit! {
    crate::trace::adapter_registry::AdapterEntry {
        name: "openai",
        factory: || Box::new(OpenAiAdapter::new()),
    }
}

// ─── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_handle_openai_response() {
        let adapter = OpenAiAdapter::new();
        let sample = r#"{"id":"chatcmpl-001","object":"chat.completion","choices":[{"message":{"role":"assistant","content":"Hello!"}}]}"#;
        assert!(
            adapter.can_handle(sample),
            "Should recognize OpenAI response format"
        );
    }

    #[test]
    fn test_can_handle_unknown_format() {
        let adapter = OpenAiAdapter::new();
        assert!(
            !adapter.can_handle("random text"),
            "Should reject unknown format"
        );
    }

    #[test]
    fn test_can_handle_empty() {
        let adapter = OpenAiAdapter::new();
        assert!(!adapter.can_handle(""), "Should reject empty input");
    }

    #[test]
    fn test_can_handle_deepseek_format() {
        let adapter = OpenAiAdapter::new();
        // DeepSeek response should NOT be handled by OpenAI adapter
        let sample = r#"{"id":"chatcmpl-001","choices":[{"message":{"role":"assistant","content":"Hello!"}}]}"#;
        assert!(
            !adapter.can_handle(sample),
            "Should NOT recognize DeepSeek format (no object:chat.completion)"
        );
    }

    #[test]
    fn test_parse_single_response() {
        let adapter = OpenAiAdapter::new();
        let sample = r#"{"id":"chatcmpl-001","object":"chat.completion","choices":[{"message":{"role":"assistant","content":"Hello!"}}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#;

        let result = adapter.parse(sample, "test.jsonl").unwrap();
        assert_eq!(result.len(), 1);
        let trace = &result[0];
        assert_eq!(trace.session_id, "chatcmpl-001");
        assert_eq!(trace.final_output, "Hello!");
        assert_eq!(trace.token_usage.input_tokens, 10);
        assert_eq!(trace.token_usage.output_tokens, 5);
    }

    #[test]
    fn test_parse_with_tool_calls() {
        let adapter = OpenAiAdapter::new();
        let sample = r#"{"id":"chatcmpl-002","choices":[{"message":{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"grep","arguments":"{\"pattern\":\"login\"}"}}]}}],"usage":{"prompt_tokens":20,"completion_tokens":10}}"#;

        let result = adapter.parse(sample, "test.jsonl").unwrap();
        assert_eq!(result.len(), 1);
        let trace = &result[0];
        assert_eq!(trace.tool_calls.len(), 1);
        assert_eq!(trace.tool_calls[0].tool_name, "grep");
        assert_eq!(trace.tool_calls[0].args, serde_json::json!({"pattern": "login"}));
    }

    #[test]
    fn test_parse_empty() {
        let adapter = OpenAiAdapter::new();
        let result = adapter.parse("", "test.jsonl").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_multiple_lines() {
        let adapter = OpenAiAdapter::new();
        let sample = concat!(
            r#"{"id":"chatcmpl-001","object":"chat.completion","choices":[{"message":{"role":"assistant","content":"A"}}]}"#,
            "\n",
            r#"{"id":"chatcmpl-002","object":"chat.completion","choices":[{"message":{"role":"assistant","content":"B"}}]}"#
        );

        let result = adapter.parse(sample, "test.jsonl").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].final_output, "A");
        assert_eq!(result[1].final_output, "B");
    }
}
