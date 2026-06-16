//! 对齐操作代价函数。

/// 对齐操作的代价定义。
#[derive(Debug, Clone)]
pub struct AlignmentCosts {
    /// 插入一个 tool/segment 的代价
    pub insertion: f64,
    /// 删除一个 tool/segment 的代价
    pub deletion: f64,
    /// 替换一个 tool/segment 的代价
    pub substitution: f64,
}

impl Default for AlignmentCosts {
    fn default() -> Self {
        Self {
            insertion: 1.0,
            deletion: 1.0,
            substitution: 1.0,
        }
    }
}

/// 计算两个 tool 之间的替换代价。
///
/// - 相同 SemanticHash → 0.0（无需替换）
/// - 同名称但不同 args → 0.5（轻量替换）
/// - 不同名称 → 1.0（完全替换）
pub fn tool_substitution_cost(hash_a: u64, hash_b: u64, name_a: &str, name_b: &str) -> f64 {
    if hash_a == hash_b {
        0.0
    } else if name_a == name_b {
        0.5
    } else {
        1.0
    }
}

// ─── 测试 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_costs() {
        let costs = AlignmentCosts::default();
        assert_eq!(costs.insertion, 1.0);
        assert_eq!(costs.deletion, 1.0);
        assert_eq!(costs.substitution, 1.0);
    }

    #[test]
    fn test_tool_substitution_same_hash() {
        let cost = tool_substitution_cost(42, 42, "read", "read");
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_tool_substitution_different_name() {
        let cost = tool_substitution_cost(42, 99, "read", "write");
        assert_eq!(cost, 1.0);
    }

    #[test]
    fn test_tool_substitution_same_name_diff_hash() {
        let cost = tool_substitution_cost(42, 99, "read", "read");
        assert_eq!(cost, 0.5);
    }

    #[test]
    fn test_tool_substitution_different_name_same_hash() {
        // Unlikely but possible hash collision — still treated as 0.0
        let cost = tool_substitution_cost(42, 42, "read", "write");
        assert_eq!(cost, 0.0);
    }
}
