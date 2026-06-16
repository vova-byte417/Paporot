//! SemanticHash 计算。
//!
//! tool 名称 + 全量 args 序列化后的 hash，用于判定两次 tool 调用是否"相同"。

use crate::trace::types::ToolCall;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// 计算单个 ToolCall 的 semantic hash。
///
/// 相同 name + same args → 相同 hash。
pub fn semantic_hash(tc: &ToolCall) -> u64 {
    let mut hasher = DefaultHasher::new();
    tc.tool_name.hash(&mut hasher);
    serde_json::to_string(&tc.args)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish()
}

/// 批量计算 ToolCall 的 semantic hash。
pub fn semantic_hashes(tools: &[ToolCall]) -> Vec<u64> {
    tools.iter().map(semantic_hash).collect()
}

// ─── 测试 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_tool(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "call_001".into(),
            tool_name: name.into(),
            args,
            timestamp: "2026-06-12T10:00:00Z".into(),
            duration_ms: 100,
            result_id: None,
        }
    }

    #[test]
    fn test_semantic_hash_deterministic() {
        let tc = make_tool("read", json!({"path": "src/main.rs"}));
        let h1 = semantic_hash(&tc);
        let h2 = semantic_hash(&tc);
        assert_eq!(h1, h2, "Same inputs must produce same hash");
    }

    #[test]
    fn test_semantic_hash_different_tool_names() {
        let a = make_tool("read", json!({"path": "src/main.rs"}));
        let b = make_tool("write", json!({"path": "src/main.rs"}));
        assert_ne!(semantic_hash(&a), semantic_hash(&b));
    }

    #[test]
    fn test_semantic_hash_different_args() {
        let a = make_tool("read", json!({"path": "src/a.rs"}));
        let b = make_tool("read", json!({"path": "src/b.rs"}));
        assert_ne!(semantic_hash(&a), semantic_hash(&b));
    }

    #[test]
    fn test_semantic_hash_same_tool_same_args() {
        let a = make_tool("grep", json!({"pattern": "login", "path": "src/"}));
        let b = make_tool("grep", json!({"pattern": "login", "path": "src/"}));
        assert_eq!(semantic_hash(&a), semantic_hash(&b));
    }

    #[test]
    fn test_semantic_hashes_batch() {
        let tools = vec![
            make_tool("read", json!({"path": "a.rs"})),
            make_tool("edit", json!({"path": "a.rs"})),
        ];
        let hashes = semantic_hashes(&tools);
        assert_eq!(hashes.len(), 2);
        assert_ne!(hashes[0], hashes[1]);
    }
}
