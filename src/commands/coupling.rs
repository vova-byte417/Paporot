//! P2 Coupling CLI 命令实现。

use std::collections::HashMap;

use crate::trace::storage::TraceStorage;
use crate::trajectory::build_state_graph;
use crate::trajectory::p1::{
    build_vector, compute_state_stability,
    feature_extractor::FeatureSnapshot,
    sequence_metrics::SequenceMetrics,
    timeseries::TimeSeries,
};
use crate::trajectory::p2::{
    cochange::CochangeEvidence,
    coupling_builder::CouplingBuilder,
    graph::Pruner,
    correlation::CorrelationEngine,
};

/// 构建 CouplingGraph。
/// 从多个 trace（映射到 capability）构建耦合图。
pub fn run_coupling_build(
    storage: &TraceStorage,
    trace_pairs: &[(String, String)], // (trace_id, capability_id)
    output: Option<String>,
) -> Result<(), String> {
    // Build vectors per capability
    let mut cap_vectors: HashMap<String, Vec<crate::trajectory::p1::vector::TrajectoryVector>> =
        HashMap::new();

    for (tid, cap_id) in trace_pairs {
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
        cap_vectors.entry(cap_id.clone()).or_default().push(v);
    }

    // Aggregate vectors per capability
    let mut aggregated: HashMap<String, crate::trajectory::p1::vector::TrajectoryVector> =
        HashMap::new();
    for (cap, vecs) in &cap_vectors {
        let agg = CouplingBuilder::aggregate_vectors(vecs);
        aggregated.insert(cap.clone(), agg);
    }

    // For now, use simplified co-change (no git data) — session co-occurrence only
    let total_sessions = cap_vectors.values().map(|v| v.len()).max().unwrap_or(1);
    let cochange_fn = |a: &str, b: &str| {
        let count_a = cap_vectors.get(a).map(|v| v.len()).unwrap_or(0) as u32;
        let count_b = cap_vectors.get(b).map(|v| v.len()).unwrap_or(0) as u32;
        let cooccur = if a == b { 1 } else { 0 }; // simplified: same-session inference not available
        CochangeEvidence::from_counts(cooccur, count_a, count_b, total_sessions as u32);
        // For no git data, produce a flat evidence
        CochangeEvidence {
            fused_score: CochangeEvidence::from_counts(1, count_a, count_b, total_sessions as u32),
            ..Default::default()
        }
    };

    let builder = CouplingBuilder::default();
    let raw_edges = builder.build_edges(&aggregated, &cochange_fn);

    // Also build a proper graph with per-pair co-change if trace-to-cap mapping is many-to-one
    let graph = builder.build(&aggregated, &cochange_fn);
    let pruner = Pruner::default();
    let pruned = pruner.prune(&graph);

    println!("Coupling Graph Built:");
    println!("  Capabilities: {}", pruned.capabilities.len());
    println!("  Raw edges: {}", raw_edges.len());
    println!("  After pruning: {}", pruned.edges.len());

    if pruned.edges.is_empty() {
        println!("\n  No edges after pruning. Try lowering pruning thresholds.");
        return Ok(());
    }

    println!("\n  Top Couplings:");
    let mut sorted_edges = pruned.edges.clone();
    sorted_edges.sort_by(|a, b| {
        b.correlation_score
            .partial_cmp(&a.correlation_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for (i, edge) in sorted_edges.iter().take(10).enumerate() {
        println!(
            "  {}. {} → {}  corr={:.4}  (cochange={:.4}, sim={:.4})",
            i + 1,
            edge.from_capability,
            edge.to_capability,
            edge.correlation_score,
            edge.cochange_score,
            edge.similarity_score
        );
    }

    if let Some(path) = output {
        let json = serde_json::to_string_pretty(&pruned)
            .map_err(|e| format!("JSON error: {}", e))?;
        std::fs::write(&path, &json)
            .map_err(|e| format!("Write error: {}", e))?;
        println!("\nGraph exported to {}", path);
    }

    Ok(())
}

/// 分析特定 capability 的耦合关系。
pub fn run_coupling_analyze(
    storage: &TraceStorage,
    trace_pairs: &[(String, String)],
    capability: &str,
) -> Result<(), String> {
    // Build graph
    let mut cap_vectors: HashMap<String, Vec<crate::trajectory::p1::vector::TrajectoryVector>> =
        HashMap::new();

    for (tid, cap_id) in trace_pairs {
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
        cap_vectors.entry(cap_id.clone()).or_default().push(v);
    }

    let mut aggregated: HashMap<String, _> = HashMap::new();
    let total_sessions = cap_vectors.values().map(|v| v.len()).max().unwrap_or(1);
    for (cap, vecs) in &cap_vectors {
        let agg = CouplingBuilder::aggregate_vectors(vecs);
        aggregated.insert(cap.clone(), agg);
    }

    let cochange_fn = |a: &str, b: &str| {
        let count_a = cap_vectors.get(a).map(|v| v.len()).unwrap_or(0) as u32;
        let count_b = cap_vectors.get(b).map(|v| v.len()).unwrap_or(0) as u32;
        CochangeEvidence {
            fused_score: CochangeEvidence::from_counts(1, count_a, count_b, total_sessions as u32),
            ..Default::default()
        }
    };

    let builder = CouplingBuilder::default();
    let graph = builder.build(&aggregated, &cochange_fn);
    let pruner = Pruner::default();
    let pruned = pruner.prune(&graph);

    let strength = CorrelationEngine::coupling_strength(&pruned.edges, capability);
    let impacts = CorrelationEngine::impact(&pruned.edges, capability, 10);

    println!("Coupling Analysis: {}", capability);
    println!("  Edge count:      {}", strength.edge_count);
    println!("  Total coupling:  {:.4}", strength.total_coupling);
    println!("  Max coupling:    {:.4}", strength.max_coupling);
    println!("  Avg coupling:    {:.4}", strength.avg_coupling);
    println!("  Std deviation:   {:.4}", strength.std_dev);

    if impacts.is_empty() {
        println!("\n  No connected capabilities.");
    } else {
        println!("\n  Impact analysis (top connected):");
        for (i, impact) in impacts.iter().enumerate() {
            let dir = match impact.direction {
                crate::trajectory::p2::correlation::ImpactDirection::Outgoing => "→",
                crate::trajectory::p2::correlation::ImpactDirection::Incoming => "←",
            };
            println!(
                "  {}. {} {}  corr={:.4}  sim={:.4}",
                i + 1,
                impact.target,
                dir,
                impact.correlation,
                impact.similarity
            );
        }
    }

    Ok(())
}

/// 导出 coupling graph 为 Mermaid 格式。
pub fn run_coupling_graph_export(
    storage: &TraceStorage,
    trace_pairs: &[(String, String)],
    format: &str,
) -> Result<(), String> {
    // Build graph (same as build)
    let mut cap_vectors: HashMap<String, Vec<crate::trajectory::p1::vector::TrajectoryVector>> =
        HashMap::new();

    for (tid, cap_id) in trace_pairs {
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
        cap_vectors.entry(cap_id.clone()).or_default().push(v);
    }

    let mut aggregated = HashMap::new();
    let total_sessions = cap_vectors.values().map(|v| v.len()).max().unwrap_or(1);
    for (cap, vecs) in &cap_vectors {
        let agg = CouplingBuilder::aggregate_vectors(vecs);
        aggregated.insert(cap.clone(), agg);
    }

    let cochange_fn = |a: &str, b: &str| {
        let count_a = cap_vectors.get(a).map(|v| v.len()).unwrap_or(0) as u32;
        let count_b = cap_vectors.get(b).map(|v| v.len()).unwrap_or(0) as u32;
        CochangeEvidence {
            fused_score: CochangeEvidence::from_counts(1, count_a, count_b, total_sessions as u32),
            ..Default::default()
        }
    };

    let builder = CouplingBuilder::default();
    let graph = builder.build(&aggregated, &cochange_fn);
    let pruner = Pruner::default();
    let pruned = pruner.prune(&graph);

    match format {
        "mermaid" => {
            println!("```mermaid");
            println!("graph LR");
            for cap in &pruned.capabilities {
                let short = &cap[..cap.len().min(20)];
                println!("    {}[\"{}\"]", sanitize_mermaid_id(cap), short);
            }
            for edge in &pruned.edges {
                println!(
                    "    {} -->|\"{:.2}\"| {}",
                    sanitize_mermaid_id(&edge.from_capability),
                    edge.correlation_score,
                    sanitize_mermaid_id(&edge.to_capability)
                );
            }
            println!("```");
        }
        "json" => {
            let json = serde_json::to_string_pretty(&pruned)
                .map_err(|e| format!("JSON error: {}", e))?;
            println!("{}", json);
        }
        _ => {
            println!("Capabilities: {}", pruned.capabilities.len());
            println!("Edges: {}", pruned.edges.len());
        }
    }

    Ok(())
}

fn sanitize_mermaid_id(s: &str) -> String {
    s.replace('-', "_").replace('.', "_").replace(' ', "_")
}
