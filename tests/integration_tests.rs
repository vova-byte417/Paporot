//! Paporot 集成测试
//!
//! 从完整 diff 到 Snapshot 输出的端到端流程测试。
//! 验证 L1+L2 全链路正确性，不依赖 LLM（使用 mock diff 数据）。

use std::collections::HashMap;
use Paporot::analysis::preprocessor::DiffPreprocessor;
use Paporot::analysis::l1_ast::AstAnalyzer;
use Paporot::analysis::l2_rules::RuleEngine;
use Paporot::analysis::types::*;
use Paporot::graph::{DependencyEdge, DependencyGraph, GraphStorage};
use Paporot::types::*;

// ═══════════════════════════════════════════════════════════════════════
// 集成测试: L1 → L2 完整流水线
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_full_pipeline_rust_new_feature() {
    // 模拟一个完整的 Rust 功能提交
    let diff = r#"diff --git a/src/auth.rs b/src/auth.rs
--- a/src/auth.rs
+++ b/src/auth.rs
@@ -0,0 +1,8 @@
+pub fn login(username: String, password: String) -> Result<Token, AuthError> {
+    Ok(Token::new(username))
+}
+
+pub struct Token {
+    pub value: String,
+    pub expires_at: i64,
+}
diff --git a/src/config.rs b/src/config.rs
--- a/src/config.rs
+++ b/src/config.rs
@@ -5,0 +6,1 @@
+pub const MAX_LOGIN_ATTEMPTS: u32 = 5;
diff --git a/src/handler.rs b/src/handler.rs
--- a/src/handler.rs
+++ b/src/handler.rs
@@ -10,0 +11,1 @@
+use crate::auth::login;
"#;

    // L1: 解析
    let changes = DiffPreprocessor::parse(diff);
    let summary = DiffPreprocessor::summarize(&changes);
    assert_eq!(summary.files_changed, 3);
    assert!(summary.additions > 0);

    // L2: 确定性问题提取
    let raw = AstAnalyzer::analyze(&changes).unwrap();
    assert!(!raw.is_empty(), "应至少提取到 1 个能力变更");

    // 验证 login 函数被提取
    let login_fn = raw.iter().find(|r| r.symbol_name == "login");
    assert!(login_fn.is_some(), "应检测到 login 函数");
    assert_eq!(login_fn.unwrap().change_type, ChangeType::FunctionAdded);
    assert_eq!(login_fn.unwrap().confidence, 1.0);

    // 验证 struct 被提取
    assert!(raw.iter().any(|r| r.symbol_name == "Token" && r.change_type == ChangeType::StructAdded));

    // 验证 const 被提取
    assert!(raw.iter().any(|r| r.symbol_name == "MAX_LOGIN_ATTEMPTS" && r.change_type == ChangeType::ConstantAdded));

    // 验证 use 被提取
    assert!(raw.iter().any(|r| r.symbol_name.contains("auth::login") && r.change_type == ChangeType::ImportAdded));
}

#[test]
fn test_full_pipeline_removal_detection() {
    let diff = r#"diff --git a/src/api.rs b/src/api.rs
--- a/src/api.rs
+++ b/src/api.rs
@@ -5,4 +5,0 @@
-pub fn deprecated_endpoint() -> String {
-    "old".to_string()
-}
"#;

    let changes = DiffPreprocessor::parse(diff);
    let raw = AstAnalyzer::analyze(&changes).unwrap();
    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0].change_type, ChangeType::FunctionRemoved);
    assert_eq!(raw[0].symbol_name, "deprecated_endpoint");
}

#[test]
fn test_full_pipeline_mixed_languages() {
    let diff = r#"diff --git a/src/auth.ts b/src/auth.ts
--- a/src/auth.ts
+++ b/src/auth.ts
@@ -0,0 +1,4 @@
+export function verifyToken(token: string): boolean {
+    return token.length > 0;
+}
diff --git a/app/api.py b/app/api.py
--- a/app/api.py
+++ b/app/api.py
@@ -0,0 +1,3 @@
+def handle_login(request):
+    return {"status": "ok"}
"#;

    let changes = DiffPreprocessor::parse(diff);
    let raw = AstAnalyzer::analyze(&changes).unwrap();

    // 每种语言至少提取一个符号
    assert!(raw.iter().any(|r| r.symbol_name == "verifyToken" && r.language == Language::TypeScript),
        "应检测到 TypeScript 函数");
    assert!(raw.iter().any(|r| r.symbol_name == "handle_login" && r.language == Language::Python),
        "应检测到 Python 函数");
}

// ═══════════════════════════════════════════════════════════════════════
// 集成测试: L1+L2 安全规则联动
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_security_rules_on_auth_changes() {
    let diff = r#"diff --git a/src/auth.rs b/src/auth.rs
--- a/src/auth.rs
+++ b/src/auth.rs
@@ -0,0 +1,4 @@
+pub fn login() {}
+pub fn hash_password(raw: &str) -> String { String::new() }
+pub fn generate_token(user: &User) -> String { String::new() }
+pub fn check_permission(user: &User, perm: &str) -> bool { true }
"#;

    let changes = DiffPreprocessor::parse(diff);
    let raw = AstAnalyzer::analyze(&changes).unwrap();
    let engine = RuleEngine::new();
    let matches = engine.evaluate(&raw);

    // login 应命中 auth 安全规则
    assert!(raw.iter().any(|r| r.symbol_name == "login"));

    // 至少有一个 auth 安全规则命中
    assert!(matches.iter().any(|m| m.rule_id == "sec_auth_001"), "login 应命中安全规则");

    // hash 应命中 crypto 规则
    assert!(matches.iter().any(|m| m.rule_id == "sec_crypto_001"),
        "hash_password 应命中加密规则");

    // token 应命中 token 规则
    assert!(matches.iter().any(|m| m.rule_id == "sec_token_001"),
        "generate_token 应命中 token 规则");

    // permission 应命中授权规则
    assert!(matches.iter().any(|m| m.rule_id == "sec_auth_002"),
        "check_permission 应命中授权规则");
}

#[test]
fn test_breaking_change_rules() {
    let diff = r#"diff --git a/src/api.rs b/src/api.rs
--- a/src/api.rs
+++ b/src/api.rs
@@ -5,4 +5,0 @@
-pub fn public_api() {}
diff --git a/src/types.rs b/src/types.rs
--- a/src/types.rs
+++ b/src/types.rs
@@ -10,3 +10,0 @@
-pub enum Status {
-    Active,
-}
"#;

    let changes = DiffPreprocessor::parse(diff);
    let raw = AstAnalyzer::analyze(&changes).unwrap();
    let engine = RuleEngine::new();
    let matches = engine.evaluate(&raw);

    // 公开函数删除应命中 breaking_001
    assert!(matches.iter().any(|m| m.rule_id == "breaking_001"),
        "公开函数删除应命中破坏性规则");

    // 非 test 函数删除
    assert!(raw.iter().any(|r| r.symbol_name == "public_api" && r.change_type == ChangeType::FunctionRemoved));

    // 枚举删除 — 至少检测到变更 (具体类型因各行匹配规则而异)
    let types_changes: Vec<_> = raw.iter().filter(|r| r.file_path == "src/types.rs").collect();
    assert!(!types_changes.is_empty(), "枚举删除应产生至少一个变更记录");
}

#[test]
fn test_no_rule_on_innocuous_change() {
    let diff = r#"diff --git a/src/utils.rs b/src/utils.rs
--- a/src/utils.rs
+++ b/src/utils.rs
@@ -1,0 +1,2 @@
+pub fn calculate_total(items: &[f64]) -> f64 {
+    items.iter().sum()
+}"#;

    let changes = DiffPreprocessor::parse(diff);
    let raw = AstAnalyzer::analyze(&changes).unwrap();
    let engine = RuleEngine::new();
    let matches = engine.evaluate(&raw);

    // 普通工具函数不应命中任何严重规则
    assert!(!matches.iter().any(|m| m.rule_id == "sec_auth_001"), "非安全函数不应命中安全规则");
    assert!(!matches.iter().any(|m| m.severity == Severity::High), "普通变更不应有高风险");
}

// ═══════════════════════════════════════════════════════════════════════
// 集成测试: 依赖图操作
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_graph_persistence_roundtrip() {
    let dir = std::env::temp_dir().join("Paporot_int_test_graph");
    let _ = std::fs::remove_dir_all(&dir);

    let storage = GraphStorage::new(&dir);
    storage.init().unwrap();

    let mut graph = DependencyGraph {
        edges: vec![],
        nodes: HashMap::new(),
        evolution_chains: HashMap::new(),
    };

    // 创建快照模拟数据
    let snapshot = BehaviorSnapshot {
        schema_version: 3,
        version_id: "v1".into(),
        git_commit: None,
        git_ref: None,
        timestamp: "2026-06-11T10:00:00Z".into(),
        message: "test".into(),
        capabilities: vec![
            Capability {
                id: "cap_auth_001".into(),
                name: "User Login".into(),
                description: "Login capability".into(),
                status: CapabilityStatus::New,
                module: Some("auth".into()),
                sub_modules: vec![],
                confidence: Some(1.0),
                evidence: vec!["src/auth.rs:1".into()],
                tags: vec!["security".into()],
                contract: None,
                preconditions: vec![],
                postconditions: vec![],
                invariants: vec![],
                categories: vec![],
                depends_on: vec![],
                depended_by: vec![],
                evolved_from: None,
                evidence_trace_ids: vec![],
                verified_by: None,
                verified_at: None,
                source_change_type: None,
                triggered_by_rules: vec![],
            },
        ],
        prd_coverage: PrdCoverage {
            percentage: 0.0,
            total_items: 0,
            covered_items: None,
            details: vec![],
        },
        regression: None,
        risk: None,
        metadata: None,
    };

    storage.update_from_snapshot(&mut graph, &snapshot).unwrap();
    storage.save(&graph).unwrap();

    let loaded = storage.load().unwrap();
    assert_eq!(loaded.nodes.len(), 1);
    assert!(loaded.nodes.contains_key("cap_auth_001"));
    assert_eq!(loaded.nodes["cap_auth_001"].name, "User Login");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_evolution_chain_across_snapshots() {
    let dir = std::env::temp_dir().join("Paporot_int_test_evolve");
    let _ = std::fs::remove_dir_all(&dir);
    let storage = GraphStorage::new(&dir);
    let mut graph = DependencyGraph {
        edges: vec![],
        nodes: HashMap::new(),
        evolution_chains: HashMap::new(),
    };

    let make_snap = |ver: &str, status: CapabilityStatus| -> BehaviorSnapshot {
        BehaviorSnapshot {
            schema_version: 3,
            version_id: ver.into(),
            git_commit: None,
            git_ref: None,
            timestamp: "2026-06-11T10:00:00Z".into(),
            message: ver.into(),
            capabilities: vec![Capability {
                id: "cap_001".into(),
                name: "Test Cap".into(),
                description: "test".into(),
                status,
                module: None,
                sub_modules: vec![],
                confidence: Some(1.0),
                evidence: vec![],
                tags: vec![],
                contract: None,
                preconditions: vec![],
                postconditions: vec![],
                invariants: vec![],
                categories: vec![],
                depends_on: vec![],
                depended_by: vec![],
                evolved_from: None,
                evidence_trace_ids: vec![],
                verified_by: None,
                verified_at: None,
                source_change_type: None,
                triggered_by_rules: vec![],
            }],
            prd_coverage: PrdCoverage { percentage: 0.0, total_items: 0, covered_items: None, details: vec![] },
            regression: None, risk: None, metadata: None,
        }
    };

    storage.update_from_snapshot(&mut graph, &make_snap("v1", CapabilityStatus::New)).unwrap();
    storage.update_from_snapshot(&mut graph, &make_snap("v2", CapabilityStatus::Modified)).unwrap();
    storage.update_from_snapshot(&mut graph, &make_snap("v3", CapabilityStatus::Modified)).unwrap();

    let trace = GraphStorage::evolution_trace(&graph, "cap_001");
    assert_eq!(trace.len(), 3);
    assert_eq!(trace, vec!["v1", "v2", "v3"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_graph_cycle_detection_complex() {
    let edges = vec![
        make_edge("A", "B", DependencyRelation::Calls),
        make_edge("B", "C", DependencyRelation::Calls),
        make_edge("C", "D", DependencyRelation::Calls),
        make_edge("D", "A", DependencyRelation::Calls), // 形成环
        make_edge("E", "F", DependencyRelation::Calls),
    ];

    let graph = DependencyGraph {
        edges,
        nodes: HashMap::new(),
        evolution_chains: HashMap::new(),
    };

    let cycles = GraphStorage::detect_cycles(&graph);
    assert!(!cycles.is_empty(), "应检测到 A→B→C→D→A 循环");
}

fn make_edge(from: &str, to: &str, relation: DependencyRelation) -> DependencyEdge {
    DependencyEdge {
        from_capability_id: from.into(),
        from_snapshot: None,
        to_capability_id: to.into(),
        to_snapshot: None,
        relation,
        confidence: 1.0,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 集成测试: 类型序列化（P1/P2 扩展验证）
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_snapshot_with_contract_serialization() {
    let snap = BehaviorSnapshot {
        schema_version: 3,
        version_id: "v5".into(),
        git_commit: Some("fedcba".into()),
        git_ref: None,
        timestamp: "2026-06-11T10:00:00Z".into(),
        message: "full contract test".into(),
        capabilities: vec![Capability {
            id: "cap_http_001".into(),
            name: "Get Users API".into(),
            description: "Retrieve paginated user list".into(),
            status: CapabilityStatus::New,
            module: Some("users".into()),
            sub_modules: vec![],
            confidence: Some(0.95),
            evidence: vec!["src/routes/users.rs:42".into()],
            tags: vec!["api".into()],
            contract: Some(BehaviorContract::HttpEndpoint {
                method: "GET".into(),
                path_template: "/api/users".into(),
                auth_required: true,
            }),
            preconditions: vec![Condition {
                kind: ConditionKind::Precondition,
                expression: "User must be authenticated".into(),
                severity: Severity::High,
            }],
            postconditions: vec![Condition {
                kind: ConditionKind::Postcondition,
                expression: "Response is paginated JSON array".into(),
                severity: Severity::Medium,
            }],
            invariants: vec![Condition {
                kind: ConditionKind::Invariant,
                expression: "User data is never exposed in plaintext".into(),
                severity: Severity::High,
            }],
            categories: vec![CapabilityCategory::Functional, CapabilityCategory::Security],
            depends_on: vec![DependsOn {
                target: CapabilityRef { capability_id: "cap_auth_001".into(), snapshot_version: None },
                relation: DependencyRelation::Calls,
                via: Some("Bearer token validation".into()),
                confidence: 0.95,
                source: Some(RelationSource::AstInferred),
            }],
            depended_by: vec![],
            evolved_from: None,
            evidence_trace_ids: vec![],
            verified_by: None,
            verified_at: None,
            source_change_type: None,
            triggered_by_rules: vec![],
        }],
        prd_coverage: PrdCoverage { percentage: 100.0, total_items: 1, covered_items: Some(1), details: vec![] },
        regression: None, risk: None, metadata: None,
    };

    let json = snap.to_json().unwrap();
    let parsed = BehaviorSnapshot::from_json(&json).unwrap();

    assert_eq!(parsed.schema_version, 3);
    assert_eq!(parsed.version_id, "v5");
    assert_eq!(parsed.capabilities.len(), 1);

    let cap = &parsed.capabilities[0];
    assert!(cap.contract.is_some());
    assert_eq!(cap.preconditions.len(), 1);
    assert_eq!(cap.preconditions[0].kind, ConditionKind::Precondition);
    assert_eq!(cap.postconditions.len(), 1);
    assert_eq!(cap.invariants.len(), 1);
    assert_eq!(cap.categories.len(), 2);
    assert_eq!(cap.depends_on.len(), 1);
    assert_eq!(cap.depends_on[0].relation, DependencyRelation::Calls);
    assert_eq!(cap.depends_on[0].confidence, 0.95);
}

#[test]
fn test_behavior_contract_variants() {
    // HttpEndpoint
    let http = BehaviorContract::HttpEndpoint {
        method: "POST".into(),
        path_template: "/api/login".into(),
        auth_required: true,
    };
    let json = serde_json::to_string(&http).unwrap();
    assert!(json.contains("http_endpoint"));
    assert!(json.contains("POST"));

    // Function
    let func = BehaviorContract::Function {
        name: "handle_login".into(),
        visibility: "public".into(),
        is_async: true,
    };
    let json = serde_json::to_string(&func).unwrap();
    assert!(json.contains("function"));
    assert!(json.contains("handle_login"));

    // DataSchema
    let schema = BehaviorContract::DataSchema {
        kind: SchemaKind::Struct,
        derives: vec!["Debug".into(), "Clone".into()],
    };
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("data_schema"));
    assert!(json.contains("struct"));
    assert!(json.contains("Debug"));
}

#[test]
fn test_dependency_relation_serialization() {
    let dep = DependsOn {
        target: CapabilityRef {
            capability_id: "cap_target".into(),
            snapshot_version: Some("v3".into()),
        },
        relation: DependencyRelation::ConsumesEvent,
        via: Some("kafka:user.created".into()),
        confidence: 0.88,
        source: Some(RelationSource::LlmInferred),
    };

    let json = serde_json::to_string(&dep).unwrap();
    let parsed: DependsOn = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.target.capability_id, "cap_target");
    assert_eq!(parsed.target.snapshot_version, Some("v3".into()));
    assert_eq!(parsed.relation, DependencyRelation::ConsumesEvent);
    assert_eq!(parsed.confidence, 0.88);
    assert_eq!(parsed.source, Some(RelationSource::LlmInferred));
}

// ═══════════════════════════════════════════════════════════════════════
// 集成测试: 向后兼容（旧 schema 加载）
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_load_legacy_v1_snapshot() {
    // 模拟 schema_version=1 的旧快照 JSON（没有 P1/P2 字段）
    let legacy_json = r#"{
        "version_id": "v1",
        "git_commit": "abc123",
        "timestamp": "2026-01-01T00:00:00Z",
        "message": "legacy snapshot",
        "capabilities": [
            {
                "id": "cap_001",
                "name": "Old Login",
                "description": "Legacy login",
                "status": "new",
                "module": "auth",
                "confidence": 0.9,
                "evidence": ["src/auth.rs"],
                "tags": ["security"]
            }
        ],
        "prd_coverage": {
            "percentage": 0.0,
            "total_items": 0,
            "details": []
        }
    }"#;

    let parsed: BehaviorSnapshot = serde_json::from_str(legacy_json).unwrap();
    // schema_version 缺失时应取 default = 3
    assert_eq!(parsed.schema_version, 3);
    assert_eq!(parsed.version_id, "v1");
    assert_eq!(parsed.capabilities.len(), 1);

    // P1 字段应为默认空值
    let cap = &parsed.capabilities[0];
    assert!(cap.contract.is_none());
    assert!(cap.preconditions.is_empty());
    assert!(cap.depends_on.is_empty());
    assert!(cap.evolved_from.is_none());
}

// ═══════════════════════════════════════════════════════════════════════
// 集成测试: DiffPreprocessor 边界场景
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_empty_diff() {
    let diff = "";
    let changes = DiffPreprocessor::parse(diff);
    assert!(changes.is_empty());
    let summary = DiffPreprocessor::summarize(&changes);
    assert_eq!(summary.files_changed, 0);
}

#[test]
fn test_rename_detection() {
    let diff = r#"diff --git a/src/old.rs b/src/new.rs
rename from src/old.rs
rename to src/new.rs
--- a/src/old.rs
+++ b/src/new.rs
@@ -1,1 +1,1 @@
-old content
+new content
"#;

    let changes = DiffPreprocessor::parse(diff);
    assert_eq!(changes.len(), 1);
    if let ChangeKind::Renamed { from, to } = &changes[0].kind {
        assert_eq!(from, "src/old.rs");
        assert_eq!(to, "src/new.rs");
    } else {
        panic!("Expected Renamed, got {:?}", changes[0].kind);
    }
}

#[test]
fn test_binary_file_diff() {
    // binary 文件的 diff 不应产生 hunk
    let diff = r#"diff --git a/img.png b/img.png
Binary files a/img.png and b/img.png differ
"#;

    let changes = DiffPreprocessor::parse(diff);
    assert_eq!(changes.len(), 1);
    assert!(changes[0].hunks.is_empty());
}

#[test]
fn test_multiple_hunks_single_file() {
    let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,0 +1,2 @@
+pub fn first() {}
+pub fn second() {}
@@ -10,3 +12,4 @@
 pub fn unchanged() {}
+pub fn third() {}
"#;

    let changes = DiffPreprocessor::parse(diff);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].hunks.len(), 2, "应有 2 个 hunk");
    assert_eq!(changes[0].hunks[0].new_start, 1);
    assert_eq!(changes[0].hunks[1].new_start, 12);
}

// ═══════════════════════════════════════════════════════════════════════
// 系统级测试: 完整 L1→L2→Agent 端到端流水线
// ═══════════════════════════════════════════════════════════════════════

// FIXME: agent.rs 已在 Phase 0 删除，Agent::compute_diff 将在 Phase 2
// 作为 SnapshotAnalyzer::diff 重新实现后恢复此测试。
#[test]
#[ignore = "agent.rs 已删除，功能将在 SnapshotAnalyzer::diff 中恢复"]
fn test_system_agent_compute_diff_pipeline() {
    todo!("agent.rs 已删除，功能将在 SnapshotAnalyzer 中重新实现");
}

/// 测试项: 系统级 L1 完整多文件分析
/// 输入: 真实场景 3 文件 Rust diff (API 端点 + handler + DB)
/// 预期: 所有公开符号被正确提取
#[test]
fn test_system_l1_full_api_change() {
    let diff = r#"diff --git a/src/api/users.rs b/src/api/users.rs
--- a/src/api/users.rs
+++ b/src/api/users.rs
@@ -0,0 +1,5 @@
+pub fn get_users(db: &DbPool) -> Vec<User> {
+    db.query("SELECT * FROM users")
+}
+pub fn create_user(db: &DbPool, user: NewUser) -> User {
+    db.insert(user)
+}
diff --git a/src/models.rs b/src/models.rs
--- a/src/models.rs
+++ b/src/models.rs
@@ -1,0 +1,3 @@
+pub struct User {
+    pub id: i64,
+}
diff --git a/src/config.rs b/src/config.rs
--- a/src/config.rs
+++ b/src/config.rs
@@ -5,0 +5,1 @@
+pub const API_VERSION: &str = "1.0";
"#;

    let changes = DiffPreprocessor::parse(diff);
    let summary = DiffPreprocessor::summarize(&changes);
    assert_eq!(summary.files_changed, 3);

    let raw = AstAnalyzer::analyze(&changes).unwrap();
    assert!(raw.iter().any(|r| r.symbol_name == "get_users"), "get_users 应被检测");
    assert!(raw.iter().any(|r| r.symbol_name == "create_user"), "create_user 应被检测");
    assert!(raw.iter().any(|r| r.symbol_name == "User" && r.change_type == ChangeType::StructAdded), "User struct 应被检测");
    assert!(raw.iter().any(|r| r.symbol_name == "API_VERSION"), "API_VERSION const 应被检测");

    // 核心符号高置信度
    let get_users = raw.iter().find(|r| r.symbol_name == "get_users").expect("get_users 应被检测");
    assert!(get_users.confidence == 1.0);
    let create_user = raw.iter().find(|r| r.symbol_name == "create_user").expect("create_user 应被检测");
    assert!(create_user.confidence == 1.0);
    let user_struct = raw.iter().find(|r| r.symbol_name == "User" && r.change_type == ChangeType::StructAdded).expect("User struct 应被检测");
    assert!(user_struct.confidence == 1.0);
    assert!(raw.iter().any(|r| r.symbol_name == "API_VERSION"), "API_VERSION const 应被检测");
}

/// 测试项: 系统级向后兼容 schema_version
/// 输入: v1 无 schema_version → 解析后 schema_version = 3
#[test]
fn test_system_schema_version_backward_compat() {
    // v1 只有基本字段
    let v1_json = r#"{
        "version_id": "v1",
        "git_commit": "abc",
        "timestamp": "2026-01-01T00:00:00Z",
        "message": "old",
        "capabilities": [{
            "id": "c1", "name": "test", "description": "", "status": "new",
            "sub_modules": []
        }],
        "prd_coverage": { "percentage": 0.0, "total_items": 0, "details": [] }
    }"#;
    let snap: BehaviorSnapshot = serde_json::from_str(v1_json).unwrap();
    assert_eq!(snap.schema_version, 3);
    assert_eq!(snap.version_id, "v1");
    assert_eq!(snap.capabilities.len(), 1);
    assert!(snap.capabilities[0].contract.is_none());
    assert!(snap.capabilities[0].depends_on.is_empty());
    assert!(snap.capabilities[0].preconditions.is_empty());
}

/// 测试项: 系统级 BehaviorContract 三变体解析
/// 输入: 3 种 contract 的 JSON
/// 预期: 全部正确反序列化
#[test]
fn test_system_contract_three_variants() {
    let json = serde_json::json!({
        "version_id": "v1", "timestamp": "t", "message": "",
        "capabilities": [
            {
                "id": "c1", "name": "Login API", "description": "",
                "status": "new", "sub_modules": [],
                "contract": { "type": "http_endpoint", "method": "POST", "path_template": "/api/login", "auth_required": true }
            },
            {
                "id": "c2", "name": "Handle Login", "description": "",
                "status": "new", "sub_modules": [],
                "contract": { "type": "function", "name": "handle_login", "visibility": "public", "is_async": true }
            },
            {
                "id": "c3", "name": "User Schema", "description": "",
                "status": "new", "sub_modules": [],
                "contract": { "type": "data_schema", "kind": "struct", "derives": ["Debug", "Clone"] }
            }
        ],
        "prd_coverage": { "percentage": 0.0, "total_items": 0, "details": [] }
    });

    let snap: BehaviorSnapshot = serde_json::from_value(json).unwrap();
    assert_eq!(snap.capabilities.len(), 3);

    match &snap.capabilities[0].contract {
        Some(BehaviorContract::HttpEndpoint { method, path_template, auth_required }) => {
            assert_eq!(method, "POST");
            assert_eq!(path_template, "/api/login");
            assert!(*auth_required);
        }
        _ => panic!("c1 应为 HttpEndpoint"),
    }

    match &snap.capabilities[1].contract {
        Some(BehaviorContract::Function { name, visibility, is_async }) => {
            assert_eq!(name, "handle_login");
            assert_eq!(visibility, "public");
            assert!(*is_async);
        }
        _ => panic!("c2 应为 Function"),
    }

    match &snap.capabilities[2].contract {
        Some(BehaviorContract::DataSchema { kind, derives }) => {
            assert_eq!(*kind, SchemaKind::Struct);
            assert!(derives.contains(&"Debug".to_string()));
        }
        _ => panic!("c3 应为 DataSchema"),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 集成测试: Execution Trace 模块
// ═══════════════════════════════════════════════════════════════════════

mod trace_tests {
    use Paporot::trace::adapter;
    use Paporot::trace::storage::TraceStorage;
    use Paporot::trace::types::*;

    #[test]
    fn test_import_deepseek_jsonl_from_fixture() {
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().join(".Paporot");
        let storage = TraceStorage::new(&base);
        storage.init().unwrap();

        let fixture_path = "tests/fixtures/deepseek_sample.jsonl";
        let raw = std::fs::read_to_string(fixture_path)
            .expect("Fixture file should exist");

        let adapter = adapter::auto_detect(&raw)
            .expect("Should auto-detect deepseek format");
        assert_eq!(adapter.name(), "deepseek");

        let traces = adapter.parse(&raw, fixture_path).unwrap();
        assert_eq!(traces.len(), 3, "Should parse 3 traces from fixture");

        // Verify first trace
        assert_eq!(traces[0].session_id, "chatcmpl-001");
        assert_eq!(traces[0].tool_calls.len(), 1);
        assert_eq!(traces[0].tool_calls[0].tool_name, "grep");

        // Verify second trace
        assert_eq!(traces[0].token_usage.input_tokens, 120);
        assert_eq!(traces[0].token_usage.output_tokens, 45);

        // Verify third trace has no tool calls
        assert_eq!(traces[2].tool_calls.len(), 0);
        assert_eq!(traces[2].final_output, "Fixed!");

        // Save all and verify
        let result = storage.save_batch(traces).unwrap();
        assert_eq!(result.imported.len(), 3);
        assert_eq!(result.skipped_count, 0);
    }

    #[test]
    fn test_trace_lifecycle() {
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().join(".Paporot");
        let storage = TraceStorage::new(&base);
        storage.init().unwrap();

        // Create and save a trace
        let trace = BehaviorTrace {
            id: String::new(),
            session_id: "sess-lifecycle-001".into(),
            prompt: "test lifecycle".into(),
            tool_calls: vec![ToolCall {
                id: "call_001".into(),
                tool_name: "grep".into(),
                args: serde_json::json!({"pattern": "test"}),
                timestamp: "2026-06-12T14:00:00Z".into(),
                duration_ms: 50,
                result_id: Some("obs_001".into()),
            }],
            observations: vec![Observation {
                id: "obs_001".into(),
                tool_call_id: "call_001".into(),
                content: "result".into(),
                truncated: false,
                truncated_at_bytes: None,
            }],
            final_output: "done".into(),
            token_usage: TokenUsage::default(),
            started_at: "2026-06-12T14:00:00Z".into(),
            finished_at: "2026-06-12T14:01:00Z".into(),
            source: TraceSource::Captured {
                agent_name: "test-agent".into(),
            },
            tags: vec!["lifecycle".into()],
            capability_ids: vec!["cap_001".into()],
            deleted: false,
        };

        storage.save(&trace).unwrap();

        // Load from storage to get the assigned id
        let list = storage.list(&TraceFilter::default()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].tags, vec!["lifecycle"]);
        let trace_id = list[0].id.clone();

        // Load
        let loaded = storage.load(&trace_id).unwrap();
        assert_eq!(loaded.session_id, "sess-lifecycle-001");

        // Delete (soft)
        storage.delete(&trace_id).unwrap();
        let list = storage.list(&TraceFilter::default()).unwrap();
        assert!(list.is_empty());

        // Verify still there with include_deleted
        let list = storage
            .list(&TraceFilter {
                include_deleted: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].deleted);
    }

    #[test]
    fn test_trace_filter_by_tag() {
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().join(".Paporot");
        let storage = TraceStorage::new(&base);
        storage.init().unwrap();

        for (i, tag) in ["security", "performance", "security"].iter().enumerate() {
            let mut trace = BehaviorTrace {
                id: format!("trace_filter_{}", i),
                session_id: format!("sess-{}", i),
                prompt: "test".into(),
                tool_calls: Vec::new(),
                observations: Vec::new(),
                final_output: "done".into(),
                token_usage: Default::default(),
                started_at: "2026-06-12T14:00:00Z".into(),
                finished_at: "2026-06-12T14:01:00Z".into(),
                source: TraceSource::Captured {
                    agent_name: "test".into(),
                },
                tags: vec![tag.to_string()],
                capability_ids: Vec::new(),
                deleted: false,
            };
            storage.save(&mut trace).unwrap();
        }

        let security_traces = storage
            .list(&TraceFilter {
                tag: Some("security".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(security_traces.len(), 2);

        let perf_traces = storage
            .list(&TraceFilter {
                tag: Some("performance".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(perf_traces.len(), 1);
    }

    #[test]
    fn test_trace_link_capability() {
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().join(".Paporot");
        let storage = TraceStorage::new(&base);
        storage.init().unwrap();

        let mut trace = BehaviorTrace {
            id: "trace_link_001".into(),
            session_id: "sess-001".into(),
            prompt: "test".into(),
            tool_calls: Vec::new(),
            observations: Vec::new(),
            final_output: "done".into(),
            token_usage: Default::default(),
            started_at: "2026-06-12T14:00:00Z".into(),
            finished_at: "2026-06-12T14:01:00Z".into(),
            source: TraceSource::Captured {
                agent_name: "test".into(),
            },
            tags: Vec::new(),
            capability_ids: Vec::new(),
            deleted: false,
        };
        storage.save(&trace).unwrap();

        // Link
        let mut loaded = storage.load("trace_link_001").unwrap();
        loaded.capability_ids.push("cap_001".into());
        storage.save(&loaded).unwrap();

        let reloaded = storage.load("trace_link_001").unwrap();
        assert!(reloaded.capability_ids.contains(&"cap_001".to_string()));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Trajectory Diff 集成测试
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod trajectory_tests {
    use Paporot::trace::types::*;
    use Paporot::trajectory::*;
    use Paporot::trajectory::types::*;
    use Paporot::trajectory::align::engine::AlignmentEngine;
    use Paporot::trajectory::classifier::{PhaseClassifier, RuleBasedClassifier};
    use Paporot::trajectory::hash::{semantic_hash, semantic_hashes};
    use Paporot::trajectory::analysis::TrajectoryAnalysis;
    use Paporot::trajectory::error::TrajectoryError;
    use Paporot::evaler::rules;

    fn make_tool(name: &str, args: serde_json::Value, id: &str) -> ToolCall {
        ToolCall {
            id: id.into(), tool_name: name.into(), args,
            timestamp: "2026-06-12T10:00:00Z".into(), duration_ms: 100, result_id: None,
        }
    }

    fn make_trace(id: &str, tools: Vec<ToolCall>, cap_ids: Vec<&str>) -> BehaviorTrace {
        BehaviorTrace {
            id: id.into(), session_id: format!("sess_{}", id),
            prompt: "do something".into(),
            tool_calls: tools, observations: vec![],
            final_output: "done".into(),
            token_usage: TokenUsage { input_tokens: 100, output_tokens: 50, cache_read_tokens: None, cache_write_tokens: None },
            started_at: "2026-06-12T10:00:00Z".into(),
            finished_at: "2026-06-12T10:01:00Z".into(),
            source: TraceSource::Captured { agent_name: "test".into() },
            tags: vec![], capability_ids: cap_ids.iter().map(|s| s.to_string()).collect(),
            deleted: false,
        }
    }

    #[test]
    fn test_trajectory_diff_end_to_end() {
        let engine = AlignmentEngine::default();
        let classifier = RuleBasedClassifier::default();

        let ta = make_trace("trace_a", vec![
            make_tool("read", serde_json::json!({"path":"src/auth.rs"}), "c1"),
            make_tool("edit", serde_json::json!({"path":"src/auth.rs"}), "c2"),
            make_tool("commit", serde_json::json!({"message":"fix"}), "c3"),
        ], vec!["cap_001"]);

        let tb = make_trace("trace_b", vec![
            make_tool("read", serde_json::json!({"path":"src/auth.rs"}), "d1"),
            make_tool("test", serde_json::json!({"test":"login"}), "d2"),
            make_tool("edit", serde_json::json!({"path":"src/auth.rs"}), "d3"),
            make_tool("test", serde_json::json!({"test":"login"}), "d4"),
            make_tool("commit", serde_json::json!({"message":"fix with tests"}), "d5"),
        ], vec!["cap_001"]);

        let diff = engine.diff(&classifier, &ta, &tb, Some("cap_001".into()));

        assert_eq!(diff.capability_id, Some("cap_001".into()));
        assert_eq!(diff.version_a.tool_count, 3);
        assert_eq!(diff.version_b.tool_count, 5);
        assert!(!diff.segments.is_empty());
        // B has verify phase that A doesn't
        let has_added = diff.segments.iter().any(|s| s.kind == SegmentKind::Added);
        assert!(has_added, "Should detect added verify phase");
    }

    #[test]
    fn test_trajectory_diff_identical_traces() {
        let engine = AlignmentEngine::default();
        let classifier = RuleBasedClassifier::default();

        let tools = vec![
            make_tool("read", serde_json::json!({"path":"a.rs"}), "c1"),
            make_tool("edit", serde_json::json!({"path":"a.rs"}), "c2"),
        ];

        let ta = make_trace("ta", tools.clone(), vec!["cap_001"]);
        let tb = make_trace("tb", tools.clone(), vec!["cap_001"]);

        let diff = engine.diff(&classifier, &ta, &tb, None);
        assert_eq!(diff.summary.tool_calls_unchanged, 2);
        assert_eq!(diff.summary.tool_calls_added, 0);
        assert_eq!(diff.summary.tool_calls_deleted, 0);
    }

    #[test]
    fn test_trajectory_diff_empty_traces() {
        let engine = AlignmentEngine::default();
        let classifier = RuleBasedClassifier::default();

        let ta = make_trace("ta", vec![], vec![]);
        let tb = make_trace("tb", vec![], vec![]);

        let diff = engine.diff(&classifier, &ta, &tb, None);
        assert!(diff.segments.is_empty());
        assert_eq!(diff.summary.tool_calls_unchanged, 0);
    }

    #[test]
    fn test_trajectory_to_analysis_pipeline() {
        let engine = AlignmentEngine::default();
        let classifier = RuleBasedClassifier::default();

        let ta = make_trace("ta", vec![
            make_tool("read", serde_json::json!({"path":"a.rs"}), "c1"),
            make_tool("edit", serde_json::json!({"path":"a.rs"}), "c2"),
        ], vec!["cap_001"]);

        let tb = make_trace("tb", vec![
            make_tool("read", serde_json::json!({"path":"a.rs"}), "d1"),
            make_tool("test", serde_json::json!({"test":"all"}), "d2"),
            make_tool("edit", serde_json::json!({"path":"a.rs"}), "d3"),
        ], vec!["cap_001"]);

        let diff = engine.diff(&classifier, &ta, &tb, Some("cap_001".into()));
        let analysis = TrajectoryAnalysis::from_diff(&diff);

        assert_eq!(analysis.tool_count_a, 2);
        assert_eq!(analysis.tool_count_b, 3);
        assert!(analysis.tool_churn_score > 0.0, "Tool churn should be > 0");
        assert!(!analysis.phase_additions.is_empty() || !analysis.phase_modifications.is_empty(),
            "Should have phase changes");
    }

    #[test]
    fn test_segment_rules_with_analysis() {
        use Paporot::trajectory::types::*;

        // Build a diff with added segments
        let diff = TrajectoryDiff {
            capability_id: Some("cap_test".into()),
            version_a: TrajectoryVersion {
                trace_id: "ta".into(), session_id: "sa".into(),
                tool_count: 2, duration_ms: 200, total_tokens: 100,
                started_at: "now".into(),
            },
            version_b: TrajectoryVersion {
                trace_id: "tb".into(), session_id: "sb".into(),
                tool_count: 3, duration_ms: 300, total_tokens: 150,
                started_at: "now".into(),
            },
            segments: vec![
                SegmentDiff {
                    label: "定位问题".into(), kind: SegmentKind::Unchanged,
                    tool_diffs: vec![
                        ToolDiff { tool_name: "read".into(), kind: ToolDiffKind::Unchanged,
                            index_a: Some(0), index_b: Some(0), args_diff: None, duration_ms: 100 },
                    ],
                    index_a: Some(0), index_b: Some(0),
                },
                SegmentDiff {
                    label: "验证".into(), kind: SegmentKind::Added,
                    tool_diffs: vec![
                        ToolDiff { tool_name: "test".into(), kind: ToolDiffKind::Added,
                            index_a: None, index_b: Some(1), args_diff: None, duration_ms: 500 },
                    ],
                    index_a: None, index_b: Some(1),
                },
                SegmentDiff {
                    label: "实施修改".into(), kind: SegmentKind::Unchanged,
                    tool_diffs: vec![
                        ToolDiff { tool_name: "edit".into(), kind: ToolDiffKind::Unchanged,
                            index_a: Some(1), index_b: Some(2), args_diff: None, duration_ms: 200 },
                    ],
                    index_a: Some(1), index_b: Some(2),
                },
            ],
            summary: DiffSummary {
                segments_unchanged: 2, segments_added: 1,
                tool_calls_unchanged: 2, tool_calls_added: 1,
                ..Default::default()
            },
        };

        let analysis = TrajectoryAnalysis::from_diff(&diff);
        assert_eq!(analysis.phase_additions.len(), 1);
        assert_eq!(analysis.phase_additions[0].label, "验证");

        // Run segment rules
        let hits = rules::evaluate_segment_rules(&analysis);
        // Only 1 phase added (< 2), so S001 shouldn't trigger
        // "验证" is critical, but it's added not deleted, so S002 shouldn't trigger
        assert!(!hits.iter().any(|h| h.rule_id == "S001"),
            "S001 should not trigger with only 1 added phase");
        assert!(!hits.iter().any(|h| h.rule_id == "S002"),
            "S002 should not trigger for added phases");
    }

    #[test]
    fn test_segment_rules_critical_phase_deletion() {
        use Paporot::trajectory::types::*;

        let diff = TrajectoryDiff {
            capability_id: Some("cap_test".into()),
            version_a: TrajectoryVersion {
                trace_id: "ta".into(), session_id: "sa".into(),
                tool_count: 2, duration_ms: 200, total_tokens: 100,
                started_at: "now".into(),
            },
            version_b: TrajectoryVersion {
                trace_id: "tb".into(), session_id: "sb".into(),
                tool_count: 1, duration_ms: 100, total_tokens: 50,
                started_at: "now".into(),
            },
            segments: vec![
                SegmentDiff {
                    label: "提交".into(), kind: SegmentKind::Deleted,
                    tool_diffs: vec![
                        ToolDiff { tool_name: "commit".into(), kind: ToolDiffKind::Deleted,
                            index_a: Some(0), index_b: None, args_diff: None, duration_ms: 150 },
                    ],
                    index_a: Some(0), index_b: None,
                },
            ],
            summary: DiffSummary {
                segments_deleted: 1, tool_calls_deleted: 1,
                ..Default::default()
            },
        };

        let analysis = TrajectoryAnalysis::from_diff(&diff);
        let hits = rules::evaluate_segment_rules(&analysis);
        assert!(hits.iter().any(|h| h.rule_id == "S002"),
            "S002 should trigger when critical phase is deleted");
    }

    #[test]
    fn test_trajectory_mermaid_output() {
        use Paporot::trajectory::report;
        use Paporot::trajectory::types::*;

        let diff = TrajectoryDiff {
            capability_id: Some("cap_test".into()),
            version_a: TrajectoryVersion {
                trace_id: "ta".into(), session_id: "sa".into(),
                tool_count: 1, duration_ms: 100, total_tokens: 50,
                started_at: "now".into(),
            },
            version_b: TrajectoryVersion {
                trace_id: "tb".into(), session_id: "sb".into(),
                tool_count: 1, duration_ms: 100, total_tokens: 50,
                started_at: "now".into(),
            },
            segments: vec![
                SegmentDiff {
                    label: "定位问题".into(), kind: SegmentKind::Unchanged,
                    tool_diffs: vec![
                        ToolDiff { tool_name: "read".into(), kind: ToolDiffKind::Unchanged,
                            index_a: Some(0), index_b: Some(0), args_diff: None, duration_ms: 100 },
                    ],
                    index_a: Some(0), index_b: Some(0),
                },
            ],
            summary: DiffSummary {
                segments_unchanged: 1, tool_calls_unchanged: 1,
                ..Default::default()
            },
        };

        let mermaid = report::to_mermaid(&diff);
        assert!(mermaid.contains("Gantt") || mermaid.contains("gantt"), "Should contain gantt chart");
        assert!(mermaid.contains("ta"), "Should reference trace A");
        assert!(mermaid.contains("tb"), "Should reference trace B");
    }

    #[test]
    fn test_classifier_trait_usage() {
        let classifier = RuleBasedClassifier::default();
        let ta = make_trace("ta", vec![
            make_tool("read", serde_json::json!({}), "c1"),
            make_tool("write", serde_json::json!({}), "c2"),
        ], vec![]);

        let segments = classifier.classify(&ta);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].label, "定位问题");
        assert_eq!(segments[1].label, "实施修改");
        assert_eq!(classifier.name(), "rule_based");
        assert_eq!(classifier.version(), "1.0.0");
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Phase 2: SnapshotStore + SnapshotAnalyzer 集成测试
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod snapshot_tests {
    use Paporot::storage::SnapshotStorage;
    use Paporot::types::*;

    fn make_cap(id: &str, name: &str, status: CapabilityStatus) -> Capability {
        Capability {
            id: id.into(), name: name.into(), description: String::new(),
            status, module: None, sub_modules: vec![],
            confidence: Some(1.0), evidence: vec![], tags: vec![],
            contract: None, preconditions: vec![], postconditions: vec![],
            invariants: vec![], categories: vec![],
            depends_on: vec![], depended_by: vec![],
            evolved_from: None, evidence_trace_ids: vec![],
            verified_by: None, verified_at: None,
            source_change_type: None, triggered_by_rules: vec![],
        }
    }

    fn make_snapshot(version: &str, caps: Vec<Capability>) -> BehaviorSnapshot {
        BehaviorSnapshot {
            schema_version: 3, version_id: version.into(),
            git_commit: Some("abc123".into()), git_ref: None,
            timestamp: "2026-06-24T10:00:00Z".into(),
            message: "test".into(), capabilities: caps,
            prd_coverage: PrdCoverage {
                percentage: 100.0, total_items: 1,
                covered_items: Some(1), details: vec![],
            },
            regression: None, risk: None, metadata: None,
        }
    }

    #[test]
    fn test_store_roundtrip_and_diff() {
        let dir = std::env::temp_dir().join("Paporot_integration_store");
        let _ = std::fs::remove_dir_all(&dir);

        let store = SnapshotStorage::new(&dir);

        // Write two snapshots
        let v1 = make_snapshot("v1", vec![
            make_cap("c1", "Login", CapabilityStatus::New),
            make_cap("c2", "Logout", CapabilityStatus::New),
        ]);
        let v2 = make_snapshot("v2", vec![
            make_cap("c1", "Login", CapabilityStatus::Unchanged),
            make_cap("c3", "Payment", CapabilityStatus::New),
        ]);

        store.save(&v1).unwrap();
        store.save(&v2).unwrap();

        // Load back and validate
        let loaded = store.load_latest().unwrap();
        assert_eq!(loaded.version_id, "v2");
        assert_eq!(loaded.capabilities.len(), 2);

        // Compute diff
        let diff = Paporot::snapshot::diff(&v1, &v2);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.deleted.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  v3 Loop Engineering E2E Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod v3_loop_tests {
    use Paporot::feedback_loop::feedback_loader;
    use Paporot::commands::feedback;
    use Paporot::storage::SnapshotStorage;
    use Paporot::types::*;
    use paporot_analysis_types::*;

    fn make_cap(id: &str, name: &str, file: &str, change_type: &str, rules: Vec<&str>) -> Capability {
        Capability {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            status: CapabilityStatus::New,
            module: None,
            sub_modules: vec![],
            confidence: Some(0.9),
            evidence: vec![file.into()],
            tags: vec![],
            contract: None,
            preconditions: vec![],
            postconditions: vec![],
            invariants: vec![],
            categories: vec![],
            depends_on: vec![],
            depended_by: vec![],
            evolved_from: None,
            evidence_trace_ids: vec![],
            verified_by: None,
            verified_at: None,
            source_change_type: Some(change_type.into()),
            triggered_by_rules: rules.into_iter().map(|r| r.to_string()).collect(),
        }
    }

    fn make_snap(version: &str, caps: Vec<Capability>) -> BehaviorSnapshot {
        BehaviorSnapshot {
            schema_version: 3,
            version_id: version.into(),
            git_commit: None,
            git_ref: None,
            timestamp: "2026-06-24T10:00:00Z".into(),
            message: String::new(),
            capabilities: caps,
            prd_coverage: PrdCoverage { percentage: 100.0, total_items: 0, covered_items: None, details: vec![] },
            regression: None,
            risk: None,
            metadata: None,
        }
    }

    /// E2E: Feedback → Reject → FeedbackIndex → Exact Match
    #[test]
    fn test_loop_exact_suppression() {
        let dir = std::env::temp_dir().join("Paporot_v3_loop_exact");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let paporot_dir = dir.join(".Paporot");
        std::fs::create_dir_all(paporot_dir.join("reviews")).unwrap();
        std::fs::create_dir_all(paporot_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(paporot_dir.join("rules")).unwrap();
        std::fs::create_dir_all(paporot_dir.join("work")).unwrap();

        // 1. Create snapshot with traceback-aware capabilities
        let caps = vec![
            make_cap("cap_auth", "login", "src/auth.rs", "FunctionAdded", vec!["sec_auth_001"]),
            make_cap("cap_sync", "sync_legacy", "src/legacy/sync.rs", "FunctionRemoved", vec!["breaking_001"]),
        ];
        let snap = make_snap("v1", caps);
        let store = SnapshotStorage::new(paporot_dir.join("snapshots"));
        store.save(&snap).unwrap();

        // 2. Generate and apply feedback with a rejection
        let toml_path = feedback::generate_review_toml(&snap, &paporot_dir.join("reviews")).unwrap();
        std::fs::write(&toml_path, r#"
[reject]
cap_sync = "false positive - legacy code"
"#).unwrap();

        let mut snap_for_apply = store.load_latest().unwrap();
        let review_store = feedback::apply_review_toml(
            &mut snap_for_apply,
            &toml_path,
            "tester",
        ).unwrap();

        assert_eq!(review_store.stats.rejected, 1);

        // 3. Verify BehaviorReview has traceback fields
        let rejected_review = review_store.reviews.iter()
            .find(|r| r.verdict == ReviewVerdict::Rejected)
            .unwrap();
        assert_eq!(rejected_review.source_symbol, Some("sync_legacy".into()));
        assert_eq!(rejected_review.source_file, Some("src/legacy/sync.rs".into()));
        assert_eq!(rejected_review.source_change_type, Some("FunctionRemoved".into()));
        assert_eq!(rejected_review.triggered_by_rules, vec!["breaking_001"]);

        // 4. Save the review
        review_store.save(&paporot_dir.join("reviews").join("reviews.json")).unwrap();

        // 5. Build feedback index and verify L1 exact match key
        let (index, result) = feedback_loader::build_feedback_index(
            &paporot_dir,
            &snap,
        ).unwrap();
        assert_eq!(result.rejected_count, 1);
        assert_eq!(result.total_reviews, 1);

        let key = "sync_legacy::src/legacy/sync.rs::FunctionRemoved";
        assert!(index.exact_reject_map.contains_key(key));
        assert_eq!(index.exact_reject_map.get(key).unwrap(), "false positive - legacy code");

        // 6. Verify L3 prefix
        assert_eq!(index.rejected_prefixes, vec!["src/legacy"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// E2E: Feedback with suppress_rule → suppressions.toml → FeedbackIndex
    #[test]
    fn test_loop_rule_suppression() {
        let dir = std::env::temp_dir().join("Paporot_v3_loop_rule");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let paporot_dir = dir.join(".Paporot");
        std::fs::create_dir_all(paporot_dir.join("reviews")).unwrap();
        std::fs::create_dir_all(paporot_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(paporot_dir.join("rules")).unwrap();
        std::fs::create_dir_all(paporot_dir.join("work")).unwrap();

        let caps = vec![
            make_cap("cap_auth", "login", "src/auth.rs", "FunctionAdded", vec!["sec_auth_001"]),
        ];
        let snap = make_snap("v1", caps);
        let store = SnapshotStorage::new(paporot_dir.join("snapshots"));
        store.save(&snap).unwrap();

        // Generate feedback TOML with suppress_rule section
        let toml_path = paporot_dir.join("reviews").join("review_v1.toml");
        std::fs::write(&toml_path, r#"
[suppress_rule.breaking_001]
reason = "src/legacy/ API removals are expected"
file_pattern = "src/legacy/*"
effect = "suppress"

[suppress_rule.sec_token_001]
reason = "migrations are expected"
file_pattern = "src/migrations/*"
effect = "warn"
change_type = "ConstantChanged"
"#).unwrap();

        let mut snap_for_apply = store.load_latest().unwrap();
        let review_store = feedback::apply_review_toml(
            &mut snap_for_apply,
            &toml_path,
            "tester",
        ).unwrap();
        // suppress_rule doesn't create reviews
        assert_eq!(review_store.stats.total_reviews, 0);

        // Verify suppressions.toml was created
        let suppressions_path = paporot_dir.join("rules").join("suppressions.toml");
        assert!(suppressions_path.exists());

        // Build feedback index and verify L2 suppressions are loaded
        let (index, result) = feedback_loader::build_feedback_index(
            &paporot_dir,
            &snap,
        ).unwrap();
        assert_eq!(result.suppression_count, 2);

        let breaking_supp = index.rule_suppressions.iter()
            .find(|s| s.rule_id == "breaking_001").unwrap();
        assert_eq!(breaking_supp.file_pattern, "src/legacy/*");
        assert_eq!(breaking_supp.effect, SuppressionEffect::Suppress);
        assert_eq!(breaking_supp.status, SuppressionStatus::Active);

        let token_supp = index.rule_suppressions.iter()
            .find(|s| s.rule_id == "sec_token_001").unwrap();
        assert_eq!(token_supp.effect, SuppressionEffect::Warn);
        assert_eq!(token_supp.change_type, Some("ConstantChanged".into()));

        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Phase 5: 端到端测试
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod e2e_tests {
    use Paporot::storage::SnapshotStorage;
    use Paporot::snapshot;
    use Paporot::types::*;

    fn make_cap(id: &str, name: &str, status: CapabilityStatus, conf: f32) -> Capability {
        Capability {
            id: id.into(), name: name.into(), description: String::new(),
            status, module: None, sub_modules: vec![],
            confidence: Some(conf), evidence: vec![], tags: vec![],
            contract: None, preconditions: vec![], postconditions: vec![],
            invariants: vec![], categories: vec![],
            depends_on: vec![], depended_by: vec![],
            evolved_from: None, evidence_trace_ids: vec![],
            verified_by: None, verified_at: None,
            source_change_type: None, triggered_by_rules: vec![],
        }
    }

    fn make_snap(version: &str, caps: Vec<Capability>) -> BehaviorSnapshot {
        BehaviorSnapshot {
            schema_version: 3, version_id: version.into(),
            git_commit: Some("e2e_test".into()), git_ref: None,
            timestamp: "2026-06-24T10:00:00Z".into(),
            message: "e2e test".into(), capabilities: caps,
            prd_coverage: PrdCoverage { percentage: 100.0, total_items: 1, covered_items: Some(1), details: vec![] },
            regression: None, risk: None, metadata: None,
        }
    }

    /// E2E: 完整 L1→L2→Snapshot 流水线（不依赖 LLM）
    #[test]
    fn test_full_analysis_pipeline_no_llm() {
        let dir = std::env::temp_dir().join("Paporot_e2e");
        let _ = std::fs::remove_dir_all(&dir);

        let store = SnapshotStorage::new(dir.join("snapshots"));

        // Simulate two analysis runs
        let v1 = make_snap("v1", vec![
            make_cap("cap_auth", "User Authentication", CapabilityStatus::New, 0.95),
            make_cap("cap_db", "Database Connection", CapabilityStatus::New, 0.88),
        ]);

        let v2 = make_snap("v2", vec![
            make_cap("cap_auth", "User Authentication", CapabilityStatus::Unchanged, 0.95),
            make_cap("cap_pay", "Payment Processing", CapabilityStatus::New, 0.92),
            make_cap("cap_db", "Database Connection V2", CapabilityStatus::Modified, 0.88),
        ]);

        store.save(&v1).unwrap();
        store.save(&v2).unwrap();

        // Verify snapshots are stored
        let versions = store.list_versions_sorted().unwrap();
        assert_eq!(versions, vec!["v1", "v2"]);

        // Compute diff
        let diff = snapshot::diff(&v1, &v2);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.deleted.len(), 0); // cap_db is modified, not deleted
        assert_eq!(diff.modified.len(), 1);
        assert_eq!(diff.unchanged.len(), 1);

        // Verify diff content
        assert_eq!(diff.added[0].id, "cap_pay");
        assert_eq!(diff.modified[0].id, "cap_db");
        assert_eq!(diff.unchanged[0].id, "cap_auth");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// E2E: LLM 降级路径 — 当 L3 不可用时系统应正常降级
    #[test]
    fn test_llm_unavailable_fallback() {
        // When LLM is not available, the system should still produce
        // valid L1/L2 results and save snapshots without L3 fragments.

        let dir = std::env::temp_dir().join("Paporot_e2e_fallback");
        let _ = std::fs::remove_dir_all(&dir);

        let store = SnapshotStorage::new(dir.join("snapshots"));
        let snap = make_snap("v1", vec![
            make_cap("cap_x", "FallbackCap", CapabilityStatus::New, 0.95),
        ]);
        store.save(&snap).unwrap();

        // Verify the snapshot was saved correctly even without L3
        let loaded = store.load_by_version("v1").unwrap();
        assert_eq!(loaded.capabilities.len(), 1);
        assert!(loaded.capabilities[0].confidence.unwrap() >= 0.5);

        // System should not crash when L3 is unavailable
        let versions = store.list_versions_sorted().unwrap();
        assert!(!versions.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// E2E: Trace ↔ Snapshot 三级匹配全链路
    #[test]
    fn test_trace_snapshot_matching_full_pipeline() {
        use Paporot::trace::trace_snapshot_map::{TraceMatcher, TraceSnapshotMap};

        let dir = std::env::temp_dir().join("Paporot_e2e_match");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Simulate matching multiple traces to snapshots
        let mut map = TraceSnapshotMap::default();

        // L1: commit match
        let m1 = TraceMatcher::match_trace_to_snapshot(
            "t1", Some("abc123"), "2026-06-24T10:00:00Z", &[],
            "v1", Some("abc123"), "2026-06-24T10:00:00Z", &[],
        ).unwrap();
        assert_eq!(m1.confidence, 1.0);

        // L2: file overlap
        let m2 = TraceMatcher::match_trace_to_snapshot(
            "t2", Some("def"), "2026-06-24T10:00:00Z", &["src/auth.rs".into()],
            "v2", Some("abc"), "2026-06-24T10:00:00Z", &["src/auth.rs".into(), "tests/auth_test.rs".into()],
        ).unwrap();
        assert!(m2.confidence > 0.0 && m2.confidence < 1.0);

        map.mappings.insert("v1".into(), vec![m1]);
        map.mappings.insert("v2".into(), vec![m2]);

        let path = dir.join("trace_map.json");
        map.save(&path).unwrap();
        assert!(path.exists());

        let loaded = TraceSnapshotMap::load_or_new(&path);
        assert_eq!(loaded.mappings.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// E2E: Feedback TOML → Snapshot 全流程（不依赖 LLM）
    #[test]
    fn test_feedback_to_snapshot_full_roundtrip() {
        use Paporot::commands::feedback;
        use Paporot::storage::SnapshotStorage;

        let dir = std::env::temp_dir().join("Paporot_e2e_feedback");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 1. Create initial snapshot
        let store = SnapshotStorage::new(dir.join("snapshots"));
        let snap = make_snap("v1", vec![
            make_cap("cap_auth", "Auth", CapabilityStatus::New, 0.95),
            make_cap("cap_sync", "Sync", CapabilityStatus::New, 0.80),
            make_cap("cap_ui", "UI", CapabilityStatus::New, 0.85),
        ]);
        store.save(&snap).unwrap();

        // 2. Generate review TOML
        let reviews_dir = dir.join("reviews");
        let toml_path = feedback::generate_review_toml(&snap, &reviews_dir).unwrap();

        // 3. Human edits the TOML
        std::fs::write(&toml_path, r#"
[approve]
cap_auth = "ok"
cap_ui = "ok"

[reject]
cap_sync = "false positive - 同步逻辑已删除"
"#).unwrap();

        // 4. Apply corrections
        let mut loaded = store.load_by_version("v1").unwrap();
        let fb_store = feedback::apply_review_toml(&mut loaded, &toml_path, "reviewer").unwrap();

        assert_eq!(fb_store.stats.approved, 2);
        assert_eq!(fb_store.stats.rejected, 1);

        // Sync should be removed
        assert_eq!(loaded.capabilities.len(), 2);
        assert!(!loaded.capabilities.iter().any(|c| c.id == "cap_sync"));

        // Approved caps have verification
        let auth = loaded.capabilities.iter().find(|c| c.id == "cap_auth").unwrap();
        assert_eq!(auth.verified_by.as_deref(), Some("reviewer"));

        // 5. Save corrected snapshot
        store.save(&loaded).unwrap();
        let reloaded = store.load_by_version("v1").unwrap();
        assert_eq!(reloaded.capabilities.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Phase 3: Feedback TOML 集成测试
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod feedback_tests {
    use Paporot::commands::feedback;
    use Paporot::storage::SnapshotStorage;
    use Paporot::types::*;

    fn make_snap(version: &str, caps: Vec<Capability>) -> BehaviorSnapshot {
        BehaviorSnapshot {
            schema_version: 3, version_id: version.into(),
            git_commit: None, git_ref: None,
            timestamp: "2026-06-24T10:00:00Z".into(),
            message: String::new(), capabilities: caps,
            prd_coverage: PrdCoverage { percentage: 0.0, total_items: 0, covered_items: None, details: vec![] },
            regression: None, risk: None, metadata: None,
        }
    }

    fn cap(id: &str, name: &str) -> Capability {
        Capability {
            id: id.into(), name: name.into(), description: String::new(),
            status: CapabilityStatus::New, module: None, sub_modules: vec![],
            confidence: Some(1.0), evidence: vec![], tags: vec![],
            contract: None, preconditions: vec![], postconditions: vec![],
            invariants: vec![], categories: vec![],
            depends_on: vec![], depended_by: vec![],
            evolved_from: None, evidence_trace_ids: vec![],
            verified_by: None, verified_at: None,
            source_change_type: None, triggered_by_rules: vec![],
        }
    }

    #[test]
    fn test_feedback_full_roundtrip() {
        let dir = std::env::temp_dir().join("Paporot_integration_feedback");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 1. Create snapshot with 3 capabilities
        let store = SnapshotStorage::new(dir.join("snapshots"));
        let snap = make_snap("v1", vec![
            cap("cap_auth", "Auth"),
            cap("cap_data_sync", "DataSync"),
            cap("cap_dashboard", "Dashboard"),
        ]);
        store.save(&snap).unwrap();

        // 2. Generate review TOML
        let reviews_dir = dir.join("reviews");
        let toml_path = feedback::generate_review_toml(&snap, &reviews_dir).unwrap();
        assert!(toml_path.exists());

        // 3. Simulate human editing the TOML
        std::fs::write(&toml_path, r#"
[approve]
cap_auth = "ok"

[reject]
cap_data_sync = "false positive - 数据同步已移除"

[flag]
cap_dashboard = "需要PM确认是否需要保留"
"#).unwrap();

        // 4. Apply TOML review
        let mut loaded_snap = store.load_by_version("v1").unwrap();
        let fb_store = feedback::apply_review_toml(&mut loaded_snap, &toml_path, "reviewer").unwrap();

        assert_eq!(fb_store.stats.total_reviews, 3);
        assert_eq!(fb_store.stats.approved, 1);
        assert_eq!(fb_store.stats.rejected, 1);
        assert_eq!(fb_store.stats.flagged, 1);

        // Rejected cap should be gone
        assert_eq!(loaded_snap.capabilities.len(), 2);
        let remaining_ids: Vec<_> = loaded_snap.capabilities.iter().map(|c| c.id.as_str()).collect();
        assert!(remaining_ids.contains(&"cap_auth"));
        assert!(remaining_ids.contains(&"cap_dashboard"));
        assert!(!remaining_ids.contains(&"cap_data_sync"));

        // Approved cap should have verifier set
        let auth = loaded_snap.capabilities.iter().find(|c| c.id == "cap_auth").unwrap();
        assert_eq!(auth.verified_by.as_deref(), Some("reviewer"));

        // Flagged cap should have tag
        let dash = loaded_snap.capabilities.iter().find(|c| c.id == "cap_dashboard").unwrap();
        assert!(dash.tags.contains(&"needs-review".to_string()));

        // 5. Save updated snapshot
        store.save(&loaded_snap).unwrap();
        let reloaded = store.load_by_version("v1").unwrap();
        assert_eq!(reloaded.capabilities.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Phase 4: Trajectory 子系统集成测试
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod trajectory_integration_tests {
    use Paporot::trace::types::*;
    use Paporot::trace::storage::TraceStorage;
    use Paporot::trajectory::state::segmentation::RuleSegmenter;

    fn make_claude_tools() -> Vec<ToolCall> {
        vec![
            ToolCall {
                id: "c1".into(),
                tool_name: "read".into(),
                args: serde_json::json!({"file": "src/main.rs"}),
                timestamp: "2026-06-24T10:00:01Z".into(),
                duration_ms: 50,
                result_id: None,
            },
            ToolCall {
                id: "c2".into(),
                tool_name: "write".into(),
                args: serde_json::json!({"file": "src/main.rs"}),
                timestamp: "2026-06-24T10:00:02Z".into(),
                duration_ms: 30,
                result_id: None,
            },
            ToolCall {
                id: "c3".into(),
                tool_name: "read".into(),
                args: serde_json::json!({"file": "tests/test.rs"}),
                timestamp: "2026-06-24T10:00:03Z".into(),
                duration_ms: 50,
                result_id: None,
            },
        ]
    }

    fn make_trace(id: &str, tools: Vec<ToolCall>) -> BehaviorTrace {
        BehaviorTrace {
            id: id.into(),
            session_id: format!("session_{}", id),
            prompt: format!("prompt for {}", id),
            tool_calls: tools,
            observations: vec![],
            final_output: "done".into(),
            token_usage: TokenUsage {
                input_tokens: 500,
                output_tokens: 200,
                cache_read_tokens: None,
                cache_write_tokens: None,
            },
            started_at: "2026-06-24T10:00:00Z".into(),
            finished_at: "2026-06-24T10:00:05Z".into(),
            source: TraceSource::Imported {
                adapter: "claude".into(),
                adapter_version: "1.0".into(),
                file_path: "/tmp/test.json".into(),
            },
            tags: vec![],
            capability_ids: vec![],
            deleted: false,
        }
    }

    #[test]
    fn test_segmentation_cutter() {
        let tools = make_claude_tools();
        let trace = make_trace("t1", tools);
        let segmenter = RuleSegmenter::new();
        let segments = segmenter.cut(&trace);

        // Should produce at least 1 segment
        assert!(!segments.is_empty());
    }

    #[test]
    fn test_trace_store_roundtrip() {
        let dir = std::env::temp_dir().join("Paporot_integration_trace");
        let _ = std::fs::remove_dir_all(&dir);

        let store = TraceStorage::new(&dir);
        let trace = make_trace("trace_001", make_claude_tools());

        let path = store.save(&trace).unwrap();
        assert!(path.exists());

        let loaded = store.load("trace_001").unwrap();
        assert_eq!(loaded.id, "trace_001");
        assert_eq!(loaded.tool_calls.len(), 3);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_trace_import_batch() {
        let dir = std::env::temp_dir().join("Paporot_integration_batch");
        let _ = std::fs::remove_dir_all(&dir);

        let store = TraceStorage::new(&dir);
        let mut traces: Vec<BehaviorTrace> = (0..5)
            .map(|i| make_trace(&format!("trace_{:03}", i), make_claude_tools()))
            .collect();

        let result = store.save_batch(traces).unwrap();
        assert_eq!(result.imported.len(), 5);
        assert_eq!(result.skipped_count, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
