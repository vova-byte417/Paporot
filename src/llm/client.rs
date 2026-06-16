//! LLM API 客户端
//!
//! 支持 Anthropic Claude 和 OpenAI 兼容接口。
//! 内置 JSON 提取 + 自动重试（最多 3 次）。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use crate::config::LlmConfig;

/// LLM 消息
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// OpenAI 兼容请求体
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
}

/// OpenAI 兼容响应体
#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: String,
}

/// LLM 客户端
#[derive(Clone)]
pub struct LlmClient {
    config: LlmConfig,
    client: reqwest::Client,
}

impl LlmClient {
    /// 创建新的 LLM 客户端
    pub fn new(config: LlmConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to build HTTP client");
        Self { config, client }
    }

    /// 发送 chat completion 请求，返回纯文本响应（会尝试提取 JSON）
    pub async fn chat(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String> {
        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: system_prompt.into(),
            },
            ChatMessage {
                role: "user".into(),
                content: user_prompt.into(),
            },
        ];

        let body = ChatRequest {
            model: self.config.model.clone(),
            messages,
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
        };

        let response = self
            .client
            .post(&self.config.endpoint)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send LLM request")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("LLM API error ({}): {}", status, text);
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .context("Failed to parse LLM response")?;

        let content = chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        Ok(extract_json_from_response(&content).unwrap_or(content))
    }

    /// 带自动重试的 chat 调用
    pub async fn chat_with_retry(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String> {
        let mut last_err = None;

        for attempt in 1..=self.config.max_retries {
            match self.chat(system_prompt, user_prompt).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    eprintln!("  [LLM] Attempt {}/{} failed: {}", attempt, self.config.max_retries, e);
                    last_err = Some(e);
                    if attempt < self.config.max_retries {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("All retries exhausted")))
    }
}

/// 从 LLM 响应中提取 JSON 内容
/// 处理可能包含 markdown code block 或多余文本的情况
pub fn extract_json_from_response(response: &str) -> Option<String> {
    let trimmed = response.trim();

    // 尝试提取 ```json ... ``` 代码块
    if let Some(start) = trimmed.find("```json") {
        let after_start = &trimmed[start + 7..];
        if let Some(end) = after_start.find("```") {
            return Some(after_start[..end].trim().to_string());
        }
    }

    // 尝试提取 ``` ... ``` 代码块
    if let Some(start) = trimmed.find("```") {
        let after_start = &trimmed[start + 3..];
        if let Some(end) = after_start.find("```") {
            let content = after_start[..end].trim();
            // 跳过可能的语言标注
            if let Some(newline) = content.find('\n') {
                return Some(content[newline + 1..].trim().to_string());
            }
            return Some(content.to_string());
        }
    }

    // 直接尝试作为 JSON 解析
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && (trimmed.ends_with('}') || trimmed.ends_with(']'))
    {
        return Some(trimmed.to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_code_block() {
        let input = "Some text\n```json\n{\"key\": \"value\"}\n```\nMore text";
        assert_eq!(extract_json_from_response(input).unwrap(), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_extract_json_plain() {
        let input = r#"{"key": "value"}"#;
        assert_eq!(extract_json_from_response(input).unwrap(), r#"{"key": "value"}"#);
    }
}
