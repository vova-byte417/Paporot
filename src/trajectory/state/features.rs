//! StateFeatures 提取器：从 tool calls 序列中计算六维行为特征向量。

use std::collections::HashMap;

use crate::trace::types::ToolCall;
use crate::trajectory::types::StateFeatures;

/// 从一组 tool calls 提取 StateFeatures。
pub fn extract_features(tools: &[ToolCall], all_tools: &[ToolCall]) -> StateFeatures {
    if tools.is_empty() {
        return StateFeatures::default();
    }

    let n = tools.len() as f32;
    let total = all_tools.len() as f32;

    // tool_histogram: 归一化频次
    let mut tool_hist: HashMap<String, f32> = HashMap::new();
    for tc in tools {
        let key = tool_category(&tc.tool_name);
        *tool_hist.entry(key).or_insert(0.0) += 1.0;
    }
    for v in tool_hist.values_mut() {
        *v /= n;
    }

    // file_clusters: 从 args 中提取 path 关键词
    let mut files: HashMap<String, f32> = HashMap::new();
    for tc in tools {
        if let Some(path) = extract_path(&tc.args) {
            let cluster = file_cluster(&path);
            *files.entry(cluster).or_insert(0.0) += 1.0;
        }
    }
    for v in files.values_mut() {
        *v /= n;
    }

    // edit_density: (edit + write + delete) tools / total
    let edit_count = tools.iter().filter(|t| is_edit_tool(&t.tool_name)).count() as f32;
    let edit_density = if n > 0.0 { edit_count / n } else { 0.0 };

    // read_write_ratio: read tools / total
    let read_count = tools.iter().filter(|t| is_read_tool(&t.tool_name)).count() as f32;
    let read_write_ratio = if n > 0.0 { read_count / n } else { 0.0 };

    // loop_intensity: 连续重复 category 的频次
    let mut loops = 0;
    let mut prev_cat: String = String::new();
    for tc in tools {
        let cat = tool_category(&tc.tool_name);
        if cat == prev_cat && !cat.is_empty() {
            loops += 1;
        }
        prev_cat = cat;
    }
    let loop_intensity = if n > 1.0 { loops as f32 / (n - 1.0) } else { 0.0 };

    // failure_rate: failure/retry tools
    let fail_count = tools.iter().filter(|t| is_failure_tool(&t.tool_name)).count() as f32;
    let failure_rate = if total > 0.0 { fail_count / total } else { 0.0 };

    StateFeatures {
        tool_histogram: tool_hist,
        file_clusters: files,
        edit_density,
        read_write_ratio,
        loop_intensity,
        failure_rate,
    }
}

/// 按语义把 tool_name 归入类别。
fn tool_category(name: &str) -> String {
    match name {
        "read" | "grep" | "glob" | "search_codebase" | "web_search" | "web_fetch"
        | "ls" | "list" => "locate".into(),
        "write" | "edit" | "search_replace" | "delete_file" | "bash" | "run_command"
        => "modify".into(),
        "test" | "cargo" | "check" | "lint" | "clippy" | "build" | "compile"
        => "verify".into(),
        "commit" | "git" | "push" | "pull_request" => "commit".into(),
        _ => "other".into(),
    }
}

fn is_edit_tool(name: &str) -> bool {
    matches!(name, "write" | "edit" | "search_replace" | "delete_file")
}

fn is_read_tool(name: &str) -> bool {
    matches!(name, "read" | "grep" | "glob" | "search_codebase" | "ls" | "list")
}

fn is_failure_tool(name: &str) -> bool {
    matches!(name, "test" | "check" | "lint" | "clippy" | "build")
}

fn extract_path(args: &serde_json::Value) -> Option<String> {
    args.get("path")
        .or_else(|| args.get("file"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn file_cluster(path: &str) -> String {
    // 简单聚类：取文件扩展名或目录前缀
    if let Some(pos) = path.rfind('.') {
        let ext = &path[pos..];
        match ext {
            ".rs" | ".toml" | ".lock" => "rust".into(),
            ".ts" | ".tsx" | ".js" | ".jsx" | ".json" => "ts_js".into(),
            ".py" => "python".into(),
            ".go" => "go".into(),
            ".md" | ".txt" => "doc".into(),
            _ => "other".into(),
        }
    } else if path.contains("src/") {
        "src".into()
    } else if path.contains("tests/") {
        "test".into()
    } else {
        "other".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::types::ToolCall;

    fn tc(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall { id: name.into(), tool_name: name.into(), args, timestamp: "now".into(), duration_ms: 100, result_id: None }
    }

    #[test]
    fn test_extract_empty() {
        let f = extract_features(&[], &[]);
        assert_eq!(f.edit_density, 0.0);
        assert_eq!(f.read_write_ratio, 0.0);
        assert!(f.tool_histogram.is_empty());
    }

    #[test]
    fn test_tool_histogram() {
        let tools = vec![
            tc("read", serde_json::json!({"path": "a.rs"})),
            tc("edit", serde_json::json!({"path": "a.rs"})),
            tc("read", serde_json::json!({"path": "b.rs"})),
            tc("test", serde_json::json!({})),
        ];
        let f = extract_features(&tools, &tools);
        assert_eq!(f.tool_histogram.get("locate"), Some(&0.5));
        assert_eq!(f.tool_histogram.get("modify"), Some(&0.25));
        assert_eq!(f.tool_histogram.get("verify"), Some(&0.25));
    }

    #[test]
    fn test_edit_density() {
        let tools = vec![
            tc("read", serde_json::json!({})),
            tc("edit", serde_json::json!({})),
            tc("write", serde_json::json!({})),
            tc("test", serde_json::json!({})),
        ];
        let f = extract_features(&tools, &tools);
        assert!((f.edit_density - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_read_write_ratio() {
        let tools = vec![
            tc("read", serde_json::json!({})),
            tc("grep", serde_json::json!({})),
            tc("ls", serde_json::json!({})),
            tc("edit", serde_json::json!({})),
        ];
        let f = extract_features(&tools, &tools);
        assert!((f.read_write_ratio - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_loop_intensity() {
        let tools = vec![
            tc("read", serde_json::json!({})),
            tc("read", serde_json::json!({})),  // loop: same category
            tc("edit", serde_json::json!({})),
            tc("edit", serde_json::json!({})),  // loop
            tc("test", serde_json::json!({})),
        ];
        let f = extract_features(&tools, &tools);
        // 2 loops / 4 gaps = 0.5
        assert!((f.loop_intensity - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_file_clusters() {
        let tools = vec![
            tc("read", serde_json::json!({"path": "src/main.rs"})),
            tc("edit", serde_json::json!({"path": "src/lib.rs"})),
            tc("read", serde_json::json!({"path": "tests/test.py"})),
        ];
        let f = extract_features(&tools, &tools);
        assert_eq!(f.file_clusters.get("rust"), Some(&(2.0 / 3.0)));
        assert_eq!(f.file_clusters.get("python"), Some(&(1.0 / 3.0)));
    }
}
