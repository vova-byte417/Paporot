//! Behavior Eval 数据类型。

use serde::{Deserialize, Serialize};

/// 单次 Behavior Eval 的完整结果。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvalResult {
    pub capability_id: String,
    pub trace_id_a: String,
    pub trace_id_b: String,
    pub evaluated_at: String,
    pub verdict: EvalVerdict,
    pub hit_rules: Vec<DegradeRuleHit>,
    pub trend_anomalies: Vec<TrendAnomaly>,
    pub recommendations: Vec<String>,
}

/// 评测判定。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum EvalVerdict {
    Pass,
    Degraded,
    Watch,
}

/// 退化检测规则。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DegradeRule {
    pub id: String,
    pub name: String,
    pub description: String,
    pub severity: RuleSeverity,
    pub metric: String,
    pub direction: String,
    pub threshold_pct: f64,
}

/// 规则严重程度。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum RuleSeverity {
    Critical,
    High,
    Medium,
    Low,
}

/// 规则命中记录。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DegradeRuleHit {
    pub rule_id: String,
    pub rule_name: String,
    pub severity: RuleSeverity,
    pub description: String,
    pub actual_value_a: f64,
    pub actual_value_b: f64,
}

/// 趋势数据点。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrendPoint {
    pub version: String,
    pub trace_id: String,
    pub tool_call_count: f64,
    pub duration_ms: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
}

/// 趋势异常。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrendAnomaly {
    pub metric: String,
    pub current_value: f64,
    pub mean: f64,
    pub std_dev: f64,
    pub sigma: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_verdict_serde() {
        assert_eq!(
            serde_json::to_string(&EvalVerdict::Pass).unwrap(),
            "\"Pass\""
        );
        assert_eq!(
            serde_json::to_string(&EvalVerdict::Degraded).unwrap(),
            "\"Degraded\""
        );
        assert_eq!(
            serde_json::to_string(&EvalVerdict::Watch).unwrap(),
            "\"Watch\""
        );
    }

    #[test]
    fn test_degrade_rule_serde_roundtrip() {
        let rule = DegradeRule {
            id: "R001".into(),
            name: "tool count explosion".into(),
            description: "Tool calls doubled".into(),
            severity: RuleSeverity::High,
            metric: "tool_call_count".into(),
            direction: "increase".into(),
            threshold_pct: 100.0,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let decoded: DegradeRule = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, "R001");
        assert_eq!(decoded.severity, RuleSeverity::High);
    }

    #[test]
    fn test_eval_result_serde_roundtrip() {
        let result = EvalResult {
            capability_id: "cap_001".into(),
            trace_id_a: "trace_a".into(),
            trace_id_b: "trace_b".into(),
            evaluated_at: "now".into(),
            verdict: EvalVerdict::Watch,
            hit_rules: vec![],
            trend_anomalies: vec![],
            recommendations: vec!["Check if intentional".into()],
        };
        let json = serde_json::to_string(&result).unwrap();
        let decoded: EvalResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.verdict, EvalVerdict::Watch);
        assert_eq!(decoded.capability_id, "cap_001");
    }
}
