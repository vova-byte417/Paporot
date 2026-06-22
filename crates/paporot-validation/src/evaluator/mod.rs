//! Evaluator 模块
//!
//! Exact match (name, status, categories) + Semantic fallback (embedding → LLM).

pub mod exact;
pub mod semantic;

use crate::types::{Actual, Expected, Verdict};
use anyhow::Result;

/// 综合评估：先 exact → 失败则 semantic
pub fn evaluate(expected: &Expected, actual: &Actual) -> Result<Verdict> {
    // 1. 尝试 exact match
    let exact_result = exact::evaluate(expected, actual);

    match exact_result {
        Ok(()) => return Ok(Verdict::Pass),
        Err(exact_err) => {
            // 2. Exact 失败 → 尝试 semantic
            match semantic::evaluate(expected, actual) {
                Ok(semantic_result) => {
                    if semantic_result.confidence > 0.85 {
                        return Ok(Verdict::SemanticPass {
                            confidence: semantic_result.confidence,
                            reason: semantic_result.reason,
                        });
                    }
                    // confidence 不够 → FAIL
                    Ok(Verdict::Fail {
                        reason: format!(
                            "Exact fail: {}. Semantic below threshold ({:.2} < 0.85): {}",
                            exact_err,
                            semantic_result.confidence,
                            semantic_result.reason
                        ),
                    })
                }
                Err(_) => Ok(Verdict::Fail {
                    reason: format!("Exact fail: {}. Semantic judge unavailable.", exact_err),
                }),
            }
        }
    }
}
