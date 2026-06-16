//! Layer 3: 相邻合并 (adjacent-only merge)。
//!
//! 基于 merge_similarity (宽松，threshold 0.85) 将相邻 StateCandidate
//! 合并为 BehaviorState。不可跨非相邻候选合并。

use crate::trajectory::types::{BehaviorState, StateCandidate, StateFeatures};
use crate::trajectory::similarity::merge_sim::merge_similarity;

/// 相邻合并器。
pub struct AdjacentMerger {
    pub threshold: f32,
}

impl Default for AdjacentMerger {
    fn default() -> Self {
        Self { threshold: 0.85 }
    }
}

impl AdjacentMerger {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }

    /// 仅合并相邻 StateCandidate。返回 BehaviorState 列表。
    pub fn merge(&self, candidates: &[StateCandidate]) -> Vec<BehaviorState> {
        if candidates.is_empty() {
            return Vec::new();
        }

        let mut states: Vec<BehaviorState> = Vec::new();
        let mut group: Vec<&StateCandidate> = vec![&candidates[0]];

        for i in 1..candidates.len() {
            let prev = &candidates[i - 1];
            let curr = &candidates[i];

            let sim = merge_similarity(&prev.features, &curr.features);

            if sim >= self.threshold {
                // 相邻且相似 → 合并到同一组
                group.push(curr);
            } else {
                // 相似度不够 → 封闭当前组，开始新组
                states.push(self.build_state(&group, states.len()));
                group = vec![curr];
            }
        }

        // 最后一组
        if !group.is_empty() {
            states.push(self.build_state(&group, states.len()));
        }

        states
    }

    /// 从一组 candidate 构造一个 BehaviorState。
    fn build_state(&self, group: &[&StateCandidate], state_idx: usize) -> BehaviorState {
        // Merge features: 取平均值
        let mut features = StateFeatures::default();
        let n = group.len() as f32;
        for c in group {
            for (k, v) in &c.features.tool_histogram {
                *features.tool_histogram.entry(k.clone()).or_insert(0.0) += v;
            }
            for (k, v) in &c.features.file_clusters {
                *features.file_clusters.entry(k.clone()).or_insert(0.0) += v;
            }
            features.edit_density += c.features.edit_density;
            features.read_write_ratio += c.features.read_write_ratio;
            features.loop_intensity += c.features.loop_intensity;
            features.failure_rate += c.features.failure_rate;
        }
        if n > 0.0 {
            for v in features.tool_histogram.values_mut() { *v /= n; }
            for v in features.file_clusters.values_mut() { *v /= n; }
            features.edit_density /= n;
            features.read_write_ratio /= n;
            features.loop_intensity /= n;
            features.failure_rate /= n;
        }

        // Merge phase distributions
        let mut phase_dist: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
        for c in group {
            for (phase, prob) in &c.phase_dist {
                *phase_dist.entry(phase.clone()).or_insert(0.0) += prob;
            }
        }
        for v in phase_dist.values_mut() {
            *v /= n;
        }

        // Primary phase = argmax
        let primary_phase = phase_dist
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(k, _)| k.clone())
            .unwrap_or_else(|| "other".into());

        // Stability score: inverse of variance in features
        let stability_score = 1.0 - features.loop_intensity.min(1.0);

        // Tool range: first candidate's first index to last candidate's last index
        let start = group.first().map(|c| c.tool_indices.first().copied().unwrap_or(0)).unwrap_or(0);
        let end = group.last().map(|c| c.tool_indices.last().copied().unwrap_or(0)).unwrap_or(0);

        BehaviorState {
            id: format!("s{}", state_idx),
            phase_dist,
            primary_phase,
            features,
            stability_score,
            tool_range: (start, end + 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::types::ToolCall;

    fn tc(name: &str, id: &str) -> ToolCall {
        ToolCall { id: id.into(), tool_name: name.into(), args: serde_json::json!({}), timestamp: "now".into(), duration_ms: 100, result_id: None }
    }

    fn make_candidate(tools: &[ToolCall], phase: &str, idx: usize) -> StateCandidate {
        let mut phase_dist = std::collections::HashMap::new();
        phase_dist.insert(phase.to_string(), 1.0);
        let all: Vec<_> = tools.iter().map(|t| t.clone()).collect();
        let features = crate::trajectory::state::features::extract_features(&all, &all);
        StateCandidate {
            segment_idx: idx,
            tool_indices: (0..tools.len()).collect(),
            features,
            phase_dist,
        }
    }

    #[test]
    fn test_merge_empty() {
        let m = AdjacentMerger::default();
        assert!(m.merge(&[]).is_empty());
    }

    #[test]
    fn test_single_candidate() {
        let m = AdjacentMerger::default();
        let tools = vec![tc("edit", "c1"), tc("edit", "c2")];
        let candidates = vec![make_candidate(&tools, "modify", 0)];
        let states = m.merge(&candidates);
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].primary_phase, "modify");
    }

    #[test]
    fn test_merge_similar_adjacent() {
        let m = AdjacentMerger::default();
        let tools1 = vec![tc("edit", "c1"), tc("write", "c2")];
        let tools2 = vec![tc("edit", "c3"), tc("delete_file", "c4")];
        let candidates = vec![
            make_candidate(&tools1, "modify", 0),
            make_candidate(&tools2, "modify", 1),
        ];
        let states = m.merge(&candidates);
        // Very similar → should merge to 1
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].primary_phase, "modify");
    }

    #[test]
    fn test_not_merge_dissimilar() {
        let m = AdjacentMerger::default();
        let tools1 = vec![tc("read", "c1"), tc("read", "c2"), tc("read", "c3")];
        let tools2 = vec![tc("commit", "c4")];
        let candidates = vec![
            make_candidate(&tools1, "locate", 0),
            make_candidate(&tools2, "commit", 1),
        ];
        let states = m.merge(&candidates);
        // Very different → should split
        assert_eq!(states.len(), 2);
        assert_eq!(states[0].primary_phase, "locate");
        assert_eq!(states[1].primary_phase, "commit");
    }

    #[test]
    fn test_stability_score() {
        let m = AdjacentMerger::default();
        let tools = vec![tc("read", "c1"), tc("grep", "c2"), tc("read", "c3")];
        let candidates = vec![make_candidate(&tools, "locate", 0)];
        let states = m.merge(&candidates);
        assert!(states[0].stability_score >= 0.0 && states[0].stability_score <= 1.0);
    }
}
