//! 段级匹配：贪心算法对齐两个 PhaseSegment 序列。

use crate::trajectory::types::{PhaseSegment, SegmentKind};

/// 段匹配结果。
#[derive(Debug, Clone)]
pub struct SegmentMatch {
    pub kind: SegmentKind,
    pub index_a: Option<usize>,
    pub index_b: Option<usize>,
    pub label: String,
}

/// 贪心匹配两个 PhaseSegment 序列。
///
/// 匹配条件：段 label 相同。贪心策略：对 B 中每个段找 A 中第一个未匹配的同 label 段。
/// 未来可扩展为 Hungarian 或 Needleman-Wunsch。
pub fn match_segments(
    segments_a: &[PhaseSegment],
    segments_b: &[PhaseSegment],
) -> Vec<SegmentMatch> {
    let mut used_a = vec![false; segments_a.len()];
    let mut results = Vec::new();

    for (j, seg_b) in segments_b.iter().enumerate() {
        // 在 A 中找第一个未匹配的同 label 段
        let matched = segments_a
            .iter()
            .enumerate()
            .find(|(i, seg_a)| !used_a[*i] && seg_a.label == seg_b.label);

        if let Some((i, _)) = matched {
            used_a[i] = true;
            results.push(SegmentMatch {
                kind: SegmentKind::Unchanged, // 初次标记为 Unchanged，后续由 engine 根据 tool 级差异修正
                index_a: Some(i),
                index_b: Some(j),
                label: seg_b.label.clone(),
            });
        } else {
            results.push(SegmentMatch {
                kind: SegmentKind::Added,
                index_a: None,
                index_b: Some(j),
                label: seg_b.label.clone(),
            });
        }
    }

    // A 中未匹配的段 → Deleted
    for (i, used) in used_a.iter().enumerate() {
        if !used {
            // Insert in correct position relative to B
            let mut insert_pos = results.len();
            for (k, sm) in results.iter().enumerate() {
                if let Some(j) = sm.index_b {
                    if i <= j {
                        insert_pos = k;
                        break;
                    }
                }
            }
            results.insert(
                insert_pos,
                SegmentMatch {
                    kind: SegmentKind::Deleted,
                    index_a: Some(i),
                    index_b: None,
                    label: segments_a[i].label.clone(),
                },
            );
        }
    }

    results
}

// ─── 测试 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::types::{PhaseSegment, ToolIndexInfo};

    fn make_segment(label: &str, indices: Vec<usize>) -> PhaseSegment {
        PhaseSegment {
            label: label.into(),
            tool_indices: indices
                .into_iter()
                .map(|i| ToolIndexInfo {
                    index: i,
                    tool_name: format!("tool_{}", i),
                })
                .collect(),
        }
    }

    #[test]
    fn test_match_identical_segments() {
        let a = vec![
            make_segment("定位问题", vec![0, 1]),
            make_segment("实施修改", vec![2]),
        ];
        let b = vec![
            make_segment("定位问题", vec![0]),
            make_segment("实施修改", vec![1]),
        ];
        let matches = match_segments(&a, &b);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].kind, SegmentKind::Unchanged);
        assert_eq!(matches[1].kind, SegmentKind::Unchanged);
        assert_eq!(matches[0].index_a, Some(0));
        assert_eq!(matches[0].index_b, Some(0));
        assert_eq!(matches[1].index_a, Some(1));
        assert_eq!(matches[1].index_b, Some(1));
    }

    #[test]
    fn test_match_added_segment() {
        let a = vec![make_segment("定位问题", vec![0])];
        let b = vec![
            make_segment("定位问题", vec![0]),
            make_segment("验证", vec![1]),
        ];
        let matches = match_segments(&a, &b);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].kind, SegmentKind::Unchanged);
        assert_eq!(matches[1].kind, SegmentKind::Added);
        assert_eq!(matches[1].index_a, None);
        assert_eq!(matches[1].index_b, Some(1));
    }

    #[test]
    fn test_match_deleted_segment() {
        let a = vec![
            make_segment("定位问题", vec![0]),
            make_segment("验证", vec![1]),
        ];
        let b = vec![make_segment("定位问题", vec![0])];
        let matches = match_segments(&a, &b);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].kind, SegmentKind::Unchanged);
        assert_eq!(matches[1].kind, SegmentKind::Deleted);
        assert_eq!(matches[1].index_a, Some(1));
        assert_eq!(matches[1].index_b, None);
    }

    #[test]
    fn test_match_mixed() {
        let a = vec![
            make_segment("定位问题", vec![0]),
            make_segment("实施修改", vec![1]),
        ];
        let b = vec![
            make_segment("定位问题", vec![0]),
            make_segment("验证", vec![1]),
            make_segment("实施修改", vec![2]),
        ];
        let matches = match_segments(&a, &b);
        assert_eq!(matches.len(), 3);
        // Unchanged: locate
        assert_eq!(matches[0].kind, SegmentKind::Unchanged);
        assert_eq!(matches[0].label, "定位问题");
        // Added: verify
        assert_eq!(matches[1].kind, SegmentKind::Added);
        assert_eq!(matches[1].label, "验证");
        // Unchanged: modify
        assert_eq!(matches[2].kind, SegmentKind::Unchanged);
        assert_eq!(matches[2].label, "实施修改");
    }

    #[test]
    fn test_match_empty_segments() {
        let a: Vec<PhaseSegment> = vec![];
        let b: Vec<PhaseSegment> = vec![];
        let matches = match_segments(&a, &b);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_match_all_deleted() {
        let a = vec![
            make_segment("定位问题", vec![0]),
            make_segment("实施修改", vec![1]),
        ];
        let b: Vec<PhaseSegment> = vec![];
        let matches = match_segments(&a, &b);
        assert_eq!(matches.len(), 2);
        assert!(matches.iter().all(|m| m.kind == SegmentKind::Deleted));
    }
}
