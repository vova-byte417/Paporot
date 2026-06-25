//! Paporot 核心数据类型定义
//!
//! Capability/Snapshot 类型统一在 paporot-analysis-types crate 中定义。
//! 本地只保留 Feedback / TestMap 等模块私有类型。

use serde::{Deserialize, Serialize};

// Re-export shared types
pub use paporot_analysis_types::*;

// ─── P3: 人机验证回路类型 ───────────────────────────────────────────────

/// 行为审查条目 —— 用户/审核者对某个能力的判断
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BehaviorReview {
    /// 审查 ID，如 "rev_001"
    pub review_id: String,
    /// 目标能力 ID
    pub capability_id: String,
    /// 目标快照版本
    pub snapshot_version: String,
    /// 审查者标识
    pub reviewer: String,
    /// 审查结论
    pub verdict: ReviewVerdict,
    /// 审查备注
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// 修正后的能力数据（仅当 verdict = Corrected 时）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corrected: Option<Capability>,
    /// 审查时间 ISO-8601
    pub reviewed_at: String,
    /// 标签
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    // ── v3 Loop Engineering: 溯源字段 ──
    /// 触发此 Capability 的 L2 规则 ID 列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggered_by_rules: Vec<String>,
    /// 原始符号名（从 RawChange.symbol_name 反向映射）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_symbol: Option<String>,
    /// 原始文件路径（从 RawChange.file_path 反向映射）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    /// 原始变更类型（从 RawChange.change_type 反向映射）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_change_type: Option<String>,
}

/// 审查结论
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReviewVerdict {
    /// 确认正确
    Approved,
    /// 标记为误报（该能力不应存在）
    Rejected,
    /// 修正后接受
    Corrected,
    /// 无法判断，标记为待定
    Flagged,
}

/// 审查反馈存储
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FeedbackStore {
    /// 审查记录列表
    pub reviews: Vec<BehaviorReview>,
    /// 审查统计
    pub stats: FeedbackStats,
}

/// 审查统计信息
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct FeedbackStats {
    pub total_reviews: u32,
    pub approved: u32,
    pub rejected: u32,
    pub corrected: u32,
    pub flagged: u32,
}

impl FeedbackStore {
    /// 从 JSON 加载或创建空存储
    pub fn load_or_new(path: &std::path::Path) -> anyhow::Result<Self> {
        if path.exists() {
            let json = std::fs::read_to_string(path)?;
            Ok(serde_json::from_str(&json)?)
        } else {
            Ok(Self {
                reviews: vec![],
                stats: FeedbackStats::default(),
            })
        }
    }

    /// 保存到文件
    pub fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// 添加一条审查记录
    pub fn add_review(&mut self, review: BehaviorReview) {
        self.stats.total_reviews += 1;
        match review.verdict {
            ReviewVerdict::Approved => self.stats.approved += 1,
            ReviewVerdict::Rejected => self.stats.rejected += 1,
            ReviewVerdict::Corrected => self.stats.corrected += 1,
            ReviewVerdict::Flagged => self.stats.flagged += 1,
        }
        self.reviews.push(review);
    }

    /// 查询某能力的审查历史
    pub fn reviews_for(&self, capability_id: &str) -> Vec<&BehaviorReview> {
        self.reviews
            .iter()
            .filter(|r| r.capability_id == capability_id)
            .collect()
    }
}

// ─── P4: 行为-测试闭环类型 ─────────────────────────────────────────────

/// 测试代码与行为能力的映射
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TestMapping {
    /// 映射 ID，如 "tmap_001"
    pub map_id: String,
    /// 关联的能力 ID
    pub capability_id: String,
    /// 测试文件路径
    pub test_file: String,
    /// 测试函数/用例名
    pub test_name: String,
    /// 测试框架
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framework: Option<String>,
    /// 测试状态
    pub test_status: TestStatus,
    /// 映射置信度 0.0-1.0
    pub confidence: f32,
    /// 映射来源
    pub source: TestMappingSource,
    /// 上次运行时间
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<String>,
}

/// 测试状态
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TestStatus {
    /// 测试存在且通过
    Passing,
    /// 测试存在但失败
    Failing,
    /// 测试文件/函数存在但无法确定结果
    Unknown,
    /// 未找到对应测试
    Missing,
}

/// 测试映射来源
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TestMappingSource {
    /// 文件名推断（user.rs → user_test.rs）
    FileNameInferred,
    /// 函数名推断（login → test_login）
    NameConventionInferred,
    /// LLM 推断
    LlmInferred,
    /// 人工标记
    Manual,
}

/// 测试映射存储
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TestMapStore {
    /// 所有测试映射
    pub mappings: Vec<TestMapping>,
    /// 统计信息
    pub stats: TestMapStats,
}

/// 测试覆盖统计
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TestMapStats {
    /// 有能力的总数（来自所有快照）
    pub total_capabilities: u32,
    /// 有测试映射的能力数
    pub mapped_capabilities: u32,
    /// 测试通过的能力数
    pub passing_count: u32,
    /// 测试失败数
    pub failing_count: u32,
    /// 未找到测试数
    pub missing_count: u32,
}

impl TestMapStore {
    /// 从 JSON 加载或创建空存储
    pub fn load_or_new(path: &std::path::Path) -> anyhow::Result<Self> {
        if path.exists() {
            let json = std::fs::read_to_string(path)?;
            Ok(serde_json::from_str(&json)?)
        } else {
            Ok(Self {
                mappings: vec![],
                stats: TestMapStats::default(),
            })
        }
    }

    /// 保存到文件
    pub fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// 添加映射并更新统计
    pub fn add_mapping(&mut self, mapping: TestMapping) {
        self.mappings.push(mapping);
        self.recalc_stats();
    }

    /// 查询某个能力的测试映射
    pub fn mappings_for(&self, capability_id: &str) -> Vec<&TestMapping> {
        self.mappings
            .iter()
            .filter(|m| m.capability_id == capability_id)
            .collect()
    }

    /// 重新计算统计
    pub fn recalc_stats(&mut self) {
        let unique_caps: std::collections::HashSet<_> = self.mappings
            .iter()
            .map(|m| &m.capability_id)
            .collect();
        self.stats.mapped_capabilities = unique_caps.len() as u32;
        self.stats.passing_count = self.mappings
            .iter()
            .filter(|m| m.test_status == TestStatus::Passing)
            .count() as u32;
        self.stats.failing_count = self.mappings
            .iter()
            .filter(|m| m.test_status == TestStatus::Failing)
            .count() as u32;
        self.stats.missing_count = self.mappings
            .iter()
            .filter(|m| m.test_status == TestStatus::Missing)
            .count() as u32;
    }
}

// ─── P3+P4: Feedback 目录路径 ─────────────────────────────────────────

/// .Paporot/feedback/ 目录下的反馈文件
pub const FEEDBACK_DIR: &str = ".Paporot/feedback";
pub const REVIEWS_FILE: &str = "reviews.json";
pub const TESTMAP_FILE: &str = "testmap.json";


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_snapshot() {
        let snap = BehaviorSnapshot {
            schema_version: 3,
            version_id: "v1".into(),
            git_commit: Some("abc123".into()),
            git_ref: Some("main".into()),
            timestamp: "2026-06-11T10:00:00Z".into(),
            message: "Initial snapshot".into(),
            capabilities: vec![Capability {
                id: "cap_001".into(),
                name: "User Login".into(),
                description: "Email/password based login".into(),
                status: CapabilityStatus::New,
                module: Some("auth".into()),
                sub_modules: vec![],
                confidence: Some(0.95),
                evidence: vec!["src/auth/login.rs".into()],
                tags: vec!["security".into()],
                contract: None,
                preconditions: vec![],
                postconditions: vec![],
                invariants: vec![],
                categories: vec![CapabilityCategory::Security],
                depends_on: vec![],
                depended_by: vec![],
                evolved_from: None,
                evidence_trace_ids: vec![],
                verified_by: None,
                verified_at: None,
                source_change_type: None,
                triggered_by_rules: vec![],
            }],
            prd_coverage: PrdCoverage {
                percentage: 100.0,
                total_items: 1,
                covered_items: Some(1),
                details: vec![],
            },
            regression: None,
            risk: None,
            metadata: None,
        };

        let json = snap.to_json().unwrap();
        let parsed: BehaviorSnapshot = BehaviorSnapshot::from_json(&json).unwrap();
        assert_eq!(parsed.version_id, "v1");
        assert_eq!(parsed.schema_version, 3);
    }

    // ─── P3: BehaviorReview 序列化测试 ──────────────────────────────

    #[test]
    fn test_behavior_review_serialization() {
        let review = BehaviorReview {
            review_id: "rev_001".into(),
            capability_id: "cap_auth".into(),
            snapshot_version: "v1".into(),
            reviewer: "human-reviewer".into(),
            verdict: ReviewVerdict::Approved,
            comment: Some("Looks correct".into()),
            corrected: None,
            reviewed_at: "2026-06-11T10:00:00Z".into(),
            tags: vec!["verified".into()],
            triggered_by_rules: vec![],
            source_symbol: None,
            source_file: None,
            source_change_type: None,
        };
        let json = serde_json::to_string(&review).unwrap();
        let parsed: BehaviorReview = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.review_id, "rev_001");
        assert_eq!(parsed.verdict, ReviewVerdict::Approved);
        assert_eq!(parsed.comment, Some("Looks correct".into()));
    }

    #[test]
    fn test_review_verdict_serialization() {
        let verdicts = [
            (ReviewVerdict::Approved, "approved"),
            (ReviewVerdict::Rejected, "rejected"),
            (ReviewVerdict::Corrected, "corrected"),
            (ReviewVerdict::Flagged, "flagged"),
        ];
        for (v, expected) in &verdicts {
            let json = serde_json::to_string(v).unwrap();
            assert!(json.contains(expected), "verdict {:?} → {}", v, json);
            let back: ReviewVerdict = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, v);
        }
    }

    #[test]
    fn test_feedback_store_serialization() {
        let store = FeedbackStore {
            reviews: vec![
                BehaviorReview {
                    review_id: "r1".into(),
                    capability_id: "c1".into(),
                    snapshot_version: "v1".into(),
                    reviewer: "alice".into(),
                    verdict: ReviewVerdict::Approved,
                    comment: None, corrected: None,
                    reviewed_at: "t".into(), tags: vec![],
                    triggered_by_rules: vec![],
                    source_symbol: None,
                    source_file: None,
                    source_change_type: None,
                },
                BehaviorReview {
                    review_id: "r2".into(),
                    capability_id: "c1".into(),
                    snapshot_version: "v1".into(),
                    reviewer: "bob".into(),
                    verdict: ReviewVerdict::Rejected,
                    comment: Some("no".into()), corrected: None,
                    reviewed_at: "t".into(), tags: vec![],
                    triggered_by_rules: vec![],
                    source_symbol: None,
                    source_file: None,
                    source_change_type: None,
                },
            ],
            stats: FeedbackStats { total_reviews: 2, approved: 1, rejected: 1, corrected: 0, flagged: 0 },
        };
        let json = serde_json::to_string(&store).unwrap();
        let parsed: FeedbackStore = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.stats.total_reviews, 2);
        assert_eq!(parsed.stats.approved, 1);
        assert_eq!(parsed.stats.rejected, 1);
        assert_eq!(parsed.reviews.len(), 2);
    }

    // ─── P4: TestMapping 序列化测试 ─────────────────────────────────

    #[test]
    fn test_test_mapping_serialization() {
        let tm = TestMapping {
            map_id: "tmap_001".into(),
            capability_id: "cap_001".into(),
            test_file: "src/login_test.rs".into(),
            test_name: "test_login_success".into(),
            framework: Some("cargo-test".into()),
            test_status: TestStatus::Passing,
            confidence: 1.0,
            source: TestMappingSource::Manual,
            last_run_at: Some("2026-06-11T10:00:00Z".into()),
        };
        let json = serde_json::to_string(&tm).unwrap();
        let parsed: TestMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.map_id, "tmap_001");
        assert_eq!(parsed.test_status, TestStatus::Passing);
        assert_eq!(parsed.test_name, "test_login_success");
        assert_eq!(parsed.source, TestMappingSource::Manual);
    }

    #[test]
    fn test_test_status_serialization() {
        let statuses = [
            (TestStatus::Passing, "passing"),
            (TestStatus::Failing, "failing"),
            (TestStatus::Unknown, "unknown"),
            (TestStatus::Missing, "missing"),
        ];
        for (s, expected) in &statuses {
            let json = serde_json::to_string(s).unwrap();
            assert!(json.contains(expected), "status {:?} → {}", s, json);
            let back: TestStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, s);
        }
    }

    #[test]
    fn test_test_map_store_serialization() {
        let store = TestMapStore {
            mappings: vec![
                TestMapping {
                    map_id: "m1".into(), capability_id: "c1".into(),
                    test_file: "a_test.rs".into(), test_name: "test_a".into(),
                    framework: None, test_status: TestStatus::Passing,
                    confidence: 1.0, source: TestMappingSource::Manual,
                    last_run_at: None,
                },
            ],
            stats: TestMapStats { total_capabilities: 10, mapped_capabilities: 1, passing_count: 1, failing_count: 0, missing_count: 9 },
        };
        let json = serde_json::to_string(&store).unwrap();
        let parsed: TestMapStore = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.mappings.len(), 1);
        assert_eq!(parsed.stats.total_capabilities, 10);
        assert_eq!(parsed.stats.missing_count, 9);
    }

    // ─── CapabilityStatus 状态名称测试 ─────────────────────────────

    #[test]
    fn test_capability_status_display() {
        let c_new = make_cap("c1", CapabilityStatus::New);
        assert_eq!(c_new.status_name(), "新增");

        let c_mod = make_cap("c2", CapabilityStatus::Modified);
        assert_eq!(c_mod.status_name(), "修改");

        let c_del = make_cap("c3", CapabilityStatus::Deleted);
        assert_eq!(c_del.status_name(), "删除");

        let c_unch = make_cap("c4", CapabilityStatus::Unchanged);
        assert_eq!(c_unch.status_name(), "未变化");
    }

    fn make_cap(id: &str, status: CapabilityStatus) -> Capability {
        Capability {
            id: id.into(), name: String::new(), description: String::new(),
            status,
            module: None, sub_modules: vec![], confidence: Some(1.0),
            evidence: vec![], tags: vec![], contract: None,
            preconditions: vec![], postconditions: vec![], invariants: vec![],
            categories: vec![], depends_on: vec![], depended_by: vec![],
            evolved_from: None, evidence_trace_ids: vec![], verified_by: None, verified_at: None,
            source_change_type: None, triggered_by_rules: vec![],
        }
    }
}

// ───────────────────────────────
