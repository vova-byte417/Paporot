//! Host Bridge —— LLM 和缓存的跨边界桥接
//!
//! 为 WASM Skill 提供 LLM 调用和中间结果缓存能力。

use anyhow::{Context, Result};
use std::collections::HashMap;
use crate::config::LlmConfig;

/// LLM 调用记录
#[derive(Debug, Clone)]
pub struct LlmCallRecord {
    pub prompt: String,
    pub schema: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub attempts: u32,
}

/// 跨边界的缓存（在 WASM 执行周期内有效）
#[derive(Default)]
pub struct SkillCache {
    data: HashMap<String, Vec<u8>>,
}

impl SkillCache {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    pub fn put(&mut self, key: &str, value: Vec<u8>) {
        self.data.insert(key.to_string(), value);
    }

    pub fn get(&self, key: &str) -> Option<&Vec<u8>> {
        self.data.get(key)
    }
}

/// LLM 桥接器 —— 同步调用 DeepSeek API
pub struct LlmBridge {
    config: LlmConfig,
    cache: Vec<LlmCallRecord>,
}

impl LlmBridge {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            config,
            cache: Vec::new(),
        }
    }

    /// 同步调用 LLM（从 WASM host function 中调用）
    ///
    /// 使用 reqwest::blocking 在 host function 线程中同步执行。
    pub fn complete_sync(&mut self, prompt: &str, output_schema: &str) -> String {
        // 构建完整的 prompt（含 schema 约束）
        let full_prompt = format!(
            "{}\n\nYou MUST respond with valid JSON matching this schema. Output ONLY the JSON, no markdown, no explanation:\n{}",
            prompt, output_schema
        );

        let mut last_error = String::new();

        for attempt in 1..=self.config.max_retries {
            match self.try_complete(&full_prompt) {
                Ok(text) => {
                    // 校验 JSON 格式
                    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&text) {
                        let formatted = serde_json::to_string(&json_val).unwrap_or_else(|_| text.clone());
                        self.cache.push(LlmCallRecord {
                            prompt: prompt.to_string(),
                            schema: output_schema.to_string(),
                            result: Some(formatted.clone()),
                            error: None,
                            attempts: attempt,
                        });
                        return formatted;
                    }
                    // JSON 解析失败，重试
                    last_error = format!("LLM response is not valid JSON (attempt {})", attempt);
                    if attempt < self.config.max_retries {
                        // 在重试 prompt 中追加 schema 提示
                        continue;
                    }
                }
                Err(e) => {
                    last_error = format!("LLM call failed (attempt {}): {}", attempt, e);
                    if attempt >= self.config.max_retries {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
        }

        self.cache.push(LlmCallRecord {
            prompt: prompt.to_string(),
            schema: output_schema.to_string(),
            result: None,
            error: Some(last_error.clone()),
            attempts: self.config.max_retries,
        });

        // 返回错误 JSON
        format!(r#"{{"error": "{}"}}"#, last_error.replace('"', "'"))
    }

    fn try_complete(&self, full_prompt: &str) -> Result<String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(self.config.timeout_secs))
            .build()
            .context("Failed to build HTTP client")?;

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {
                    "role": "user",
                    "content": full_prompt
                }
            ],
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens
        });

        let response = client
            .post(&self.config.endpoint)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .context("HTTP request failed")?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("LLM API returned HTTP {}", status);
        }

        let json: serde_json::Value = response.json().context("Failed to parse response")?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(content)
    }
}
