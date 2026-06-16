//! Trajectory Diff 报告生成：Mermaid + JSON。

use super::types::{SegmentKind, ToolDiffKind, TrajectoryDiff};

/// 生成 Mermaid 时序图代码（gantt 格式作为 timeline 替代）。
pub fn to_mermaid(diff: &TrajectoryDiff) -> String {
    let mut out = String::new();

    out.push_str("```mermaid\n");
    out.push_str("gantt\n");
    out.push_str(&format!(
        "    title Trajectory Diff: {} vs {}\n",
        &diff.version_a.trace_id[..diff.version_a.trace_id.len().min(20)],
        &diff.version_b.trace_id[..diff.version_b.trace_id.len().min(20)],
    ));

    if let Some(ref cap) = diff.capability_id {
        out.push_str(&format!("    dateFormat YYYY-MM-DD\n    section Capability\n    {} :cap, 2026-01-01, 1d\n", cap));
    }

    out.push_str(&format!(
        "    section Version A ({} tools)\n",
        diff.version_a.tool_count
    ));

    for (i, seg) in diff.segments.iter().enumerate() {
        let prefix = match seg.kind {
            SegmentKind::Deleted | SegmentKind::Unchanged | SegmentKind::Modified => {
                format!("    {}", seg.label)
            }
            _ => String::new(),
        };
        if !prefix.is_empty() {
            let tool_names: Vec<_> = seg.tool_diffs.iter()
                .filter(|d| d.kind != ToolDiffKind::Added)
                .map(|d| d.tool_name.as_str())
                .collect();
            out.push_str(&format!(
                "{} :a{}, 2026-01-{:02}, 1d\n",
                prefix,
                i + 1,
                i + 1,
            ));
            if !tool_names.is_empty() {
                out.push_str(&format!(
                    "    {} [{}] :a{}, 2026-01-{:02}, 1d\n",
                    seg.label,
                    tool_names.join(", "),
                    i + 1,
                    i + 1,
                ));
            }
        }
    }

    out.push_str(&format!(
        "    section Version B ({} tools)\n",
        diff.version_b.tool_count
    ));

    for (i, seg) in diff.segments.iter().enumerate() {
        let prefix = match seg.kind {
            SegmentKind::Added | SegmentKind::Unchanged | SegmentKind::Modified => {
                format!("    {}", seg.label)
            }
            _ => String::new(),
        };
        if !prefix.is_empty() {
            let tool_names: Vec<_> = seg.tool_diffs.iter()
                .filter(|d| d.kind != ToolDiffKind::Deleted)
                .map(|d| d.tool_name.as_str())
                .collect();
            out.push_str(&format!(
                "{} :b{}, after a{}, 1d\n",
                prefix,
                i + 1,
                i + 1,
            ));
            if !tool_names.is_empty() {
                out.push_str(&format!(
                    "    {} [{}] :b{}, after a{}, 1d\n",
                    seg.label,
                    tool_names.join(", "),
                    i + 1,
                    i + 1,
                ));
            }
        }
    }

    out.push_str("```\n");
    out
}

/// 生成简单的 JSON 报告（不含完整的 tool_calls 详情体）。
pub fn to_json_report(diff: &TrajectoryDiff) -> String {
    serde_json::to_string_pretty(diff).unwrap_or_else(|e| format!("{{ \"error\": \"{}\" }}", e))
}

/// 生成供 dashboard 消费的 JSON 文件内容。
#[derive(serde::Serialize)]
pub struct DashboardJson {
    pub id: String,
    pub capability_id: Option<String>,
    pub trace_id_a: String,
    pub trace_id_b: String,
    pub diff: TrajectoryDiff,
    pub mermaid: String,
    pub computed_at: String,
}

/// 生成终端友好的摘要输出。
pub fn to_terminal_summary(diff: &TrajectoryDiff) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Trajectory Diff: {} → {}\n",
        diff.version_a.trace_id, diff.version_b.trace_id
    ));
    out.push_str(&format!(
        "  Tools: {} → {} (Δ {})\n",
        diff.version_a.tool_count,
        diff.version_b.tool_count,
        diff.version_b.tool_count as i64 - diff.version_a.tool_count as i64
    ));
    out.push_str(&format!(
        "  Segments: +{} -{} ~{} ={}\n",
        diff.summary.segments_added,
        diff.summary.segments_deleted,
        diff.summary.segments_modified,
        diff.summary.segments_unchanged
    ));
    out.push_str(&format!(
        "  Tool Diffs: +{} -{} ~{} ={}\n",
        diff.summary.tool_calls_added,
        diff.summary.tool_calls_deleted,
        diff.summary.tool_calls_modified,
        diff.summary.tool_calls_unchanged
    ));
    out.push_str(&format!(
        "  Token delta: {:+}  Duration delta: {:+}ms\n",
        diff.summary.token_delta, diff.summary.duration_delta_ms
    ));
    out
}

// ─── 测试 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::types::{
        DiffSummary, SegmentDiff, SegmentKind, ToolDiff, ToolDiffKind, TrajectoryVersion,
    };

    fn make_sample_diff() -> TrajectoryDiff {
        TrajectoryDiff {
            capability_id: Some("cap_bug_fix".into()),
            version_a: TrajectoryVersion {
                trace_id: "trace_a_short".into(),
                session_id: "sess_a".into(),
                tool_count: 2,
                duration_ms: 500,
                total_tokens: 300,
                started_at: "2026-06-12T10:00:00Z".into(),
            },
            version_b: TrajectoryVersion {
                trace_id: "trace_b_short".into(),
                session_id: "sess_b".into(),
                tool_count: 3,
                duration_ms: 700,
                total_tokens: 450,
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
        }
    }

    #[test]
    fn test_mermaid_generation() {
        let diff = make_sample_diff();
        let mermaid = to_mermaid(&diff);
        assert!(mermaid.contains("```mermaid"));
        assert!(mermaid.contains("Version A"));
        assert!(mermaid.contains("Version B"));
        assert!(mermaid.contains("trace_a"));
        assert!(mermaid.contains("trace_b"));
    }

    #[test]
    fn test_json_report() {
        let diff = make_sample_diff();
        let json = to_json_report(&diff);
        assert!(json.contains("trace_a_short"));
        assert!(json.contains("cap_bug_fix"));
    }

    #[test]
    fn test_terminal_summary() {
        let diff = make_sample_diff();
        let summary = to_terminal_summary(&diff);
        assert!(summary.contains("trace_a_short"));
        assert!(summary.contains("trace_b_short"));
        assert!(summary.contains("Tools:"));
        assert!(summary.contains("Segments:"));
    }

    #[test]
    fn test_dashboard_json() {
        let diff = make_sample_diff();
        let mermaid = to_mermaid(&diff);
        let dashboard = DashboardJson {
            id: "tdiff_001".into(),
            capability_id: diff.capability_id.clone(),
            trace_id_a: diff.version_a.trace_id.clone(),
            trace_id_b: diff.version_b.trace_id.clone(),
            mermaid,
            computed_at: "2026-06-12T12:00:00Z".into(),
            diff,
        };
        let json = serde_json::to_string_pretty(&dashboard).unwrap();
        assert!(json.contains("tdiff_001"));
        assert!(json.contains("cap_bug_fix"));
        assert!(json.contains("mermaid"));
    }
}
