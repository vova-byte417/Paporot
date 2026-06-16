//! L3 LLM 证据提供者抽象接口。
//!
//! 用户通过实现 `LlmEvidenceProvider` trait 注入自己的 LLM 服务。
//! Paporot 在 `evidence generate` 时调用它获取 L3 证据。
//! 如果不配置 L3 provider，证据仅包含 L1+L2。

use crate::evidence::types::{L1Evidence, L2Evidence, L3Evidence};

/// L3 LLM 证据提供者。
///
/// # 实现要求
///
/// - `Send + Sync`: 可在多线程环境下使用
/// - 错误处理: 返回 `None` 表示 LLM 不可用，证据降级为 L1+L2 only
pub trait LlmEvidenceProvider: Send + Sync {
    /// 根据 L1 + L2 证据生成 L3 推断证据。
    ///
    /// # 参数
    ///
    /// - `l1_evidence`: L1 AST 符号列表
    /// - `l2_evidence`: L2 规则匹配列表
    /// - `diff_context`: 本次 diff 的上下文（文件变更摘要）
    ///
    /// # 返回
    ///
    /// - `Some(L3Evidence)` 推断成功
    /// - `None` LLM 不可用（跳过 L3 评分）
    fn infer(
        &self,
        l1_evidence: &[L1Evidence],
        l2_evidence: &[L2Evidence],
        diff_context: &str,
    ) -> Option<L3Evidence>;

    /// LLM 服务名称。
    fn name(&self) -> &str;

    /// LLM 模型名称。
    fn model(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的 Mock L3 provider。
    struct MockLlmProvider;

    impl LlmEvidenceProvider for MockLlmProvider {
        fn infer(
            &self,
            l1: &[L1Evidence],
            _l2: &[L2Evidence],
            _diff_context: &str,
        ) -> Option<L3Evidence> {
            let symbols: Vec<String> = l1.iter().map(|e| e.symbol.clone()).collect();
            let fragment = format!("Analysis of: {}", symbols.join(", "));
            Some(L3Evidence {
                prompt_hash: "mock_hash".into(),
                fragment,
                model: "mock-model".into(),
                timestamp: "2026-06-12T14:00:00Z".into(),
            })
        }

        fn name(&self) -> &str {
            "mock"
        }

        fn model(&self) -> &str {
            "mock-model"
        }
    }

    #[test]
    fn test_mock_provider_returns_l3() {
        let provider = MockLlmProvider;

        let l1 = vec![L1Evidence {
            symbol: "login".into(),
            file_path: "src/auth.rs".into(),
            line: 42,
            kind: crate::evidence::types::SymbolKind::Function,
            visibility: "pub".into(),
        }];

        let result = provider.infer(&l1, &[], "added login function");
        assert!(result.is_some());
        let l3 = result.unwrap();
        assert!(l3.fragment.contains("login"));
        assert_eq!(l3.model, "mock-model");
    }

    #[test]
    fn test_mock_provider_trait_is_object_safe() {
        let _provider: Box<dyn LlmEvidenceProvider> = Box::new(MockLlmProvider);
    }
}
