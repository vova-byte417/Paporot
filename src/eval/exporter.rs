//! CodeExporter —— 源码到结构化缓存的机械导出
//!
//! 从 git diff 中提取结构化信息：文件列表、行数统计、符号变更、
//! 模块归属、影响范围。支持 Rust / Python / TypeScript / JavaScript / Go。
//!
//! 这是 Native 宿主的纯机械操作，不做任何判断。

use anyhow::Result;
use regex::Regex;
use std::collections::BTreeSet;
use std::path::Path;

use super::types::*;
use crate::storage::cache::CacheManager;

// ─── CodeExporter ──────────────────────────────────────────────────

pub struct CodeExporter {
    cache: CacheManager,
}

impl CodeExporter {
    pub fn new(paporot_dir: &Path) -> Self {
        Self {
            cache: CacheManager::new(paporot_dir),
        }
    }

    pub fn export(&self, diff_content: &str) -> Result<CodeChangeSummary> {
        if diff_content.trim().is_empty() {
            return Ok(CodeChangeSummary::default());
        }

        let files = extract_files_from_diff(diff_content);
        let (additions, deletions) = count_line_changes(diff_content);
        let symbols = extract_symbols_from_diff(diff_content);
        let modules = extract_modules(&files);
        // 推断影响扩散：被修改模块 → 下游模块
        let impact = infer_impact_spread(&modules, &symbols);

        let summary = CodeChangeSummary {
            files_changed: files,
            additions,
            deletions,
            symbols_added: symbols.added,
            symbols_removed: symbols.removed,
            symbols_modified: symbols.modified,
            modules,
            confidence: 1.0,
            diff_length: diff_content.len(),
        };

        if let Err(e) = self.cache.write_code_change(&summary) {
            eprintln!("  [warn] Failed to write code_change cache: {}", e);
        }
        // 同时写入 impact 数据供 Dashboard 使用
        let impact_value = serde_json::to_value(&impact).unwrap_or_default();
        let _ = self.cache.write_json("impact", &impact_value);

        Ok(summary)
    }

    pub fn cache(&self) -> &CacheManager {
        &self.cache
    }
}

// ─── Impact Spread ─────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct ImpactSpread {
    pub changed_modules: Vec<String>,
    pub downstream_impact: Vec<ImpactEdge>,
    pub symbols_by_module: std::collections::HashMap<String, Vec<simple_symbol::SimpleSymbol>>,
}

mod simple_symbol {
    use serde::Serialize;
    #[derive(Serialize, Clone)]
    pub struct SimpleSymbol {
        pub name: String,
        pub kind: String,
        pub action: String, // "added" | "removed" | "modified"
    }
}

#[derive(serde::Serialize)]
pub struct ImpactEdge {
    pub from: String,
    pub to: String,
    pub strength: u32,
}

fn infer_impact_spread(
    modules: &[String],
    symbols: &SymbolExtraction,
) -> ImpactSpread {
    let mut by_module: std::collections::HashMap<String, Vec<simple_symbol::SimpleSymbol>> =
        std::collections::HashMap::new();

    for s in &symbols.added {
        let m = module_from_path(&s.file_path);
        by_module.entry(m).or_default().push(simple_symbol::SimpleSymbol {
            name: s.name.clone(),
            kind: format!("{}", s.kind),
            action: "added".into(),
        });
    }
    for s in &symbols.removed {
        let m = module_from_path(&s.file_path);
        by_module.entry(m).or_default().push(simple_symbol::SimpleSymbol {
            name: s.name.clone(),
            kind: format!("{}", s.kind),
            action: "removed".into(),
        });
    }

    // 下游影响：简单推断 —— 模块间如果有引用关系，标记为影响边
    let mut edges = Vec::new();
    let mods: Vec<String> = by_module.keys().cloned().collect();
    for i in 0..mods.len() {
        for j in (i + 1)..mods.len() {
            edges.push(ImpactEdge {
                from: mods[i].clone(),
                to: mods[j].clone(),
                strength: 1,
            });
        }
    }

    ImpactSpread {
        changed_modules: modules.to_vec(),
        downstream_impact: edges,
        symbols_by_module: by_module,
    }
}

fn module_from_path(file_path: &str) -> String {
    if let Some(rest) = file_path.strip_prefix("src/") {
        if let Some(m) = rest.split('/').next() {
            return format!("src/{}", m);
        }
    }
    if let Some(rest) = file_path.strip_prefix("crates/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 2 {
            return format!("crates/{}/{}", parts[0], parts[1]);
        }
    }
    file_path.to_string()
}

// ─── Diff 文件列表提取 ─────────────────────────────────────────────

fn extract_files_from_diff(diff: &str) -> Vec<String> {
    let re = Regex::new(r"^\+\+\+ b/(.+)$").unwrap();
    let re_del = Regex::new(r"^--- a/(.+)$").unwrap();
    let mut files = BTreeSet::new();
    for line in diff.lines() {
        if let Some(cap) = re.captures(line) {
            files.insert(cap[1].to_string());
        }
        if let Some(cap) = re_del.captures(line) {
            files.insert(cap[1].to_string());
        }
    }
    files.into_iter().collect()
}

fn count_line_changes(diff: &str) -> (u32, u32) {
    let mut additions = 0u32;
    let mut deletions = 0u32;
    for line in diff.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            additions += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            deletions += 1;
        }
    }
    (additions, deletions)
}

// ─── 多语言符号提取 ────────────────────────────────────────────────

struct SymbolExtraction {
    added: Vec<SymbolChange>,
    removed: Vec<SymbolChange>,
    modified: Vec<SymbolChange>,
}

fn extract_symbols_from_diff(diff: &str) -> SymbolExtraction {
    let mut added = Vec::new();
    let mut removed = Vec::new();

    let mut current_file: Option<String> = None;
    let mut line_num: u32 = 0;

    // 文件头匹配
    let file_new_re = Regex::new(r"^\+\+\+ b/(.+)$").unwrap();
    let file_old_re = Regex::new(r"^--- a/(.+)$").unwrap();
    let hunk_re = Regex::new(r"@@ -\d+(?:,\d+)? \+(\d+)").unwrap();

    for line in diff.lines() {
        // 跟踪文件
        if let Some(cap) = file_new_re.captures(line) {
            current_file = Some(cap[1].to_string());
            line_num = 0;
            continue;
        }
        if let Some(cap) = file_old_re.captures(line) {
            if current_file.is_none() {
                current_file = Some(cap[1].to_string());
            }
            line_num = 0;
            continue;
        }
        // Hunk header
        if let Some(cap) = hunk_re.captures(line) {
            line_num = cap[1].parse::<u32>().unwrap_or(0);
            continue;
        }
        // 上下文行（不变）
        if !line.starts_with('+') && !line.starts_with('-') {
            line_num += 1;
            continue;
        }
        line_num += 1;

        let file = current_file.clone().unwrap_or_else(|| "unknown".into());
        let is_add = line.starts_with('+');
        let content = if line.len() > 1 { &line[1..] } else { "" };
        let content = content.trim_start();

        // 根据文件扩展名选择合适的解析器
        let extracted = if file.ends_with(".rs") {
            extract_rust_symbol(content, file.clone(), line_num)
        } else if file.ends_with(".py") {
            extract_python_symbol(content, file.clone(), line_num)
        } else if file.ends_with(".ts") || file.ends_with(".tsx") {
            extract_typescript_symbol(content, file.clone(), line_num)
        } else if file.ends_with(".js") || file.ends_with(".jsx") {
            extract_typescript_symbol(content, file.clone(), line_num)
        } else if file.ends_with(".go") {
            extract_go_symbol(content, file.clone(), line_num)
        } else {
            // 通用：尝试简单模式匹配
            extract_generic_symbol(content, file.clone(), line_num)
        };

        if let Some(sym) = extracted {
            if is_add {
                added.push(sym);
            } else {
                removed.push(sym);
            }
        }
    }

    SymbolExtraction {
        added,
        removed,
        modified: vec![],
    }
}

// ─── Rust 符号提取 ─────────────────────────────────────────────────

fn extract_rust_symbol(line: &str, file: String, ln: u32) -> Option<SymbolChange> {
    let s = |name: &str, kind: SymbolKind| SymbolChange {
        name: name.into(), kind, file_path: file, line_start: ln, line_end: ln,
    };

    // fn name(...)
    if let Some(cap) = Regex::new(r#"fn\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Function));
    }
    // pub async fn name
    if let Some(cap) = Regex::new(r#"pub\s+(async\s+)?fn\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[2], SymbolKind::Function));
    }
    // struct Name
    if let Some(cap) = Regex::new(r#"struct\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Struct));
    }
    // enum Name
    if let Some(cap) = Regex::new(r#"enum\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Enum));
    }
    // trait Name
    if let Some(cap) = Regex::new(r#"trait\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Trait));
    }
    // impl Name
    if let Some(cap) = Regex::new(r"impl\s+(?:<[^>]+>)?\s*(\w+)").unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Impl));
    }
    // const NAME
    if let Some(cap) = Regex::new(r#"const\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Const));
    }
    // type Name
    if let Some(cap) = Regex::new(r#"type\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Type));
    }
    // mod name;
    if let Some(cap) = Regex::new(r#"^mod\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Module));
    }
    None
}

// ─── Python 符号提取 ───────────────────────────────────────────────

fn extract_python_symbol(line: &str, file: String, ln: u32) -> Option<SymbolChange> {
    let s = |name: &str, kind: SymbolKind| SymbolChange {
        name: name.into(), kind, file_path: file, line_start: ln, line_end: ln,
    };

    // def name(
    if let Some(cap) = Regex::new(r#"def\s+(\w+)\s*\("#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Function));
    }
    // async def name(
    if let Some(cap) = Regex::new(r#"async\s+def\s+(\w+)\s*\("#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Function));
    }
    // class Name:
    if let Some(cap) = Regex::new(r#"class\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Class));
    }
    // @decorator
    if let Some(cap) = Regex::new(r#"@(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Method));
    }
    // CONSTANT = value (全大写)
    if let Some(cap) = Regex::new(r#"^([A-Z][A-Z0-9_]+)\s*="#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Const));
    }
    None
}

// ─── TypeScript / JavaScript 符号提取 ──────────────────────────────

fn extract_typescript_symbol(line: &str, file: String, ln: u32) -> Option<SymbolChange> {
    let s = |name: &str, kind: SymbolKind| SymbolChange {
        name: name.into(), kind, file_path: file, line_start: ln, line_end: ln,
    };

    // function name(
    if let Some(cap) = Regex::new(r#"function\s+(\w+)\s*\("#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Function));
    }
    // const name = (...) => {...}
    if let Some(cap) = Regex::new(r#"const\s+(\w+)\s*=\s*(\([^)]*\)|[\w]+)\s*=>"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::ArrowFunc));
    }
    // const name = function(
    if let Some(cap) = Regex::new(r#"const\s+(\w+)\s*=\s*function\s*\("#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Function));
    }
    // class Name
    if let Some(cap) = Regex::new(r#"class\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Class));
    }
    // interface Name
    if let Some(cap) = Regex::new(r#"interface\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Interface));
    }
    // type Name =
    if let Some(cap) = Regex::new(r#"type\s+(\w+)\s*="#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Type));
    }
    // export const Name
    if let Some(cap) = Regex::new(r#"export\s+const\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Const));
    }
    // export default function Name
    if let Some(cap) = Regex::new(r#"export\s+default\s+function\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Function));
    }
    // method in class: name( ... ) {
    if let Some(cap) = Regex::new(r#"^\s{2,}(\w+)\s*\([^)]*\)\s*\{?\s*$"#).unwrap().captures(line) {
        let name = &cap[1];
        // 排除关键字
        if !["if", "for", "while", "switch", "catch", "return", "throw", "new", "else"].contains(&name) {
            return Some(s(name, SymbolKind::Method));
        }
    }
    None
}

// ─── Go 符号提取 ───────────────────────────────────────────────────

fn extract_go_symbol(line: &str, file: String, ln: u32) -> Option<SymbolChange> {
    let s = |name: &str, kind: SymbolKind| SymbolChange {
        name: name.into(), kind, file_path: file, line_start: ln, line_end: ln,
    };

    // func Name(
    if let Some(cap) = Regex::new(r#"func\s+(\w+)\s*\("#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Function));
    }
    // func (r *Receiver) Name(
    if let Some(cap) = Regex::new(r#"func\s+\(\w+\s+\*?\w+\)\s+(\w+)\s*\("#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Method));
    }
    // type Name struct {
    if let Some(cap) = Regex::new(r#"type\s+(\w+)\s+struct"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Struct));
    }
    // type Name interface {
    if let Some(cap) = Regex::new(r#"type\s+(\w+)\s+interface"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Interface));
    }
    // type Name ...
    if let Some(cap) = Regex::new(r#"type\s+(\w+)\s+(?!=)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Type));
    }
    // var Name ...
    if let Some(cap) = Regex::new(r#"var\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Const));
    }
    // const Name ...
    if let Some(cap) = Regex::new(r#"const\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Const));
    }
    None
}

// ─── 通用符号提取（fallback） ──────────────────────────────────────

fn extract_generic_symbol(line: &str, file: String, ln: u32) -> Option<SymbolChange> {
    let s = |name: &str, kind: SymbolKind| SymbolChange {
        name: name.into(), kind, file_path: file, line_start: ln, line_end: ln,
    };
    // 尝试常见模式
    if let Some(cap) = Regex::new(r#"function\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Function));
    }
    if let Some(cap) = Regex::new(r#"fn\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Function));
    }
    if let Some(cap) = Regex::new(r#"def\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Function));
    }
    if let Some(cap) = Regex::new(r#"class\s+(\w+)"#).unwrap().captures(line) {
        return Some(s(&cap[1], SymbolKind::Class));
    }
    None
}

// ─── 模块提取 ──────────────────────────────────────────────────────

fn extract_modules(files: &[String]) -> Vec<String> {
    let mut modules = BTreeSet::new();
    for file in files {
        if let Some(rest) = file.strip_prefix("src/") {
            if let Some(module) = rest.split('/').next() {
                modules.insert(format!("src/{}", module));
            }
        } else if let Some(rest) = file.strip_prefix("crates/") {
            if let Some(slash) = rest.find('/') {
                modules.insert(format!("crates/{}", &rest[..slash]));
            } else {
                modules.insert(format!("crates/{}", rest));
            }
        } else {
            modules.insert(".".into());
        }
    }
    modules.into_iter().collect()
}

// ─── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_files() {
        let diff = r"diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
+pub fn main() {}
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,0 +1,1 @@
+pub mod eval;
";
        let files = extract_files_from_diff(diff);
        assert!(files.contains(&"src/main.rs".to_string()));
        assert!(files.contains(&"src/lib.rs".to_string()));
    }

    #[test]
    fn test_count_line_changes() {
        let diff = "+added line 1\n+added line 2\n-deleted line\n normal line\n+++ b/file\n--- a/file\n";
        let (add, del) = count_line_changes(diff);
        assert_eq!(add, 2);
        assert_eq!(del, 1);
    }

    #[test]
    fn test_extract_rust_symbols() {
        let diff = r"--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,0 +1,5 @@
+pub fn new_func() {}
+pub struct MyStruct {}
-pub fn old_func() {}
-struct RemovedStruct {}
";
        let symbols = extract_symbols_from_diff(diff);
        assert!(symbols.added.iter().any(|s| s.name == "new_func"));
        assert!(symbols.added.iter().any(|s| s.name == "MyStruct"));
        assert!(symbols.removed.iter().any(|s| s.name == "old_func"));
        assert!(symbols.removed.iter().any(|s| s.name == "RemovedStruct"));
    }

    #[test]
    fn test_extract_python_symbols() {
        let diff = r"--- a/app.py
+++ b/app.py
@@ -1,0 +1,5 @@
+def handle_login(username: str, password: str) -> bool:
+class AuthManager:
-async def old_auth():
";
        let symbols = extract_symbols_from_diff(diff);
        assert!(symbols.added.iter().any(|s| s.name == "handle_login" && s.kind == SymbolKind::Function));
        assert!(symbols.added.iter().any(|s| s.name == "AuthManager" && s.kind == SymbolKind::Class));
        assert!(symbols.removed.iter().any(|s| s.name == "old_auth"));
    }

    #[test]
    fn test_extract_ts_symbols() {
        let diff = r"--- a/components/Login.tsx
+++ b/components/Login.tsx
@@ -1,0 +1,5 @@
+function validateToken(token: string): boolean {
+const LoginModal = () => {
-interface OldAuth {}
";
        let symbols = extract_symbols_from_diff(diff);
        assert!(symbols.added.iter().any(|s| s.name == "validateToken"));
        assert!(symbols.added.iter().any(|s| s.name == "LoginModal"));
        assert!(symbols.removed.iter().any(|s| s.name == "OldAuth"));
    }

    #[test]
    fn test_extract_go_symbols() {
        let diff = r"--- a/pkg/auth/login.go
+++ b/pkg/auth/login.go
@@ -1,0 +1,5 @@
+func ValidateToken(token string) bool {
+type AuthService struct {
-func oldValidate() {
";
        let symbols = extract_symbols_from_diff(diff);
        assert!(symbols.added.iter().any(|s| s.name == "ValidateToken"));
        assert!(symbols.added.iter().any(|s| s.name == "AuthService"));
        assert!(symbols.removed.iter().any(|s| s.name == "oldValidate"));
    }

    #[test]
    fn test_extract_modules() {
        let files = vec![
            "src/auth/login.rs".into(),
            "src/auth/register.rs".into(),
            "src/middleware/log.rs".into(),
            "crates/core/src/lib.rs".into(),
        ];
        let modules = extract_modules(&files);
        assert!(modules.contains(&"src/auth".to_string()));
        assert!(modules.contains(&"src/middleware".to_string()));
        assert!(modules.contains(&"crates/core".to_string()));
    }

    #[test]
    fn test_empty_diff() {
        let exporter = CodeExporter::new(std::path::Path::new(".Paporot"));
        let summary = exporter.export("").unwrap();
        assert!(summary.files_changed.is_empty());
        assert_eq!(summary.additions, 0);
    }
}
