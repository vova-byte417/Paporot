//! TrajectoryAnalysis：Diff → Eval 的中间层。
//!
//! Eval 规则消费此类型而非直接消费 TrajectoryDiff。
//! 纯确定性计算，不调用 LLM。

use serde::{Deserialize, Serialize};

use super::types::{SegmentKind, ToolDiffKind, TrajectoryDiff};

/// 对 TrajectoryDiff 的结构化分析。
///
/// 将原始 diff 数据聚合为可直接供 Eval 规则使用的统计量。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrajectoryAnalysis {
    pub trace_id_a: String,
    pub trace_id_b: String,

    // ── 阶段变化 ──
    /// 新增的阶段
    pub phase_additions: Vec<PhaseChange>,
    /// 删除的阶段
    pub phase_deletions: Vec<PhaseChange>,
    /// 修改的阶段
    pub phase_modifications: Vec<PhaseModification>,

    // ── 评分类指标 (0.0 ~ 1.0) ──
    /// Tool churn: (additions + deletions) / total tools
    pub tool_churn_score: f32,
    /// 阶段重排序程度: (additions + deletions) / total phases
    pub phase_reorder_score: f32,
    /// 相同 tool 但 args 变化的比例
    pub capability_shift_score: f32,

    // ── 统计摘要 ──
    pub tool_count_a: usize,
    pub tool_count_b: usize,
    pub shared_tool_count: usize,
    pub added_tool_count: usize,
    pub deleted_tool_count: usize,
    pub args_changed_tool_count: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PhaseChange {
    pub label: String,
    pub tool_names: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PhaseModification {
    pub label: String,
    pub tool_count_before: usize,
    pub tool_count_after: usize,
    pub added_tools: Vec<String>,
    pub deleted_tools: Vec<String>,
}

impl TrajectoryAnalysis {
    /// 从 TrajectoryDiff 计算分析结果。纯确定性，零 LLM 调用。
    pub fn from_diff(diff: &TrajectoryDiff) -> Self {
        let mut analysis = TrajectoryAnalysis {
            trace_id_a: diff.version_a.trace_id.clone(),
            trace_id_b: diff.version_b.trace_id.clone(),
            phase_additions: Vec::new(),
            phase_deletions: Vec::new(),
            phase_modifications: Vec::new(),
            tool_churn_score: 0.0,
            phase_reorder_score: 0.0,
            capability_shift_score: 0.0,
            tool_count_a: diff.version_a.tool_count,
            tool_count_b: diff.version_b.tool_count,
            shared_tool_count: diff.summary.tool_calls_unchanged,
            added_tool_count: diff.summary.tool_calls_added,
            deleted_tool_count: diff.summary.tool_calls_deleted,
            args_changed_tool_count: diff.summary.tool_calls_modified,
        };

        for seg in &diff.segments {
            match seg.kind {
                SegmentKind::Added => {
                    analysis.phase_additions.push(PhaseChange {
                        label: seg.label.clone(),
                        tool_names: seg.tool_diffs.iter().map(|t| t.tool_name.clone()).collect(),
                    });
                }
                SegmentKind::Deleted => {
                    analysis.phase_deletions.push(PhaseChange {
                        label: seg.label.clone(),
                        tool_names: seg.tool_diffs.iter().map(|t| t.tool_name.clone()).collect(),
                    });
                }
                SegmentKind::Modified => {
                    let mut mod_info = PhaseModification {
                        label: seg.label.clone(),
                        tool_count_before: 0,
                        tool_count_after: 0,
                        added_tools: Vec::new(),
                        deleted_tools: Vec::new(),
                    };
                    for td in &seg.tool_diffs {
                        match td.kind {
                            ToolDiffKind::Added => {
                                mod_info.tool_count_after += 1;
                                mod_info.added_tools.push(td.tool_name.clone());
                            }
                            ToolDiffKind::Deleted => {
                                mod_info.tool_count_before += 1;
                                mod_info.deleted_tools.push(td.tool_name.clone());
                            }
                            _ => {
                                mod_info.tool_count_before += 1;
                                mod_info.tool_count_after += 1;
                            }
                        }
                    }
                    analysis.phase_modifications.push(mod_info);
                }
                SegmentKind::Unchanged => {}
            }
        }

        // Compute scores
        let total_tools = (analysis.tool_count_a + analysis.tool_count_b) as f32;
        if total_tools > 0.0 {
            analysis.tool_churn_score =
                (analysis.added_tool_count + analysis.deleted_tool_count) as f32 / total_tools;
            analysis.capability_shift_score =
                analysis.args_changed_tool_count as f32 / total_tools;
        }

        let total_phases = (analysis.phase_additions.len()
            + analysis.phase_deletions.len()
            + analysis.phase_modifications.len()
            + diff.summary.segments_unchanged) as f32;
        if total_phases > 0.0 {
            analysis.phase_reorder_score =
                (analysis.phase_additions.len() + analysis.phase_deletions.len()) as f32
                    / total_phases;
        }

        analysis
    }
}

// ─── 测试 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::types::{
        DiffSummary, SegmentDiff, SegmentKind, ToolDiff, ToolDiffKind, TrajectoryDiff,
        TrajectoryVersion,
    };

    fn empty_diff() -> TrajectoryDiff {
        TrajectoryDiff {
            capability_id: None,
            version_a: TrajectoryVersion {
                trace_id: "ta".into(), session_id: "sa".into(),
                tool_count: 0, duration_ms: 0, total_tokens: 0,
                started_at: "now".into(),
            },
            version_b: TrajectoryVersion {
                trace_id: "tb".into(), session_id: "sb".into(),
                tool_count: 0, duration_ms: 0, total_tokens: 0,
                started_at: "now".into(),
            },
            segments: vec![],
            summary: DiffSummary::default(),
        }
    }

    fn make_tool_diff(name: &str, kind: ToolDiffKind) -> ToolDiff {
        ToolDiff {
            tool_name: name.into(),
            kind,
            index_a: None, index_b: None,
            args_diff: None, duration_ms: 0,
        }
    }

    #[test]
    fn test_from_diff_empty() {
        let analysis = TrajectoryAnalysis::from_diff(&empty_diff());
        assert_eq!(analysis.tool_churn_score, 0.0);
        assert_eq!(analysis.phase_reorder_score, 0.0);
        assert_eq!(analysis.capability_shift_score, 0.0);
        assert!(analysis.phase_additions.is_empty());
        assert!(analysis.phase_deletions.is_empty());
    }

    #[test]
    fn test_from_diff_phase_additions() {
        let mut diff = empty_diff();
        diff.version_a.tool_count = 2;
        diff.version_b.tool_count = 5;
        diff.segments = vec![SegmentDiff {
            label: "验证".into(),
            kind: SegmentKind::Added,
            tool_diffs: vec![
                make_tool_diff("test", ToolDiffKind::Added),
                make_tool_diff("lint", ToolDiffKind::Added),
            ],
            index_a: None, index_b: Some(0),
        }];
        diff.summary.segments_added = 1;
        diff.summary.tool_calls_added = 2;

        let analysis = TrajectoryAnalysis::from_diff(&diff);
        assert_eq!(analysis.phase_additions.len(), 1);
        assert_eq!(analysis.phase_additions[0].label, "验证");
        assert_eq!(analysis.phase_additions[0].tool_names.len(), 2);
        assert_eq!(analysis.added_tool_count, 2);
    }

    #[test]
    fn test_from_diff_phase_deletions() {
        let mut diff = empty_diff();
        diff.version_a.tool_count = 3;
        diff.version_b.tool_count = 1;
        diff.segments = vec![SegmentDiff {
            label: "提交".into(),
            kind: SegmentKind::Deleted,
            tool_diffs: vec![
                make_tool_diff("commit", ToolDiffKind::Deleted),
            ],
            index_a: Some(0), index_b: None,
        }];
        diff.summary.segments_deleted = 1;
        diff.summary.tool_calls_deleted = 1;

        let analysis = TrajectoryAnalysis::from_diff(&diff);
        assert_eq!(analysis.phase_deletions.len(), 1);
        assert_eq!(analysis.phase_deletions[0].label, "提交");
        assert_eq!(analysis.deleted_tool_count, 1);
    }

    #[test]
    fn test_from_diff_phase_modifications() {
        let mut diff = empty_diff();
        diff.version_a.tool_count = 3;
        diff.version_b.tool_count = 4;
        diff.segments = vec![SegmentDiff {
            label: "实施修改".into(),
            kind: SegmentKind::Modified,
            tool_diffs: vec![
                make_tool_diff("edit", ToolDiffKind::Unchanged),
                make_tool_diff("write", ToolDiffKind::Unchanged),
                make_tool_diff("run_command", ToolDiffKind::Added),
            ],
            index_a: Some(0), index_b: Some(0),
        }];
        diff.summary.segments_modified = 1;
        diff.summary.tool_calls_unchanged = 2;
        diff.summary.tool_calls_added = 1;

        let analysis = TrajectoryAnalysis::from_diff(&diff);
        assert_eq!(analysis.phase_modifications.len(), 1);
        let pm = &analysis.phase_modifications[0];
        assert_eq!(pm.label, "实施修改");
        assert_eq!(pm.tool_count_before, 2);
        assert_eq!(pm.tool_count_after, 3);
        assert_eq!(pm.added_tools.len(), 1);
    }

    #[test]
    fn test_tool_churn_score() {
        let mut diff = empty_diff();
        diff.version_a.tool_count = 2;
        diff.version_b.tool_count = 2;
        diff.summary.tool_calls_added = 1;
        diff.summary.tool_calls_deleted = 1;

        let analysis = TrajectoryAnalysis::from_diff(&diff);
        // (1 + 1) / (2 + 2) = 0.5
        assert!((analysis.tool_churn_score - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_phase_reorder_score() {
        let mut diff = empty_diff();
        diff.version_a.tool_count = 2;
        diff.version_b.tool_count = 2;
        diff.segments = vec![
            SegmentDiff {
                label: "定位问题".into(),
                kind: SegmentKind::Unchanged,
                tool_diffs: vec![make_tool_diff("read", ToolDiffKind::Unchanged)],
                index_a: Some(0), index_b: Some(0),
            },
            SegmentDiff {
                label: "验证".into(),
                kind: SegmentKind::Added,
                tool_diffs: vec![make_tool_diff("test", ToolDiffKind::Added)],
                index_a: None, index_b: Some(1),
            },
        ];
        diff.summary.segments_unchanged = 1;
        diff.summary.segments_added = 1;
        diff.summary.tool_calls_unchanged = 1;
        diff.summary.tool_calls_added = 1;

        let analysis = TrajectoryAnalysis::from_diff(&diff);
        // (1 addition + 0 deletions) / (1 + 0 + 0 + 1) = 0.5
        assert!((analysis.phase_reorder_score - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_capability_shift_score() {
        let mut diff = empty_diff();
        diff.version_a.tool_count = 2;
        diff.version_b.tool_count = 2;
        diff.summary.tool_calls_modified = 1;
        diff.summary.tool_calls_unchanged = 1;

        let analysis = TrajectoryAnalysis::from_diff(&diff);
        // 1 / (2 + 2) = 0.25
        assert!((analysis.capability_shift_score - 0.25).abs() < 0.001);
    }
}
