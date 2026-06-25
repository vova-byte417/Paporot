//! Snapshot diff 包装器（纯函数，复用 paporot-analysis-types）
//!
//! 在 native 端提供 diff 接口，实际逻辑与 paporot-core 的 SnapshotAnalyzer 一致。

use crate::types::*;
use std::collections::HashMap;

/// 计算两个 Snapshot 之间的行为差异（纯函数，零 I/O）
pub fn diff(prev: &BehaviorSnapshot, curr: &BehaviorSnapshot) -> BehaviorDiff {
    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut deleted = Vec::new();
    let mut unchanged = Vec::new();

    let prev_map: HashMap<&str, &Capability> =
        prev.capabilities.iter().map(|c| (c.id.as_str(), c)).collect();
    let curr_map: HashMap<&str, &Capability> =
        curr.capabilities.iter().map(|c| (c.id.as_str(), c)).collect();

    for cap in &curr.capabilities {
        match prev_map.get(cap.id.as_str()) {
            None => added.push(cap.clone()),
            Some(prev_cap) => {
                if prev_cap.name != cap.name || prev_cap.description != cap.description {
                    modified.push(cap.clone());
                } else {
                    unchanged.push(cap.clone());
                }
            }
        }
    }

    for cap in &prev.capabilities {
        if !curr_map.contains_key(cap.id.as_str()) {
            deleted.push(cap.clone());
        }
    }

    let a_len = added.len();
    let m_len = modified.len();
    let d_len = deleted.len();
    let u_len = unchanged.len();
    let total = a_len + m_len + d_len + u_len;

    BehaviorDiff {
        from_version: prev.version_id.clone(),
        to_version: curr.version_id.clone(),
        timestamp: curr.timestamp.clone(),
        added,
        modified,
        deleted,
        unchanged,
        impact_summary: format!(
            "{} caps changed ({} total): +{} ~{} -{} ={}",
            a_len + m_len + d_len, total, a_len, m_len, d_len, u_len
        ),
        risks_and_notes: vec![],
    }
}
