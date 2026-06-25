//! 三层置信度评分计算。
//!
//! 初期：三层独立评分，不做加权合并。
//! 后期：积累数据后切换为自适应加权。

use crate::types::{EvidenceConfidence, L1Evidence, L2Evidence, L3Evidence};

/// 计算 L1 置信度 (0.0–1.0)。
///
/// 基于：符号数量在合理范围 + 公开符号占比
pub fn compute_l1_score(l1_evidence: &[L1Evidence]) -> f64 {
    if l1_evidence.is_empty() {
        return 0.0;
    }

    let total = l1_evidence.len() as f64;
    let pub_count = l1_evidence
        .iter()
        .filter(|e| e.visibility == "pub")
        .count() as f64;

    let visibility_bonus = if total > 0.0 { pub_count / total } else { 0.0 };

    let count_score = if total <= 20.0 {
        1.0
    } else if total <= 50.0 {
        0.7
    } else {
        0.4
    };

    (count_score * 0.6 + visibility_bonus * 0.4).clamp(0.0, 1.0)
}

/// 计算 L2 置信度 (0.0–1.0)。
///
/// 基于：规则命中数在理想范围 + high severity 比例
pub fn compute_l2_score(l2_evidence: &[L2Evidence]) -> f64 {
    if l2_evidence.is_empty() {
        return 0.0;
    }

    let total = l2_evidence.len() as f64;
    let high_severity = l2_evidence
        .iter()
        .filter(|e| e.severity == "high" || e.severity == "critical")
        .count() as f64;

    let severity_score = if total > 0.0 { high_severity / total } else { 0.0 };

    let count_score = match total as usize {
        0 => 0.0,
        1 => 0.5,
        2..=5 => 1.0,
        6..=10 => 0.7,
        _ => 0.4,
    };

    (count_score * 0.5 + severity_score * 0.5).clamp(0.0, 1.0)
}

/// 计算 L3 置信度 (0.0–1.0)。
///
/// 基于：LLM 输出与 L1 符号的交叉引用率
pub fn compute_l3_score(l3: &L3Evidence, l1: &[L1Evidence]) -> f64 {
    let mut l1_ref_count = 0u32;
    for l1e in l1 {
        if l3.fragment.contains(&l1e.symbol) {
            l1_ref_count += 1;
        }
    }
    let l1_ref_rate = if !l1.is_empty() {
        l1_ref_count as f64 / l1.len() as f64
    } else {
        0.0
    };

    let has_content = !l3.fragment.is_empty();
    let base = if has_content { 0.5 } else { 0.0 };
    let ref_bonus = l1_ref_rate * 0.5;

    (base + ref_bonus).clamp(0.0, 1.0)
}

/// 计算完整的 EvidenceConfidence。
pub fn compute_confidence(
    l1: &[L1Evidence],
    l2: &[L2Evidence],
    l3: Option<&L3Evidence>,
) -> EvidenceConfidence {
    EvidenceConfidence {
        l1_score: compute_l1_score(l1),
        l2_score: compute_l2_score(l2),
        l3_score: l3.map(|e| compute_l3_score(e, l1)),
    }
}

// ─── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_l1(symbol: &str, visibility: &str) -> L1Evidence {
        L1Evidence {
            symbol: symbol.into(),
            file_path: "src/test.rs".into(),
            line: 1,
            kind: crate::types::SymbolKind::Function,
            visibility: visibility.into(),
        }
    }

    fn make_l2(severity: &str) -> L2Evidence {
        L2Evidence {
            rule_id: "r001".into(),
            rule_name: "test_rule".into(),
            matched_symbol: "test".into(),
            file_change: "src/test.rs".into(),
            reason: "test".into(),
            severity: severity.into(),
        }
    }

    #[test]
    fn test_l1_score_empty_is_zero() {
        assert_eq!(compute_l1_score(&[]), 0.0);
    }

    #[test]
    fn test_l1_score_single_pub_is_high() {
        let evidence = vec![make_l1("test", "pub")];
        let score = compute_l1_score(&evidence);
        assert!(score > 0.8, "Single pub symbol should score > 0.8, got {}", score);
    }

    #[test]
    fn test_l1_score_many_symbols_reduces_score() {
        let evidence: Vec<_> = (0..60).map(|i| make_l1(&format!("fn_{}", i), "pub")).collect();
        let score = compute_l1_score(&evidence);
        assert!(score < 0.65, "60 symbols should score < 0.65, got {}", score);
    }

    #[test]
    fn test_l1_score_no_pub_reduces_score() {
        let evidence = vec![make_l1("private_fn", "private")];
        let score = compute_l1_score(&evidence);
        let evidence2 = vec![make_l1("pub_fn", "pub")];
        let score2 = compute_l1_score(&evidence2);
        assert!(score < score2, "Private should score lower than pub");
    }

    #[test]
    fn test_l2_score_empty_is_zero() {
        assert_eq!(compute_l2_score(&[]), 0.0);
    }

    #[test]
    fn test_l2_score_ideal_range() {
        let evidence: Vec<_> = (0..3).map(|_| make_l2("high")).collect();
        let score = compute_l2_score(&evidence);
        assert!(score > 0.8, "3 high-severity rules should score > 0.8, got {}", score);
    }

    #[test]
    fn test_l2_score_single_rule_lower() {
        let evidence = vec![make_l2("low")];
        let score = compute_l2_score(&evidence);
        assert!(score < 0.6, "Single low-severity rule should score < 0.6, got {}", score);
    }

    #[test]
    fn test_l3_score_with_references() {
        let l1 = vec![make_l1("login", "pub"), make_l1("auth", "pub")];
        let l3 = L3Evidence {
            prompt_hash: "abc".into(),
            fragment: "The login function handles authentication".into(),
            model: "test".into(),
            timestamp: "now".into(),
        };
        let score = compute_l3_score(&l3, &l1);
        assert!(score > 0.7, "L3 referencing L1 symbols should score > 0.7, got {}", score);
    }

    #[test]
    fn test_l3_score_no_references() {
        let l1 = vec![make_l1("login", "pub")];
        let l3 = L3Evidence {
            prompt_hash: "abc".into(),
            fragment: "Something unrelated".into(),
            model: "test".into(),
            timestamp: "now".into(),
        };
        let score = compute_l3_score(&l3, &l1);
        assert!(score < 0.7, "L3 not referencing L1 should score < 0.7, got {}", score);
    }

    #[test]
    fn test_compute_confidence_full() {
        let l1 = vec![make_l1("fn_a", "pub"), make_l1("fn_b", "pub")];
        let l2 = vec![make_l2("high"), make_l2("medium")];
        let l3 = L3Evidence {
            prompt_hash: "abc".into(),
            fragment: "fn_a and fn_b are related".into(),
            model: "test".into(),
            timestamp: "now".into(),
        };

        let conf = compute_confidence(&l1, &l2, Some(&l3));
        assert!(conf.l1_score > 0.0);
        assert!(conf.l2_score > 0.0);
        assert!(conf.l3_score.unwrap() > 0.0);
    }
}
