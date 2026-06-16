//! Trajectory Diff 核心数据类型。
//!
//! 零内部依赖，仅依赖 serde。

use serde::{Deserialize, Serialize};

// ─── TrajectoryDiff ───────────────────────────────────────────────

/// 两条 BehaviorTrace 的完整差异对比结果。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrajectoryDiff {
    /// 对比的 Capability ID
    pub capability_id: Option<String>,
    /// 版本 A 的 trace 信息
    pub version_a: TrajectoryVersion,
    /// 版本 B 的 trace 信息
    pub version_b: TrajectoryVersion,
    /// 段级差异
    pub segments: Vec<SegmentDiff>,
    /// 整体摘要
    pub summary: DiffSummary,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrajectoryVersion {
    pub trace_id: String,
    pub session_id: String,
    pub tool_count: usize,
    pub duration_ms: u64,
    pub total_tokens: u64,
    pub started_at: String,
}

// ─── SegmentDiff ──────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SegmentDiff {
    /// 段标签，如 "定位问题"、"实施修改"、"验证"、"提交"
    pub label: String,
    pub kind: SegmentKind,
    pub tool_diffs: Vec<ToolDiff>,
    pub index_a: Option<usize>,
    pub index_b: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum SegmentKind {
    Unchanged,
    Modified,
    Added,
    Deleted,
}

// ─── ToolDiff ─────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolDiff {
    pub tool_name: String,
    pub kind: ToolDiffKind,
    pub index_a: Option<usize>,
    pub index_b: Option<usize>,
    pub args_diff: Option<ArgsDiff>,
    pub duration_ms: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ToolDiffKind {
    Unchanged,
    Added,
    Deleted,
    ArgsChanged,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ArgsDiff {
    pub args_a: serde_json::Value,
    pub args_b: serde_json::Value,
}

// ─── DiffSummary ──────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DiffSummary {
    pub segments_added: usize,
    pub segments_deleted: usize,
    pub segments_modified: usize,
    pub segments_unchanged: usize,
    pub tool_calls_added: usize,
    pub tool_calls_deleted: usize,
    pub tool_calls_modified: usize,
    pub tool_calls_unchanged: usize,
    pub token_delta: i64,
    pub duration_delta_ms: i64,
}

// ─── DiffInput ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum DiffInput {
    ByCapability { capability_id: String },
    Manual { trace_id_a: String, trace_id_b: String },
}

// ─── PhaseSegment (分类器输出) ─────────────────────────────────────

/// PhaseClassifier 的输出：一条 trace 被切割为多个 PhaseSegment。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseSegment {
    pub label: String,
    pub tool_indices: Vec<ToolIndexInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolIndexInfo {
    pub index: usize,
    pub tool_name: String,
}

// ─── P0: BehaviorState & StateGraph ───────────────────────────────

/// PhaseLabel 类型别名。
pub type PhaseLabel = String;

/// 统一行为特征空间。Merge 和 Alignment 共享此结构，
/// 但使用不同的 decision function。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct StateFeatures {
    /// 工具类型直方图: tool_name → normalized frequency
    pub tool_histogram: std::collections::HashMap<String, f32>,
    /// 文件范围向量: file_pattern → normalized frequency
    pub file_clusters: std::collections::HashMap<String, f32>,
    /// 编辑密度: (edit+write+delete tools) / total tools
    pub edit_density: f32,
    /// 读写比例: read tools / total tools (0 if none)
    pub read_write_ratio: f32,
    /// 循环强度: 重复 phase 出现的频次密度
    pub loop_intensity: f32,
    /// 失败率: failure/retry/error tools / total tools
    pub failure_rate: f32,
}

/// State 是 stabilized behavioral cluster。
/// Phase 是 probability distribution，不是分类标签。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BehaviorState {
    pub id: String,
    /// Multi-phase distribution: PhaseLabel → probability
    pub phase_dist: std::collections::HashMap<String, f32>,
    /// Dominant phase (仅用于 UI 显示)
    pub primary_phase: String,
    /// 统一特征向量
    pub features: StateFeatures,
    /// 状态稳定性评分 (0.0–1.0)
    pub stability_score: f32,
    /// 该状态包含的 tool 索引范围 (inclusive start, exclusive end)
    pub tool_range: (usize, usize),
}

/// Layer A: 不可变事件序列，用于 replay / debug / embedding。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TransitionEvent {
    pub from: String,
    pub to: String,
    pub trigger_tool: String,
    pub timestamp: u64,
}

/// Layer B: 聚合图边，用于 visualization / evaluation。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TransitionEdge {
    pub from: String,
    pub to: String,
    pub count: u32,
    pub avg_cost: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BehaviorStateGraph {
    pub trace_id: String,
    pub session_id: String,
    pub states: Vec<BehaviorState>,
    /// Transition 事件序列 (Layer A — 不可变)
    pub event_log: Vec<TransitionEvent>,
    /// Transition 聚合图 (Layer B — 分析视图)
    pub edges: Vec<TransitionEdge>,
    pub total_tools: usize,
}

// ─── P0: StateDiff ────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StateDiff {
    pub graph_a: String,
    pub graph_b: String,
    pub state_pairs: Vec<StatePair>,
    pub metrics: StateDiffMetrics,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StatePair {
    pub state_a: Option<String>,
    pub state_b: Option<String>,
    pub kind: StateDiffKind,
    pub similarity: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum StateDiffKind {
    Matched,
    Added,
    Deleted,
    Split,
    Merged,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct StateDiffMetrics {
    pub state_churn: f32,
    pub transition_churn: f32,
    pub path_divergence: f32,
}

// ─── 内部 Pipeline 类型 ───────────────────────────────────────────

/// Layer 1 输出的原始切段。
#[derive(Debug, Clone)]
pub struct RawSegment {
    pub tool_indices: Vec<usize>,
    pub boundary_reason: BoundaryReason,
}

/// 边界触发原因。
#[derive(Debug, Clone, PartialEq)]
pub enum BoundaryReason {
    ToolTypeChange,
    FailureLoop,
    IdleGap { ms: u64 },
    FileScopeJump,
    Start,
}

/// Layer 2 输出的候选状态。
#[derive(Debug, Clone)]
pub struct StateCandidate {
    pub segment_idx: usize,
    pub tool_indices: Vec<usize>,
    pub features: StateFeatures,
    pub phase_dist: std::collections::HashMap<String, f32>,
}

// ─── 测试 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trajectory_diff_serde_roundtrip() {
        let diff = TrajectoryDiff {
            capability_id: Some("cap_001".into()),
            version_a: TrajectoryVersion {
                trace_id: "trace_a".into(),
                session_id: "sess_a".into(),
                tool_count: 3,
                duration_ms: 1000,
                total_tokens: 500,
                started_at: "2026-06-12T10:00:00Z".into(),
            },
            version_b: TrajectoryVersion {
                trace_id: "trace_b".into(),
                session_id: "sess_b".into(),
                tool_count: 5,
                duration_ms: 1500,
                total_tokens: 800,
                started_at: "2026-06-12T11:00:00Z".into(),
            },
            segments: vec![SegmentDiff {
                label: "定位问题".into(),
                kind: SegmentKind::Unchanged,
                tool_diffs: vec![ToolDiff {
                    tool_name: "read".into(),
                    kind: ToolDiffKind::Unchanged,
                    index_a: Some(0),
                    index_b: Some(0),
                    args_diff: None,
                    duration_ms: 100,
                }],
                index_a: Some(0),
                index_b: Some(0),
            }],
            summary: DiffSummary {
                segments_unchanged: 1,
                tool_calls_unchanged: 1,
                ..Default::default()
            },
        };

        let json = serde_json::to_string_pretty(&diff).unwrap();
        let decoded: TrajectoryDiff = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.capability_id, Some("cap_001".into()));
        assert_eq!(decoded.summary.segments_unchanged, 1);
    }

    #[test]
    fn test_segment_kind_serde() {
        assert_eq!(
            serde_json::to_string(&SegmentKind::Added).unwrap(),
            "\"Added\""
        );
        assert_eq!(
            serde_json::to_string(&SegmentKind::Modified).unwrap(),
            "\"Modified\""
        );
    }

    #[test]
    fn test_tool_diff_kind_serde() {
        assert_eq!(
            serde_json::to_string(&ToolDiffKind::ArgsChanged).unwrap(),
            "\"ArgsChanged\""
        );
    }

    #[test]
    fn test_diff_summary_default() {
        let summary = DiffSummary::default();
        assert_eq!(summary.segments_added, 0);
        assert_eq!(summary.tool_calls_unchanged, 0);
        assert_eq!(summary.token_delta, 0);
    }

    #[test]
    fn test_phase_segment_serde() {
        let seg = PhaseSegment {
            label: "定位问题".into(),
            tool_indices: vec![
                ToolIndexInfo { index: 0, tool_name: "read".into() },
                ToolIndexInfo { index: 1, tool_name: "grep".into() },
            ],
        };
        let json = serde_json::to_string(&seg).unwrap();
        let decoded: PhaseSegment = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.label, "定位问题");
        assert_eq!(decoded.tool_indices.len(), 2);
        assert_eq!(decoded.tool_indices[0].tool_name, "read");
    }
}
