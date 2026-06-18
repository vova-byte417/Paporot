/// Skill: Dependency Analysis
///
/// Goal: 构建模块依赖图，计算耦合指标，检测循环依赖和架构违规
///
/// Inputs: import_graph, symbol_references, call_graph
/// Output: dependency_analysis_output JSON

use paporot_skill_sdk::prelude::*;
use std::collections::{HashMap, HashSet};

#[no_mangle]
pub extern "C" fn paporot_skill_execute() -> i32 {
    let import_graph = match read_input("import_graph") {
        Some(s) => s,
        None => { write_error("Missing import_graph"); return 1; }
    };
    let _symbol_refs = read_input("symbol_references").unwrap_or_default();
    let call_graph = read_input("call_graph").unwrap_or_default();

    // Parse import graph
    let deps: Vec<(String, String)> = import_graph
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(2, "->").collect();
            if parts.len() == 2 {
                Some((parts[0].trim().to_string(), parts[1].trim().to_string()))
            } else {
                None
            }
        })
        .collect();

    // Compute fan-in / fan-out
    let mut fan_in: HashMap<String, usize> = HashMap::new();
    let mut fan_out: HashMap<String, usize> = HashMap::new();
    for (from, to) in &deps {
        *fan_out.entry(from.clone()).or_default() += 1;
        *fan_in.entry(to.clone()).or_default() += 1;
    }

    // Detect cycles (simple DFS)
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for (from, to) in &deps {
        graph.entry(from.clone()).or_default().push(to.clone());
    }

    let cycles = find_cycles(&graph);

    // Identify high-coupling modules
    let mut high_coupling: Vec<Value> = Vec::new();
    for (module, fo) in &fan_out {
        let fi = fan_in.get(module).copied().unwrap_or(0);
        if fi + fo > 10 {
            high_coupling.push(json!({
                "name": module,
                "fan_in": fi,
                "fan_out": fo,
                "risk": if fi + fo > 20 { "high" } else { "medium" }
            }));
        }
    }

    // Build dependencies output
    let dependencies: Vec<Value> = deps
        .iter()
        .map(|(from, to)| json!({"from": from, "to": to, "type": "import"}))
        .collect();

    // Generate Mermaid
    let mermaid_parts: Vec<String> = deps
        .iter()
        .map(|(from, to)| format!("  {} --> {}", sanitize_mermaid(from), sanitize_mermaid(to)))
        .collect();
    let mermaid = format!("graph TD\n{}", mermaid_parts.join("\n"));

    let output = json!({
        "dependencies": dependencies,
        "cycles": cycles,
        "high_coupling_modules": high_coupling,
        "architecture_violations": [],
        "mermaid": mermaid,
        "total_dependencies": deps.len()
    });

    write_output(&output);
    0
}

fn sanitize_mermaid(s: &str) -> String {
    s.replace(['-', '.', ':', '/'], "_")
}

fn find_cycles(graph: &HashMap<String, Vec<String>>) -> Vec<Value> {
    let mut cycles = Vec::new();
    let mut visited = HashSet::new();
    let mut path = Vec::new();

    for node in graph.keys() {
        dfs(node, graph, &mut visited, &mut path, &mut cycles);
    }
    cycles
}

fn dfs(
    node: &str,
    graph: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
    cycles: &mut Vec<Value>,
) {
    if path.contains(&node.to_string()) {
        let start = path.iter().position(|x| x == node).unwrap();
        let cycle: Vec<String> = path[start..].to_vec();
        if cycle.len() > 1 {
            cycles.push(json!({
                "modules": cycle,
                "length": cycle.len()
            }));
        }
        return;
    }
    if visited.contains(node) {
        return;
    }
    visited.insert(node.to_string());
    path.push(node.to_string());
    if let Some(neighbors) = graph.get(node) {
        for neighbor in neighbors {
            dfs(neighbor, graph, visited, path, cycles);
        }
    }
    path.pop();
}

fn main() {}
