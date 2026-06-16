//! `paporot trace` 子命令实现。

use crate::trace::adapter;
use crate::trace::error::TraceError;
use crate::trace::storage::TraceStorage;
use crate::trace::types::{BehaviorTrace, ImportResult, RedactConfig, TraceFilter, TraceSummary};

// ─── import ────────────────────────────────────────────────────

pub fn cmd_import(
    storage: &TraceStorage,
    file_path: &str,
    adapter_name: Option<&str>,
    auto_redact_config: Option<&RedactConfig>,
) -> anyhow::Result<ImportResult> {
    let raw = std::fs::read_to_string(file_path)
        .map_err(|e| anyhow::anyhow!("Failed to read input file {}: {}", file_path, e))?;

    let adapter = if let Some(name) = adapter_name {
        adapter::find_adapter(name).ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown adapter: {}. Use 'paporot trace adapter list'",
                name
            )
        })?
    } else {
        adapter::auto_detect(&raw).ok_or_else(|| {
            anyhow::anyhow!("Could not auto-detect format. Specify --adapter")
        })?
    };

    let mut traces = match adapter.parse(&raw, file_path) {
        Ok(t) => t,
        Err(TraceError::PartialImport {
            imported,
            skipped,
            reasons,
        }) => {
            eprintln!(
                "  [WARN] Partial import: {} imported, {} skipped",
                imported, skipped
            );
            for reason in &reasons {
                eprintln!("         {}", reason);
            }
            // 重新解析以获取成功的那部分
            adapter
                .parse(&raw, file_path)
                .unwrap_or_else(|_| Vec::new())
        }
        Err(e) => return Err(anyhow::anyhow!("Parse error: {}", e)),
    };

    if traces.is_empty() {
        anyhow::bail!("No valid traces found in {}", file_path);
    }

    // 自动脱敏（当配置了 auto_redact_config 时）
    if let Some(config) = auto_redact_config {
        let count = traces.len();
        for trace in &mut traces {
            apply_redact(trace, config);
        }
        eprintln!("  [INFO] Auto-redaction applied to {} traces", count);
    }

    let result = storage.save_batch(traces)?;

    Ok(ImportResult {
        source_path: file_path.to_string(),
        adapter: adapter.name().to_string(),
        auto_detected: adapter_name.is_none(),
        ..result
    })
}

// ─── list ──────────────────────────────────────────────────────

pub fn cmd_list(
    storage: &TraceStorage,
    filter: TraceFilter,
) -> anyhow::Result<Vec<TraceSummary>> {
    storage
        .list(&filter)
        .map_err(|e| anyhow::anyhow!("{}", e))
}

// ─── show ──────────────────────────────────────────────────────

pub enum ShowFormat {
    Summary,
    Json,
    Full,
}

pub fn cmd_show(
    storage: &TraceStorage,
    trace_id: &str,
    format: ShowFormat,
) -> anyhow::Result<()> {
    let trace = storage
        .load(trace_id)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    match format {
        ShowFormat::Summary => {
            print_summary_from_trace(&trace);
        }
        ShowFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&trace)
                    .map_err(|e| anyhow::anyhow!("Serialize error: {}", e))?
            );
        }
        ShowFormat::Full => {
            print_trace_full(&trace);
        }
    }
    Ok(())
}

fn print_summary_from_trace(trace: &BehaviorTrace) {
    let tool_names: Vec<String> = trace
        .tool_calls
        .iter()
        .map(|tc| tc.tool_name.clone())
        .collect();

    let prompt_preview = if trace.prompt.len() > 200 {
        format!("{}...", &trace.prompt[..200])
    } else {
        trace.prompt.clone()
    };

    println!("Trace: {}", trace.id);
    println!("  Session     : {}", trace.session_id);
    println!("  Prompt      : {}", prompt_preview);
    println!("  Tools       : {}", tool_names.join(", "));
    println!(
        "  Tokens      : in={}, out={}",
        trace.token_usage.input_tokens, trace.token_usage.output_tokens
    );
    println!("  Time        : {} → {}", trace.started_at, trace.finished_at);
    println!("  Capabilities: {}", trace.capability_ids.len());
    println!(
        "  Tags        : {}",
        if trace.tags.is_empty() {
            "-".into()
        } else {
            trace.tags.join(", ")
        }
    );
}

fn print_trace_full(trace: &BehaviorTrace) {
    println!("Trace: {}", trace.id);
    println!("  Session     : {}", trace.session_id);
    println!("  Prompt      : {}", trace.prompt);
    println!("  Started     : {}", trace.started_at);
    println!("  Finished    : {}", trace.finished_at);
    println!(
        "  Token Usage : in={}, out={}, cache_read={:?}, cache_write={:?}",
        trace.token_usage.input_tokens,
        trace.token_usage.output_tokens,
        trace.token_usage.cache_read_tokens,
        trace.token_usage.cache_write_tokens,
    );
    println!("  Source      : {:?}", trace.source);
    println!("  Tags        : {:?}", trace.tags);
    println!("  Capabilities: {:?}", trace.capability_ids);
    println!("  Deleted     : {}", trace.deleted);
    println!("  ── Tool Calls ({}) ──", trace.tool_calls.len());
    for tc in &trace.tool_calls {
        println!(
            "    [{}] {} @ {} ({}ms)",
            tc.id, tc.tool_name, tc.timestamp, tc.duration_ms
        );
        println!(
            "      args: {}",
            serde_json::to_string(&tc.args).unwrap_or_default()
        );
        println!("      result_id: {:?}", tc.result_id);
    }
    println!("  ── Observations ({}) ──", trace.observations.len());
    for obs in &trace.observations {
        let preview = if obs.content.len() > 200 {
            format!(
                "{}...[truncated:{}]",
                &obs.content[..200],
                obs.truncated_at_bytes.unwrap_or(0)
            )
        } else {
            obs.content.clone()
        };
        println!("    [{}] <- {}: {}", obs.id, obs.tool_call_id, preview);
    }
    println!("  ── Final Output ──");
    println!("  {}", trace.final_output);
}

// ─── delete ────────────────────────────────────────────────────

pub fn cmd_delete(storage: &TraceStorage, trace_id: &str) -> anyhow::Result<()> {
    storage
        .delete(trace_id)
        .map_err(|e| anyhow::anyhow!("{}", e))
}

// ─── link / unlink ─────────────────────────────────────────────

pub fn cmd_link(storage: &TraceStorage, trace_id: &str, cap_id: &str) -> anyhow::Result<()> {
    let mut trace = storage
        .load(trace_id)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    if !trace.capability_ids.contains(&cap_id.to_string()) {
        trace.capability_ids.push(cap_id.to_string());
    }
    storage.save(&trace)?;
    Ok(())
}

pub fn cmd_unlink(storage: &TraceStorage, trace_id: &str, cap_id: &str) -> anyhow::Result<()> {
    let mut trace = storage
        .load(trace_id)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    trace.capability_ids.retain(|c| c != cap_id);
    storage.save(&trace)?;
    Ok(())
}

// ─── redact ────────────────────────────────────────────────────

pub fn cmd_redact(
    storage: &TraceStorage,
    trace_id: &str,
    config: &RedactConfig,
) -> anyhow::Result<()> {
    let mut trace = storage
        .load(trace_id)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    apply_redact(&mut trace, config);
    storage.save(&trace)?;
    Ok(())
}

fn apply_redact(trace: &mut BehaviorTrace, config: &RedactConfig) {
    use regex::Regex;

    // 默认脱敏模式
    let patterns: Vec<(&str, &str)> = {
        let mut p = Vec::new();
        if config.redact_api_keys {
            p.push(("(?i)(api_?key|apikey)[=:]\\s*[^\\s,;]+", "api_key=***REDACTED***"));
            p.push(("(?i)(token|secret|password)[=:]\\s*[^\\s,;]+", "$1=***REDACTED***"));
        }
        if config.redact_auth_header {
            p.push(("(?i)(authorization|auth)[=:]\\s*[^\\s,;]+", "$1=***REDACTED***"));
            p.push(("(?i)bearer\\s+[^\\s,;]+", "Bearer ***REDACTED***"));
        }
        if config.redact_env_values {
            p.push(("(?i)(SECRET|PASSWORD|TOKEN|KEY)_?[=:]?\\s*[^\\s,;]+", "$1=***REDACTED***"));
        }
        p
    };

    let redact_string = |s: &mut String| {
        for (pattern, replacement) in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                *s = re.replace_all(s, *replacement).to_string();
            }
        }
        // 自定义规则
        for (pattern, replacement) in &config.custom_rules {
            *s = s.replace(pattern.as_str(), replacement.as_str());
        }
    };

    redact_string(&mut trace.prompt);
    redact_string(&mut trace.final_output);

    for tc in &mut trace.tool_calls {
        if let serde_json::Value::String(ref mut s) = tc.args {
            redact_string(s);
        } else if let Some(s) = tc.args.as_str() {
            let mut owned = s.to_string();
            redact_string(&mut owned);
            tc.args = serde_json::Value::String(owned);
        }
    }

    for obs in &mut trace.observations {
        redact_string(&mut obs.content);
    }
}

// ─── adapter list ──────────────────────────────────────────────

pub fn cmd_adapter_list() -> anyhow::Result<Vec<adapter::AdapterInfo>> {
    Ok(adapter::list_adapters())
}

// ─── 配置转换 ─────────────────────────────────────────────────

/// 从 config::TraceRedactConfig 转换为 trace::RedactConfig。
pub fn make_redact_config_from_trace_config(
    tc: &crate::config::TraceRedactConfig,
) -> crate::trace::types::RedactConfig {
    let mut rc = crate::trace::types::RedactConfig::default();
    rc.redact_auth_header = tc.redact_auth_header;
    rc.redact_api_keys = tc.redact_api_keys;
    rc.redact_env_values = tc.redact_env_values;
    rc.custom_rules = tc.custom_rules.clone();
    rc
}

// ─── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::types::{Observation, ToolCall, TraceSource};

    fn make_test_trace() -> BehaviorTrace {
        BehaviorTrace {
            id: "trace_test_001".into(),
            session_id: "sess-abc".into(),
            prompt: "fix the login bug".into(),
            tool_calls: vec![ToolCall {
                id: "call_001".into(),
                tool_name: "grep".into(),
                args: serde_json::json!({"pattern": "login"}),
                timestamp: "2026-06-12T14:00:00Z".into(),
                duration_ms: 100,
                result_id: Some("obs_001".into()),
            }],
            observations: vec![Observation {
                id: "obs_001".into(),
                tool_call_id: "call_001".into(),
                content: "src/auth.rs:42".into(),
                truncated: false,
                truncated_at_bytes: None,
            }],
            final_output: "Fixed".into(),
            token_usage: Default::default(),
            started_at: "2026-06-12T14:00:00Z".into(),
            finished_at: "2026-06-12T14:01:00Z".into(),
            source: TraceSource::Captured {
                agent_name: "test".into(),
            },
            tags: Vec::new(),
            capability_ids: Vec::new(),
            deleted: false,
        }
    }

    #[test]
    fn test_redact_api_keys() {
        let mut trace = make_test_trace();
        trace.prompt = "Use api_key=secret123".into();

        let config = RedactConfig::default();
        apply_redact(&mut trace, &config);
        // 正则脱敏保留 key 名称，但值被替换
        assert!(trace.prompt.contains("***REDACTED***"));
        assert!(!trace.prompt.contains("secret123"));
    }

    #[test]
    fn test_redact_custom_rules() {
        let mut trace = make_test_trace();
        trace.prompt = "Hello world".into();

        let config = RedactConfig {
            custom_rules: vec![("Hello".into(), "Hi".into())],
            ..Default::default()
        };
        apply_redact(&mut trace, &config);
        assert_eq!(trace.prompt, "Hi world");
    }

    #[test]
    fn test_link_and_unlink() {
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().join(".Paporot");
        let storage = TraceStorage::new(&base);

        let trace = make_test_trace();
        storage.save(&trace).unwrap();

        // Link
        cmd_link(&storage, "trace_test_001", "cap_001").unwrap();
        let loaded = storage.load("trace_test_001").unwrap();
        assert!(loaded.capability_ids.contains(&"cap_001".to_string()));

        // Unlink
        cmd_unlink(&storage, "trace_test_001", "cap_001").unwrap();
        let loaded = storage.load("trace_test_001").unwrap();
        assert!(!loaded.capability_ids.contains(&"cap_001".to_string()));
    }
}
