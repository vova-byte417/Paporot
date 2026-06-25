//! `Paporot feedback` — 人机验证回路
//!
//! ## 子命令
//! - `feedback apply`              — 读取 .Paporot/reviews/review_v{N}.toml 并写回 Snapshot
//! - `feedback stats`              — 查看审查统计
//! - `feedback show [version_id]`  — 查看指定版本的审查记录
//!
//! ## TOML 审查文件
//!
//! 用户编辑 .Paporot/reviews/review_v{N}.toml，格式见 PRD §3.5-bis

use anyhow::{Context, Result};
use serde::Deserialize;
use crate::types::*;

// ─── TOML review file format ─────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ReviewToml {
    approve: Option<std::collections::HashMap<String, String>>,
    reject: Option<std::collections::HashMap<String, String>>,
    correct: Option<std::collections::HashMap<String, CorrectEntry>>,
    flag: Option<std::collections::HashMap<String, String>>,
    /// v3: Rule-level suppression
    #[serde(default)]
    suppress_rule: Option<std::collections::HashMap<String, SuppressRuleEntry>>,
}

#[derive(Debug, Deserialize)]
struct CorrectEntry {
    name: Option<String>,
    description: Option<String>,
    status: Option<String>,
}

/// v3: suppress_rule TOML 条目
#[derive(Debug, Deserialize)]
struct SuppressRuleEntry {
    reason: String,
    file_pattern: String,
    #[serde(default)]
    change_type: Option<String>,
    #[serde(default = "default_effect")]
    effect: String,
}

fn default_effect() -> String {
    "suppress".into()
}

// ─── Suppressions TOML serialization for feedback::apply ──────────

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SuppressionsToml {
    suppression: Vec<SuppressionTomlEntry>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SuppressionTomlEntry {
    rule_id: String,
    file_pattern: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    change_type: Option<String>,
    effect: String,
    reason: String,
    created_by: String,
    created_at: String,
    source_review: String,
    hit_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_hit: Option<String>,
    status: String,
}

/// Generate a review TOML file for a given snapshot
pub fn generate_review_toml(snapshot: &BehaviorSnapshot, output_dir: &std::path::Path) -> Result<std::path::PathBuf> {
    let path = output_dir.join(format!("review_{}.toml", snapshot.version_id));

    let mut content = String::new();
    content.push_str(&format!("# 自动生成于 {}\n", snapshot.timestamp));
    content.push_str(&format!("# 版本: {}, Capabilities: {} 个\n", snapshot.version_id, snapshot.capabilities.len()));
    content.push_str("# 只改你要纠正的行，不改的留空即可\n\n");

    content.push_str("[approve]\n");
    for cap in &snapshot.capabilities {
        content.push_str(&format!("# {} = \"ok\"\n", cap.id));
    }
    content.push_str("\n[reject]\n");
    content.push_str("# cap_id = \"拒绝原因\"\n\n");

    content.push_str("# [correct.cap_id]\n");
    content.push_str("# name = \"修改后名称\"\n# description = \"修改后描述\"\n# status = \"new\"\n\n");

    content.push_str("[flag]\n");
    content.push_str("# cap_id = \"待定原因\"\n\n");

    content.push_str("# ── v3: Rule-level suppression ──\n");
    content.push_str("# [suppress_rule.breaking_001]\n");
    content.push_str("# reason = \"src/legacy/ 下所有公开 API 删除都是预期废弃\"\n");
    content.push_str("# file_pattern = \"src/legacy/*\"\n");
    content.push_str("# effect = \"suppress\"\n");
    content.push_str("# change_type = \"HttpRouteRemoved\"  # optional\n");

    std::fs::create_dir_all(output_dir)?;
    std::fs::write(&path, &content)?;
    println!("  generated {}", path.display());
    Ok(path)
}

/// Apply a review TOML file back to the snapshot and save
pub fn apply_review_toml(
    snapshot: &mut BehaviorSnapshot,
    toml_path: &std::path::Path,
    reviewer: &str,
) -> Result<FeedbackStore> {
    let raw = std::fs::read_to_string(toml_path)
        .with_context(|| format!("Cannot read review file: {}", toml_path.display()))?;
    let review: ReviewToml = toml::from_str(&raw)
        .with_context(|| format!("Invalid TOML in: {}", toml_path.display()))?;

    let mut store = FeedbackStore { reviews: vec![], stats: FeedbackStats::default() };
    let now = chrono::Utc::now().to_rfc3339();

    // Apply approves
    if let Some(approve_map) = &review.approve {
        for (cap_id, _comment) in approve_map {
            for cap in snapshot.capabilities.iter_mut() {
                if cap.id == *cap_id {
                    cap.verified_by = Some(reviewer.into());
                    cap.verified_at = Some(now.clone());
                    break;
                }
            }
            store.add_review(BehaviorReview {
                review_id: format!("rev_{}", store.stats.total_reviews + 1),
                capability_id: cap_id.clone(), snapshot_version: snapshot.version_id.clone(),
                reviewer: reviewer.into(), verdict: ReviewVerdict::Approved,
                comment: None, corrected: None, reviewed_at: now.clone(), tags: vec![],
                triggered_by_rules: vec![],
                source_symbol: None,
                source_file: None,
                source_change_type: None,
            });
            println!("  ✓ Capability '{}' approved.", cap_id);
        }
    }

    // Apply rejects
    if let Some(reject_map) = &review.reject {
        for (cap_id, reason) in reject_map {
            // v3: extract traceback info before removing capability
            let source_symbol = snapshot.capabilities.iter()
                .find(|c| c.id == *cap_id)
                .map(|c| c.name.clone());
            let source_file = snapshot.capabilities.iter()
                .find(|c| c.id == *cap_id)
                .and_then(|c| c.evidence.first().cloned());
            let source_change_type = snapshot.capabilities.iter()
                .find(|c| c.id == *cap_id)
                .and_then(|c| c.source_change_type.clone());
            let triggered_by_rules = snapshot.capabilities.iter()
                .find(|c| c.id == *cap_id)
                .map(|c| c.triggered_by_rules.clone())
                .unwrap_or_default();

            snapshot.capabilities.retain(|c| c.id != *cap_id);
            store.add_review(BehaviorReview {
                review_id: format!("rev_{}", store.stats.total_reviews + 1),
                capability_id: cap_id.clone(), snapshot_version: snapshot.version_id.clone(),
                reviewer: reviewer.into(), verdict: ReviewVerdict::Rejected,
                comment: Some(reason.clone()), corrected: None, reviewed_at: now.clone(), tags: vec![],
                triggered_by_rules,
                source_symbol,
                source_file,
                source_change_type,
            });
            println!("  ✗ Capability '{}' rejected: {}", cap_id, reason);
        }
    }

    // Apply corrections
    if let Some(correct_map) = &review.correct {
        for (cap_id, entry) in correct_map {
            for cap in snapshot.capabilities.iter_mut() {
                if cap.id == *cap_id {
                    if let Some(ref name) = entry.name { cap.name = name.clone(); }
                    if let Some(ref desc) = entry.description { cap.description = desc.clone(); }
                    if let Some(ref s) = entry.status {
                        cap.status = match s.as_str() {
                            "new" => CapabilityStatus::New,
                            "modified" => CapabilityStatus::Modified,
                            "deleted" => CapabilityStatus::Deleted,
                            _ => CapabilityStatus::Unchanged,
                        };
                    }
                    cap.verified_by = Some(reviewer.into());
                    cap.verified_at = Some(now.clone());
                    break;
                }
            }
            store.add_review(BehaviorReview {
                review_id: format!("rev_{}", store.stats.total_reviews + 1),
                capability_id: cap_id.clone(), snapshot_version: snapshot.version_id.clone(),
                reviewer: reviewer.into(), verdict: ReviewVerdict::Corrected,
                comment: None, corrected: None, reviewed_at: now.clone(), tags: vec![],
                triggered_by_rules: vec![],
                source_symbol: None,
                source_file: None,
                source_change_type: None,
            });
            println!("  ~ Capability '{}' corrected.", cap_id);
        }
    }

    // Apply flags
    if let Some(flag_map) = &review.flag {
        for (cap_id, note) in flag_map {
            for cap in snapshot.capabilities.iter_mut() {
                if cap.id == *cap_id {
                    cap.tags.push("needs-review".into());
                    break;
                }
            }
            store.add_review(BehaviorReview {
                review_id: format!("rev_{}", store.stats.total_reviews + 1),
                capability_id: cap_id.clone(), snapshot_version: snapshot.version_id.clone(),
                reviewer: reviewer.into(), verdict: ReviewVerdict::Flagged,
                comment: Some(note.clone()), corrected: None, reviewed_at: now.clone(), tags: vec![],
                triggered_by_rules: vec![],
                source_symbol: None,
                source_file: None,
                source_change_type: None,
            });
            println!("  ? Capability '{}' flagged: {}", cap_id, note);
        }
    }

    // ── v3: Apply rule-level suppressions ──
    if let Some(sr_map) = &review.suppress_rule {
        let rules_dir = toml_path
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("rules"))
            .unwrap_or_else(|| std::path::PathBuf::from(".Paporot/rules"));
        std::fs::create_dir_all(&rules_dir)?;
        let suppressions_path = rules_dir.join("suppressions.toml");

        // Read existing suppressions or create new
        let mut existing: Vec<SuppressionTomlEntry> = if suppressions_path.exists() {
            let raw = std::fs::read_to_string(&suppressions_path)?;
            let parsed: SuppressionsToml = toml::from_str(&raw)?;
            parsed.suppression
        } else {
            vec![]
        };

        for (rule_id, entry) in sr_map {
            let new_entry = SuppressionTomlEntry {
                rule_id: rule_id.clone(),
                file_pattern: entry.file_pattern.clone(),
                change_type: entry.change_type.clone(),
                effect: entry.effect.clone(),
                reason: entry.reason.clone(),
                created_by: reviewer.to_string(),
                created_at: now.clone(),
                source_review: toml_path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                hit_count: 0,
                last_hit: None,
                status: "active".to_string(),
            };
            println!("  ⊘ Rule suppression '{}' created: {} → {}", rule_id, entry.effect, entry.file_pattern);
            existing.push(new_entry);
        }

        let suppressions_file = SuppressionsToml { suppression: existing };
        let toml_str = toml::to_string(&suppressions_file)?;
        std::fs::write(&suppressions_path, toml_str)?;
        println!("  → wrote {}", suppressions_path.display());
    }

    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snap(version: &str, caps: Vec<Capability>) -> BehaviorSnapshot {
        BehaviorSnapshot {
            schema_version: 3, version_id: version.into(),
            git_commit: None, git_ref: None,
            timestamp: "2026-06-24T10:00:00Z".into(),
            message: String::new(), capabilities: caps,
            prd_coverage: PrdCoverage { percentage: 0.0, total_items: 0, covered_items: None, details: vec![] },
            regression: None, risk: None, metadata: None,
        }
    }

    fn make_cap(id: &str, name: &str) -> Capability {
        Capability {
            id: id.into(), name: name.into(), description: String::new(),
            status: CapabilityStatus::New, module: None, sub_modules: vec![],
            confidence: Some(1.0), evidence: vec![], tags: vec![],
            contract: None, preconditions: vec![], postconditions: vec![],
            invariants: vec![], categories: vec![],
            depends_on: vec![], depended_by: vec![],
            evolved_from: None, evidence_trace_ids: vec![],
            verified_by: None, verified_at: None,
            source_change_type: None, triggered_by_rules: vec![],
        }
    }

    #[test]
    fn test_generate_review_toml() {
        let snap = make_snap("v1", vec![
            make_cap("cap_001", "Login"),
            make_cap("cap_002", "Logout"),
        ]);
        let dir = std::env::temp_dir().join("Paporot_test_review");
        let _ = std::fs::remove_dir_all(&dir);

        let path = generate_review_toml(&snap, &dir).unwrap();
        assert!(path.exists());

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("cap_001"));
        assert!(content.contains("cap_002"));
        assert!(content.contains("[approve]"));
        assert!(content.contains("[reject]"));
        assert!(content.contains("[flag]"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_apply_toml_approve() {
        let dir = std::env::temp_dir().join("Paporot_test_apply");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let toml_path = dir.join("test_review.toml");
        std::fs::write(&toml_path, r#"
[approve]
cap_auth = "ok"
"#).unwrap();

        let mut snap = make_snap("v1", vec![
            make_cap("cap_auth", "Auth"),
            make_cap("cap_other", "Other"),
        ]);

        let store = apply_review_toml(&mut snap, &toml_path, "tester").unwrap();
        assert_eq!(store.stats.approved, 1);
        assert_eq!(store.stats.total_reviews, 1);

        let approved = snap.capabilities.iter().find(|c| c.id == "cap_auth").unwrap();
        assert_eq!(approved.verified_by.as_deref(), Some("tester"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_apply_toml_reject() {
        let dir = std::env::temp_dir().join("Paporot_test_reject");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let toml_path = dir.join("test_review.toml");
        std::fs::write(&toml_path, r#"
[reject]
cap_bad = "false positive"
"#).unwrap();

        let mut snap = make_snap("v1", vec![
            make_cap("cap_bad", "Bad"),
            make_cap("cap_good", "Good"),
        ]);

        apply_review_toml(&mut snap, &toml_path, "tester").unwrap();
        assert_eq!(snap.capabilities.len(), 1);
        assert_eq!(snap.capabilities[0].id, "cap_good");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_apply_toml_correct() {
        let dir = std::env::temp_dir().join("Paporot_test_correct");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let toml_path = dir.join("test_review.toml");
        std::fs::write(&toml_path, r#"
[correct.cap_old]
name = "New Name"
description = "Better description"
status = "modified"
"#).unwrap();

        let mut snap = make_snap("v1", vec![make_cap("cap_old", "Old")]);

        apply_review_toml(&mut snap, &toml_path, "tester").unwrap();
        let cap = &snap.capabilities[0];
        assert_eq!(cap.name, "New Name");
        assert_eq!(cap.description, "Better description");
        assert_eq!(cap.status, CapabilityStatus::Modified);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_apply_toml_flag() {
        let dir = std::env::temp_dir().join("Paporot_test_flag");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let toml_path = dir.join("test_review.toml");
        std::fs::write(&toml_path, r#"
[flag]
cap_weird = "不确定是否为实验性代码"
"#).unwrap();

        let mut snap = make_snap("v1", vec![make_cap("cap_weird", "Weird")]);

        let store = apply_review_toml(&mut snap, &toml_path, "tester").unwrap();
        assert_eq!(store.stats.flagged, 1);
        assert!(snap.capabilities[0].tags.contains(&"needs-review".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_apply_toml_suppress_rule() {
        let dir = std::env::temp_dir().join("Paporot_test_suppress");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let toml_path = dir.join("test_review.toml");
        std::fs::write(&toml_path, r#"
[suppress_rule.breaking_001]
reason = "legacy code false positive"
file_pattern = "src/legacy/*"
effect = "suppress"

[suppress_rule.sec_token_001]
reason = "migrations are expected"
file_pattern = "src/migrations/*"
effect = "warn"
change_type = "ConstantChanged"
"#).unwrap();

        let mut snap = make_snap("v1", vec![make_cap("cap_001", "Test")]);

        let store = apply_review_toml(&mut snap, &toml_path, "tester").unwrap();
        assert_eq!(store.stats.total_reviews, 0); // suppress_rule doesn't add reviews

        // Check suppressions.toml was written
        // dir is reviews/, so rules/ is dir/../rules
        let rules_dir = dir.parent().unwrap().join("rules");
        let suppressions_path = rules_dir.join("suppressions.toml");
        assert!(suppressions_path.exists(), "suppressions.toml should exist at {}", suppressions_path.display());

        let content = std::fs::read_to_string(&suppressions_path).unwrap();
        assert!(content.contains("breaking_001"));
        assert!(content.contains("legacy code false positive"));
        assert!(content.contains("src/legacy/*"));
        assert!(content.contains("suppress"));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&rules_dir);
    }
}
