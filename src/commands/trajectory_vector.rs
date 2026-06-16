//! P1 Trajectory Vector CLI 命令实现。

use crate::trace::storage::TraceStorage;
use crate::trajectory::build_state_graph;
use crate::trajectory::p1::{
    build_vector, compute_state_stability,
    feature_extractor::FeatureSnapshot,
    sequence_metrics::SequenceMetrics,
    timeseries::TimeSeries,
    Clusterer, TrajectoryVector,
};

/// 从 trace 构建 TrajectoryVector。
pub fn run_vector_build(
    storage: &TraceStorage,
    trace_id: &str,
    output: Option<String>,
) -> Result<(), String> {
    let trace = storage
        .load(trace_id)
        .map_err(|e| format!("Trace '{}' not found: {:?}", trace_id, e))?;

    let graph = build_state_graph(&trace);

    // Extract P1 features
    let snapshot = FeatureSnapshot::from_graph_and_tools(&graph, &trace.tool_calls);
    let metrics = SequenceMetrics::from_graph(&graph, &trace.tool_calls);
    let ts = TimeSeries::from_graph(&graph, 1000);
    let state_stability = compute_state_stability(&graph);

    // Build vector
    let vector = build_vector(&snapshot, &metrics, &ts, 1);

    // Override with computed state_stability (build_vector has placeholder)
    let vector = TrajectoryVector {
        state_stability_score: state_stability,
        ..vector
    };

    let json = serde_json::to_string_pretty(&vector)
        .map_err(|e| format!("JSON error: {}", e))?;

    if let Some(path) = output {
        std::fs::write(&path, &json)
            .map_err(|e| format!("Write error: {}", e))?;
        println!("TrajectoryVector written to {}", path);
    } else {
        println!("{}", json);
    }

    // Print summary
    println!("\nTrajectoryVector Summary:");
    println!("  Tool entropy:       {:.4}", vector.tool_entropy);
    println!("  Phase entropy:      {:.4}", vector.phase_entropy);
    println!("  Transition entropy: {:.4}", vector.transition_entropy);
    println!("  Loop ratio:         {:.4}", vector.loop_ratio);
    println!("  Backtrack ratio:    {:.4}", vector.backtrack_ratio);
    println!("  Burst ratio:        {:.4}", vector.burst_ratio);
    println!("  State stability:    {:.4}", vector.state_stability_score);

    Ok(())
}

/// Diff two TrajectoryVector JSON files.
pub fn run_vector_diff(file_a: &str, file_b: &str) -> Result<(), String> {
    let content_a = std::fs::read_to_string(file_a)
        .map_err(|e| format!("Read {}: {}", file_a, e))?;
    let content_b = std::fs::read_to_string(file_b)
        .map_err(|e| format!("Read {}: {}", file_b, e))?;

    let va: TrajectoryVector = serde_json::from_str(&content_a)
        .map_err(|e| format!("Parse {}: {}", file_a, e))?;
    let vb: TrajectoryVector = serde_json::from_str(&content_b)
        .map_err(|e| format!("Parse {}: {}", file_b, e))?;

    let sim = crate::trajectory::p2::similarity::cosine_sim(&va, &vb);
    let groups = crate::trajectory::p2::similarity::compute_grouped_contributions(&va, &vb);

    println!("TrajectoryVector Diff: {} vs {}", file_a, file_b);
    println!("  Cosine similarity: {:.4}", sim);
    println!("  Feature contributions:");
    println!("    Entropy:    {:.4}", groups.entropy);
    println!("    Structural: {:.4}", groups.structural);
    println!("    Temporal:   {:.4}", groups.temporal);
    println!("    Density:    {:.4}", groups.density);

    // Per-field diff
    let sa = va.to_scalar_vec();
    let sb = vb.to_scalar_vec();
    let labels = [
        "tool_entropy", "phase_entropy", "transition_entropy",
        "loop_ratio", "backtrack_ratio", "burst_ratio", "state_stability",
    ];
    println!("\n  Per-field comparison:");
    for (i, label) in labels.iter().enumerate() {
        let diff = sa[i] - sb[i];
        println!("    {:<22} {:.4} → {:.4}  (Δ={:.4})", label, sa[i], sb[i], diff);
    }

    Ok(())
}

/// 聚类分析。
pub fn run_cluster_analyze(
    storage: &TraceStorage,
    trace_ids: &[String],
) -> Result<(), String> {
    if trace_ids.len() < 2 {
        return Err("Need at least 2 traces for clustering".into());
    }

    let mut vectors = Vec::new();
    for tid in trace_ids {
        let trace = storage
            .load(tid)
            .map_err(|e| format!("Trace '{}' not found: {:?}", tid, e))?;
        let graph = build_state_graph(&trace);
        let snapshot = FeatureSnapshot::from_graph_and_tools(&graph, &trace.tool_calls);
        let metrics = SequenceMetrics::from_graph(&graph, &trace.tool_calls);
        let ts = TimeSeries::from_graph(&graph, 1000);
        let state_stability = compute_state_stability(&graph);
        let mut v = build_vector(&snapshot, &metrics, &ts, 1);
        v.state_stability_score = state_stability;
        vectors.push(v);
    }

    let clusterer = Clusterer::default();
    let result = clusterer.cluster(&vectors);
    let quality = clusterer.cluster_quality(&result);

    println!("Cluster Analysis ({} traces):", vectors.len());
    println!("  Clusters found: {}", result.cluster_count);
    println!("  Cluster quality: {:.4}", quality);

    for (ci, cluster) in result.clusters.iter().enumerate() {
        println!("\n  Cluster {}: {} members", ci + 1, cluster.len());
        for &idx in cluster {
            println!("    Trace: {}", trace_ids[idx]);
        }
    }

    // Noise points
    let noise: Vec<&String> = result
        .labels
        .iter()
        .enumerate()
        .filter(|(_, l)| **l <= 0)
        .map(|(i, _)| &trace_ids[i])
        .collect();
    if !noise.is_empty() {
        println!("\n  Noise (unclustered): {} traces", noise.len());
        for tid in noise {
            println!("    {}", tid);
        }
    }

    Ok(())
}

/// 异常检测：检测与群体偏差最大的 trace。
pub fn run_anomaly_detect(
    storage: &TraceStorage,
    trace_ids: &[String],
) -> Result<(), String> {
    if trace_ids.len() < 3 {
        return Err("Need at least 3 traces for anomaly detection".into());
    }

    // Build vectors
    let mut vectors: Vec<(String, TrajectoryVector)> = Vec::new();
    for tid in trace_ids {
        let trace = storage
            .load(tid)
            .map_err(|e| format!("Trace '{}' not found: {:?}", tid, e))?;
        let graph = build_state_graph(&trace);
        let snapshot = FeatureSnapshot::from_graph_and_tools(&graph, &trace.tool_calls);
        let metrics = SequenceMetrics::from_graph(&graph, &trace.tool_calls);
        let ts = TimeSeries::from_graph(&graph, 1000);
        let state_stability = compute_state_stability(&graph);
        let mut v = build_vector(&snapshot, &metrics, &ts, 1);
        v.state_stability_score = state_stability;
        vectors.push((tid.clone(), v));
    }

    // Compute mean vector
    let n = vectors.len() as f32;
    let mut mean_scalar = vec![0.0_f32; 7];
    for (_, v) in &vectors {
        for (i, &s) in v.to_scalar_vec().iter().enumerate() {
            mean_scalar[i] += s / n;
        }
    }

    // Compute distance from mean for each
    let mut scores: Vec<(String, f32)> = vectors
        .iter()
        .map(|(tid, v)| {
            let dist = euclidean_distance(&v.to_scalar_vec(), &mean_scalar);
            (tid.clone(), dist)
        })
        .collect();

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    println!("Anomaly Detection ({} traces):", vectors.len());
    println!("  ── Ranking by distance from mean ──");
    for (i, (tid, score)) in scores.iter().enumerate() {
        let marker = if i == 0 { " ← HIGHEST" } else { "" };
        println!("  {}. {:30}  anomaly_score={:.4}{}", i + 1, tid, score, marker);
    }

    Ok(())
}

fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f32>()
        .sqrt()
}
