//! Trace 适配器注册表。
//!
//! 基于 `inventory` crate 的编译时插件注册机制。
//! 每个实现 `TraceAdapter` trait 的适配器只需用
//! `inventory::submit!` 提交注册条目，无需手动修改 `all_adapters()`。

use crate::trace::adapter::TraceAdapter;

/// 适配器注册条目。
///
/// 每个适配器在各自的模块中通过 `inventory::submit!` 提交一个条目，
/// `all_adapters()` 会在运行时自动收集所有条目。
#[derive(Debug)]
pub struct AdapterEntry {
    /// 适配器名称（与 TraceAdapter::name() 一致）
    pub name: &'static str,
    /// 创建适配器实例的工厂函数
    pub factory: fn() -> Box<dyn TraceAdapter>,
}

// `inventory::collect!` 定义全局收集器。
// 所有用 `inventory::submit!` 提交的 AdapterEntry 都会被收集到这里。
inventory::collect!(AdapterEntry);

/// 获取所有已注册的适配器。
///
/// 遍历编译时收集的所有 `AdapterEntry`，调用工厂函数创建适配器实例。
/// 新适配器无需修改此函数。
pub fn all_adapters() -> Vec<Box<dyn TraceAdapter>> {
    inventory::iter::<AdapterEntry>
        .into_iter()
        .map(|entry| (entry.factory)())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试：all_adapters() 返回的列表非空且包含 deepseek。
    /// 验证 inventory 注册机制正常工作。
    #[test]
    fn test_registry_contains_adapters() {
        let adapters = all_adapters();
        assert!(
            !adapters.is_empty(),
            "Registry should contain at least one adapter"
        );

        let names: Vec<&str> = adapters.iter().map(|a| a.name()).collect();
        assert!(
            names.contains(&"deepseek"),
            "Registry should contain deepseek adapter, got: {:?}",
            names
        );
    }

    /// 测试：all_adapters() 可被多次调用，每次返回独立实例。
    #[test]
    fn test_registry_callable_multiple_times() {
        let a = all_adapters();
        let b = all_adapters();
        assert_eq!(a.len(), b.len());
    }
}
