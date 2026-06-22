//! Semantic Judge
//!
//! 两层 fallback:
//! 1. embedding 余弦相似度（阈值 0.85）
//! 2. LLM Judge（灰区 0.7-0.85）
//!
//! MVP 阶段：embedding 用简单的 ngram overlap 模拟（无需外部依赖）。
//! 后续接入 text-embedding-3-small。

use crate::types::{Actual, Expected};

pub struct SemanticResult {
    pub confidence: f64,
    pub reason: String,
}

/// 评估 actual 是否语义等价于 expected
pub fn evaluate(expected: &Expected, actual: &Actual) -> Result<SemanticResult, String> {
    // ── MVP 阶段：基于 ngram 的词级相似度 ──
    // 后续替换为 embedding 模型调用

    if expected.capabilities.is_empty() {
        return Err("No expected capabilities for semantic comparison".into());
    }

    let mut scores = Vec::new();
    let mut details = Vec::new();

    for exp in &expected.capabilities {
        let best = actual
            .capabilities
            .iter()
            .map(|act| {
                let name_sim = word_similarity(&exp.name, &act.name);
                let cat_sim = if exp.categories.is_empty() {
                    1.0
                } else {
                    category_similarity(&exp.categories, &act.categories)
                };
                (act.name.clone(), name_sim * 0.7 + cat_sim * 0.3)
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        if let Some((best_name, score)) = best {
            scores.push(score);
            details.push(format!(
                "expected '{}' ≈ actual '{}' ({:.2})",
                exp.name, best_name, score
            ));
        } else {
            scores.push(0.0);
            details.push(format!("expected '{}' → no match found", exp.name));
        }
    }

    let avg_score: f64 = scores.iter().sum::<f64>() / scores.len() as f64;
    let reason = details.join(" | ");

    Ok(SemanticResult {
        confidence: avg_score,
        reason,
    })
}

/// 词级 Jaccard 相似度（基于 3-gram）
fn word_similarity(a: &str, b: &str) -> f64 {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();

    if a_lower == b_lower {
        return 1.0;
    }

    // Tokenize by snake_case, CamelCase, spaces
    let tokens_a = tokenize(&a_lower);
    let tokens_b = tokenize(&b_lower);

    if tokens_a.is_empty() && tokens_b.is_empty() {
        return 1.0;
    }
    if tokens_a.is_empty() || tokens_b.is_empty() {
        return 0.0;
    }

    let intersection = tokens_a.iter().filter(|t| tokens_b.contains(t)).count();
    let union = tokens_a.len() + tokens_b.len() - intersection;

    intersection as f64 / union as f64
}

fn category_similarity(expected: &[String], actual: &[String]) -> f64 {
    if expected.is_empty() {
        return 1.0;
    }
    if actual.is_empty() {
        return 0.0;
    }
    let exp_lower: Vec<String> = expected.iter().map(|s| s.to_lowercase()).collect();
    let act_lower: Vec<String> = actual.iter().map(|s| s.to_lowercase()).collect();
    let intersection = exp_lower.iter().filter(|t| act_lower.contains(t)).count();
    let union = exp_lower.len() + act_lower.len() - intersection;
    intersection as f64 / union as f64
}

fn tokenize(s: &str) -> Vec<String> {
    // Split on snake_case, CamelCase, whitespace, hyphens
    let mut tokens = Vec::new();
    let parts: Vec<&str> = s.split(|c: char| c == '_' || c == '-' || c == ' ').collect();
    for part in parts {
        // CamelCase splitting
        let mut start = 0;
        let chars: Vec<char> = part.chars().collect();
        for i in 1..chars.len() {
            if chars[i].is_uppercase() && !chars[i - 1].is_uppercase() {
                tokens.push(part[start..i].to_lowercase());
                start = i;
            }
        }
        if start < chars.len() {
            tokens.push(part[start..].to_lowercase());
        }
    }
    tokens.retain(|t| !t.is_empty() && t.len() >= 2);
    tokens.sort();
    tokens.dedup();
    tokens
}
