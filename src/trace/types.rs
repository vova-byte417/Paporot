//! Execution Trace 核心数据类型。
//!
//! 零内部依赖，仅依赖 serde。

use serde::{Deserialize, Serialize};

// ─── BehaviorTrace ─────────────────────────────────────────────────

/// 单次 Agent 执行的完整轨迹。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BehaviorTrace {
    /// 唯一标识符，格式: `trace_{YYYYMMDD}_{NNN}`
    pub id: String,

    /// 关联的 Agent session ID
    pub session_id: String,

    /// 原始用户输入 / 系统 prompt
    pub prompt: String,

    /// 按时间排序的 tool 调用序列
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,

    /// tool 调用对应的观察结果
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observations: Vec<Observation>,

    /// Agent 最终输出文本
    pub final_output: String,

    /// 累计 token 消耗
    pub token_usage: TokenUsage,

    /// 执行开始时间 (ISO-8601)
    pub started_at: String,

    /// 执行结束时间 (ISO-8601)
    pub finished_at: String,

    /// 数据来源标记
    pub source: TraceSource,

    /// 用户自定义标签
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    /// 关联的 Capability ID 列表（弱关联）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capability_ids: Vec<String>,

    /// 删除标记（soft delete）
    #[serde(default)]
    pub deleted: bool,
}

// ─── ToolCall ──────────────────────────────────────────────────────

/// 单次 tool 调用记录。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolCall {
    /// 唯一标识符
    pub id: String,

    /// tool 名称
    pub tool_name: String,

    /// 完整参数（JSON value，不预设 schema）
    pub args: serde_json::Value,

    /// 调用时间戳 (ISO-8601)
    pub timestamp: String,

    /// 调用耗时（毫秒）
    #[serde(default)]
    pub duration_ms: u64,

    /// 关联的 Observation ID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_id: Option<String>,
}

// ─── Observation ───────────────────────────────────────────────────

/// Tool 调用返回的观察结果。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Observation {
    /// 唯一标识符
    pub id: String,

    /// 关联的 ToolCall ID
    pub tool_call_id: String,

    /// 完整结果内容
    pub content: String,

    /// 结果是否被截断
    #[serde(default)]
    pub truncated: bool,

    /// 截断时的原始字节位置
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated_at_bytes: Option<u64>,
}

// ─── TokenUsage ────────────────────────────────────────────────────

/// Token 消耗统计。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TokenUsage {
    /// 输入 token 数
    pub input_tokens: u64,

    /// 输出 token 数
    pub output_tokens: u64,

    /// 缓存读取 token 数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,

    /// 缓存写入 token 数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
}

// ─── TraceSource ───────────────────────────────────────────────────

/// 轨迹数据来源。
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceSource {
    /// 被动导入：从外部 Agent 日志文件解析
    Imported {
        adapter: String,
        adapter_version: String,
        file_path: String,
    },
    /// Paporot wrapper 实时捕获
    Captured {
        agent_name: String,
    },
}

// ─── TraceSummary ──────────────────────────────────────────────────

/// 轻量级摘要，用于 list 命令。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TraceSummary {
    pub id: String,
    pub session_id: String,
    pub prompt_preview: String,
    pub tool_names: Vec<String>,
    pub tool_call_count: usize,
    pub total_tokens: u64,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u64,
    pub source_type: String,
    pub adapter_name: Option<String>,
    pub capability_count: usize,
    pub tags: Vec<String>,
    pub deleted: bool,
}

// ─── TraceFilter ───────────────────────────────────────────────────

/// 列表查询的过滤条件。
#[derive(Debug, Clone, Default)]
pub struct TraceFilter {
    pub session_id: Option<String>,
    pub tool_name: Option<String>,
    pub tag: Option<String>,
    pub capability_id: Option<String>,
    pub from_date: Option<String>,
    pub to_date: Option<String>,
    pub source_type: Option<String>,
    pub include_deleted: bool,
    pub limit: usize,
    pub offset: usize,
}

// ─── ImportResult ──────────────────────────────────────────────────

/// 单次 import 操作的结果。
#[derive(Debug, Clone)]
pub struct ImportResult {
    pub source_path: String,
    pub adapter: String,
    pub auto_detected: bool,
    pub imported: Vec<TraceSummary>,
    pub skipped_count: usize,
    pub skip_reasons: Vec<String>,
}

// ─── RedactConfig ──────────────────────────────────────────────────

/// 脱敏配置。
#[derive(Debug, Clone)]
pub struct RedactConfig {
    pub redact_auth_header: bool,
    pub redact_api_keys: bool,
    pub redact_env_values: bool,
    pub custom_rules: Vec<(String, String)>,
}

impl Default for RedactConfig {
    fn default() -> Self {
        Self {
            redact_auth_header: true,
            redact_api_keys: true,
            redact_env_values: false,
            custom_rules: Vec::new(),
        }
    }
}

// ─── 测试 ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_behavior_trace_serde_roundtrip() {
        let trace = BehaviorTrace {
            id: "trace_20260612_001".into(),
            session_id: "sess-abc".into(),
            prompt: "fix the login bug".into(),
            tool_calls: vec![ToolCall {
                id: "call_001".into(),
                tool_name: "grep".into(),
                args: serde_json::json!({"pattern": "login", "path": "src/"}),
                timestamp: "2026-06-12T14:30:00Z".into(),
                duration_ms: 150,
                result_id: Some("obs_001".into()),
            }],
            observations: vec![Observation {
                id: "obs_001".into(),
                tool_call_id: "call_001".into(),
                content: "src/auth.rs:42: fn login".into(),
                truncated: false,
                truncated_at_bytes: None,
            }],
            final_output: "Fixed the login bug".into(),
            token_usage: TokenUsage {
                input_tokens: 500,
                output_tokens: 200,
                cache_read_tokens: Some(100),
                cache_write_tokens: None,
            },
            started_at: "2026-06-12T14:29:00Z".into(),
            finished_at: "2026-06-12T14:32:00Z".into(),
            source: TraceSource::Captured {
                agent_name: "test-agent".into(),
            },
            tags: vec!["security".into()],
            capability_ids: vec!["cap_001".into()],
            deleted: false,
        };

        let json = serde_json::to_string(&trace).unwrap();
        let decoded: BehaviorTrace = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.id, "trace_20260612_001");
        assert_eq!(decoded.tool_calls.len(), 1);
        assert_eq!(decoded.tool_calls[0].tool_name, "grep");
        assert_eq!(decoded.observations.len(), 1);
        assert!(decoded.tags.contains(&"security".to_string()));

        // 验证 TraceSource tagged enum
        match &decoded.source {
            TraceSource::Captured { agent_name } => {
                assert_eq!(agent_name, "test-agent");
            }
            _ => panic!("expected Captured"),
        }
    }

    #[test]
    fn test_trace_source_tagged_serialization() {
        let imported = TraceSource::Imported {
            adapter: "deepseek".into(),
            adapter_version: "1.0.0".into(),
            file_path: "test.jsonl".into(),
        };
        let json = serde_json::to_string(&imported).unwrap();
        assert!(json.contains("\"type\":\"imported\""));
        assert!(json.contains("\"deepseek\""));

        let decoded: TraceSource = serde_json::from_str(&json).unwrap();
        match decoded {
            TraceSource::Imported { adapter, .. } => {
                assert_eq!(adapter, "deepseek");
            }
            _ => panic!("expected Imported"),
        }
    }

    #[test]
    fn test_trace_filter_default() {
        let filter = TraceFilter::default();
        assert!(!filter.include_deleted);
        assert_eq!(filter.limit, 0);
        assert_eq!(filter.offset, 0);
        assert!(filter.session_id.is_none());
    }

    #[test]
    fn test_redact_config_default() {
        let config = RedactConfig::default();
        assert!(config.redact_auth_header);
        assert!(config.redact_api_keys);
        assert!(!config.redact_env_values);
        assert!(config.custom_rules.is_empty());
    }

    #[test]
    fn test_token_usage_default() {
        let usage = TokenUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert!(usage.cache_read_tokens.is_none());
    }

    #[test]
    fn test_observation_truncated() {
        let obs = Observation {
            id: "obs_001".into(),
            tool_call_id: "call_001".into(),
            content: "short".into(),
            truncated: false,
            truncated_at_bytes: None,
        };
        let json = serde_json::to_string(&obs).unwrap();
        // truncated=false 不应序列化 truncated_at_bytes
        assert!(!json.contains("truncated_at_bytes"));

        let obs2 = Observation {
            id: "obs_002".into(),
            tool_call_id: "call_002".into(),
            content: "long...".into(),
            truncated: true,
            truncated_at_bytes: Some(100000),
        };
        let json2 = serde_json::to_string(&obs2).unwrap();
        assert!(json2.contains("truncated_at_bytes"));
    }
}
