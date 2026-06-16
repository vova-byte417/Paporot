//! L1 AST 解析器：确定性提取结构化行为变更
//!
//! 基于正则模式匹配，支持 Rust、TypeScript、Python、Go、Java。
//! 不需要 LLM，纯确定性分析。
//!
//! 对应 PRD P0 §3.2
//!
//! ## 架构
//!
//! ```text
//! FileChange[] → 逐行正则匹配 → RawChange[] → 去重
//!                    │
//!     ┌──────────────┼──────────────┐
//!     │ Rust   │ TypeScript │ Python/Go/Java │ Generic │
//!     │ fn/struct/enum/trait/use/const       │ HTTP route/config │
//! ```
//!
//! ## 设计决策
//!
//! - **纯正则匹配，不调用 LLM**：置信度由匹配模式的严格程度决定
//! - **仅提取公开符号**：跳过私有函数/类（Python 跳过 `_`，Go 跳过小写）
//! - **去重**：同文件同行号同符号 + 同变更类型只保留一条

use regex::Regex;
use super::types::*;

/// AST 分析器（L1 层）
pub struct AstAnalyzer;

/// Regex 编译缓存
///
/// 使用懒加载模式，`PatternCache::new()` 在首次调用 `analyze()` 时编译所有正则。
/// 每个正则负责匹配一种语言的特定语法结构。
struct PatternCache {
    // ── Rust ──
    /// `pub (async)? fn name(params) -> ReturnType`
    rust_fn: Regex,
    /// `pub struct Name`
    rust_struct: Regex,
    /// `pub enum Name`
    rust_enum: Regex,
    /// `pub trait Name`
    rust_trait: Regex,
    /// `use path::to::module;`
    rust_use: Regex,
    /// `pub const NAME: Type`
    rust_const: Regex,

    // ── TypeScript / JavaScript ──
    /// `export (async)? function name(`
    ts_export_function: Regex,
    /// `(export)? class Name`
    ts_class: Regex,
    /// `import ... from 'module'`
    ts_import: Regex,

    // ── Python ──
    /// `def name(`
    py_function: Regex,
    /// `class Name:`
    py_class: Regex,

    // ── Go ──
    /// `func Name(`
    go_function: Regex,
    /// `type Name struct`
    go_struct: Regex,

    // ── Java ──
    /// `public/private/protected Type name(`
    java_method: Regex,
    /// `(public)? class Name`
    java_class: Regex,

    // ── 通用 ──
    /// HTTP 路由注册：`.get('/path'`, `.post('/path'` 等
    http_route: Regex,
    /// 配置文件中的键值变更：`+KEY=value` 或 `-KEY=value`
    config_change: Regex,
}

impl PatternCache {
    fn new() -> Self {
        Self {
            // ── Rust ──
            rust_fn: Regex::new(r"(?m)^\s*pub\s+(async\s+)?fn\s+(\w+)\s*([<(].*)$").unwrap(),
            rust_struct: Regex::new(r"(?m)^\s*pub\s+struct\s+(\w+)").unwrap(),
            rust_enum: Regex::new(r"(?m)^\s*pub\s+enum\s+(\w+)").unwrap(),
            rust_trait: Regex::new(r"(?m)^\s*pub\s+trait\s+(\w+)").unwrap(),
            rust_use: Regex::new(r"(?m)^\s*use\s+(.+);").unwrap(),
            rust_const: Regex::new(r"(?m)^\s*pub\s+const\s+(\w+)\s*:").unwrap(),

            // ── TypeScript/JavaScript ──
            ts_export_function: Regex::new(r"(?m)^\s*export\s+(?:async\s+)?function\s+(\w+)").unwrap(),
            ts_class: Regex::new(r"(?m)^\s*(?:export\s+)?class\s+(\w+)").unwrap(),
            ts_import: Regex::new(r#"(?m)^\s*import\s+(.+)\s+from\s+['"](\S+)['"]"#).unwrap(),

            // ── Python ──
            py_function: Regex::new(r"(?m)^\s*def\s+(\w+)\s*[<(]").unwrap(),
            py_class: Regex::new(r"(?m)^\s*class\s+(\w+)\s*[:(]").unwrap(),

            // ── Go ──
            go_function: Regex::new(r"(?m)^\s*func\s+(\w+)\s*[<(]").unwrap(),
            go_struct: Regex::new(r"(?m)^\s*type\s+(\w+)\s+struct").unwrap(),

            // ── Java ──
            java_method: Regex::new(r"(?m)^\s*(?:public|private|protected)\s+(?:\w+\s+)+(\w+)\s*[<(]").unwrap(),
            java_class: Regex::new(r"(?m)^\s*(?:public\s+)?class\s+(\w+)").unwrap(),

            // ── 通用 ──
            http_route: Regex::new(r#"(?i)(?:\.get|\.post|\.put|\.delete|\.patch)\s*[<(]\s*['\"](/[/\w{}\-]*)['\"]"#).unwrap(),
            config_change: Regex::new(r"(?m)^[-+]\s*([A-Z_]+)\s*[=:]").unwrap(),
        }
    }
}

impl AstAnalyzer {
    /// 对 diff 产出的 FileChange 列表执行 L1 分析
    ///
    /// # Arguments
    /// * `changes` - DiffPreprocessor::parse() 产出的结构化变更列表
    ///
    /// # Returns
    /// 按语言匹配到的 RawChange 列表（已去重）
    ///
    /// # Examples
    /// ```
    /// use Paporot::analysis::preprocessor::DiffPreprocessor;
    /// use Paporot::analysis::l1_ast::AstAnalyzer;
    /// let diff = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -0,0 +1 @@\n+pub fn hello() {}";
    /// let changes = DiffPreprocessor::parse(diff);
    /// let raw = AstAnalyzer::analyze(&changes).unwrap();
    /// assert!(!raw.is_empty());
    /// ```
    pub fn analyze(changes: &[FileChange]) -> anyhow::Result<Vec<RawChange>> {
        let mut raw_changes = Vec::new();
        let mut id_counter = 0u32;
        let cache = PatternCache::new();

        for change in changes {
            let lang = change.language;
            for hunk in &change.hunks {
                let added: Vec<_> = hunk.lines.iter()
                    .filter_map(|l| if let DiffLine::Addition(s) = l { Some(s.as_str()) } else { None })
                    .collect();
                let deleted: Vec<_> = hunk.lines.iter()
                    .filter_map(|l| if let DiffLine::Deletion(s) = l { Some(s.as_str()) } else { None })
                    .collect();

                for line in &added {
                    if let Some(rc) = Self::detect_symbol(
                        &cache, lang, line, &change.path, hunk.new_start, true, &mut id_counter,
                    ) {
                        raw_changes.push(rc);
                    }
                }

                for line in &deleted {
                    if let Some(rc) = Self::detect_symbol(
                        &cache, lang, line, &change.path, hunk.old_start, false, &mut id_counter,
                    ) {
                        raw_changes.push(rc);
                    }
                }
            }
        }

        Self::deduplicate(&mut raw_changes);
        Ok(raw_changes)
    }

    /// 检测单行代码中的符号，分发到对应语言的检测器
    fn detect_symbol(
        cache: &PatternCache,
        lang: Language,
        line: &str,
        file_path: &str,
        base_line: usize,
        is_addition: bool,
        id_counter: &mut u32,
    ) -> Option<RawChange> {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
            return None;
        }

        let module = Self::infer_module(lang, file_path);

        // 分发到语言特定检测器，miss 时回退到通用检测
        let result = match lang {
            Language::Rust => detect_rust(cache, line, file_path, base_line, is_addition, id_counter, module.clone()),
            Language::TypeScript | Language::JavaScript => {
                detect_typescript(cache, line, file_path, base_line, is_addition, id_counter, module.clone(), lang)
            }
            Language::Python => detect_python(cache, line, file_path, base_line, is_addition, id_counter, module.clone()),
            Language::Go => detect_go(cache, line, file_path, base_line, is_addition, id_counter, module.clone()),
            Language::Java => detect_java(cache, line, file_path, base_line, is_addition, id_counter, module.clone()),
            Language::Unknown => detect_generic(cache, line, file_path, base_line, is_addition, id_counter, module.clone()),
        };

        // 语言特定检测未命中，尝试通用检测（如 HTTP 路由）
        if result.is_some() {
            return result;
        }

        detect_generic(cache, line, file_path, base_line, is_addition, id_counter, module)
    }

    // ─── 工具方法 ───

    fn next_id(id_counter: &mut u32) -> String {
        *id_counter += 1;
        format!("rc_{:05}", id_counter)
    }

    fn infer_module(lang: Language, file_path: &str) -> Option<String> {
        match lang {
            Language::Rust => Self::infer_rust_module(file_path),
            _ => Self::infer_module_from_path(file_path),
        }
    }

    fn infer_rust_module(file_path: &str) -> Option<String> {
        let path = std::path::Path::new(file_path);
        if let Some(parent) = path.parent() {
            let parent_str = parent.to_string_lossy().replace('\\', "/");
            if let Some(after_src) = parent_str.strip_prefix("src/") {
                if let Some(first) = after_src.split('/').next() {
                    if !first.is_empty() {
                        return Some(first.to_string());
                    }
                }
            }
            if parent_str.ends_with("src") || parent_str == "src" {
                return path.file_stem().map(|s| s.to_string_lossy().to_string());
            }
        }
        None
    }

    fn infer_module_from_path(file_path: &str) -> Option<String> {
        let path = std::path::Path::new(file_path);
        let components: Vec<&str> = path.iter()
            .map(|c| c.to_str().unwrap_or(""))
            .filter(|c| !c.is_empty() && *c != "src" && *c != "lib" && *c != "app")
            .collect();

        if !components.is_empty() {
            let mut name = components.join("/");
            if let Some(dot_pos) = name.rfind('.') {
                name = name[..dot_pos].to_string();
            }
            Some(name)
        } else {
            None
        }
    }

    fn deduplicate(changes: &mut Vec<RawChange>) {
        let mut seen = std::collections::HashSet::new();
        changes.retain(|rc| {
            let key = (
                rc.file_path.clone(),
                rc.line_start,
                rc.symbol_name.clone(),
                rc.change_type.clone(),
            );
            seen.insert(key)
        });
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 自由函数：各语言检测器
//
// 作为模块级函数而非 impl 方法，因为：
// 1. 它们不需要 self，逻辑上是纯函数
// 2. 避免闭包 Fn/FnMut 的复杂生命周期问题
// 3. 每个函数职责单一，易于单独测试
// ═══════════════════════════════════════════════════════════════════════

/// 构建 RawChange 的辅助函数
fn mk_rc(
    id_counter: &mut u32,
    lang: Language,
    change_type: ChangeType,
    file_path: &str,
    line_num: usize,
    symbol_name: String,
    sig: Option<String>,
    confidence: f32,
    module: Option<String>,
) -> RawChange {
    RawChange {
        id: AstAnalyzer::next_id(id_counter),
        source: ChangeSource::Ast,
        change_type,
        file_path: file_path.to_string(),
        language: lang,
        line_start: line_num,
        line_end: line_num,
        symbol_name,
        old_signature: None,
        new_signature: sig,
        confidence,
        module,
        tags: vec![],
    }
}

// ─── Rust 检测 ───

fn detect_rust(
    cache: &PatternCache,
    line: &str,
    file_path: &str,
    line_num: usize,
    is_addition: bool,
    id_counter: &mut u32,
    module: Option<String>,
) -> Option<RawChange> {
    let lang = Language::Rust;

    // pub fn name(...)
    if let Some(caps) = cache.rust_fn.captures(line) {
        let name = caps.get(2).unwrap().as_str().to_string();
        let sig = caps.get(0).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::FunctionAdded } else { ChangeType::FunctionRemoved },
            file_path, line_num, name, Some(sig), 1.0, module,
        ));
    }

    // pub struct Name
    if let Some(caps) = cache.rust_struct.captures(line) {
        let name = caps.get(1).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::StructAdded } else { ChangeType::UnknownChange },
            file_path, line_num, name, Some(line.to_string()),
            if is_addition { 1.0 } else { 0.7 }, module,
        ));
    }

    // pub enum Name
    if let Some(caps) = cache.rust_enum.captures(line) {
        let name = caps.get(1).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::EnumAdded } else { ChangeType::UnknownChange },
            file_path, line_num, name, Some(line.to_string()),
            if is_addition { 1.0 } else { 0.7 }, module,
        ));
    }

    // pub trait Name
    if let Some(caps) = cache.rust_trait.captures(line) {
        let name = caps.get(1).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::TraitAdded } else { ChangeType::UnknownChange },
            file_path, line_num, name, Some(line.to_string()),
            if is_addition { 1.0 } else { 0.7 }, module,
        ));
    }

    // use path::to::module;
    if let Some(caps) = cache.rust_use.captures(line) {
        let import_path = caps.get(1).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::ImportAdded } else { ChangeType::ImportRemoved },
            file_path, line_num, import_path, Some(line.to_string()), 0.95, module,
        ));
    }

    // pub const NAME: Type
    if let Some(caps) = cache.rust_const.captures(line) {
        let name = caps.get(1).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::ConstantAdded } else { ChangeType::ConstantRemoved },
            file_path, line_num, name, Some(line.to_string()), 0.9, module,
        ));
    }

    None
}

// ─── TypeScript 检测 ───

fn detect_typescript(
    cache: &PatternCache,
    line: &str,
    file_path: &str,
    line_num: usize,
    is_addition: bool,
    id_counter: &mut u32,
    module: Option<String>,
    lang: Language,
) -> Option<RawChange> {
    // export function name
    if let Some(caps) = cache.ts_export_function.captures(line) {
        let name = caps.get(1).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::FunctionAdded } else { ChangeType::FunctionRemoved },
            file_path, line_num, name, Some(line.to_string()), 0.95, module,
        ));
    }

    // (export)? class Name
    if let Some(caps) = cache.ts_class.captures(line) {
        let name = caps.get(1).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::StructAdded } else { ChangeType::UnknownChange },
            file_path, line_num, name, Some(line.to_string()),
            if is_addition { 1.0 } else { 0.7 }, module,
        ));
    }

    // import ... from '...'
    if let Some(caps) = cache.ts_import.captures(line) {
        let import_info = caps.get(0).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::ImportAdded } else { ChangeType::ImportRemoved },
            file_path, line_num, import_info, Some(line.to_string()), 0.95, module,
        ));
    }

    None
}

// ─── Python 检测 ───

fn detect_python(
    cache: &PatternCache,
    line: &str,
    file_path: &str,
    line_num: usize,
    is_addition: bool,
    id_counter: &mut u32,
    module: Option<String>,
) -> Option<RawChange> {
    let lang = Language::Python;

    // def name(
    if let Some(caps) = cache.py_function.captures(line) {
        let name = caps.get(1).unwrap().as_str().to_string();
        // 跳过私有函数（单个下划线前缀）
        if name.len() > 1 && name.starts_with('_') && !name.starts_with("__") {
            return None;
        }
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::FunctionAdded } else { ChangeType::FunctionRemoved },
            file_path, line_num, name, Some(line.to_string()), 0.95, module,
        ));
    }

    // class Name:
    if let Some(caps) = cache.py_class.captures(line) {
        let name = caps.get(1).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::StructAdded } else { ChangeType::UnknownChange },
            file_path, line_num, name, Some(line.to_string()),
            if is_addition { 1.0 } else { 0.7 }, module,
        ));
    }

    None
}

// ─── Go 检测 ───

fn detect_go(
    cache: &PatternCache,
    line: &str,
    file_path: &str,
    line_num: usize,
    is_addition: bool,
    id_counter: &mut u32,
    module: Option<String>,
) -> Option<RawChange> {
    let lang = Language::Go;

    // func Name(  -- 首字母大写 = 公开
    if let Some(caps) = cache.go_function.captures(line) {
        let name = caps.get(1).unwrap().as_str().to_string();
        if !name.chars().next().map_or(false, |c| c.is_uppercase()) {
            return None;
        }
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::FunctionAdded } else { ChangeType::FunctionRemoved },
            file_path, line_num, name, Some(line.to_string()), 0.95, module,
        ));
    }

    // type Name struct
    if let Some(caps) = cache.go_struct.captures(line) {
        let name = caps.get(1).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::StructAdded } else { ChangeType::UnknownChange },
            file_path, line_num, name, Some(line.to_string()),
            if is_addition { 1.0 } else { 0.7 }, module,
        ));
    }

    None
}

// ─── Java 检测 ───

fn detect_java(
    cache: &PatternCache,
    line: &str,
    file_path: &str,
    line_num: usize,
    is_addition: bool,
    id_counter: &mut u32,
    module: Option<String>,
) -> Option<RawChange> {
    let lang = Language::Java;

    // (public)? class Name
    if let Some(caps) = cache.java_class.captures(line) {
        let name = caps.get(1).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::StructAdded } else { ChangeType::UnknownChange },
            file_path, line_num, name, Some(line.to_string()),
            if is_addition { 1.0 } else { 0.7 }, module,
        ));
    }

    // public/private/protected Type name(
    if let Some(caps) = cache.java_method.captures(line) {
        let name = caps.get(1).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::FunctionAdded } else { ChangeType::FunctionRemoved },
            file_path, line_num, name, Some(line.to_string()), 0.9, module,
        ));
    }

    None
}

// ─── 通用检测 ───

fn detect_generic(
    cache: &PatternCache,
    line: &str,
    file_path: &str,
    line_num: usize,
    is_addition: bool,
    id_counter: &mut u32,
    module: Option<String>,
) -> Option<RawChange> {
    let lang = Language::Unknown;

    // HTTP 路由
    if let Some(caps) = cache.http_route.captures(line) {
        let route = caps.get(1).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::HttpRouteAdded } else { ChangeType::HttpRouteRemoved },
            file_path, line_num, route, Some(line.to_string()), 0.85, module,
        ));
    }

    // 配置项变更
    if let Some(caps) = cache.config_change.captures(line) {
        let key = caps.get(1).unwrap().as_str().to_string();
        return Some(mk_rc(id_counter, lang,
            if is_addition { ChangeType::ConstantAdded } else { ChangeType::ConstantChanged },
            file_path, line_num, key, Some(line.to_string()), 0.7, module,
        ));
    }

    None
}

// ═══════════════════════════════════════════════════════════════════════
// 单元测试
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::preprocessor::DiffPreprocessor;
    use crate::analysis::l2_rules::RuleEngine;

    // ── Rust 检测测试 ──

    #[test]
    fn test_detect_rust_pub_fn() {
        let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,0 +1,3 @@
+pub fn new_function(param: String) -> Result<()> {
+    Ok(())
+}"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        assert!(!raw.is_empty());
        let func = raw.iter().find(|r| r.symbol_name == "new_function").unwrap();
        assert_eq!(func.change_type, ChangeType::FunctionAdded);
        assert_eq!(func.confidence, 1.0);
        assert_eq!(func.source, ChangeSource::Ast);
        assert_eq!(func.language, Language::Rust);
    }

    #[test]
    fn test_detect_rust_pub_fn_removed() {
        let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,0 @@
-pub fn old_function() {
-    println!("gone");
-}"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        let func = raw.iter().find(|r| r.symbol_name == "old_function").unwrap();
        assert_eq!(func.change_type, ChangeType::FunctionRemoved);
    }

    #[test]
    fn test_detect_rust_struct() {
        let diff = r#"diff --git a/src/models.rs b/src/models.rs
--- a/src/models.rs
+++ b/src/models.rs
@@ -1,0 +1,3 @@
+pub struct User {
+    pub name: String,
+}"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        let strukt = raw.iter().find(|r| r.symbol_name == "User").unwrap();
        assert_eq!(strukt.change_type, ChangeType::StructAdded);
    }

    #[test]
    fn test_detect_rust_enum() {
        let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,0 +1,3 @@
+pub enum Status {
+    Active,
+}"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        let e = raw.iter().find(|r| r.symbol_name == "Status").unwrap();
        assert_eq!(e.change_type, ChangeType::EnumAdded);
    }

    #[test]
    fn test_detect_rust_trait() {
        let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,0 +1,2 @@
+pub trait Handler {
+}"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        let t = raw.iter().find(|r| r.symbol_name == "Handler").unwrap();
        assert_eq!(t.change_type, ChangeType::TraitAdded);
    }

    #[test]
    fn test_detect_rust_use_added() {
        let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,1 +1,2 @@
+use std::collections::HashMap;
"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        let u = raw.iter().find(|r| r.symbol_name.contains("HashMap")).unwrap();
        assert_eq!(u.change_type, ChangeType::ImportAdded);
    }

    #[test]
    fn test_detect_rust_const() {
        let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,1 +1,2 @@
+pub const MAX_RETRIES: u32 = 3;
"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        let c = raw.iter().find(|r| r.symbol_name == "MAX_RETRIES").unwrap();
        assert_eq!(c.change_type, ChangeType::ConstantAdded);
    }

    #[test]
    fn test_skip_private_rust_fn() {
        // 非 pub 函数不应被提取
        let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,1 +1,2 @@
+fn private_helper() {}
"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        assert!(raw.is_empty(), "私有函数不应被提取");
    }

    // ── TypeScript 检测测试 ──

    #[test]
    fn test_detect_ts_export_fn() {
        let diff = r#"diff --git a/src/auth.ts b/src/auth.ts
--- a/src/auth.ts
+++ b/src/auth.ts
@@ -1,0 +1,3 @@
+export function login(email: string, password: string): Promise<Token> {
+    return fetch('/api/login');
+}"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        let func = raw.iter().find(|r| r.symbol_name == "login").unwrap();
        assert_eq!(func.change_type, ChangeType::FunctionAdded);
        assert_eq!(func.language, Language::TypeScript);
    }

    #[test]
    fn test_detect_ts_class() {
        let diff = r#"diff --git a/src/models.ts b/src/models.ts
--- a/src/models.ts
+++ b/src/models.ts
@@ -1,0 +1,3 @@
+export class UserModel {
+    name: string;
+}"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        let cls = raw.iter().find(|r| r.symbol_name == "UserModel").unwrap();
        assert_eq!(cls.change_type, ChangeType::StructAdded);
    }

    #[test]
    fn test_detect_ts_import() {
        let diff = r#"diff --git a/src/app.ts b/src/app.ts
--- a/src/app.ts
+++ b/src/app.ts
@@ -1,1 +1,2 @@
+import { Router } from 'express';
"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        let imp = raw.iter().find(|r| r.symbol_name.contains("Router")).unwrap();
        assert_eq!(imp.change_type, ChangeType::ImportAdded);
    }

    // ── Python 检测测试 ──

    #[test]
    fn test_detect_python_fn() {
        let diff = r#"diff --git a/app/api.py b/app/api.py
--- a/app/api.py
+++ b/app/api.py
@@ -1,0 +1,2 @@
+def create_user(name: str, email: str) -> User:
+    pass"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        let func = raw.iter().find(|r| r.symbol_name == "create_user").unwrap();
        assert_eq!(func.change_type, ChangeType::FunctionAdded);
        assert_eq!(func.language, Language::Python);
    }

    #[test]
    fn test_detect_python_class() {
        let diff = r#"diff --git a/app/api.py b/app/api.py
--- a/app/api.py
+++ b/app/api.py
@@ -1,0 +1,3 @@
+class UserService:
+    def __init__(self):
+        pass"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        let cls = raw.iter().find(|r| r.symbol_name == "UserService").unwrap();
        assert_eq!(cls.change_type, ChangeType::StructAdded);
    }

    #[test]
    fn test_skip_private_python_fn() {
        let diff = r#"diff --git a/app/api.py b/app/api.py
--- a/app/api.py
+++ b/app/api.py
@@ -1,0 +1,2 @@
+def _helper():
+    pass"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        assert!(raw.is_empty(), "Python 私有函数不应被提取");
    }

    // ── Go 检测测试 ──

    #[test]
    fn test_detect_go_pub_fn() {
        let diff = r#"diff --git a/pkg/handler.go b/pkg/handler.go
--- a/pkg/handler.go
+++ b/pkg/handler.go
@@ -1,0 +1,2 @@
+func HandleRequest(w http.ResponseWriter, r *http.Request) {
+}"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        let f = raw.iter().find(|r| r.symbol_name == "HandleRequest").unwrap();
        assert_eq!(f.change_type, ChangeType::FunctionAdded);
        assert_eq!(f.language, Language::Go);
    }

    #[test]
    fn test_skip_private_go_fn() {
        let diff = r#"diff --git a/pkg/handler.go b/pkg/handler.go
--- a/pkg/handler.go
+++ b/pkg/handler.go
@@ -1,0 +1,2 @@
+func parseBody(r io.Reader) error {
+}"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        assert!(raw.is_empty(), "Go 私有函数不应被提取");
    }

    // ── 通用检测测试 ──

    #[test]
    fn test_detect_http_route() {
        let diff = r#"diff --git a/src/routes.ts b/src/routes.ts
--- a/src/routes.ts
+++ b/src/routes.ts
@@ -1,1 +1,2 @@
+app.get('/api/users', getUsers);
"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        let route = raw.iter().find(|r| r.change_type == ChangeType::HttpRouteAdded);
        assert!(route.is_some(), "HTTP 路由应被检测到");
        assert_eq!(route.unwrap().symbol_name, "/api/users");
    }

    // ── L1+L2 集成测试 ──

    #[test]
    fn test_l1_l2_integration_auth() {
        let diff = r#"diff --git a/src/auth.rs b/src/auth.rs
--- a/src/auth.rs
+++ b/src/auth.rs
@@ -1,0 +1,3 @@
+pub fn login(user: String, pass: String) -> bool {
+    true
+}"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        assert_eq!(raw.len(), 1);

        let engine = RuleEngine::new();
        let matches = engine.evaluate(&raw);
        assert!(
            matches.iter().any(|m| m.rule_id == "sec_auth_001"),
            "auth 相关函数应命中安全规则"
        );
    }

    #[test]
    fn test_l1_l2_integration_breaking() {
        let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,0 @@
-pub fn deprecated_api() -> String {
-    "old".to_string()
-}"#;

        let changes = DiffPreprocessor::parse(diff);
        let raw = AstAnalyzer::analyze(&changes).unwrap();
        assert_eq!(raw.len(), 1);

        let engine = RuleEngine::new();
        let matches = engine.evaluate(&raw);
        assert!(
            matches.iter().any(|m| m.rule_id == "breaking_001"),
            "公开函数删除应命中破坏性变更规则"
        );
    }
}
