//! 趋势检测：基于时间序列数据发现行为退化趋势。
//!
//! 与 rules 互补：rules 做即时比较，trends 做时间维度分析。

use super::types::{TrendAnomaly, TrendPoint};

/// 检测趋势异常 (rule of thumb: 2σ 标准).
pub fn detect_anomalies(points: &[TrendPoint], sigma_threshold: f64) -> Vec<TrendAnomaly> {
    if points.len() < 2 {
        return Vec::new();
    }

    let mut anomalies = Vec::new();

    for metric in &["tool_call_count", "duration_ms", "input_tokens", "output_tokens"] {
        let values: Vec<f64> = points
            .iter()
            .map(|p| match *metric {
                "tool_call_count" => p.tool_call_count,
                "duration_ms" => p.duration_ms,
                "input_tokens" => p.input_tokens,
                "output_tokens" => p.output_tokens,
                _ => 0.0,
            })
            .collect();

        let (mean, std_dev) = compute_mean_std(&values);

        if std_dev == 0.0 {
            continue;
        }

        let latest = values[values.len() - 1];
        let sigma = (latest - mean).abs() / std_dev;

        if sigma >= sigma_threshold {
            anomalies.push(TrendAnomaly {
                metric: metric.to_string(),
                current_value: latest,
                mean,
                std_dev,
                sigma,
            });
        }
    }

    anomalies
}

fn compute_mean_std(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    if n == 0.0 {
        return (0.0, 0.0);
    }
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / n;
    (mean, variance.sqrt())
}

/// 生成建议文字。
pub fn generate_recommendations(anomalies: &[TrendAnomaly]) -> Vec<String> {
    anomalies
        .iter()
        .map(|a| {
            format!(
                "Metric '{}': value {} deviates by {:.1}σ from mean {:.1}±{:.1} — check if intentional",
                a.metric, a.current_value, a.sigma, a.mean, a.std_dev
            )
        })
        .collect()
}

// ─── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_point(version: &str, tool_calls: f64, duration: f64, input: f64, output: f64) -> TrendPoint {
        TrendPoint {
            version: version.into(),
            trace_id: format!("trace_{}", version),
            tool_call_count: tool_calls,
            duration_ms: duration,
            input_tokens: input,
            output_tokens: output,
        }
    }

    #[test]
    fn test_no_anomalies_on_stable_trend() {
        let points = vec![
            make_point("v1", 3.0, 500.0, 100.0, 50.0),
            make_point("v2", 3.0, 520.0, 110.0, 55.0),
            make_point("v3", 4.0, 510.0, 105.0, 52.0),
        ];

        let anomalies = detect_anomalies(&points, 2.0);
        assert!(anomalies.is_empty(), "Stable trend should have no anomalies, got: {:?}", anomalies);
    }

    #[test]
    fn test_detect_anomalous_spike() {
        // 5 个数据点，最后一点异常偏高
        let points = vec![
            make_point("v1", 3.0, 500.0, 100.0, 50.0),
            make_point("v2", 3.0, 520.0, 110.0, 55.0),
            make_point("v3", 4.0, 510.0, 105.0, 52.0),
            make_point("v4", 3.0, 530.0, 115.0, 58.0),
            make_point("v5", 12.0, 550.0, 120.0, 60.0),
        ];

        let anomalies = detect_anomalies(&points, 1.5);
        assert!(!anomalies.is_empty(), "Spike should be detected");
        assert!(anomalies.iter().any(|a| a.metric == "tool_call_count"),
            "Tool call spike should be detected, got: {:?}", anomalies);
    }

    #[test]
    fn test_single_point_no_anomalies() {
        let points = vec![make_point("v1", 3.0, 500.0, 100.0, 50.0)];
        let anomalies = detect_anomalies(&points, 2.0);
        assert!(anomalies.is_empty());
    }

    #[test]
    fn test_generate_recommendations() {
        let anomalies = vec![TrendAnomaly {
            metric: "tool_call_count".into(),
            current_value: 15.0,
            mean: 5.0,
            std_dev: 2.0,
            sigma: 5.0,
        }];
        let recs = generate_recommendations(&anomalies);
        assert_eq!(recs.len(), 1);
        assert!(recs[0].contains("tool_call_count"));
        assert!(recs[0].contains("5.0σ"));
    }

    #[test]
    fn test_compute_mean_std() {
        let values = vec![2.0, 4.0, 6.0];
        let (mean, std) = compute_mean_std(&values);
        assert!((mean - 4.0).abs() < 0.01, "mean={}", mean);
        assert!((std - 1.633).abs() < 0.1, "std={}", std);
    }

    #[test]
    fn test_empty_values() {
        let (mean, std) = compute_mean_std(&[]);
        assert_eq!(mean, 0.0);
        assert_eq!(std, 0.0);
    }
}
