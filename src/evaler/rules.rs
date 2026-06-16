//! 退化检测规则引擎。
//!
//! 内置规则检测以下退化类型：
//! - 工具调用数目暴增
//! - 耗时暴增
//! - Token 消耗暴增
//! - 输出长度缩减

use crate::trace::types::BehaviorTrace;

use super::types::{DegradeRule, DegradeRuleHit, EvalVerdict, RuleSeverity};

/// 内置退化检测规则。
pub fn builtin_rules() -> Vec<DegradeRule> {
    vec![
        DegradeRule {
            id: "R001".into(),
            name: "tool count explosion".into(),
            description: "工具调用次数相比之前增长超过阈值".into(),
            severity: RuleSeverity::High,
            metric: "tool_call_count".into(),
            direction: "increase".into(),
            threshold_pct: 100.0,
        },
        DegradeRule {
            id: "R002".into(),
            name: "tool count slight increase".into(),
            description: "工具调用次数小幅增长".into(),
            severity: RuleSeverity::Medium,
            metric: "tool_call_count".into(),
            direction: "increase".into(),
            threshold_pct: 50.0,
        },
        DegradeRule {
            id: "R003".into(),
            name: "output length collapse".into(),
            description: "最终输出长度严重缩减".into(),
            severity: RuleSeverity::Critical,
            metric: "output_length".into(),
            direction: "decrease".into(),
            threshold_pct: 50.0,
        },
        DegradeRule {
            id: "R004".into(),
            name: "token usage explosion".into(),
            description: "Token 用量翻倍".into(),
            severity: RuleSeverity::High,
            metric: "total_tokens".into(),
            direction: "increase".into(),
            threshold_pct: 100.0,
        },
        DegradeRule {
            id: "R005".into(),
            name: "output token reduction".into(),
            description: "Output token 大幅减少".into(),
            severity: RuleSeverity::Medium,
            metric: "output_tokens".into(),
            direction: "decrease".into(),
            threshold_pct: 30.0,
        },
    ]
}

/// 获取 trace 中指定 metric 的值。
fn get_metric(trace: &BehaviorTrace, metric: &str) -> f64 {
    match metric {
        "tool_call_count" => trace.tool_calls.len() as f64,
        "output_length" => trace.final_output.len() as f64,
        "total_tokens" => (trace.token_usage.input_tokens + trace.token_usage.output_tokens) as f64,
        "input_tokens" => trace.token_usage.input_tokens as f64,
        "output_tokens" => trace.token_usage.output_tokens as f64,
        _ => 0.0,
    }
}

/// 判定两个 trace 之间是否符合规则。
fn check_rule(
    rule: &DegradeRule,
    trace_a: &BehaviorTrace,
    trace_b: &BehaviorTrace,
) -> Option<DegradeRuleHit> {
    let val_a = get_metric(trace_a, &rule.metric);
    let val_b = get_metric(trace_b, &rule.metric);

    if val_a == 0.0 && val_b == 0.0 {
        return None;
    }

    let pct_change = if val_a != 0.0 {
        ((val_b - val_a) / val_a) * 100.0
    } else if val_b > 0.0 {
        f64::INFINITY
    } else {
        0.0
    };

    let triggered = match rule.direction.as_str() {
        "increase" => pct_change >= rule.threshold_pct,
        "decrease" => pct_change <= -rule.threshold_pct,
        _ => false,
    };

    if triggered {
        Some(DegradeRuleHit {
            rule_id: rule.id.clone(),
            rule_name: rule.name.clone(),
            severity: rule.severity.clone(),
            description: format!(
                "{}: {} → {} ({:.1}% {})",
                rule.description, val_a, val_b, pct_change.abs(), rule.direction
            ),
            actual_value_a: val_a,
            actual_value_b: val_b,
        })
    } else {
        None
    }
}

/// 执行退化检测。
pub fn evaluate(
    trace_a: &BehaviorTrace,
    trace_b: &BehaviorTrace,
) -> Vec<DegradeRuleHit> {
    builtin_rules()
        .iter()
        .filter_map(|rule| check_rule(rule, trace_a, trace_b))
        .collect()
}

/// 根据命中规则判定最终 verdict。
pub fn compute_verdict(hits: &[DegradeRuleHit]) -> EvalVerdict {
    if hits.iter().any(|h| matches!(h.severity, RuleSeverity::Critical)) {
        EvalVerdict::Degraded
    } else if hits.len() >= 2 {
        EvalVerdict::Degraded
    } else if hits.len() == 1 {
        EvalVerdict::Watch
    } else {
        EvalVerdict::Pass
    }
}

// ─── 段级退化规则（消费 TrajectoryAnalysis） ──────────────────────

use crate::trajectory::analysis::TrajectoryAnalysis;

/// 基于 TrajectoryAnalysis 的段级退化规则。
pub fn segment_rules() -> Vec<DegradeRule> {
    vec![
        DegradeRule {
            id: "S001".into(),
            name: "phase explosion".into(),
            description: "新增阶段数超过阈值（>= 2）".into(),
            severity: RuleSeverity::High,
            metric: "phase_additions".into(),
            direction: "increase".into(),
            threshold_pct: 0.0,
        },
        DegradeRule {
            id: "S002".into(),
            name: "phase missing".into(),
            description: "关键阶段（验证/提交）被删除".into(),
            severity: RuleSeverity::Critical,
            metric: "phase_deletions".into(),
            direction: "increase".into(),
            threshold_pct: 0.0,
        },
        DegradeRule {
            id: "S003".into(),
            name: "tool churn spike".into(),
            description: "tool_churn_score 过高".into(),
            severity: RuleSeverity::High,
            metric: "tool_churn".into(),
            direction: "increase".into(),
            threshold_pct: 50.0,
        },
        DegradeRule {
            id: "S004".into(),
            name: "phase reorder".into(),
            description: "阶段执行顺序大幅变化".into(),
            severity: RuleSeverity::Medium,
            metric: "phase_reorder".into(),
            direction: "increase".into(),
            threshold_pct: 50.0,
        },
        DegradeRule {
            id: "S005".into(),
            name: "capability shift".into(),
            description: "相同 tool 但 args 大幅变化".into(),
            severity: RuleSeverity::Medium,
            metric: "capability_shift".into(),
            direction: "increase".into(),
            threshold_pct: 30.0,
        },
    ]
}

/// 对 TrajectoryAnalysis 执行段级规则检测。
pub fn evaluate_segment_rules(analysis: &TrajectoryAnalysis) -> Vec<DegradeRuleHit> {
    let rules = segment_rules();
    let mut hits = Vec::new();

    for rule in &rules {
        match rule.metric.as_str() {
            "phase_additions" => {
                if analysis.phase_additions.len() >= 2 {
                    hits.push(DegradeRuleHit {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        severity: rule.severity.clone(),
                        description: format!(
                            "{} new phases added: {}",
                            analysis.phase_additions.len(),
                            analysis
                                .phase_additions
                                .iter()
                                .map(|p| p.label.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        actual_value_a: 0.0,
                        actual_value_b: analysis.phase_additions.len() as f64,
                    });
                }
            }
            "phase_deletions" => {
                let critical_phases = ["验证", "提交", "verify", "commit"];
                let has_critical = analysis
                    .phase_deletions
                    .iter()
                    .any(|p| critical_phases.contains(&p.label.as_str()));
                if has_critical {
                    hits.push(DegradeRuleHit {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        severity: rule.severity.clone(),
                        description: format!(
                            "Critical phases deleted: {}",
                            analysis
                                .phase_deletions
                                .iter()
                                .map(|p| p.label.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        actual_value_a: 0.0,
                        actual_value_b: 1.0,
                    });
                }
            }
            "tool_churn" => {
                let churn_pct = analysis.tool_churn_score * 100.0;
                if churn_pct >= rule.threshold_pct as f32 {
                    hits.push(DegradeRuleHit {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        severity: rule.severity.clone(),
                        description: format!("Tool churn score: {:.1}%", churn_pct),
                        actual_value_a: 0.0,
                        actual_value_b: analysis.tool_churn_score as f64,
                    });
                }
            }
            "phase_reorder" => {
                let reorder_pct = analysis.phase_reorder_score * 100.0;
                if reorder_pct >= rule.threshold_pct as f32 {
                    hits.push(DegradeRuleHit {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        severity: rule.severity.clone(),
                        description: format!("Phase reorder score: {:.1}%", reorder_pct),
                        actual_value_a: 0.0,
                        actual_value_b: analysis.phase_reorder_score as f64,
                    });
                }
            }
            "capability_shift" => {
                let shift_pct = analysis.capability_shift_score * 100.0;
                if shift_pct >= rule.threshold_pct as f32 {
                    hits.push(DegradeRuleHit {
                        rule_id: rule.id.clone(),
                        rule_name: rule.name.clone(),
                        severity: rule.severity.clone(),
                        description: format!("Capability shift score: {:.1}%", shift_pct),
                        actual_value_a: 0.0,
                        actual_value_b: analysis.capability_shift_score as f64,
                    });
                }
            }
            _ => {}
        }
    }

    hits
}

// ─── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::types::TokenUsage;

    fn make_trace(
        tool_calls: usize,
        final_output: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> BehaviorTrace {
        BehaviorTrace {
            id: String::new(),
            session_id: String::new(),
            prompt: String::new(),
            tool_calls: vec![
                crate::trace::types::ToolCall {
                    id: String::new(),
                    tool_name: "test".into(),
                    args: serde_json::Value::Null,
                    timestamp: String::new(),
                    duration_ms: 0,
                    result_id: None,
                };
                tool_calls
            ],
            observations: vec![],
            final_output: final_output.into(),
            token_usage: TokenUsage {
                input_tokens,
                output_tokens,
                cache_read_tokens: None,
                cache_write_tokens: None,
            },
            started_at: String::new(),
            finished_at: String::new(),
            source: crate::trace::types::TraceSource::Captured {
                agent_name: "test".into(),
            },
            tags: vec![],
            capability_ids: vec![],
            deleted: false,
        }
    }

    #[test]
    fn test_pass_on_similar_traces() {
        let a = make_trace(3, "result", 100, 50);
        let b = make_trace(3, "result", 110, 55);
        let hits = evaluate(&a, &b);
        assert!(hits.is_empty(), "Expected no hits, got {:?}", hits);
    }

    #[test]
    fn test_detect_tool_count_explosion() {
        let a = make_trace(2, "result", 100, 50);
        let b = make_trace(6, "result", 200, 80);
        let hits = evaluate(&a, &b);
        assert!(!hits.is_empty(), "Should detect tool count explosion");
        assert!(hits.iter().any(|h| h.rule_id == "R001"),
            "Should trigger R001 tool count explosion, got: {:?}", hits);
    }

    #[test]
    fn test_detect_output_collapse() {
        let a = make_trace(2, "very long output text", 100, 50);
        let b = make_trace(2, "short", 100, 50);
        let hits = evaluate(&a, &b);
        assert!(hits.iter().any(|h| h.rule_id == "R003"),
            "Should trigger R003 output collapse, got: {:?}", hits);
    }

    #[test]
    fn test_detect_token_explosion() {
        let a = make_trace(2, "result", 50, 25);
        let b = make_trace(2, "result", 120, 60);
        let hits = evaluate(&a, &b);
        assert!(hits.iter().any(|h| h.rule_id == "R004"),
            "Should trigger R004 token explosion, got: {:?}", hits);
    }

    #[test]
    fn test_verdict_pass() {
        assert_eq!(compute_verdict(&[]), EvalVerdict::Pass);
    }

    #[test]
    fn test_verdict_watch_single() {
        let hits = vec![DegradeRuleHit {
            rule_id: "R002".into(),
            rule_name: "test".into(),
            severity: RuleSeverity::Medium,
            description: "test".into(),
            actual_value_a: 1.0,
            actual_value_b: 2.0,
        }];
        assert_eq!(compute_verdict(&hits), EvalVerdict::Watch);
    }

    #[test]
    fn test_verdict_degraded_critical() {
        let hits = vec![DegradeRuleHit {
            rule_id: "R003".into(),
            rule_name: "output collapse".into(),
            severity: RuleSeverity::Critical,
            description: "test".into(),
            actual_value_a: 100.0,
            actual_value_b: 10.0,
        }];
        assert_eq!(compute_verdict(&hits), EvalVerdict::Degraded);
    }

    #[test]
    fn test_verdict_degraded_multiple() {
        let hits = vec![
            DegradeRuleHit {
                rule_id: "R002".into(),
                rule_name: "test1".into(),
                severity: RuleSeverity::Medium,
                description: "test1".into(),
                actual_value_a: 1.0,
                actual_value_b: 2.0,
            },
            DegradeRuleHit {
                rule_id: "R005".into(),
                rule_name: "test2".into(),
                severity: RuleSeverity::Medium,
                description: "test2".into(),
                actual_value_a: 100.0,
                actual_value_b: 50.0,
            },
        ];
        assert_eq!(compute_verdict(&hits), EvalVerdict::Degraded);
    }
}
