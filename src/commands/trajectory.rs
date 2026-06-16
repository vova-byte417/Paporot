//! Trajectory Diff CLI 命令实现。

use std::path::PathBuf;

use crate::trace::storage::TraceStorage;
use crate::trace::types::TraceFilter;
use crate::trajectory::{
    AlignmentEngine, PhaseClassifier, RuleBasedClassifier, TrajectoryAnalysis, cache::TrajectoryCache, error::TrajectoryError, report,
};

/// 执行 trajectory diff 命令。
pub fn run_diff(
    storage: &TraceStorage,
    base_dir: &PathBuf,
    capability: Option<String>,
    trace_a: Option<String>,
    trace_b: Option<String>,
    format: &str,
    output: Option<String>,
) -> Result<(), TrajectoryError> {
    // Resolve trace IDs
    let (id_a, id_b) = if let Some(cap_id) = &capability {
        // Find traces linked to the capability
        let filter = TraceFilter {
            capability_id: Some(cap_id.clone()),
            ..Default::default()
        };
        let traces = storage.list(&filter)
            .map_err(|e| TrajectoryError::CacheError(format!("Storage error: {:?}", e)))?;

        if traces.len() < 2 {
            return Err(TrajectoryError::InsufficientTraces(traces.len()));
        }
        // Use the two most recent traces
        let idx_a = traces.len() - 2;
        let idx_b = traces.len() - 1;
        (traces[idx_a].id.clone(), traces[idx_b].id.clone())
    } else if let (Some(a), Some(b)) = (trace_a, trace_b) {
        (a, b)
    } else {
        return Err(TrajectoryError::CacheError(
            "Either --capability or both --trace-a and --trace-b must be provided".into()
        ));
    };

    // Load traces
    let trace_a = storage.load(&id_a)
        .map_err(|_| TrajectoryError::TraceNotFound(id_a.to_string()))?;

    let trace_b = storage.load(&id_b)
        .map_err(|_| TrajectoryError::TraceNotFound(id_b.to_string()))?;

    // Compute diff
    let engine = AlignmentEngine::default();
    let classifier = RuleBasedClassifier::default();
    let diff = engine.diff(&classifier, &trace_a, &trace_b, capability.clone());

    // Compute analysis
    let analysis = TrajectoryAnalysis::from_diff(&diff);

    // Generate output
    match format {
        "json" => {
            let json = report::to_json_report(&diff);
            if let Some(path) = output {
                std::fs::write(&path, &json).map_err(TrajectoryError::Io)?;
                println!("JSON report written to {}", path);
            } else {
                println!("{}", json);
            }
        }
        "mermaid" => {
            let mermaid = report::to_mermaid(&diff);
            if let Some(path) = output {
                std::fs::write(&path, &mermaid).map_err(TrajectoryError::Io)?;
                println!("Mermaid diagram written to {}", path);
            } else {
                println!("{}", mermaid);
            }
        }
        _ => {
            // Default: terminal + mermaid + cache
            println!("{}", report::to_terminal_summary(&diff));
            println!();
            println!("{}", report::to_mermaid(&diff));

            // Cache the result
            let cache = TrajectoryCache::new(base_dir);
            cache.init()?;

            let diff_id = format!("tdiff_{}", chrono::Utc::now().format("%Y%m%d_%H%M%S"));
            let analysis_json = serde_json::to_string(&analysis)
                .map_err(TrajectoryError::Json)?;
            let mermaid = report::to_mermaid(&diff);

            cache.store(
                &diff_id,
                &id_a, &id_b,
                capability.as_deref(),
                &diff,
                &analysis_json,
                &mermaid,
                classifier.name(),
                classifier.version(),
                analysis.tool_churn_score,
                analysis.phase_reorder_score,
                analysis.capability_shift_score,
            )?;
            println!("\nDiff cached as {}", diff_id);
        }
    }

    Ok(())
}

/// 列出缓存的 trajectory diff。
pub fn run_list(base_dir: &PathBuf) -> Result<(), TrajectoryError> {
    let cache = TrajectoryCache::new(base_dir);
    cache.init()?;

    let export_dir = base_dir.join("trajectory");
    if !export_dir.exists() {
        println!("No cached trajectory diffs found.");
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&export_dir)
        .map_err(TrajectoryError::Io)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().map(|t| t.is_file()).unwrap_or(false)
                && e.file_name().to_string_lossy().ends_with(".json")
        })
        .collect();

    entries.sort_by_key(|e| e.file_name());

    if entries.is_empty() {
        println!("No cached trajectory diffs found.");
        return Ok(());
    }

    println!("Cached Trajectory Diffs:");
    for entry in &entries {
        println!("  {}", entry.file_name().to_string_lossy().trim_end_matches(".json"));
    }

    Ok(())
}

/// 查看缓存的 trajectory diff 详情。
pub fn run_show(base_dir: &PathBuf, diff_id: &str) -> Result<(), TrajectoryError> {
    let file_path = base_dir.join("trajectory").join(format!("{}.json", diff_id));
    let content = std::fs::read_to_string(&file_path)
        .map_err(|_| TrajectoryError::CacheError(format!("Diff '{}' not found", diff_id)))?;

    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(TrajectoryError::Json)?;

    println!("{}", serde_json::to_string_pretty(&value).unwrap_or(content));
    Ok(())
}
