//! Trace 适配器 trait + 注册与自动检测。

use crate::trace::adapter_registry;
use crate::trace::error::TraceError;
use crate::trace::types::BehaviorTrace;

/// 外部 trace 格式 → BehaviorTrace 的转换适配器。
pub trait TraceAdapter: Send + Sync {
    /// 适配器唯一名称。
    fn name(&self) -> &str;

    /// 适配器版本号。
    fn version(&self) -> &str;

    /// 检测输入是否为本适配器支持的格式。
    fn can_handle(&self, raw: &str) -> bool;

    /// 解析原始 trace 文本，返回 BehaviorTrace 列表。
    fn parse(&self, raw: &str, file_path: &str) -> Result<Vec<BehaviorTrace>, TraceError>;

    /// 适配器的人类可读描述。
    fn description(&self) -> &str;
}

// ─── 适配器注册表（委托到 adapter_registry） ────────────────────

/// 获取所有已注册的适配器。
pub fn all_adapters() -> Vec<Box<dyn TraceAdapter>> {
    adapter_registry::all_adapters()
}

/// 按名称查找适配器（大小写不敏感）。
pub fn find_adapter(name: &str) -> Option<Box<dyn TraceAdapter>> {
    let name_lower = name.to_lowercase();
    all_adapters()
        .into_iter()
        .find(|a| a.name().to_lowercase() == name_lower)
}

/// 自动检测格式，返回第一个 can_handle() 返回 true 的适配器。
pub fn auto_detect(raw: &str) -> Option<Box<dyn TraceAdapter>> {
    all_adapters()
        .into_iter()
        .find(|a| a.can_handle(raw))
}

/// 列出所有适配器的信息。
pub fn list_adapters() -> Vec<AdapterInfo> {
    all_adapters()
        .iter()
        .map(|a| AdapterInfo {
            name: a.name().to_string(),
            version: a.version().to_string(),
            description: a.description().to_string(),
        })
        .collect()
}

/// 适配器元信息（用于 CLI 展示）。
#[derive(Debug, Clone)]
pub struct AdapterInfo {
    pub name: String,
    pub version: String,
    pub description: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_adapters_nonempty() {
        let adapters = list_adapters();
        assert!(!adapters.is_empty());
        let deepseek = adapters.iter().find(|a| a.name == "deepseek");
        assert!(deepseek.is_some());
        assert_eq!(deepseek.unwrap().version, "1.0.0");
    }

    #[test]
    fn test_find_adapter_case_insensitive() {
        assert!(find_adapter("deepseek").is_some());
        assert!(find_adapter("DeepSeek").is_some());
        assert!(find_adapter("DEEPSEEK").is_some());
        assert!(find_adapter("nonexistent").is_none());
    }

    #[test]
    fn test_auto_detect() {
        // DeepSeek format
        let sample = r#"{"id":"chatcmpl-123","choices":[{"message":{"role":"assistant","content":"Hello"}}]}"#;
        let result = auto_detect(sample);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name(), "deepseek");
    }

    #[test]
    fn test_auto_detect_unknown() {
        assert!(auto_detect("random text").is_none());
    }
}
