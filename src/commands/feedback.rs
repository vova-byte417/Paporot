//! `Paporot feedback` — 人机验证回路
//!
//! ## 子命令
//! - `feedback approve <capability_id>` — 确认某个能力
//! - `feedback reject <capability_id>`  — 标记为误报
//! - `feedback correct <capability_id>` — 修正能力和依赖关系
//! - `feedback flag <capability_id>`    — 标记为待定
//! - `feedback show [capability_id]`    — 查看审查记录
//! - `feedback stats`                   — 查看审查统计

use anyhow::Result;
use crate::types::*;

/// 执行 feedback approve
pub fn approve(feedback: &mut FeedbackStore, capability_id: &str, snapshot_version: &str, reviewer: &str, comment: Option<&str>) -> Result<()> {
    let review = BehaviorReview {
        review_id: format!("rev_{}", feedback.stats.total_reviews + 1),
        capability_id: capability_id.into(),
        snapshot_version: snapshot_version.into(),
        reviewer: reviewer.into(),
        verdict: ReviewVerdict::Approved,
        comment: comment.map(String::from),
        corrected: None,
        reviewed_at: chrono::Utc::now().to_rfc3339(),
        tags: vec![],
    };

    feedback.add_review(review);
    println!("  ✓ Capability '{}' approved.", capability_id);
    Ok(())
}

/// 执行 feedback reject
pub fn reject(feedback: &mut FeedbackStore, capability_id: &str, snapshot_version: &str, reviewer: &str, reason: Option<&str>) -> Result<()> {
    let review = BehaviorReview {
        review_id: format!("rev_{}", feedback.stats.total_reviews + 1),
        capability_id: capability_id.into(),
        snapshot_version: snapshot_version.into(),
        reviewer: reviewer.into(),
        verdict: ReviewVerdict::Rejected,
        comment: reason.map(Into::into),
        corrected: None,
        reviewed_at: chrono::Utc::now().to_rfc3339(),
        tags: vec![],
    };

    feedback.add_review(review);
    println!("  ✗ Capability '{}' rejected.", capability_id);
    Ok(())
}

/// 执行 feedback correct
pub fn correct(feedback: &mut FeedbackStore, capability_id: &str, snapshot_version: &str, reviewer: &str, name: &str, description: &str, comment: Option<&str>) -> Result<()> {
    let corrected_cap = Capability {
        id: capability_id.into(),
        name: name.into(),
        description: description.into(),
        status: CapabilityStatus::Modified,
        module: None,
        sub_modules: vec![],
        confidence: Some(1.0),
        evidence: vec![],
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
        verified_by: Some(reviewer.into()),
        verified_at: Some(chrono::Utc::now().to_rfc3339()),
    };

    let review = BehaviorReview {
        review_id: format!("rev_{}", feedback.stats.total_reviews + 1),
        capability_id: capability_id.into(),
        snapshot_version: snapshot_version.into(),
        reviewer: reviewer.into(),
        verdict: ReviewVerdict::Corrected,
        comment: comment.map(String::from),
        corrected: Some(corrected_cap),
        reviewed_at: chrono::Utc::now().to_rfc3339(),
        tags: vec![],
    };

    feedback.add_review(review);
    println!("  ~ Capability '{}' corrected.", capability_id);
    Ok(())
}

/// 执行 feedback flag
pub fn flag(feedback: &mut FeedbackStore, capability_id: &str, snapshot_version: &str, reviewer: &str, note: Option<&str>) -> Result<()> {
    let review = BehaviorReview {
        review_id: format!("rev_{}", feedback.stats.total_reviews + 1),
        capability_id: capability_id.into(),
        snapshot_version: snapshot_version.into(),
        reviewer: reviewer.into(),
        verdict: ReviewVerdict::Flagged,
        comment: note.map(String::from),
        corrected: None,
        reviewed_at: chrono::Utc::now().to_rfc3339(),
        tags: vec![],
    };

    feedback.add_review(review);
    println!("  ? Capability '{}' flagged for review.", capability_id);
    Ok(())
}

/// 执行 feedback show
pub fn show(feedback: &FeedbackStore, capability_id: Option<&str>) -> Result<()> {
    println!("Paporot Feedback Review");
    println!("=======================\n");

    let reviews: Vec<&BehaviorReview> = if let Some(cid) = capability_id {
        feedback.reviews_for(cid)
    } else {
        feedback.reviews.iter().rev().take(20).collect()
    };

    if reviews.is_empty() {
        println!("  No reviews found.");
        if feedback.stats.total_reviews == 0 {
            println!("  Run 'Paporot feedback approve/reject/correct' to start reviewing.");
        }
    } else {
        for r in &reviews {
            let icon = match r.verdict {
                ReviewVerdict::Approved => "✓",
                ReviewVerdict::Rejected => "✗",
                ReviewVerdict::Corrected => "~",
                ReviewVerdict::Flagged => "?",
            };
            println!("  {} {}  [{:?}] by {} — {}",
                icon, r.capability_id,
                r.verdict, r.reviewer, r.reviewed_at);

            if let Some(ref c) = r.comment {
                println!("      comment: {}", c);
            }
        }
    }
    Ok(())
}

/// 执行 feedback stats
pub fn stats(feedback: &FeedbackStore) -> Result<()> {
    println!("Paporot Feedback Statistics");
    println!("===========================\n");
    println!("  Total reviews  : {}", feedback.stats.total_reviews);
    println!("  Approved       : {} ({:.1}%)",
        feedback.stats.approved,
        percentage(feedback.stats.approved, feedback.stats.total_reviews));
    println!("  Rejected       : {} ({:.1}%)",
        feedback.stats.rejected,
        percentage(feedback.stats.rejected, feedback.stats.total_reviews));
    println!("  Corrected      : {} ({:.1}%)",
        feedback.stats.corrected,
        percentage(feedback.stats.corrected, feedback.stats.total_reviews));
    println!("  Flagged        : {} ({:.1}%)",
        feedback.stats.flagged,
        percentage(feedback.stats.flagged, feedback.stats.total_reviews));
    Ok(())
}

fn percentage(part: u32, total: u32) -> f32 {
    if total == 0 { 0.0 } else { (part as f32 / total as f32) * 100.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn empty_feedback() -> FeedbackStore {
        FeedbackStore {
            reviews: vec![],
            stats: FeedbackStats::default(),
        }
    }

    #[test]
    fn test_approve_adds_review() {
        let mut fb = empty_feedback();
        approve(&mut fb, "cap_001", "v1", "tester", None).unwrap();
        assert_eq!(fb.stats.total_reviews, 1);
        assert_eq!(fb.stats.approved, 1);
        assert_eq!(fb.reviews[0].verdict, ReviewVerdict::Approved);
        assert_eq!(fb.reviews[0].capability_id, "cap_001");
    }

    #[test]
    fn test_reject_adds_review() {
        let mut fb = empty_feedback();
        reject(&mut fb, "cap_002", "v1", "tester", Some("false positive")).unwrap();
        assert_eq!(fb.stats.rejected, 1);
        assert_eq!(fb.reviews[0].comment.as_deref(), Some("false positive"));
    }

    #[test]
    fn test_correct_sets_verified_fields() {
        let mut fb = empty_feedback();
        correct(&mut fb, "cap_003", "v1", "tester", "New Name", "New Desc", None).unwrap();
        assert_eq!(fb.stats.corrected, 1);

        let correct = fb.reviews[0].corrected.as_ref().unwrap();
        assert_eq!(correct.name, "New Name");
        assert!(correct.verified_by.is_some());
        assert!(correct.verified_at.is_some());
    }

    #[test]
    fn test_flag_adds_review() {
        let mut fb = empty_feedback();
        flag(&mut fb, "cap_004", "v1", "tester", Some("needs more info")).unwrap();
        assert_eq!(fb.stats.flagged, 1);
        assert_eq!(fb.reviews[0].verdict, ReviewVerdict::Flagged);
    }

    #[test]
    fn test_reviews_for_filters() {
        let mut fb = empty_feedback();
        approve(&mut fb, "cap_A", "v1", "alice", None).unwrap();
        approve(&mut fb, "cap_B", "v1", "bob", None).unwrap();
        approve(&mut fb, "cap_A", "v1", "carol", None).unwrap();

        let a_reviews = fb.reviews_for("cap_A");
        assert_eq!(a_reviews.len(), 2);
        let b_reviews = fb.reviews_for("cap_B");
        assert_eq!(b_reviews.len(), 1);
        let c_reviews = fb.reviews_for("cap_X");
        assert!(c_reviews.is_empty());
    }

    #[test]
    fn test_feedback_persistence_roundtrip() {
        let dir = std::env::temp_dir().join("Paporot_test_feedback");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("reviews.json");

        let mut fb = empty_feedback();
        approve(&mut fb, "cap_001", "v1", "qa", None).unwrap();
        reject(&mut fb, "cap_002", "v1", "qa", Some("no such feature")).unwrap();
        flag(&mut fb, "cap_003", "v1", "qa", None).unwrap();

        fb.save(&path).unwrap();
        let loaded = FeedbackStore::load_or_new(&path).unwrap();
        assert_eq!(loaded.stats.total_reviews, 3);
        assert_eq!(loaded.stats.approved, 1);
        assert_eq!(loaded.stats.rejected, 1);
        assert_eq!(loaded.stats.flagged, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
