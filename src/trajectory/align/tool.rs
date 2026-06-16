//! Tool 级对齐：编辑距离 + 贪心降级。

use crate::trace::types::ToolCall;
use crate::trajectory::types::{ArgsDiff, ToolDiff, ToolDiffKind};

/// Tool 级匹配结果。
#[derive(Debug, Clone)]
pub struct ToolMatch {
    pub kind: ToolDiffKind,
    pub index_a: Option<usize>,
    pub index_b: Option<usize>,
}

/// Maximum tool sequence length for full edit-distance alignment.
/// Sequences longer than this fall back to greedy matching.
const MAX_EDIT_DISTANCE: usize = 200;

/// 对齐两个 tool call 序列。
///
/// - `tools_a` / `tools_b`: 两个待比对的 tool call 序列
/// - `hashes_a` / `hashes_b`: 对应的 semantic hash
pub fn align_tools(
    tools_a: &[ToolCall],
    tools_b: &[ToolCall],
    hashes_a: &[u64],
    hashes_b: &[u64],
) -> Vec<ToolDiff> {
    if tools_a.len() > MAX_EDIT_DISTANCE || tools_b.len() > MAX_EDIT_DISTANCE {
        return greedy_align(tools_a, tools_b, hashes_a, hashes_b);
    }
    levenshtein_align(tools_a, tools_b, hashes_a, hashes_b)
}

/// 编辑距离对齐（Needleman-Wunsch / Levenshtein）。
fn levenshtein_align(
    tools_a: &[ToolCall],
    tools_b: &[ToolCall],
    hashes_a: &[u64],
    hashes_b: &[u64],
) -> Vec<ToolDiff> {
    let m = tools_a.len();
    let n = tools_b.len();

    // dp[i][j] = min cost to align first i of A with first j of B
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }

    for i in 1..=m {
        for j in 1..=n {
            let cost = if hashes_a[i - 1] == hashes_b[j - 1] {
                0
            } else {
                1
            };
            dp[i][j] = (dp[i - 1][j] + 1)        // deletion
                .min(dp[i][j - 1] + 1)            // insertion
                .min(dp[i - 1][j - 1] + cost);    // substitution/match
        }
    }

    // Backtrack
    let mut result = Vec::new();
    let (mut i, mut j) = (m, n);

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && hashes_a[i - 1] == hashes_b[j - 1] {
            result.push(ToolDiff {
                tool_name: tools_b[j - 1].tool_name.clone(),
                kind: ToolDiffKind::Unchanged,
                index_a: Some(i - 1),
                index_b: Some(j - 1),
                args_diff: None,
                duration_ms: tools_b[j - 1].duration_ms,
            });
            i -= 1;
            j -= 1;
        } else if i > 0 && j > 0
            && tools_a[i - 1].tool_name == tools_b[j - 1].tool_name
        {
            // Same tool name, different args
            result.push(ToolDiff {
                tool_name: tools_b[j - 1].tool_name.clone(),
                kind: ToolDiffKind::ArgsChanged,
                index_a: Some(i - 1),
                index_b: Some(j - 1),
                args_diff: Some(ArgsDiff {
                    args_a: tools_a[i - 1].args.clone(),
                    args_b: tools_b[j - 1].args.clone(),
                }),
                duration_ms: tools_b[j - 1].duration_ms,
            });
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] + 1 <= dp[i - 1][j] + 1) {
            // Insertion (B has a tool A doesn't)
            result.push(ToolDiff {
                tool_name: tools_b[j - 1].tool_name.clone(),
                kind: ToolDiffKind::Added,
                index_a: None,
                index_b: Some(j - 1),
                args_diff: None,
                duration_ms: tools_b[j - 1].duration_ms,
            });
            j -= 1;
        } else {
            // Deletion (A has a tool B doesn't)
            result.push(ToolDiff {
                tool_name: tools_a[i - 1].tool_name.clone(),
                kind: ToolDiffKind::Deleted,
                index_a: Some(i - 1),
                index_b: None,
                args_diff: None,
                duration_ms: tools_a[i - 1].duration_ms,
            });
            i -= 1;
        }
    }

    result.reverse();
    result
}

/// 贪心匹配降级策略：对长序列 (>200 tools) 使用 hash 集合匹配。
fn greedy_align(
    tools_a: &[ToolCall],
    tools_b: &[ToolCall],
    hashes_a: &[u64],
    hashes_b: &[u64],
) -> Vec<ToolDiff> {
    use std::collections::HashSet;

    let hash_set_a: HashSet<u64> = hashes_a.iter().copied().collect();
    let mut used_a = vec![false; tools_a.len()];

    let mut result = Vec::new();

    for (j, tb) in tools_b.iter().enumerate() {
        if hash_set_a.contains(&hashes_b[j]) {
            // Find first unused match in A
            let matched = tools_a.iter().enumerate().find(|(i, ta)| {
                !used_a[*i] && ta.tool_name == tb.tool_name && hashes_a[*i] == hashes_b[j]
            });

            if let Some((i, _)) = matched {
                used_a[i] = true;
                result.push(ToolDiff {
                    tool_name: tb.tool_name.clone(),
                    kind: ToolDiffKind::Unchanged,
                    index_a: Some(i),
                    index_b: Some(j),
                    args_diff: None,
                    duration_ms: tb.duration_ms,
                });
                continue;
            }
        }

        // Check for same-name-diff-args
        let same_name_match = tools_a.iter().enumerate().find(|(i, ta)| {
            !used_a[*i] && ta.tool_name == tb.tool_name
        });
        if let Some((i, _)) = same_name_match {
            used_a[i] = true;
            result.push(ToolDiff {
                tool_name: tb.tool_name.clone(),
                kind: ToolDiffKind::ArgsChanged,
                index_a: Some(i),
                index_b: Some(j),
                args_diff: Some(ArgsDiff {
                    args_a: tools_a[i].args.clone(),
                    args_b: tb.args.clone(),
                }),
                duration_ms: tb.duration_ms,
            });
        } else {
            result.push(ToolDiff {
                tool_name: tb.tool_name.clone(),
                kind: ToolDiffKind::Added,
                index_a: None,
                index_b: Some(j),
                args_diff: None,
                duration_ms: tb.duration_ms,
            });
        }
    }

    // Unmatched tools in A → Deleted
    for (i, used) in used_a.iter().enumerate() {
        if !used {
            result.push(ToolDiff {
                tool_name: tools_a[i].tool_name.clone(),
                kind: ToolDiffKind::Deleted,
                index_a: Some(i),
                index_b: None,
                args_diff: None,
                duration_ms: tools_a[i].duration_ms,
            });
        }
    }

    result
}

// ─── 测试 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::hash::semantic_hash;

    fn make_tool(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: format!("call_{}", name),
            tool_name: name.into(),
            args,
            timestamp: "2026-06-12T10:00:00Z".into(),
            duration_ms: 100,
            result_id: None,
        }
    }

    #[test]
    fn test_align_identical_tools() {
        let tools_a = vec![
            make_tool("read", serde_json::json!({"path": "a.rs"})),
            make_tool("edit", serde_json::json!({"path": "a.rs"})),
        ];
        let tools_b = tools_a.clone();
        let ha = tools_a.iter().map(semantic_hash).collect::<Vec<_>>();
        let hb = tools_b.iter().map(semantic_hash).collect::<Vec<_>>();

        let diffs = align_tools(&tools_a, &tools_b, &ha, &hb);
        assert_eq!(diffs.len(), 2);
        assert!(diffs.iter().all(|d| d.kind == ToolDiffKind::Unchanged));
    }

    #[test]
    fn test_align_added_tool() {
        let tools_a = vec![make_tool("read", serde_json::json!({"path": "a.rs"}))];
        let tools_b = vec![
            make_tool("read", serde_json::json!({"path": "a.rs"})),
            make_tool("test", serde_json::json!({})),
        ];
        let ha = tools_a.iter().map(semantic_hash).collect::<Vec<_>>();
        let hb = tools_b.iter().map(semantic_hash).collect::<Vec<_>>();

        let diffs = align_tools(&tools_a, &tools_b, &ha, &hb);
        assert_eq!(diffs.len(), 2);
        assert_eq!(diffs[0].kind, ToolDiffKind::Unchanged);
        assert_eq!(diffs[0].tool_name, "read");
        assert_eq!(diffs[1].kind, ToolDiffKind::Added);
        assert_eq!(diffs[1].tool_name, "test");
    }

    #[test]
    fn test_align_deleted_tool() {
        let tools_a = vec![
            make_tool("read", serde_json::json!({"path": "a.rs"})),
            make_tool("test", serde_json::json!({})),
        ];
        let tools_b = vec![make_tool("read", serde_json::json!({"path": "a.rs"}))];
        let ha = tools_a.iter().map(semantic_hash).collect::<Vec<_>>();
        let hb = tools_b.iter().map(semantic_hash).collect::<Vec<_>>();

        let diffs = align_tools(&tools_a, &tools_b, &ha, &hb);
        assert_eq!(diffs.len(), 2);
        assert_eq!(diffs[0].kind, ToolDiffKind::Unchanged);
        assert_eq!(diffs[1].kind, ToolDiffKind::Deleted);
        assert_eq!(diffs[1].tool_name, "test");
    }

    #[test]
    fn test_align_args_changed() {
        let tools_a = vec![make_tool("edit", serde_json::json!({"path": "a.rs"}))];
        let tools_b = vec![make_tool("edit", serde_json::json!({"path": "b.rs"}))];
        let ha = tools_a.iter().map(semantic_hash).collect::<Vec<_>>();
        let hb = tools_b.iter().map(semantic_hash).collect::<Vec<_>>();

        let diffs = align_tools(&tools_a, &tools_b, &ha, &hb);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].kind, ToolDiffKind::ArgsChanged);
        assert_eq!(diffs[0].tool_name, "edit");
        assert!(diffs[0].args_diff.is_some());
        let ad = diffs[0].args_diff.as_ref().unwrap();
        assert_eq!(ad.args_a, serde_json::json!({"path": "a.rs"}));
        assert_eq!(ad.args_b, serde_json::json!({"path": "b.rs"}));
    }

    #[test]
    fn test_align_empty_traces() {
        let tools_a: Vec<ToolCall> = vec![];
        let tools_b: Vec<ToolCall> = vec![];
        let ha: Vec<u64> = vec![];
        let hb: Vec<u64> = vec![];

        let diffs = align_tools(&tools_a, &tools_b, &ha, &hb);
        assert!(diffs.is_empty());
    }

    #[test]
    fn test_greedy_fallback_used_for_long_sequences() {
        // Create 250 tools — exceeds MAX_EDIT_DISTANCE=200, should use greedy
        let tools_a: Vec<_> = (0..250)
            .map(|i| make_tool("read", serde_json::json!({"idx": i})))
            .collect();
        let tools_b: Vec<_> = (0..250)
            .map(|i| make_tool("read", serde_json::json!({"idx": i})))
            .collect();
        let ha = tools_a.iter().map(semantic_hash).collect::<Vec<_>>();
        let hb = tools_b.iter().map(semantic_hash).collect::<Vec<_>>();

        let diffs = align_tools(&tools_a, &tools_b, &ha, &hb);
        assert_eq!(diffs.len(), 250);
        assert!(diffs.iter().all(|d| d.kind == ToolDiffKind::Unchanged),
            "Greedy fallback should correctly match identical tools");
    }
}
