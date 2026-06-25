//! Feedback Suppressor — WASM 侧
//!
//! 读取 .Paporot/work/feedback_index.json，对 PreprocessorOutput 中的
//! L1 RawChange 应用三层抑制：
//!   Layer 1: Exact match — (symbol, file, type) ∈ reject_map
//!   Layer 2: Rule-level suppress — (rule_id, file_pattern) ∈ suppressions
//!   Layer 3: Prefix warning — file_path 匹配 reject 历史前缀

use crate::host;
use chrono::Utc;
use paporot_analysis_types::{ChangeType, FeedbackIndex, RawChange, RuleSuppression, SuppressionEffect, SuppressionStatus};

/// 抑制结果
#[derive(Debug, serde::Serialize)]
pub struct SuppressionResult {
    pub change_id: String,
    pub level: SuppressionLevel,
    pub reason: String,
    pub new_confidence: f32,
    pub matched_rule: Option<String>,
}

#[derive(Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SuppressionLevel {
    Exact,
    Rule,
    Warning,
}

/// 应用三层抑制到 RawChange 列表
///
/// 返回被抑制/警告的条目列表。调用方负责修改 RawChange 的 confidence 和 tags。
/// 同时更新 feedback_index 中命中规则的 hit_count。
pub fn apply_suppressions(
    changes: &mut [RawChange],
    rule_matches: &[paporot_analysis_types::RuleMatch],
    feedback_index: &mut FeedbackIndex,
) -> Vec<SuppressionResult> {
    let mut results = Vec::new();

    for rc in changes.iter_mut() {
        // Layer 1: Exact match
        let key = make_exact_key(&rc.symbol_name, &rc.file_path, &rc.change_type);
        if let Some(reason) = feedback_index.exact_reject_map.get(&key) {
            let old_conf = rc.confidence;
            rc.confidence = 0.2;
            rc.tags.push("rejected-by-feedback".into());
            results.push(SuppressionResult {
                change_id: rc.id.clone(),
                level: SuppressionLevel::Exact,
                reason: reason.clone(),
                new_confidence: 0.2,
                matched_rule: None,
            });
            eprintln!(
                "[suppress] L1 EXACT  {} | {} → {:?} | conf {:.2}→0.2",
                rc.symbol_name, rc.file_path, rc.change_type, old_conf
            );
            continue;
        }

        // Layer 2: Rule-level
        let triggered_rules: Vec<&str> = rule_matches
            .iter()
            .filter(|m| m.raw_change_id == rc.id)
            .map(|m| m.rule_id.as_str())
            .collect();

        let mut suppressed = false;
        for suppression in &mut feedback_index.rule_suppressions {
            if suppression.status != SuppressionStatus::Active {
                continue;
            }
            if !triggered_rules.iter().any(|r| *r == suppression.rule_id) {
                continue;
            }
            if !file_matches_pattern(&rc.file_path, &suppression.file_pattern) {
                continue;
            }
            if let Some(ref ct) = suppression.change_type {
                let change_type_str = format!("{:?}", rc.change_type);
                if change_type_str != *ct {
                    continue;
                }
            }

            // v3: increment hit count
            suppression.hit_count += 1;
            suppression.last_hit = Some(Utc::now().to_rfc3339());

            if suppression.effect == SuppressionEffect::Suppress {
                let old_conf = rc.confidence;
                rc.confidence = 0.2;
                rc.tags.push("suppressed-by-rule".into());
                results.push(SuppressionResult {
                    change_id: rc.id.clone(),
                    level: SuppressionLevel::Rule,
                    reason: suppression.reason.clone(),
                    new_confidence: 0.2,
                    matched_rule: Some(suppression.rule_id.clone()),
                });
                eprintln!(
                    "[suppress] L2 RULE   {} | {} → {:?} | rule={} | conf {:.2}→0.2",
                    rc.symbol_name, rc.file_path, rc.change_type, suppression.rule_id, old_conf
                );
                suppressed = true;
                break;
            } else {
                // Warn: only tag, don't change confidence
                rc.tags.push(format!("warn-rule:{}", suppression.rule_id));
                results.push(SuppressionResult {
                    change_id: rc.id.clone(),
                    level: SuppressionLevel::Warning,
                    reason: suppression.reason.clone(),
                    new_confidence: rc.confidence,
                    matched_rule: Some(suppression.rule_id.clone()),
                });
            }
        }
        if suppressed {
            continue;
        }

        // Layer 3: Prefix warning
        for prefix in &feedback_index.rejected_prefixes {
            if rc.file_path.starts_with(prefix.as_str()) {
                if !rc.tags.iter().any(|t| t == "fp-history-warning") {
                    rc.tags.push("fp-history-warning".into());
                    results.push(SuppressionResult {
                        change_id: rc.id.clone(),
                        level: SuppressionLevel::Warning,
                        reason: format!("file path prefix '{}' has rejection history", prefix),
                        new_confidence: rc.confidence,
                        matched_rule: None,
                    });
                }
                break;
            }
        }
    }

    results
}

/// 从 .Paporot/work/feedback_index.json 加载索引/// 加载反馈索引。
///
/// 优先读 native 侧生成的 work/feedback_index.json；
/// 若不存在则直接从 reviews.json + suppressions.toml 构建。
pub fn load_feedback_index() -> Option<FeedbackIndex> {
    // Fast path: native 侧已构建
    if let Some(json) = host::read_file("work/feedback_index.json") {
        if let Ok(idx) = serde_json::from_str(&json) {
            return Some(idx);
        }
    }

    // Fallback: 从源文件构建
    build_index_from_sources()
}

/// 从 reviews.json + suppressions.toml 直接构建 FeedbackIndex
fn build_index_from_sources() -> Option<FeedbackIndex> {
    use std::collections::HashMap;

    let mut exact_reject_map: HashMap<String, String> = HashMap::new();
    let mut rejected_prefixes: Vec<String> = Vec::new();
    let mut rule_suppressions: Vec<RuleSuppression> = Vec::new();

    // ── Parse reviews.json ──
    if let Some(rjson) = host::read_file("reviews/reviews.json") {
        if let Ok(root) = serde_json::from_str::<serde_json::Value>(&rjson) {
            if let Some(reviews) = root.get("reviews").and_then(|v| v.as_array()) {
                for review in reviews {
                    let verdict = review.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
                    if verdict != "rejected" {
                        continue;
                    }
                    // Exact match key: symbol::file::change_type
                    if let (Some(sym), Some(file), Some(ct)) = (
                        review.get("source_symbol").and_then(|v| v.as_str()),
                        review.get("source_file").and_then(|v| v.as_str()),
                        review.get("source_change_type").and_then(|v| v.as_str()),
                    ) {
                        let key = format!("{}::{}::{}", sym, file, ct);
                        let reason = review.get("comment")
                            .and_then(|v| v.as_str())
                            .unwrap_or("rejected in review")
                            .to_string();
                        exact_reject_map.insert(key, reason);
                    }
                    // Prefix: first 2 path levels
                    if let Some(file) = review.get("source_file").and_then(|v| v.as_str()) {
                        let prefix = file_prefix_2level(file);
                        if !rejected_prefixes.contains(&prefix) {
                            rejected_prefixes.push(prefix);
                        }
                    }
                }
            }
        }
    }

    // ── Parse suppressions.toml ──
    if let Some(toml_str) = host::read_file("rules/suppressions.toml") {
        #[derive(serde::Deserialize)]
        struct SuppressionsFile {
            suppression: Vec<SuppressionTomlEntry>,
        }
        #[derive(serde::Deserialize)]
        struct SuppressionTomlEntry {
            rule_id: String,
            file_pattern: String,
            #[serde(default)]
            change_type: Option<String>,
            effect: String,
            reason: String,
            created_by: String,
            created_at: String,
            source_review: String,
            #[serde(default)]
            hit_count: u32,
            #[serde(default)]
            last_hit: Option<String>,
            #[serde(default = "default_status")]
            status: String,
        }
        fn default_status() -> String { "active".into() }

        if let Ok(parsed) = toml::from_str::<SuppressionsFile>(&toml_str) {
            for s in parsed.suppression {
                let effect = match s.effect.as_str() {
                    "warn" => SuppressionEffect::Warn,
                    _ => SuppressionEffect::Suppress,
                };
                let status = match s.status.as_str() {
                    "stale" => SuppressionStatus::Stale,
                    "revoked" => SuppressionStatus::Revoked,
                    _ => SuppressionStatus::Active,
                };
                rule_suppressions.push(RuleSuppression {
                    rule_id: s.rule_id,
                    file_pattern: s.file_pattern,
                    change_type: s.change_type,
                    effect,
                    reason: s.reason,
                    created_by: s.created_by,
                    created_at: s.created_at,
                    source_review: s.source_review,
                    hit_count: s.hit_count,
                    last_hit: s.last_hit,
                    status,
                });
            }
        }
    }

    Some(FeedbackIndex {
        exact_reject_map,
        rule_suppressions,
        rejected_prefixes,
    })
}

/// 写回更新后的 feedback_index.json
pub fn write_feedback_index(index: &FeedbackIndex) -> Result<(), String> {
    let json = serde_json::to_string(index).map_err(|e| e.to_string())?;
    host::write_file("work/feedback_index.json", &json)
        .map_err(|e| format!("write error: {}", e))
}

/// 将 rule_suppressions 写回 suppressions.toml
pub fn write_suppressions_toml(index: &FeedbackIndex) -> Result<(), String> {
    let mut toml_str = String::from("# Paporot Rule Suppressions\n");
    for s in &index.rule_suppressions {
        toml_str.push_str(&format!("\n[[suppression]]\n"));
        toml_str.push_str(&format!("rule_id = \"{}\"\n", s.rule_id));
        toml_str.push_str(&format!("file_pattern = \"{}\"\n", s.file_pattern));
        if let Some(ref ct) = s.change_type {
            toml_str.push_str(&format!("change_type = \"{}\"\n", ct));
        }
        toml_str.push_str(&format!("effect = \"{}\"\n", match s.effect {
            SuppressionEffect::Suppress => "suppress",
            SuppressionEffect::Warn => "warn",
        }));
        toml_str.push_str(&format!("reason = \"{}\"\n", s.reason));
        toml_str.push_str(&format!("created_by = \"{}\"\n", s.created_by));
        toml_str.push_str(&format!("created_at = \"{}\"\n", s.created_at));
        toml_str.push_str(&format!("source_review = \"{}\"\n", s.source_review));
        toml_str.push_str(&format!("hit_count = {}\n", s.hit_count));
        if let Some(ref lh) = s.last_hit {
            toml_str.push_str(&format!("last_hit = \"{}\"\n", lh));
        }
        toml_str.push_str(&format!("status = \"{}\"\n", match s.status {
            SuppressionStatus::Active => "active",
            SuppressionStatus::Stale => "stale",
            SuppressionStatus::Revoked => "revoked",
        }));
    }
    host::write_file("rules/suppressions.toml", &toml_str)
        .map_err(|e| format!("write error: {}", e))
}

/// 生成回路报告 JSON（供 loop.html 使用）
pub fn build_loop_report_json(
    changes: &[RawChange],
    rule_matches: &[paporot_analysis_types::RuleMatch],
    suppress_results: &[SuppressionResult],
    feedback_loaded: bool,
    exact_reject_count: usize,
    rule_suppression_count: usize,
    rejected_prefixes_count: usize,
    rule_details: &[paporot_analysis_types::RuleSuppression],
) -> serde_json::Value {
    let changes_json: Vec<serde_json::Value> = changes.iter().map(|rc| {
        let triggered: Vec<&str> = rule_matches.iter()
            .filter(|m| m.raw_change_id == rc.id)
            .map(|m| m.rule_id.as_str())
            .collect();
        let suppressed = suppress_results.iter()
            .find(|r| r.change_id == rc.id);
        serde_json::json!({
            "id": rc.id,
            "symbol": rc.symbol_name,
            "file": rc.file_path,
            "change_type": format!("{:?}", rc.change_type),
            "confidence": rc.confidence,
            "rules": triggered,
            "tags": rc.tags,
            "suppressed": suppressed.map(|s| serde_json::json!({
                "level": s.level,
                "reason": s.reason,
                "new_confidence": s.new_confidence,
                "matched_rule": s.matched_rule,
            })),
        })
    }).collect();

    let l1_count = suppress_results.iter().filter(|r| r.level == SuppressionLevel::Exact).count();
    let l2_count = suppress_results.iter().filter(|r| r.level == SuppressionLevel::Rule).count();
    let l3_count = suppress_results.iter().filter(|r| r.level == SuppressionLevel::Warning).count();

    let rules_json: Vec<serde_json::Value> = rule_details.iter().map(|s| {
        serde_json::json!({
            "rule_id": s.rule_id,
            "file_pattern": s.file_pattern,
            "effect": match s.effect { SuppressionEffect::Suppress => "suppress", SuppressionEffect::Warn => "warn" },
            "reason": s.reason,
            "status": match s.status { SuppressionStatus::Active => "active", SuppressionStatus::Stale => "stale", SuppressionStatus::Revoked => "revoked" },
            "hit_count": s.hit_count,
        })
    }).collect();

    serde_json::json!({
        "feedback_loaded": feedback_loaded,
        "exact_reject_count": exact_reject_count,
        "rule_suppression_count": rule_suppression_count,
        "rejected_prefixes_count": rejected_prefixes_count,
        "total_changes": changes.len(),
        "suppressed_l1": l1_count,
        "suppressed_l2": l2_count,
        "suppressed_l3": l3_count,
        "changes": changes_json,
        "rules": rules_json,
    })
}

// ─── Internal helpers ──────────────────────────────────────────────

fn make_exact_key(symbol: &str, file: &str, change_type: &ChangeType) -> String {
    format!("{}::{}::{:?}", symbol, file, change_type)
}

fn file_matches_pattern(file: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return file == pattern || file.starts_with(pattern);
    }
    let escaped = regex::escape(pattern);
    let re_pattern = format!("^{}$", escaped.replace(r"\*", ".*"));
    regex::Regex::new(&re_pattern)
        .map(|re| re.is_match(file))
        .unwrap_or(false)
}

fn file_prefix_2level(file: &str) -> String {
    let parts: Vec<&str> = file.split('/').collect();
    if parts.len() >= 2 {
        format!("{}/{}", parts[0], parts[1])
    } else {
        file.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_raw_change(id: &str, symbol: &str, file: &str, ct: ChangeType) -> RawChange {
        RawChange {
            id: id.into(),
            source: "test".into(),
            change_type: ct,
            file_path: file.into(),
            language: "rust".into(),
            line_start: 1,
            line_end: 1,
            symbol_name: symbol.into(),
            old_signature: None,
            new_signature: None,
            confidence: 0.9,
            module: None,
            tags: vec![],
        }
    }

    #[test]
    fn test_file_matches_pattern() {
        assert!(file_matches_pattern("src/legacy/sync.rs", "src/legacy/*"));
        assert!(file_matches_pattern("src/legacy/sync.rs", "src/*"));
        assert!(!file_matches_pattern("src/active/mod.rs", "src/legacy/*"));
        assert!(file_matches_pattern("src/auth.rs", "src/auth.rs"));
    }

    #[test]
    fn test_layer1_exact_match() {
        let key = make_exact_key("sync_legacy", "src/legacy/sync.rs", &ChangeType::FunctionRemoved);
        let mut exact_map = HashMap::new();
        exact_map.insert(key, "false positive".to_string());

        let mut index = FeedbackIndex {
            exact_reject_map: exact_map,
            rule_suppressions: vec![],
            rejected_prefixes: vec![],
        };

        let mut changes = vec![
            make_raw_change("rc1", "sync_legacy", "src/legacy/sync.rs", ChangeType::FunctionRemoved),
            make_raw_change("rc2", "login", "src/auth.rs", ChangeType::FunctionAdded),
        ];

        let results = apply_suppressions(&mut changes, &[], &mut index);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].change_id, "rc1");
        assert_eq!(results[0].level, SuppressionLevel::Exact);
        assert_eq!(changes[0].confidence, 0.2);
        assert!(changes[0].tags.contains(&"rejected-by-feedback".to_string()));
        assert_eq!(changes[1].confidence, 0.9);
    }

    #[test]
    fn test_layer2_rule_suppression() {
        let suppression = RuleSuppression {
            rule_id: "breaking_001".into(),
            file_pattern: "src/legacy/*".into(),
            change_type: None,
            effect: SuppressionEffect::Suppress,
            reason: "legacy code expected".into(),
            created_by: "test".into(),
            created_at: "2026-01-01".into(),
            source_review: "test.toml".into(),
            hit_count: 0,
            last_hit: None,
            status: SuppressionStatus::Active,
        };

        let mut index = FeedbackIndex {
            exact_reject_map: HashMap::new(),
            rule_suppressions: vec![suppression],
            rejected_prefixes: vec![],
        };

        let mut changes = vec![
            make_raw_change("rc1", "old_api", "src/legacy/api.rs", ChangeType::HttpRouteRemoved),
        ];

        let rule_matches = vec![paporot_analysis_types::RuleMatch {
            rule_id: "breaking_001".into(),
            raw_change_id: "rc1".into(),
            matched_tags: vec![],
            severity: paporot_analysis_types::Severity::High,
            category: paporot_analysis_types::RuleCategory::Breaking,
            description: "breaking change".into(),
        }];

        let results = apply_suppressions(&mut changes, &rule_matches, &mut index);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].level, SuppressionLevel::Rule);
        assert_eq!(changes[0].confidence, 0.2);
    }

    #[test]
    fn test_layer2_scope_mismatch() {
        let suppression = RuleSuppression {
            rule_id: "breaking_001".into(),
            file_pattern: "src/legacy/*".into(),
            change_type: None,
            effect: SuppressionEffect::Suppress,
            reason: "legacy only".into(),
            created_by: "test".into(),
            created_at: "2026-01-01".into(),
            source_review: "test.toml".into(),
            hit_count: 0,
            last_hit: None,
            status: SuppressionStatus::Active,
        };

        let mut c_index = FeedbackIndex {
            exact_reject_map: HashMap::new(),
            rule_suppressions: vec![suppression],
            rejected_prefixes: vec![],
        };

        // This change is in src/active/, not src/legacy/ → should NOT be suppressed
        let mut changes = vec![
            make_raw_change("rc1", "active_api", "src/active/api.rs", ChangeType::HttpRouteRemoved),
        ];

        let rule_matches = vec![paporot_analysis_types::RuleMatch {
            rule_id: "breaking_001".into(),
            raw_change_id: "rc1".into(),
            matched_tags: vec![],
            severity: paporot_analysis_types::Severity::High,
            category: paporot_analysis_types::RuleCategory::Breaking,
            description: "breaking change".into(),
        }];

        let results = apply_suppressions(&mut changes, &rule_matches, &mut c_index);
        assert_eq!(results.len(), 0);
        assert_eq!(changes[0].confidence, 0.9);
    }

    #[test]
    fn test_layer3_prefix_warning() {
        let mut index = FeedbackIndex {
            exact_reject_map: HashMap::new(),
            rule_suppressions: vec![],
            rejected_prefixes: vec!["src/legacy".into()],
        };

        let mut changes = vec![
            make_raw_change("rc1", "other_fn", "src/legacy/other.rs", ChangeType::FunctionAdded),
            make_raw_change("rc2", "safe_fn", "src/active/mod.rs", ChangeType::FunctionAdded),
        ];

        let results = apply_suppressions(&mut changes, &[], &mut index);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].level, SuppressionLevel::Warning);
        assert!(changes[0].tags.contains(&"fp-history-warning".to_string()));
        assert_eq!(changes[0].confidence, 0.9);
        assert!(!changes[1].tags.contains(&"fp-history-warning".to_string()));
    }

    #[test]
    fn test_layer2_stale_skipped() {
        let suppression = RuleSuppression {
            rule_id: "breaking_001".into(),
            file_pattern: "src/*".into(),
            change_type: None,
            effect: SuppressionEffect::Suppress,
            reason: "stale rule".into(),
            created_by: "test".into(),
            created_at: "2026-01-01".into(),
            source_review: "test.toml".into(),
            hit_count: 0,
            last_hit: None,
            status: SuppressionStatus::Stale,
        };

        let mut index = FeedbackIndex {
            exact_reject_map: HashMap::new(),
            rule_suppressions: vec![suppression],
            rejected_prefixes: vec![],
        };

        let mut changes = vec![
            make_raw_change("rc1", "anything", "src/any.rs", ChangeType::FunctionRemoved),
        ];

        let rule_matches = vec![paporot_analysis_types::RuleMatch {
            rule_id: "breaking_001".into(),
            raw_change_id: "rc1".into(),
            matched_tags: vec![],
            severity: paporot_analysis_types::Severity::High,
            category: paporot_analysis_types::RuleCategory::Breaking,
            description: "breaking".into(),
        }];

        let results = apply_suppressions(&mut changes, &rule_matches, &mut index);
        assert_eq!(results.len(), 0);
        assert_eq!(changes[0].confidence, 0.9);
    }
}
