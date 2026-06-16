//! Claude Code session 日志原生格式的反序列化类型。
//!
//! 基于 Claude API 的 tool_use / tool_result 结构。
//! 只定义适配器需要用到的字段，其他字段由 serde 自动忽略。

use serde::Deserialize;

/// Claude session 日志中的单条消息。
#[derive(Deserialize, Debug)]
pub(crate) struct ClaudeSessionMessage {
    /// 消息类型: "user" / "assistant" / "tool_result"
    #[serde(rename = "type", default)]
    pub msg_type: String,
    /// 文本内容
    #[serde(default)]
    pub text: Option<String>,
    /// tool_use 块列表
    #[serde(default)]
    pub content: Option<Vec<ClaudeContentBlock>>,
    /// tool_result 的内容
    #[serde(default)]
    pub tool_use_id: Option<String>,
}

/// Claude content block（可包含 text 或 tool_use）。
#[derive(Deserialize, Debug)]
pub(crate) struct ClaudeContentBlock {
    /// block 类型: "text" / "tool_use"
    #[serde(rename = "type", default)]
    pub block_type: String,
    /// 文本内容（text block 时）
    #[serde(default)]
    pub text: Option<String>,
    /// tool_use 内容（tool_use block 时）
    #[serde(default)]
    pub id: Option<String>,
    /// tool 名称
    #[serde(default)]
    pub name: Option<String>,
    /// tool 参数（JSON value）
    #[serde(default)]
    pub input: Option<serde_json::Value>,
}

/// Claude session 日志完整格式：
/// 一个 JSON 文件包含 messages 数组。
#[derive(Deserialize, Debug)]
pub(crate) struct ClaudeSessionLog {
    /// 消息列表
    pub messages: Vec<ClaudeSessionMessage>,
}

/// Claude API 单次 response 格式（JSONL）。
/// 也支持每行一个 message 对象的格式。
#[derive(Deserialize, Debug)]
pub(crate) struct ClaudeApiResponse {
    /// response ID
    #[serde(default)]
    pub id: Option<String>,
    /// 消息类型
    #[serde(rename = "type", default)]
    pub msg_type: String,
    /// 内容块
    #[serde(default)]
    pub content: Option<Vec<ClaudeContentBlock>>,
    /// token 用量
    #[serde(default)]
    pub usage: Option<ClaudeUsage>,
    /// 模型名称
    #[serde(default)]
    pub model: Option<String>,
}

/// Claude token usage。
#[derive(Deserialize, Debug)]
pub(crate) struct ClaudeUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_claude_session_message() {
        let json = r#"{"type":"assistant","content":[{"type":"tool_use","id":"tool_001","name":"read","input":{"file_path":"src/auth.rs","limit":50}}]}"#;
        let msg: ClaudeSessionMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.msg_type, "assistant");
        let blocks = msg.content.unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].name.as_ref().unwrap(), "read");
    }

    #[test]
    fn test_parse_claude_api_response() {
        let json = r#"{"id":"msg_001","type":"assistant","content":[{"type":"text","text":"Hello!"}],"usage":{"input_tokens":10,"output_tokens":5},"model":"claude-3"}"#;
        let resp: ClaudeApiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id.unwrap(), "msg_001");
        assert_eq!(resp.usage.unwrap().input_tokens, 10);
    }

    #[test]
    fn test_parse_claude_session_log() {
        let json = r#"{"messages":[{"type":"user","text":"fix the bug"},{"type":"assistant","content":[{"type":"tool_use","id":"call_1","name":"grep","input":{"pattern":"bug"}}]}]}"#;
        let log: ClaudeSessionLog = serde_json::from_str(json).unwrap();
        assert_eq!(log.messages.len(), 2);
        assert_eq!(log.messages[0].msg_type, "user");
        assert_eq!(log.messages[1].msg_type, "assistant");
    }
}
