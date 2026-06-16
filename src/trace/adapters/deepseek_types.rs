//! DeepSeek API 原生格式的反序列化类型。
//!
//! 只定义适配器需要用到的字段，其他字段由 serde 自动忽略。

use serde::Deserialize;

/// DeepSeek Chat Completion 单次 API 调用的完整响应。
#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekResponse {
    pub id: String,
    #[serde(default)]
    pub model: String,
    pub choices: Vec<DeepSeekChoice>,
    #[serde(default)]
    pub usage: Option<DeepSeekUsage>,
    pub created: Option<u64>,
}

/// DeepSeek API 单条 choice。
#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekChoice {
    #[serde(default)]
    pub index: u32,
    pub message: DeepSeekMessage,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// DeepSeek API 消息。
#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekMessage {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<DeepSeekToolCall>>,
}

/// DeepSeek tool 调用。
#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: DeepSeekFunctionCall,
}

/// DeepSeek 函数调用详情。
#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekFunctionCall {
    pub name: String,
    pub arguments: String,
}

/// DeepSeek token usage。
#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekUsage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

/// DeepSeek Platform Run Log（批量导入）。
#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekRunLog {
    pub run_id: String,
    pub turns: Vec<DeepSeekRunTurn>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct DeepSeekRunTurn {
    #[serde(default)]
    pub index: u32,
    pub prompt: Option<String>,
    pub response: DeepSeekResponse,
    pub timestamp: Option<String>,
}
