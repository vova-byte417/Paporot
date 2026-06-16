//! `Paporot testmap`  — 行为-测试闭环
//!
//! ## 子命令
//! - `testmap scan <snapshot_version>`    — 从 diff 自动发现测试映射
//! - `testmap add <cap> <file> <fn>`     — 手动添加测试映射
//! - `testmap show [capability_id]`       — 查看映射
//! - `testmap stats`                      — 测试覆盖统计
//! - `testmap verify <capability_id>`     — 验证特定能力的测试是否存在

use anyhow::Result;
use crate::types::*;

/// 执行 testmap scan — 从文件命名规则和 git diff 自动推断测试映射
pub fn scan(testmap: &mut TestMapStore, diff: &str, _snapshot_version: &str) -> Result<()> {
    println!("Paporot Test Map Scanner");
    println!("========================\n");
    println!("  Scanning diff for test files...");

    // 从 diff 中检测测试文件 (仅从 --- a/+++ b/ 行提取)
    let mut found = 0u32;
    for line in diff.lines() {
        if let Some(test_file) = extract_test_file(line) {
            // 检查是否为测试文件
            let is_test = test_file.ends_with("_test.rs")
                || test_file.ends_with("_test.ts")
                || test_file.ends_with("_test.py")
                || test_file.ends_with("_test.go")
                || test_file.ends_with("Test.java")
                || test_file.ends_with("_test.java");
            if !is_test { continue; }
            // 推断对应的源文件
            if let Some((_source_file, capability_name)) = infer_source_from_test(&test_file) {
                // 检查是否已有映射
                let exists = testmap.mappings.iter().any(|m|
                    m.test_file == test_file && m.capability_id == capability_name
                );
                if !exists {
                    let mapping = TestMapping {
                        map_id: format!("tmap_{}", testmap.mappings.len() + 1),
                        capability_id: capability_name.clone(),
                        test_file: test_file.clone(),
                        test_name: format!("test_{}", capability_name),
                        framework: infer_framework(&test_file),
                        test_status: TestStatus::Unknown,
                        confidence: 0.7,
                        source: TestMappingSource::FileNameInferred,
                        last_run_at: None,
                    };
                    testmap.add_mapping(mapping);
                    found += 1;
                    println!("    + {} → {}", test_file, capability_name);
                }
            }
        }
    }

    println!("  Found {} new test mappings.", found);
    println!("  Total mappings: {}", testmap.mappings.len());
    Ok(())
}

/// 执行 testmap add — 手动添加映射
pub fn add(
    testmap: &mut TestMapStore,
    capability_id: &str,
    test_file: &str,
    test_name: &str,
    status: &str,
    framework: Option<&str>,
    source: &str,
) -> Result<()> {
    let test_status = match status.to_lowercase().as_str() {
        "pass" | "passing" => TestStatus::Passing,
        "fail" | "failing" => TestStatus::Failing,
        "missing" => TestStatus::Missing,
        _ => TestStatus::Unknown,
    };

    let ts = match source.to_lowercase().as_str() {
        "manual" => TestMappingSource::Manual,
        "name" => TestMappingSource::NameConventionInferred,
        "file" => TestMappingSource::FileNameInferred,
        _ => TestMappingSource::Manual,
    };

    let mapping = TestMapping {
        map_id: format!("tmap_{}", testmap.mappings.len() + 1),
        capability_id: capability_id.into(),
        test_file: test_file.into(),
        test_name: test_name.into(),
        framework: framework.map(String::from),
        test_status: test_status,
        confidence: 1.0,
        source: ts,
        last_run_at: None,
    };

    testmap.add_mapping(mapping);
    println!("  + test mapping: {} ← {}", capability_id, test_name);
    Ok(())
}

/// 执行 testmap show
pub fn show(testmap: &TestMapStore, capability_id: Option<&str>) -> Result<()> {
    println!("Paporot Test Mappings");
    println!("=====================\n");

    let mappings: Vec<&TestMapping> = if let Some(cid) = capability_id {
        testmap.mappings_for(cid)
    } else {
        testmap.mappings.iter().collect()
    };

    if mappings.is_empty() {
        println!("  No test mappings found.");
        println!("  Run 'Paporot testmap scan' or 'Paporot testmap add' to create mappings.");
    } else {
        // Group by capability id using owned data
        let mut grouped: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
        for (i, m) in mappings.iter().enumerate() {
            grouped.entry(m.capability_id.clone()).or_default().push(i);
        }

        for (cap_id, indices) in &grouped {
            println!("  {}:", cap_id);
            for &idx in indices {
                let m = &mappings[idx];
                let icon = match m.test_status {
                    TestStatus::Passing => "✓",
                    TestStatus::Failing => "✗",
                    TestStatus::Unknown => "?",
                    TestStatus::Missing => "!",
                };
                println!("    {} {}::{}  [{:?}]  conf: {:.2}",
                    icon, m.test_file, m.test_name,
                    m.test_status, m.confidence);
            }
            println!();
        }
    }
    Ok(())
}

/// 执行 testmap stats
pub fn stats(testmap: &TestMapStore) -> Result<()> {
    println!("Paporot Test Map Statistics");
    println!("===========================\n");
    println!("  Total capabilities : {}", testmap.stats.total_capabilities);
    println!("  Mapped capabilities: {}", testmap.stats.mapped_capabilities);
    println!("  Coverage           : {:.1}%",
        coverage_pct(testmap.stats.mapped_capabilities, testmap.stats.total_capabilities));
    println!("  ────────────────────");
    println!("  Passing            : {}", testmap.stats.passing_count);
    println!("  Failing            : {}", testmap.stats.failing_count);
    println!("  Missing            : {}", testmap.stats.missing_count);
    Ok(())
}

/// 执行 testmap verify — 验证测试文件是否存在
pub fn verify(testmap: &TestMapStore, capability_id: &str) -> Result<()> {
    println!("Paporot Test Verification");
    println!("========================\n");
    println!("  Capability: {}\n", capability_id);

    let mappings = testmap.mappings_for(capability_id);
    if mappings.is_empty() {
        println!("  No test mappings found for this capability.");
        return Ok(());
    }

    for m in mappings {
        let exists = std::path::Path::new(&m.test_file).exists();
        let icon = if exists { "✓" } else { "✗" };
        println!("    {} {}::{}  (file {})",
            icon, m.test_file, m.test_name,
            if exists { "exists" } else { "missing" });
    }

    Ok(())
}

// ─── Helpers ────────────────────────────────────────────────────────

fn extract_test_file(line: &str) -> Option<String> {
    // 仅匹配 diff 文件路径行: "--- a/path" 或 "+++ b/path"
    // 排除 diff 命令行 (以 "diff --git" 开头)
    if line.starts_with("diff ") {
        return None;
    }

    let clean = line
        .strip_prefix("--- a/")
        .or_else(|| line.strip_prefix("+++ b/"))?;

    if clean.ends_with(".rs") || clean.ends_with(".ts") || clean.ends_with(".py") ||
       clean.ends_with(".go") || clean.ends_with(".java") {
        Some(clean.to_string())
    } else {
        None
    }
}

fn infer_source_from_test(test_file: &str) -> Option<(String, String)> {
    // user_test.rs → (user.rs, user)
    // auth_login_test.rs → (auth_login.rs, auth_login)
    // UserTest.java → (User.java, User)
    let mut name = test_file
        .trim_end_matches(".rs")
        .trim_end_matches(".ts")
        .trim_end_matches(".py")
        .trim_end_matches(".go");

    // Java: handle .java separately since we also strip "Test" suffix
    let is_java = test_file.ends_with(".java");
    if is_java {
        name = test_file.trim_end_matches(".java");
    }

    // 去掉 Test 后缀 (Java)
    if let Some(stripped) = name.strip_suffix("Test") {
        let source_file = if is_java {
            format!("{}.java", stripped)
        } else {
            format!("{}.rs", stripped)
        };
        let cap_name = stripped.to_string();
        return Some((source_file, cap_name));
    }

    // 去掉 _test 后缀
    if let Some(stripped) = name.strip_suffix("_test") {
        let source_file = if is_java {
            format!("{}.java", stripped)
        } else {
            format!("{}.rs", stripped)
        };
        let cap_name = stripped.to_string();
        return Some((source_file, cap_name));
    }

    None
}

fn infer_framework(test_file: &str) -> Option<String> {
    if test_file.ends_with(".rs") { Some("cargo-test".into()) }
    else if test_file.ends_with(".ts") { Some("jest".into()) }
    else if test_file.ends_with(".py") { Some("pytest".into()) }
    else if test_file.ends_with(".go") { Some("go-test".into()) }
    else if test_file.ends_with(".java") { Some("junit".into()) }
    else { None }
}

fn coverage_pct(mapped: u32, total: u32) -> f32 {
    if total == 0 { 0.0 } else { (mapped as f32 / total as f32) * 100.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_testmap() -> TestMapStore {
        TestMapStore {
            mappings: vec![],
            stats: TestMapStats::default(),
        }
    }

    #[test]
    fn test_extract_test_file_from_diff_line() {
        // 仅匹配 --- a/ 或 +++ b/ 前缀
        assert_eq!(extract_test_file("+++ b/src/user_test.rs"), Some("src/user_test.rs".into()));
        assert_eq!(extract_test_file("--- a/src/user_test.rs"), Some("src/user_test.rs".into()));
        // diff 命令行应被跳过
        assert_eq!(extract_test_file("diff --git a/test.rs b/test.rs"), None);
        // 普通内容行应被跳过
        assert_eq!(extract_test_file("+fn test_something() {}"), None);
    }

    #[test]
    fn test_infer_source_from_test() {
        let (source, cap) = infer_source_from_test("src/user_test.rs").unwrap();
        assert_eq!(source, "src/user.rs");
        assert_eq!(cap, "src/user");

        // 含路径
        let (source2, cap2) = infer_source_from_test("lib/auth/login_test.py").unwrap();
        assert_eq!(source2, "lib/auth/login.rs");
        assert_eq!(cap2, "lib/auth/login");
    }

    #[test]
    fn test_infer_source_java_test() {
        let (source, cap) = infer_source_from_test("com/app/UserTest.java").unwrap();
        assert_eq!(source, "com/app/User.java");
        assert_eq!(cap, "com/app/User");
    }

    #[test]
    fn test_infer_framework() {
        assert_eq!(infer_framework("test.rs"), Some("cargo-test".into()));
        assert_eq!(infer_framework("test.ts"), Some("jest".into()));
        assert_eq!(infer_framework("test.py"), Some("pytest".into()));
        assert_eq!(infer_framework("test.go"), Some("go-test".into()));
    }

    #[test]
    fn test_add_mapping() {
        let mut tm = empty_testmap();
        add(&mut tm, "cap_auth", "src/auth_test.rs", "test_login", "pass", None, "manual").unwrap();
        assert_eq!(tm.mappings.len(), 1);
        assert_eq!(tm.stats.passing_count, 1);
    }

    #[test]
    fn test_mappings_for() {
        let mut tm = empty_testmap();
        add(&mut tm, "cap_A", "a_test.rs", "test_a", "pass", None, "manual").unwrap();
        add(&mut tm, "cap_B", "b_test.rs", "test_b", "fail", None, "name").unwrap();
        add(&mut tm, "cap_A", "a2_test.rs", "test_a2", "pass", None, "manual").unwrap();

        let a_maps = tm.mappings_for("cap_A");
        assert_eq!(a_maps.len(), 2);
        let b_maps = tm.mappings_for("cap_B");
        assert_eq!(b_maps.len(), 1);
    }

    #[test]
    fn test_stats_after_multiple_adds() {
        let mut tm = empty_testmap();
        add(&mut tm, "c1", "t1.rs", "t1", "pass", None, "manual").unwrap();
        add(&mut tm, "c2", "t2.rs", "t2", "pass", None, "manual").unwrap();
        add(&mut tm, "c3", "t3.rs", "t3", "fail", None, "manual").unwrap();
        add(&mut tm, "c4", "t4.rs", "t4", "missing", None, "manual").unwrap();

        assert_eq!(tm.stats.mapped_capabilities, 4);
        assert_eq!(tm.stats.passing_count, 2);
        assert_eq!(tm.stats.failing_count, 1);
        assert_eq!(tm.stats.missing_count, 1);
    }

    #[test]
    fn test_testmap_persistence_roundtrip() {
        let dir = std::env::temp_dir().join("Paporot_test_testmap");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("testmap.json");

        let mut tm = empty_testmap();
        add(&mut tm, "cap_001", "src/login_test.rs", "test_login", "pass", Some("cargo-test"), "manual").unwrap();
        tm.save(&path).unwrap();

        let loaded = TestMapStore::load_or_new(&path).unwrap();
        assert_eq!(loaded.mappings.len(), 1);
        assert_eq!(loaded.mappings[0].test_name, "test_login");
        assert_eq!(loaded.mappings[0].test_status, TestStatus::Passing);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_scan_from_diff() {
        let diff = r#"diff --git a/src/login_test.rs b/src/login_test.rs
new file mode 100644
--- /dev/null
+++ b/src/login_test.rs
@@ -0,0 +1,5 @@
+fn test_login_success() {}
"#;

        let mut tm = empty_testmap();
        scan(&mut tm, diff, "v1").unwrap();
        assert_eq!(tm.mappings.len(), 1);
        assert_eq!(tm.mappings[0].capability_id, "src/login");
        assert_eq!(tm.mappings[0].source, TestMappingSource::FileNameInferred);
    }
}
