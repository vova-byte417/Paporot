//! DiffPreprocessor: 将 git diff 原始文本解析为结构化 FileChange
//!
//! 对应 PRD P0 §3.1

use regex::Regex;
use super::types::*;

/// Diff 预处理器
pub struct DiffPreprocessor;

impl DiffPreprocessor {
    /// 解析 unified diff 文本
    pub fn parse(diff_text: &str) -> Vec<FileChange> {
        let mut changes = Vec::new();
        let files = Self::split_by_file(diff_text);

        for file_block in files {
            if let Some(change) = Self::parse_file_block(&file_block) {
                changes.push(change);
            }
        }

        changes
    }

    /// 生成 diff 摘要
    pub fn summarize(changes: &[FileChange]) -> DiffSummary {
        let mut additions = 0usize;
        let mut deletions = 0usize;
        let mut lang_map: std::collections::HashMap<Language, usize> =
            std::collections::HashMap::new();

        for change in changes {
            *lang_map.entry(change.language).or_insert(0) += 1;
            for hunk in &change.hunks {
                for line in &hunk.lines {
                    match line {
                        DiffLine::Addition(_) => additions += 1,
                        DiffLine::Deletion(_) => deletions += 1,
                        _ => {}
                    }
                }
            }
        }

        let mut by_language: Vec<_> = lang_map.into_iter().collect();
        by_language.sort_by_key(|(_, count)| -(count.clone() as isize));

        DiffSummary {
            files_changed: changes.len(),
            additions,
            deletions,
            by_language,
        }
    }

    /// 按文件边界拆分 diff
    fn split_by_file(diff_text: &str) -> Vec<String> {
        let mut files = Vec::new();
        let mut current_start = None;

        for (i, _) in diff_text.match_indices("diff --git ") {
            if let Some(start) = current_start {
                files.push(diff_text[start..i].to_string());
            }
            current_start = Some(i);
        }

        // 最后一个文件
        if let Some(start) = current_start {
            files.push(diff_text[start..].to_string());
        }

        files
    }

    /// 解析单个文件的 diff 块
    fn parse_file_block(block: &str) -> Option<FileChange> {
        let lines: Vec<&str> = block.lines().collect();
        if lines.is_empty() {
            return None;
        }

        // 解析 diff header: diff --git a/path b/path
        let path = Self::parse_file_path(lines.first()?)?;
        let language = Language::from_filename(&path);

        // 解析文件变更类型
        let kind = Self::detect_change_kind(&lines);

        // 解析 hunks
        let hunks = Self::parse_hunks(&lines);

        Some(FileChange {
            path,
            language,
            kind,
            hunks,
            old_content: None,
            new_content: None,
        })
    }

    /// 从 "diff --git a/xxx b/xxx" 提取文件路径
    fn parse_file_path(header: &str) -> Option<String> {
        let re = Regex::new(r"^diff --git a/(\S+) b/(\S+)").unwrap();
        re.captures(header)
            .map(|caps| caps[2].to_string())
    }

    /// 检测文件变更类型：新增/删除/修改/重命名
    fn detect_change_kind(lines: &[&str]) -> ChangeKind {
        for line in lines {
            if line.starts_with("new file mode") {
                return ChangeKind::Added;
            }
            if line.starts_with("deleted file mode") {
                return ChangeKind::Deleted;
            }
            if line.starts_with("rename from ") {
                let from = line.strip_prefix("rename from ").unwrap_or("").to_string();
                let to = lines
                    .iter()
                    .find(|l| l.starts_with("rename to "))
                    .map(|l| l.strip_prefix("rename to ").unwrap_or("").to_string())
                    .unwrap_or_default();
                return ChangeKind::Renamed { from, to };
            }
        }
        ChangeKind::Modified
    }

    /// 解析 diff hunks
    fn parse_hunks(lines: &[&str]) -> Vec<Hunk> {
        let mut hunks = Vec::new();
        let hunk_header_re =
            Regex::new(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@").unwrap();

        let mut i = 0;
        while i < lines.len() {
            if let Some(caps) = hunk_header_re.captures(lines[i]) {
                let old_start: usize = caps[1].parse().unwrap_or(0);
                let old_count: usize = caps.get(2).map(|m| m.as_str().parse().unwrap_or(1)).unwrap_or(1);
                let new_start: usize = caps[3].parse().unwrap_or(0);
                let new_count: usize = caps.get(4).map(|m| m.as_str().parse().unwrap_or(1)).unwrap_or(1);
                let header = lines[i].to_string();

                let mut hunk_lines = Vec::new();
                i += 1;
                while i < lines.len() && !hunk_header_re.is_match(lines[i]) {
                    let line = lines[i];
                    if let Some(content) = line.strip_prefix('+') {
                        hunk_lines.push(DiffLine::Addition(content.to_string()));
                    } else if let Some(content) = line.strip_prefix('-') {
                        hunk_lines.push(DiffLine::Deletion(content.to_string()));
                    } else if let Some(content) = line.strip_prefix(' ') {
                        hunk_lines.push(DiffLine::Context(content.to_string()));
                    }
                    // 跳过空行和其他行（如 "\\ No newline at end of file"）
                    i += 1;
                }

                hunks.push(Hunk {
                    old_start,
                    old_count,
                    new_start,
                    new_count,
                    header,
                    lines: hunk_lines,
                });
            } else {
                i += 1;
            }
        }

        hunks
    }

    /// 获取所有变更的行内容（用于后续分析）
    pub fn get_added_lines(changes: &[FileChange]) -> Vec<(String, String)> {
        // (文件路径, 新增行内容)
        let mut result = Vec::new();
        for change in changes {
            for hunk in &change.hunks {
                for line in &hunk.lines {
                    if let DiffLine::Addition(content) = line {
                        result.push((change.path.clone(), content.clone()));
                    }
                }
            }
        }
        result
    }

    /// 获取所有删除的行内容
    pub fn get_deleted_lines(changes: &[FileChange]) -> Vec<(String, String)> {
        let mut result = Vec::new();
        for change in changes {
            for hunk in &change.hunks {
                for line in &hunk.lines {
                    if let DiffLine::Deletion(content) = line {
                        result.push((change.path.clone(), content.clone()));
                    }
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_diff() {
        let diff = r#"diff --git a/src/main.rs b/src/main.rs
index abc..def 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
-    println!("hello");
+    println!("hello, world");
+    greet();
 }"#;

        let changes = DiffPreprocessor::parse(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "src/main.rs");
        assert_eq!(changes[0].language, Language::Rust);
        assert_eq!(changes[0].kind, ChangeKind::Modified);
        assert_eq!(changes[0].hunks.len(), 1);
        assert_eq!(changes[0].hunks[0].lines.len(), 5);
    }

    #[test]
    fn test_parse_multiple_files() {
        let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
new file mode 100644
--- /dev/null
+++ b/src/lib.rs
@@ -0,0 +1,2 @@
+pub fn hello() {}
+pub fn world() {}
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,1 +1,1 @@
-fn old() {}
+fn new() {}"#;

        let changes = DiffPreprocessor::parse(diff);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].kind, ChangeKind::Added);
        assert_eq!(changes[1].kind, ChangeKind::Modified);
    }

    #[test]
    fn test_summarize() {
        let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -0,0 +1,3 @@
+line1
+line2
+line3
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,1 +1,1 @@
-old
+new"#;

        let changes = DiffPreprocessor::parse(diff);
        let summary = DiffPreprocessor::summarize(&changes);
        assert_eq!(summary.files_changed, 2);
        assert_eq!(summary.additions, 4);
        assert_eq!(summary.deletions, 1);
    }
}
