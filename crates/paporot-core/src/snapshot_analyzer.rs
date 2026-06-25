//! Snapshot 分析器 — SnapshotAnalyzer
//!
//! 所有分析逻辑都是纯函数（&[BehaviorSnapshot] → 结果），零 I/O 依赖。
//! diff/coverage/regression/risk 分析均在此模块。
//!
//! 对应 PRD §3.2：SnapshotStore + SnapshotAnalyzer 拆分。

use crate::types::*;

/// Snapshot 分析器（全静态方法，纯函数）
pub struct SnapshotAnalyzer;

impl SnapshotAnalyzer {
    // ─── Diff ───────────────────────────────────────────────────

    /// 计算两个 Snapshot 之间的行为差异
    pub fn diff(prev: &BehaviorSnapshot, curr: &BehaviorSnapshot) -> BehaviorDiff {
        let mut added = Vec::new();
        let mut modified = Vec::new();
        let mut deleted = Vec::new();
        let mut unchanged = Vec::new();

        let prev_map: std::collections::HashMap<&str, &Capability> =
            prev.capabilities.iter().map(|c| (c.id.as_str(), c)).collect();
        let curr_map: std::collections::HashMap<&str, &Capability> =
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
        let impact = Self::summarize_impact(&added, &modified, &deleted);
        let risks = Self::assess_diff_risks(&added, &deleted, &modified);

        BehaviorDiff {
            from_version: prev.version_id.clone(),
            to_version: curr.version_id.clone(),
            timestamp: curr.timestamp.clone(),
            added,
            modified,
            deleted,
            unchanged,
            impact_summary: format!(
                "{} capabilities changed across {} total: {} added, {} modified, {} deleted, {} unchanged.",
                a_len + m_len + d_len, total, a_len, m_len, d_len, u_len
            ),
            risks_and_notes: impact.into_iter().chain(risks).collect(),
        }
    }

    fn summarize_impact(added: &[Capability], modified: &[Capability], deleted: &[Capability]) -> Vec<String> {
        let mut notes = Vec::new();
        if !added.is_empty() {
            notes.push(format!("New behaviors: {}", added.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")));
        }
        if !modified.is_empty() {
            notes.push(format!("Modified behaviors: {}", modified.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")));
        }
        if !deleted.is_empty() {
            notes.push(format!("Removed behaviors: {}", deleted.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")));
        }
        notes
    }

    fn assess_diff_risks(added: &[Capability], deleted: &[Capability], _modified: &[Capability]) -> Vec<String> {
        let mut risks = Vec::new();
        if !deleted.is_empty() && deleted.iter().any(|c| c.categories.contains(&CapabilityCategory::Security)) {
            risks.push("WARNING: Security-related capability was removed.".into());
        }
        if added.is_empty() && deleted.is_empty() {
            risks.push("No significant behavioral changes detected.".into());
        }
        risks
    }

    // ─── Coverage ───────────────────────────────────────────────

    /// 计算 PRD 需求覆盖率
    pub fn coverage(
        snapshot: &BehaviorSnapshot,
        prd_items: &[(&str, &str)],
        capability_matcher: fn(&Capability, &str) -> bool,
    ) -> PrdCoverage {
        let total = prd_items.len() as u32;
        let mut covered = 0u32;
        let mut details = Vec::new();

        for (prd_id, requirement) in prd_items {
            let matched: Vec<_> = snapshot
                .capabilities
                .iter()
                .filter(|c| capability_matcher(c, prd_id))
                .collect();

            let status = if !matched.is_empty() {
                covered += 1;
                CoverageStatus::Pass
            } else {
                CoverageStatus::NotDetected
            };

            details.push(PrdCoverageDetail {
                prd_id: prd_id.to_string(),
                requirement: requirement.to_string(),
                status,
                mapped_capabilities: matched.iter().map(|c| c.id.clone()).collect(),
                evidence: None,
                confidence: matched.first().and_then(|c| c.confidence),
            });
        }

        let percentage = if total > 0 {
            (covered as f32 / total as f32) * 100.0
        } else {
            0.0
        };

        PrdCoverage {
            percentage,
            total_items: total,
            covered_items: Some(covered),
            details,
        }
    }

    // ─── Regression ─────────────────────────────────────────────

    /// 检测两个版本之间的行为退化
    pub fn regression(prev: &BehaviorSnapshot, curr: &BehaviorSnapshot) -> Regression {
        let mut items = Vec::new();

        for prev_cap in &prev.capabilities {
            let curr_cap = curr.capabilities.iter().find(|c| c.id == prev_cap.id);

            match curr_cap {
                None => {
                    // Capability disappeared — potential regression
                    if prev_cap.status != CapabilityStatus::Deleted {
                        items.push(RegressionItem {
                            workflow: prev_cap.name.clone(),
                            previous_status: prev_cap.status_name().to_string(),
                            current_status: "Removed".to_string(),
                            description: format!("Capability '{}' was present in {} but missing in {}",
                                prev_cap.name, prev.version_id, curr.version_id),
                            severity: Severity::High,
                        });
                    }
                }
                Some(curr) => {
                    // Confidence decreased significantly
                    if let (Some(prev_conf), Some(curr_conf)) = (prev_cap.confidence, curr.confidence) {
                        if curr_conf < prev_conf - 0.2 {
                            items.push(RegressionItem {
                                workflow: prev_cap.name.clone(),
                                previous_status: format!("confidence {}", prev_conf),
                                current_status: format!("confidence {}", curr_conf),
                                description: format!("Confidence dropped from {:.2} to {:.2}", prev_conf, curr_conf),
                                severity: Severity::Medium,
                            });
                        }
                    }
                }
            }
        }

        let status = if items.is_empty() {
            RegressionStatus::Pass
        } else {
            RegressionStatus::Warning
        };

        Regression {
            status,
            detected_regressions: items,
        }
    }

    // ─── Risk ───────────────────────────────────────────────────

    /// 评估当前 Snapshot 的风险等级
    pub fn risk(snapshot: &BehaviorSnapshot) -> RiskAssessment {
        let mut score: u8 = 0;
        let mut factors = Vec::new();

        for cap in &snapshot.capabilities {
            if cap.categories.contains(&CapabilityCategory::Security) {
                score += 20;
                factors.push(RiskFactor {
                    category: "security".into(),
                    description: format!("Security capability: {}", cap.name),
                    severity: Severity::High,
                });
            }
            if cap.categories.contains(&CapabilityCategory::DataIntegrity) {
                score += 15;
                factors.push(RiskFactor {
                    category: "data integrity".into(),
                    description: format!("Data integrity capability: {}", cap.name),
                    severity: Severity::High,
                });
            }
            if cap.status == CapabilityStatus::Deleted {
                score += 10;
                factors.push(RiskFactor {
                    category: "removal".into(),
                    description: format!("Removed capability: {}", cap.name),
                    severity: Severity::Medium,
                });
            }
            if cap.status == CapabilityStatus::Modified {
                score += 5;
                factors.push(RiskFactor {
                    category: "modification".into(),
                    description: format!("Modified capability: {}", cap.name),
                    severity: Severity::Low,
                });
            }
        }

        let level = match score {
            0..=9 => RiskLevel::Low,
            10..=24 => RiskLevel::Medium,
            25..=49 => RiskLevel::High,
            _ => RiskLevel::Critical,
        };

        let mitigations = if score > 25 {
            vec!["Review all security and data integrity changes carefully.".into(),
                 "Run regression tests before deploy.".into()]
        } else if score > 10 {
            vec!["Review modified capabilities before deploy.".into()]
        } else {
            vec![]
        };

        RiskAssessment {
            level,
            score,
            factors,
            mitigations,
        }
    }

    // ─── Evolution ──────────────────────────────────────────────

    /// 获取某个 Capability 在多个版本中的演化历史
    pub fn evolution(snapshots: &[BehaviorSnapshot], capability_id: &str) -> Vec<(String, String)> {
        snapshots
            .iter()
            .filter_map(|s| {
                s.capabilities
                    .iter()
                    .find(|c| c.id == capability_id)
                    .map(|c| (s.version_id.clone(), c.name.clone()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cap(id: &str, name: &str, status: CapabilityStatus, cats: Vec<CapabilityCategory>) -> Capability {
        Capability {
            id: id.into(), name: name.into(), description: String::new(),
            status, module: None, sub_modules: vec![], confidence: Some(1.0),
            evidence: vec![], tags: vec![], contract: None,
            preconditions: vec![], postconditions: vec![], invariants: vec![],
            categories: cats, depends_on: vec![], depended_by: vec![],
            evolved_from: None, evidence_trace_ids: vec![], verified_by: None, verified_at: None,
            source_change_type: None, triggered_by_rules: vec![],
        }
    }

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

    #[test]
    fn test_diff_added_and_deleted() {
        let v1 = make_snap("v1", vec![
            make_cap("c1", "Login", CapabilityStatus::New, vec![]),
            make_cap("c2", "Logout", CapabilityStatus::New, vec![]),
        ]);
        let v2 = make_snap("v2", vec![
            make_cap("c1", "Login", CapabilityStatus::Unchanged, vec![]),
            make_cap("c3", "Payment", CapabilityStatus::New, vec![]),
        ]);

        let diff = SnapshotAnalyzer::diff(&v1, &v2);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].id, "c3");
        assert_eq!(diff.deleted.len(), 1);
        assert_eq!(diff.deleted[0].id, "c2");
        assert_eq!(diff.unchanged.len(), 1);
    }

    #[test]
    fn test_diff_modified() {
        let v1 = make_snap("v1", vec![
            make_cap("c1", "Login", CapabilityStatus::New, vec![]),
        ]);
        let v2 = make_snap("v2", vec![
            make_cap("c1", "LoginV2", CapabilityStatus::Modified, vec![]),
        ]);

        let diff = SnapshotAnalyzer::diff(&v1, &v2);
        assert_eq!(diff.modified.len(), 1);
    }

    #[test]
    fn test_risk_high_security() {
        let snap = make_snap("v1", vec![
            make_cap("c1", "Auth", CapabilityStatus::New, vec![CapabilityCategory::Security]),
            make_cap("c2", "DB", CapabilityStatus::New, vec![CapabilityCategory::DataIntegrity]),
        ]);

        let risk = SnapshotAnalyzer::risk(&snap);
        assert_eq!(risk.level, RiskLevel::High);
        assert!(risk.score > 25);
    }

    #[test]
    fn test_risk_low() {
        let snap = make_snap("v1", vec![
            make_cap("c1", "UI", CapabilityStatus::New, vec![]),
        ]);

        let risk = SnapshotAnalyzer::risk(&snap);
        assert_eq!(risk.level, RiskLevel::Low);
    }

    #[test]
    fn test_regression_capability_removed() {
        let v1 = make_snap("v1", vec![
            make_cap("c1", "Auth", CapabilityStatus::New, vec![]),
        ]);
        let v2 = make_snap("v2", vec![]);

        let reg = SnapshotAnalyzer::regression(&v1, &v2);
        assert_eq!(reg.status, RegressionStatus::Warning);
        assert_eq!(reg.detected_regressions.len(), 1);
    }

    #[test]
    fn test_regression_pass() {
        let v1 = make_snap("v1", vec![
            make_cap("c1", "Auth", CapabilityStatus::New, vec![]),
        ]);
        let v2 = make_snap("v2", vec![
            make_cap("c1", "Auth", CapabilityStatus::Unchanged, vec![]),
        ]);

        let reg = SnapshotAnalyzer::regression(&v1, &v2);
        assert_eq!(reg.status, RegressionStatus::Pass);
    }

    #[test]
    fn test_evolution() {
        let v1 = make_snap("v1", vec![
            make_cap("c1", "AuthV1", CapabilityStatus::New, vec![]),
        ]);
        let v2 = make_snap("v2", vec![
            make_cap("c1", "AuthV2", CapabilityStatus::Modified, vec![]),
        ]);
        let v3 = make_snap("v3", vec![
            make_cap("c1", "AuthV3", CapabilityStatus::Modified, vec![]),
        ]);

        let evo = SnapshotAnalyzer::evolution(&[v1, v2, v3], "c1");
        assert_eq!(evo.len(), 3);
        assert_eq!(evo[0], ("v1".to_string(), "AuthV1".to_string()));
        assert_eq!(evo[2], ("v3".to_string(), "AuthV3".to_string()));
    }
}
