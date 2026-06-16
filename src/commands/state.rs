//! State CLI 命令实现。

use std::path::PathBuf;

use crate::trace::storage::TraceStorage;
use crate::trajectory::build_state_graph;
use crate::trajectory::evaler::evaluate;

/// 执行 state build 命令。
pub fn run_build(
    storage: &TraceStorage,
    trace_id: &str,
) -> Result<(), String> {
    let trace = storage.load(trace_id)
        .map_err(|e| format!("Trace '{}' not found: {:?}", trace_id, e))?;

    let graph = build_state_graph(&trace);

    println!("BehaviorStateGraph built:");
    println!("  Trace: {}", graph.trace_id);
    println!("  States: {}", graph.states.len());
    println!("  Transitions (events): {}", graph.event_log.len());
    println!("  Edges (aggregated): {}", graph.edges.len());
    println!("  Total tools: {}", graph.total_tools);

    for state in &graph.states {
        println!("\n  State {}:", state.id);
        println!("    Primary phase: {}", state.primary_phase);
        println!("    Stability: {:.2}", state.stability_score);
        println!("    Tools: {}..{}", state.tool_range.0, state.tool_range.1);
        if !state.phase_dist.is_empty() {
            print!("    Phase dist: ");
            for (k, v) in &state.phase_dist {
                print!("{}={:.2} ", k, v);
            }
            println!();
        }
    }

    if !graph.edges.is_empty() {
        println!("\n  Transition Graph:");
        for edge in &graph.edges {
            println!("    {} → {}  (×{})", edge.from, edge.to, edge.count);
        }
    }

    Ok(())
}

/// 执行 state show 命令。
pub fn run_show(
    storage: &TraceStorage,
    trace_id: &str,
    format: &str,
    _base_dir: &PathBuf,
) -> Result<(), String> {
    let trace = storage.load(trace_id)
        .map_err(|e| format!("Trace '{}' not found: {:?}", trace_id, e))?;

    let graph = build_state_graph(&trace);

    match format {
        "json" => {
            let json = serde_json::to_string_pretty(&graph)
                .map_err(|e| format!("JSON error: {}", e))?;
            println!("{}", json);
        }
        "mermaid" => {
            println!("{}", state_graph_to_mermaid(&graph));
        }
        _ => {
            // terminal: reuse build display
            println!("BehaviorStateGraph for trace '{}':", trace_id);
            println!("  States: {}", graph.states.len());
            for state in &graph.states {
                println!("    {}: {} (stability: {:.2})",
                    state.id, state.primary_phase, state.stability_score);
            }
            if !graph.edges.is_empty() {
                println!("  Edges:");
                for edge in &graph.edges {
                    println!("    {} → {} (×{})", edge.from, edge.to, edge.count);
                }
            }
        }
    }

    Ok(())
}

/// 生成 StateGraph 的 Mermaid 表示。
fn state_graph_to_mermaid(graph: &crate::trajectory::types::BehaviorStateGraph) -> String {
    let mut out = String::new();
    out.push_str("```mermaid\ngraph LR\n");
    for state in &graph.states {
        out.push_str(&format!(
            "    {}[\"{} ({})\"]\n",
            state.id, state.primary_phase, state.id
        ));
    }
    for edge in &graph.edges {
        out.push_str(&format!(
            "    {} -->|\"×{}\"| {}\n",
            edge.from, edge.count, edge.to
        ));
    }
    out.push_str("```\n");
    out
}

/// 执行 state diff 命令。
pub fn run_diff(
    storage: &TraceStorage,
    trace_a: &str,
    trace_b: &str,
    _format: &str,
) -> Result<(), String> {
    let ta = storage.load(trace_a)
        .map_err(|e| format!("Trace '{}' not found: {:?}", trace_a, e))?;
    let tb = storage.load(trace_b)
        .map_err(|e| format!("Trace '{}' not found: {:?}", trace_b, e))?;

    let ga = build_state_graph(&ta);
    let gb = build_state_graph(&tb);

    // Simple state diff
    println!("State Diff: {} → {}", ga.trace_id, gb.trace_id);
    println!("  States: {} → {}", ga.states.len(), gb.states.len());
    println!("  Events: {} → {}", ga.event_log.len(), gb.event_log.len());
    println!("  Total tools: {} → {}", ga.total_tools, gb.total_tools);

    // Align states by position
    let max_len = ga.states.len().max(gb.states.len());
    println!("\n  State alignment:");
    for i in 0..max_len {
        let sa = ga.states.get(i);
        let sb = gb.states.get(i);
        match (sa, sb) {
            (Some(a), Some(b)) => {
                let mark = if a.primary_phase == b.primary_phase { " = " } else { " ~ " };
                println!("    {}{}({}) ↔ ({}){}",
                    mark, a.primary_phase, a.id, b.id, b.primary_phase);
            }
            (Some(a), None) => println!("    - ({}) {} (deleted)", a.id, a.primary_phase),
            (None, Some(b)) => println!("    + ({}) {} (added)", b.id, b.primary_phase),
            (None, None) => unreachable!(),
        }
    }

    // Compute evaluation for both graphs
    let eval_a = evaluate(&ga);
    let eval_b = evaluate(&gb);
    println!("\n  Eval A ({}): {:?} ({} hits)", ga.trace_id, eval_a.verdict, eval_a.hits.len());
    println!("  Eval B ({}): {:?} ({} hits)", gb.trace_id, eval_b.verdict, eval_b.hits.len());

    Ok(())
}

/// 执行 state eval 命令。
pub fn run_eval(
    storage: &TraceStorage,
    trace_id: &str,
) -> Result<(), String> {
    let trace = storage.load(trace_id)
        .map_err(|e| format!("Trace '{}' not found: {:?}", trace_id, e))?;

    let graph = build_state_graph(&trace);
    let result = evaluate(&graph);

    println!("State Eval for '{}':", trace_id);
    println!("  Verdict: {:?}", result.verdict);
    println!("  State metrics:");
    println!("    phase_entropy: {:.3}", result.state_metrics.phase_entropy);
    println!("    loop_ratio: {:.3}", result.state_metrics.loop_ratio);
    println!("    state_count: {}", result.state_metrics.state_count);
    println!("  Transition metrics:");
    println!("    oscillation_count: {}", result.transition_metrics.oscillation_count);
    println!("    reversal_ratio: {:.3}", result.transition_metrics.reversal_ratio);
    println!("    total_transitions: {}", result.transition_metrics.total_transitions);
    println!("  Graph metrics:");
    println!("    structural_entropy: {:.3}", result.graph_metrics.structural_entropy);

    if !result.hits.is_empty() {
        println!("\n  Rule Hits:");
        for hit in &result.hits {
            println!("    [{}] {}: {}", hit.severity, hit.rule_id, hit.description);
        }
    }

    Ok(())
}
