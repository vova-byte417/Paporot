//! Feedback Loader
//!
//! 在 paporot analyze 之前，从 .Paporot/reviews/reviews.json
//! 和 .Paporot/rules/suppressions.toml 加载历史反馈数据，
//! 构建 paporot_analysis_types::FeedbackIndex，序列化为 JSON。

use crate::types::*;
use paporot_analysis_types::FeedbackIndex;
use std::collections::HashMap;
use std::path::Path;

/// 加载结果统计
#[derive(Debug)]
pub struct LoadResult {
    pub total_reviews: usize,
    pub rejected_count: u32,
    pub suppression_count: usize,
    pub prefix_count: usize,
}

/// 加载反馈数据并构建 FeedbackIndex
pub fn build_feedback_index(
    paporot_dir: &Path,
    snapshot: &BehaviorSnapshot,
) -> anyhow::Result<(FeedbackIndex, LoadResult)> {
    let reviews_path = paporot_dir.join("reviews").join("reviews.json");
    let suppressions_path = paporot_dir.join("rules").join("suppressions.toml");

    // ── Layer 1: Exact reject map ──
    let mut exact_reject_map: HashMap<String, String> = HashMap::new();
    let mut rejected_prefixes: Vec<String> = Vec::new();

    let fb = if reviews_path.exists() {
        FeedbackStore::load_or_new(&reviews_path)?
    } else {
        FeedbackStore {
            reviews: vec![],
            stats: FeedbackStats::default(),
        }
    };

    let total_reviews = fb.reviews.len();
    let mut rejected_count = 0u32;

    for review in &fb.reviews {
        if review.verdict == ReviewVerdict::Rejected {
            rejected_count += 1;

            // 构建 exact match key: (symbol_name, file_path, change_type)
            if let (Some(ref symbol), Some(ref file), Some(ref change_type_str)) =
                (&review.source_symbol, &review.source_file, &review.source_change_type)
            {
                let key = make_exact_key(symbol, file, change_type_str);
                let reason = review
                    .comment
                    .clone()
                    .unwrap_or_else(|| "rejected in review".into());
                exact_reject_map.insert(key, reason);
            } else {
                // 旧版 review 没有溯源字段：回退到 capability_id → capability 匹配
                if let Some(cap) = snapshot
                    .capabilities
                    .iter()
                    .find(|c| c.id == review.capability_id)
                {
                    let file = cap.evidence.first().cloned().unwrap_or_default();
                    let key = make_exact_key(&cap.name, &file, "unknown");
                    let reason = review
                        .comment
                        .clone()
                        .unwrap_or_else(|| "rejected in review".into());
                    exact_reject_map.insert(key, reason);
                }
            }

            // Layer 3: 收集文件前缀
            if let Some(ref file) = review.source_file {
                let prefix = file_prefix_2level(file);
                if !rejected_prefixes.contains(&prefix) {
                    rejected_prefixes.push(prefix);
                }
            } else if let Some(cap) = snapshot
                .capabilities
                .iter()
                .find(|c| c.id == review.capability_id)
            {
                if let Some(file) = cap.evidence.first() {
                    let prefix = file_prefix_2level(file);
                    if !rejected_prefixes.contains(&prefix) {
                        rejected_prefixes.push(prefix);
                    }
                }
            }
        }
    }

    // ── Layer 2: Rule suppressions ──
    let mut rule_suppressions = Vec::new();
    let suppression_count;

    if suppressions_path.exists() {
        let toml_str = std::fs::read_to_string(&suppressions_path)?;
        let parsed: SuppressionsFile = toml::from_str(&toml_str)?;
        suppression_count = parsed.suppression.len();
        rule_suppressions = parsed
            .suppression
            .into_iter()
            .map(|s| s.into_shared())
            .collect();
    } else {
        suppression_count = 0;
    }

    let prefix_count = rejected_prefixes.len();

    let index = FeedbackIndex {
        exact_reject_map,
        rule_suppressions,
        rejected_prefixes,
    };

    Ok((
        index,
        LoadResult {
            total_reviews,
            rejected_count,
            suppression_count,
            prefix_count,
        },
    ))
}

/// 将 FeedbackIndex 写入 .Paporot/work/feedback_index.json
pub fn write_feedback_index(paporot_dir: &Path, index: &FeedbackIndex) -> anyhow::Result<()> {
    let work_dir = paporot_dir.join("work");
    std::fs::create_dir_all(&work_dir)?;
    let path = work_dir.join("feedback_index.json");
    let json = serde_json::to_string(index)?;
    std::fs::write(&path, json)?;
    Ok(())
}

// ─── 内部工具函数 ───────────────────────────────────────────────────

fn make_exact_key(symbol: &str, file: &str, change_type: &str) -> String {
    format!("{}::{}::{}", symbol, file, change_type)
}

fn file_prefix_2level(file: &str) -> String {
    let parts: Vec<&str> = file.split('/').collect();
    if parts.len() >= 2 {
        format!("{}/{}", parts[0], parts[1])
    } else {
        file.to_string()
    }
}

// ─── Suppressions TOML 解析结构 ──────────────────────────────────────

#[derive(serde::Deserialize)]
struct SuppressionsFile {
    suppression: Vec<SuppressionEntry>,
}

#[derive(serde::Deserialize)]
struct SuppressionEntry {
    rule_id: String,
    file_pattern: String,
    #[serde(default)]
    change_type: Option<String>,
    #[serde(default = "default_effect")]
    effect: String,
    reason: String,
    #[serde(default)]
    created_by: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    source_review: String,
    #[serde(default)]
    hit_count: u32,
    #[serde(default)]
    last_hit: Option<String>,
    #[serde(default = "default_status")]
    status: String,
}

fn default_effect() -> String {
    "suppress".into()
}

fn default_status() -> String {
    "active".into()
}

impl SuppressionEntry {
    fn into_shared(self) -> paporot_analysis_types::RuleSuppression {
        paporot_analysis_types::RuleSuppression {
            rule_id: self.rule_id,
            file_pattern: self.file_pattern,
            change_type: self.change_type,
            effect: match self.effect.as_str() {
                "warn" => paporot_analysis_types::SuppressionEffect::Warn,
                _ => paporot_analysis_types::SuppressionEffect::Suppress,
            },
            reason: self.reason,
            created_by: self.created_by,
            created_at: self.created_at,
            source_review: self.source_review,
            hit_count: self.hit_count,
            last_hit: self.last_hit,
            status: match self.status.as_str() {
                "stale" => paporot_analysis_types::SuppressionStatus::Stale,
                "revoked" => paporot_analysis_types::SuppressionStatus::Revoked,
                _ => paporot_analysis_types::SuppressionStatus::Active,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_empty_snapshot() -> BehaviorSnapshot {
        BehaviorSnapshot {
            schema_version: 3,
            version_id: "v1".into(),
            git_commit: None,
            git_ref: None,
            timestamp: "2026-01-01T00:00:00Z".into(),
            message: "empty".into(),
            capabilities: vec![],
            prd_coverage: PrdCoverage {
                percentage: 0.0,
                total_items: 0,
                covered_items: None,
                details: vec![],
            },
            regression: None,
            risk: None,
            metadata: None,
        }
    }

    fn make_cap(id: &str, name: &str, evidence_file: &str) -> Capability {
        Capability {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            status: CapabilityStatus::New,
            module: None,
            sub_modules: vec![],
            confidence: Some(0.9),
            evidence: vec![evidence_file.into()],
            tags: vec![],
            contract: None,
            preconditions: vec![],
            postconditions: vec![],
            invariants: vec![],
            categories: vec![],
            depends_on: vec![],
            depended_by: vec![],
            evolved_from: None,
            evidence_trace_ids: vec![],
            verified_by: None,
            verified_at: None,
            source_change_type: None,
            triggered_by_rules: vec![],
        }
    }

    #[test]
    fn test_make_exact_key() {
        let key = make_exact_key("login", "src/auth.rs", "FunctionSignatureChanged");
        assert_eq!(key, "login::src/auth.rs::FunctionSignatureChanged");
    }

    #[test]
    fn test_file_prefix_2level() {
        assert_eq!(file_prefix_2level("src/legacy/sync.rs"), "src/legacy");
        assert_eq!(file_prefix_2level("src/auth.rs"), "src/auth.rs");
        assert_eq!(
            file_prefix_2level("tests/integration/api/login_test.rs"),
            "tests/integration"
        );
    }

    #[test]
    fn test_build_feedback_index_empty() {
        let snap = make_empty_snapshot();
        let tmp = tempfile::TempDir::new().unwrap();
        let paporot_dir = tmp.path().join(".Paporot");
        std::fs::create_dir_all(paporot_dir.join("reviews")).unwrap();
        std::fs::create_dir_all(paporot_dir.join("rules")).unwrap();
        std::fs::create_dir_all(paporot_dir.join("work")).unwrap();

        let (index, result) = build_feedback_index(&paporot_dir, &snap).unwrap();
        assert_eq!(result.total_reviews, 0);
        assert_eq!(result.rejected_count, 0);
        assert!(index.exact_reject_map.is_empty());
        assert!(index.rejected_prefixes.is_empty());
    }

    #[test]
    fn test_build_feedback_index_with_rejects() {
        let snap = BehaviorSnapshot {
            capabilities: vec![make_cap("cap_sync", "sync_legacy", "src/legacy/sync.rs")],
            ..make_empty_snapshot()
        };

        let tmp = tempfile::TempDir::new().unwrap();
        let paporot_dir = tmp.path().join(".Paporot");
        std::fs::create_dir_all(paporot_dir.join("reviews")).unwrap();
        std::fs::create_dir_all(paporot_dir.join("rules")).unwrap();
        std::fs::create_dir_all(paporot_dir.join("work")).unwrap();

        let review = BehaviorReview {
            review_id: "rev_001".into(),
            capability_id: "cap_sync".into(),
            snapshot_version: "v1".into(),
            reviewer: "zxgzx".into(),
            verdict: ReviewVerdict::Rejected,
            comment: Some("false positive".into()),
            corrected: None,
            reviewed_at: "2026-01-01T00:00:00Z".into(),
            tags: vec![],
            triggered_by_rules: vec!["breaking_001".into()],
            source_symbol: Some("sync_legacy".into()),
            source_file: Some("src/legacy/sync.rs".into()),
            source_change_type: Some("FunctionRemoved".into()),
        };

        let fb = FeedbackStore {
            reviews: vec![review],
            stats: FeedbackStats {
                total_reviews: 1,
                approved: 0,
                rejected: 1,
                corrected: 0,
                flagged: 0,
            },
        };
        fb.save(&paporot_dir.join("reviews").join("reviews.json"))
            .unwrap();

        let (index, result) = build_feedback_index(&paporot_dir, &snap).unwrap();
        assert_eq!(result.total_reviews, 1);
        assert_eq!(result.rejected_count, 1);

        let key = make_exact_key("sync_legacy", "src/legacy/sync.rs", "FunctionRemoved");
        assert!(index.exact_reject_map.contains_key(&key));
        assert_eq!(index.rejected_prefixes, vec!["src/legacy"]);
    }
}
