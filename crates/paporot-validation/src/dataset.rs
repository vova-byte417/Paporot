//! Golden Dataset 加载器
//!
//! 从 YAML 文件反序列化 Case，支持按 category/id 过滤。

use crate::types::{Case, CaseCategory};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// 从 YAML 文件加载单个 Case
pub fn load_case(path: &Path) -> Result<Case> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read case file: {}", path.display()))?;
    let case: Case = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse YAML: {}", path.display()))?;
    Ok(case)
}

/// 从目录递归加载所有 YAML Case
pub fn load_all(dir: &Path) -> Result<Vec<Case>> {
    let mut cases = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("yaml"))
    {
        match load_case(entry.path()) {
            Ok(c) => cases.push(c),
            Err(e) => eprintln!("  [skip] {} → {:#}", entry.path().display(), e),
        }
    }
    cases.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(cases)
}

/// 按 category 过滤
pub fn filter_by_category(cases: Vec<Case>, cat: CaseCategory) -> Vec<Case> {
    cases.into_iter().filter(|c| c.category == cat).collect()
}

/// 按 id 过滤
pub fn filter_by_id(cases: Vec<Case>, id: &str) -> Vec<Case> {
    cases.into_iter().filter(|c| c.id == id).collect()
}

/// 把 fixture 相对路径转为相对 dataset 目录的绝对路径
pub fn resolve_fixture_path(case_path: &Path, relative: &Path) -> PathBuf {
    let case_dir = case_path.parent().unwrap_or(Path::new("."));
    case_dir.join(relative)
}
