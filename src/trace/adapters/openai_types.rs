//! OpenAI API 原生格式的反序列化类型。
//!
//! 基于 OpenAI Chat Completion API 响应格式。
//! 只定义适配器需要用到的字段。

use serde::Deserialize;

/// OpenAI Chat Completion 单次 API 调用的完整响应。
#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiResponse {
    /// response ID
    pub id: String,
    /// object 类型: "chat.completion"
    #[serde(default)]
    pub object: String,
    /// choices 列表
    pub choices: Vec<OpenAiChoice>,
    /// token 用量
    #[serde(default)]
    pub usage: Option<OpenAiUsage>,
    /// 模型名称
    #[serde(default)]
    pub model: String,
    /// 创建时间戳（Unix 秒）
    #[serde(default)]
    pub created: Option<u64>,
}

/// OpenAI 单条 choice。
#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiChoice {
    /// choice 序号
    #[serde(default)]
    pub index: u32,
    /// 消息内容
    pub message: OpenAiMessage,
    /// 结束原因: "stop" | "tool_calls" | ...
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// OpenAI 消息。
#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiMessage {
    /// 消息角色: "assistant" | "user" | "system" | "tool"
    #[serde(default)]
    pub role: String,
    /// 文本内容
    #[serde(default)]
    pub content: Option<String>,
    /// tool 调用列表
    #[serde(default)]
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
}

/// OpenAI tool 调用。
#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiToolCall {
    /// tool 调用 ID
    pub id: String,
    /// tool 类型: "function"
    #[serde(rename = "type", default)]
    pub call_type: String,
    /// 函数调用详情
    pub function: OpenAiFunctionCall,
}

/// OpenAI 函数调用详情。
#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiFunctionCall {
    /// 函数名称
    pub name: String,
    /// 参数（JSON 字符串）
    pub arguments: String,
}

/// OpenAI token usage。
#[derive(Deserialize, Debug)]
pub(crate) struct OpenAiUsage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_openai_response_with_content() {
        let json = r#"{"id":"chatcmpl-001","object":"chat.completion","choices":[{"message":{"role":"assistant","content":"Hello!"}}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15},"model":"gpt-4"}"#;
        let resp: OpenAiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, "chatcmpl-001");
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.model, "gpt-4");
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content.as_ref().unwrap(), "Hello!");
    }

    #[test]
    fn test_parse_openai_response_with_tool_calls() {
        let json = r#"{"id":"chatcmpl-002","choices":[{"message":{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"grep","arguments":"{\"pattern\":\"login\"}"}}]}}],"usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30}}"#;
        let resp: OpenAiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.choices[0].message.tool_calls.as_ref().unwrap().len(), 1);
        assert_eq!(
            resp.choices[0].message.tool_calls.as_ref().unwrap()[0].function.name,
            "grep"
        );
    }

    #[test]
    fn test_parse_openai_response_no_usage() {
        let json = r#"{"id":"chatcmpl-003","choices":[{"message":{"role":"assistant","content":"done"}}]}"#;
        let resp: OpenAiResponse = serde_json::from_str(json).unwrap();
        assert!(resp.usage.is_none());
    }
}
