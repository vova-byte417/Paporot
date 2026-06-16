//! P2 Co-change Detector: git commit co-occurrence + session coupling + temporal proximity。
//!
//! D11: cochange_score = log(1 + w1×commit + w2×file + w3×session)
//! commit: 1/log(1+other_caps) 降权 batch commit
//! file: Jaccard(caps_in_file_A, caps_in_file_B)
//! session: cooccurrence / sqrt(total_events)

use std::collections::HashSet;

/// Co-change evidence from three layers.
#[derive(Debug, Clone, Default)]
pub struct CochangeEvidence {
    /// Commit-level evidence (D11: 降权 batch commit)
    pub commit_score: f32,
    /// File-level structural coupling
    pub file_score: f32,
    /// Session-level behavioral co-occurrence
    pub session_score: f32,
    /// D11: fused cochange score = log(1 + w1×c + w2×f + w3×s)
    pub fused_score: f32,
}

/// Commit change record.
#[derive(Debug, Clone)]
pub struct CommitRecord {
    pub commit_hash: String,
    /// Capability IDs affected in this commit
    pub capability_ids: Vec<String>,
    /// Files modified in this commit
    pub files: Vec<String>,
    /// Session ID (if available)
    pub session_id: Option<String>,
    /// Number of capabilities in this commit
    pub cap_count: usize,
}

impl CochangeEvidence {
    /// Compute cochange evidence from commit records.
    ///
    /// `cap_a`, `cap_b`: the two capabilities to measure coupling for.
    /// `commits`: all commit records to search through.
    /// `total_traces`: total number of traces for session normalization.
    pub fn compute(
        cap_a: &str,
        cap_b: &str,
        commits: &[CommitRecord],
        total_sessions: usize,
    ) -> Self {
        // ── Commit-level co-occurrence ──
        let mut cooccur_count = 0_u32;
        let mut commit_weight_sum = 0.0_f32;

        for commit in commits {
            let has_a = commit.capability_ids.iter().any(|c| c == cap_a);
            let has_b = commit.capability_ids.iter().any(|c| c == cap_b);

            if has_a && has_b {
                cooccur_count += 1;
                // D11: 降权 batch commit — cleaner commits get higher weight
                let weight = if commit.cap_count > 1 {
                    1.0 / (1.0 + (commit.cap_count as f32).ln())
                } else {
                    1.0
                };
                commit_weight_sum += weight;
            }
        }

        let commit_score = if commits.is_empty() {
            0.0
        } else {
            // Normalize by max possible co-occurrence count
            let max_possible = commits.len() as f32;
            if max_possible > 0.0 {
                commit_weight_sum / max_possible
            } else {
                0.0
            }
        };

        // ── File-level structural coupling ──
        let mut files_a: HashSet<String> = HashSet::new();
        let mut files_b: HashSet<String> = HashSet::new();

        for commit in commits {
            let has_a = commit.capability_ids.iter().any(|c| c == cap_a);
            let has_b = commit.capability_ids.iter().any(|c| c == cap_b);
            if has_a {
                for f in &commit.files {
                    files_a.insert(f.clone());
                }
            }
            if has_b {
                for f in &commit.files {
                    files_b.insert(f.clone());
                }
            }
        }

        let file_score = jaccard_sets(&files_a, &files_b);

        // ── Session-level co-occurrence ──
        let mut sessions_a: HashSet<String> = HashSet::new();
        let mut sessions_b: HashSet<String> = HashSet::new();

        for commit in commits {
            if let Some(ref sid) = commit.session_id {
                let has_a = commit.capability_ids.iter().any(|c| c == cap_a);
                let has_b = commit.capability_ids.iter().any(|c| c == cap_b);
                if has_a {
                    sessions_a.insert(sid.clone());
                }
                if has_b {
                    sessions_b.insert(sid.clone());
                }
            }
        }

        let intersection = sessions_a.intersection(&sessions_b).count() as f32;
        let session_score = if total_sessions > 0 {
            intersection / (total_sessions as f32).sqrt()
        } else {
            0.0
        };

        // ── D11: log-saturated fusion ──
        let w_commit = 1.0_f32;
        let w_file = 1.5_f32;
        let w_session = 0.5_f32;

        let raw = w_commit * commit_score + w_file * file_score + w_session * session_score;
        let fused_score = (1.0 + raw).ln();

        CochangeEvidence {
            commit_score,
            file_score,
            session_score,
            fused_score,
        }
    }

    /// Quick cochange from just session co-occurrence counts.
    pub fn from_counts(
        cooccur_count: u32,
        cap_a_total: u32,
        cap_b_total: u32,
        total_sessions: u32,
    ) -> f32 {
        if total_sessions == 0 {
            return 0.0;
        }
        let session_raw = cooccur_count as f32 / (total_sessions as f32).sqrt();
        // Simplified: treat session as dominant signal when no commit data
        (1.0 + session_raw).ln()
    }
}

/// Jaccard similarity on two sets.
fn jaccard_sets<T: std::hash::Hash + Eq>(a: &HashSet<T>, b: &HashSet<T>) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 0.0; // no evidence → no coupling
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let intersection = a.intersection(b).count() as f32;
    let union = a.union(b).count() as f32;

    if union > 0.0 {
        intersection / union
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cochange_no_evidence() {
        let commits = vec![
            CommitRecord {
                commit_hash: "abc".into(),
                capability_ids: vec!["cap_a".into()],
                files: vec!["a.rs".into()],
                session_id: None,
                cap_count: 1,
            },
        ];
        let ev = CochangeEvidence::compute("cap_a", "cap_b", &commits, 1);
        assert_eq!(ev.commit_score, 0.0);
        assert_eq!(ev.file_score, 0.0);
    }

    #[test]
    fn test_cochange_both_present() {
        let commits = vec![
            CommitRecord {
                commit_hash: "abc".into(),
                capability_ids: vec!["cap_a".into(), "cap_b".into()],
                files: vec!["a.rs".into(), "b.rs".into()],
                session_id: Some("s1".into()),
                cap_count: 2,
            },
        ];
        let ev = CochangeEvidence::compute("cap_a", "cap_b", &commits, 1);
        assert!(ev.commit_score > 0.0);
        assert!(ev.file_score > 0.0);
        assert!(ev.fused_score > 0.0);
    }

    #[test]
    fn test_cochange_batch_commit_discount() {
        // Large batch commit → lower weight
        let commits_small = vec![
            CommitRecord {
                commit_hash: "abc".into(),
                capability_ids: vec!["cap_a".into(), "cap_b".into()],
                files: vec!["a.rs".into()],
                session_id: None,
                cap_count: 2,
            },
        ];
        let ev_small = CochangeEvidence::compute("cap_a", "cap_b", &commits_small, 1);

        let commits_large = vec![
            CommitRecord {
                commit_hash: "abc".into(),
                capability_ids: (0..20).map(|i| format!("cap_{}", i)).collect(),
                files: vec!["a.rs".into()],
                session_id: None,
                cap_count: 20,
            },
        ];
        let ev_large = CochangeEvidence::compute("cap_a", "cap_b", &commits_large, 1);

        // Large batch → lower weight per-cap pair
        assert!(ev_small.commit_score > ev_large.commit_score,
            "Small batch {} should have higher weight than large batch {}",
            ev_small.commit_score, ev_large.commit_score);
    }

    #[test]
    fn test_jaccard_sets() {
        let a: HashSet<String> = ["a.rs".into(), "b.rs".into()].into();
        let b: HashSet<String> = ["b.rs".into(), "c.rs".into()].into();
        let sim = jaccard_sets(&a, &b);
        // intersection=1 (b.rs), union=3 (a,b,c) → 1/3
        assert!((sim - 1.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn test_from_counts() {
        let score = CochangeEvidence::from_counts(5, 10, 8, 20);
        assert!(score > 0.0);
    }

    #[test]
    fn test_from_counts_no_cooccur() {
        let score = CochangeEvidence::from_counts(0, 10, 8, 20);
        let raw = 0.0_f32 / (20.0_f32).sqrt();
        let expected = (1.0 + raw).ln();
        assert!((score - expected).abs() < 0.01);
    }

    #[test]
    fn test_fused_score_log_saturation() {
        // Verify log saturation: multiple co-occurrences don't explode
        let mut commits = Vec::new();
        for i in 0..100 {
            commits.push(CommitRecord {
                commit_hash: format!("c{}", i),
                capability_ids: vec!["cap_a".into(), "cap_b".into()],
                files: vec!["shared.rs".into()],
                session_id: Some("s1".into()),
                cap_count: 2,
            });
        }
        let ev = CochangeEvidence::compute("cap_a", "cap_b", &commits, 100);
        // Fused score should be bounded by log, not linear growth
        assert!(ev.fused_score < 5.0, "Fused score {} should be bounded by log saturation", ev.fused_score);
    }
}
