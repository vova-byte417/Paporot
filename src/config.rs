//! Paporot 配置管理
//!
//! 支持 `.Paporot/config.toml` 和命令行覆盖。

use serde::{Deserialize, Serialize};

/// 完整 Paporot 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// LLM 配置
    #[serde(default)]
    pub llm: LlmConfig,
    /// 存储配置
    #[serde(default)]
    pub storage: StorageConfig,
    /// Agent 行为配置
    #[serde(default)]
    pub agent: AgentConfig,
    /// Trace 脱敏配置
    #[serde(default)]
    pub trace: TraceRedactConfig,
}

/// LLM 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// API 端点 URL（兼容 OpenAI / Anthropic）
    #[serde(default = "default_llm_endpoint")]
    pub endpoint: String,
    /// API Key（也支持环境变量 Paporot_API_KEY）
    #[serde(default)]
    pub api_key: String,
    /// 模型名称
    #[serde(default = "default_llm_model")]
    pub model: String,
    /// 温度（推荐 0.2-0.4）
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    /// 最大输出 token
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// 最大重试次数
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// 请求超时秒数
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_llm_endpoint() -> String {
    "https://api.openai.com/v1/chat/completions".into()
}

fn default_llm_model() -> String {
    "gpt-4o".into()
}

fn default_temperature() -> f32 {
    0.3
}

fn default_max_tokens() -> u32 {
    4096
}

fn default_max_retries() -> u32 {
    3
}

fn default_timeout_secs() -> u64 {
    120
}

/// 存储配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// snapshot 存储目录
    #[serde(default = "default_snapshots_dir")]
    pub snapshots_dir: String,
}

fn default_snapshots_dir() -> String {
    ".Paporot/snapshots".into()
}

/// Agent 行为配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// diff 超过此字节数时发出警告
    #[serde(default = "default_diff_warn_threshold")]
    pub diff_warn_threshold: usize,
    /// diff 超过此字节数时截断（避免 token 耗尽）
    #[serde(default = "default_diff_truncate_threshold")]
    pub diff_truncate_threshold: usize,
}

fn default_diff_warn_threshold() -> usize {
    32_000 // 32KB
}

fn default_diff_truncate_threshold() -> usize {
    96_000 // 96KB
}

/// Trace 脱敏配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceRedactConfig {
    /// 是否在 trace import 时自动脱敏（默认 false）
    #[serde(default)]
    pub auto_redact: bool,
    /// 是否脱敏 Authorization header
    #[serde(default = "default_true")]
    pub redact_auth_header: bool,
    /// 是否脱敏 API key 模式
    #[serde(default = "default_true")]
    pub redact_api_keys: bool,
    /// 是否脱敏环境变量值
    #[serde(default)]
    pub redact_env_values: bool,
    /// 自定义正则替换规则 (pattern, replacement)
    #[serde(default)]
    pub custom_rules: Vec<(String, String)>,
}

fn default_true() -> bool {
    true
}

impl Default for TraceRedactConfig {
    fn default() -> Self {
        Self {
            auto_redact: false,
            redact_auth_header: true,
            redact_api_keys: true,
            redact_env_values: false,
            custom_rules: Vec::new(),
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            endpoint: default_llm_endpoint(),
            api_key: String::new(),
            model: default_llm_model(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            max_retries: default_max_retries(),
            timeout_secs: default_timeout_secs(),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            snapshots_dir: default_snapshots_dir(),
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            diff_warn_threshold: default_diff_warn_threshold(),
            diff_truncate_threshold: default_diff_truncate_threshold(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            llm: LlmConfig::default(),
            storage: StorageConfig::default(),
            agent: AgentConfig::default(),
            trace: TraceRedactConfig::default(),
        }
    }
}

impl Config {
    /// 从配置文件加载，不存在则使用默认值
    pub fn load_or_default(path: &str) -> Self {
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                match toml::from_str(&contents) {
                    Ok(config) => {
                        eprintln!("  config  : loaded from {}", path);
                        config
                    }
                    Err(e) => {
                        eprintln!(
                            "  config  : failed to parse {} ({}) — using defaults",
                            path, e
                        );
                        Config::default()
                    }
                }
            }
            Err(_) => {
                // 配置文件不存在，使用默认值并尝试从环境变量取 API Key
                let mut config = Config::default();
                if let Ok(key) = std::env::var("Paporot_API_KEY") {
                    config.llm.api_key = key;
                    eprintln!("  config  : using Paporot_API_KEY from env");
                } else {
                    eprintln!("  config  : no config file found, using defaults (tip: set Paporot_API_KEY)");
                }
                config
            }
        }
    }

    /// 生成示例配置文件的 toml 内容
    pub fn sample_toml() -> &'static str {
        r#"# Paporot 配置文件示例
# 复制此文件到 .Paporot/config.toml 并按需修改

[llm]
# LLM API 端点（OpenAI 兼容接口）
endpoint = "https://api.openai.com/v1/chat/completions"
# API Key（也可通过环境变量 Paporot_API_KEY 设置）
api_key = ""
# 模型名称
model = "gpt-4o"
# 温度（0.0-1.0，推荐 0.2-0.4）
temperature = 0.3
# 最大输出 token
max_tokens = 4096
# 请求超时秒数
timeout_secs = 120

[storage]
# snapshot 存储目录
snapshots_dir = ".Paporot/snapshots"

[agent]
# diff 警告阈值（字节）
diff_warn_threshold = 32000
# diff 截断阈值（字节）
diff_truncate_threshold = 96000

[trace]
# 是否在 trace import 时自动脱敏
auto_redact = false
# 是否脱敏 Authorization header
redact_auth_header = true
# 是否脱敏 API key 模式
redact_api_keys = true
# 是否脱敏环境变量值
redact_env_values = false
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.llm.endpoint.contains("openai"));
        assert_eq!(config.llm.temperature, 0.3);
        assert_eq!(config.storage.snapshots_dir, ".Paporot/snapshots");
        assert!(!config.trace.auto_redact);
        assert!(config.trace.redact_api_keys);
        assert!(config.trace.redact_auth_header);
        assert!(!config.trace.redact_env_values);
    }

    #[test]
    fn test_sample_toml_parses() {
        let config: Config = toml::from_str(Config::sample_toml()).unwrap();
        assert_eq!(config.llm.model, "gpt-4o");
        assert_eq!(config.agent.diff_warn_threshold, 32000);
        assert!(!config.trace.auto_redact);
    }

    #[test]
    fn test_trace_redact_config_default() {
        let config = TraceRedactConfig::default();
        assert!(!config.auto_redact);
        assert!(config.redact_auth_header);
        assert!(config.redact_api_keys);
        assert!(!config.redact_env_values);
        assert!(config.custom_rules.is_empty());
    }

    #[test]
    fn test_trace_redact_config_toml_parse() {
        let toml_str = r#"
auto_redact = true
redact_auth_header = false
redact_api_keys = true
redact_env_values = true
custom_rules = [["sk-\\w{20,}", "sk-***REDACTED***"]]
"#;
        let config: TraceRedactConfig = toml::from_str(toml_str).unwrap();
        assert!(config.auto_redact);
        assert!(!config.redact_auth_header);
        assert!(config.redact_api_keys);
        assert!(config.redact_env_values);
        assert_eq!(config.custom_rules.len(), 1);
    }

    #[test]
    fn test_full_config_with_trace_section() {
        let toml_str = r#"
[agent]
diff_warn_threshold = 16000

[trace]
auto_redact = true
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.trace.auto_redact);
        assert_eq!(config.agent.diff_warn_threshold, 16000);
        // 未指定的字段使用默认值
        assert!(config.trace.redact_api_keys);
    }
}
