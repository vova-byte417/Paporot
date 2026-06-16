//! Agent 调度层
//!
//! Paporot 的核心编排引擎。负责：
//! - 三层分析流水线 (L1 AST → L2 规则 → L3 LLM)
//! - JSON Schema 校验 + 自动重试
//! - Snapshot 生命周期管理
//! - Review 流水线编排
//! - 依赖图维护

use anyhow::{Context, Result};
use crate::analysis::preprocessor::DiffPreprocessor;
use crate::analysis::l1_ast::AstAnalyzer;
use crate::analysis::l2_rules::RuleEngine;
use crate::analysis::l3_llm_bridge::LlmBridge;
use crate::config::Config;
use crate::graph::GraphStorage;
use crate::llm::client::LlmClient;
use crate::prompts;
use crate::storage::SnapshotStorage;
use crate::types::*;

/// Paporot Agent — 核心调度器
pub struct Agent {
    pub config: Config,
    pub storage: SnapshotStorage,
    pub graph_storage: GraphStorage,
    llm: LlmClient,
}

impl Agent {
    /// 从配置创建 Agent
    pub fn new(config: Config) -> Self {
        let storage = SnapshotStorage::new(&config.storage.snapshots_dir);
        let graph_storage = GraphStorage::new(&config.storage.snapshots_dir);
        let llm = LlmClient::new(config.llm.clone());
        Self { config, storage, graph_storage, llm }
    }

    // ─── Snapshot Create (带 L1+L2+L3 分析流水线) ─────────────────────

    /// 使用三层分析流水线创建 Behavior Snapshot
    ///
    /// L1: AST/正则确定性提取 → L2: 规则引擎标注 → L3: LLM 补充语义
    pub async fn create_snapshot_with_analysis(
        &self,
        git_diff: &str,
        message: &str,
        _prd_content: Option<&str>,
        _prev_snapshot_summary: Option<&str>,
    ) -> Result<BehaviorSnapshot> {
        println!("  [Agent] Running L1+L2+L3 analysis pipeline...");

        // L1: 确定性解析
        println!("  [L1] AST/Pattern analysis...");
        let file_changes = DiffPreprocessor::parse(git_diff);
        let summary = DiffPreprocessor::summarize(&file_changes);
        println!("  [L1] {} files changed (+{}/-{})",
            summary.files_changed, summary.additions, summary.deletions);

        let l1_changes = AstAnalyzer::analyze(&file_changes)?;
        let l1_count = l1_changes.len();
        println!("  [L1] Extracted {} raw changes", l1_count);

        // L2: 规则引擎
        println!("  [L2] Rule evaluation...");
        let engine = RuleEngine::new();
        let l2_matches = engine.evaluate(&l1_changes);
        println!("  [L2] {} rule matches", l2_matches.len());

        // 统计 L1+L2 覆盖情况
        let high_conf: Vec<_> = l1_changes.iter()
            .filter(|c| c.confidence >= 0.5)
            .collect();
        let low_conf_count = l1_changes.len() - high_conf.len();

        // L3: LLM 处理低置信度部分
        let l3_capabilities = if low_conf_count > 0 || l1_count == 0 {
            println!("  [L3] LLM enhancement for {} low-conf changes...", low_conf_count);
            let bridge = LlmBridge::new(self.llm.clone());
            let low_conf: Vec<_> = l1_changes.iter()
                .filter(|c| c.confidence < 0.5)
                .cloned()
                .collect();
            let fragments = bridge.enhance(&low_conf, git_diff).await?;
            LlmBridge::merge_fragments(&fragments)
        } else {
            println!("  [L3] Skipped — L1+L2 fully covered");
            vec![]
        };

        // 组装 Capability 列表
        let mut capabilities = Self::l1_changes_to_capabilities(&high_conf, &l2_matches);
        capabilities.extend(l3_capabilities);

        // 构建 Snapshot
        let snapshot = BehaviorSnapshot {
            schema_version: 3,
            version_id: String::new(), // 外部填充
            git_commit: None,
            git_ref: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            message: message.to_string(),
            capabilities,
            prd_coverage: PrdCoverage {
                percentage: 0.0,
                total_items: 0,
                covered_items: None,
                details: vec![],
            },
            regression: None,
            risk: None,
            metadata: Some(serde_json::json!({
                "diff_length": git_diff.len(),
                "l1_changes": l1_count,
                "l2_matches": l2_matches.len(),
                "l3_fragments": 0,
            })),
        };

        println!("  [Agent] Generated {} capabilities", snapshot.capabilities.len());
        Ok(snapshot)
    }

    /// 将 L1 RawChange + L2 RuleMatch 转换为 Capability
    fn l1_changes_to_capabilities(
        changes: &[&crate::analysis::types::RawChange],
        matches: &[crate::analysis::types::RuleMatch],
    ) -> Vec<Capability> {
        changes.iter().map(|rc| {
            let matched_tags: Vec<_> = matches.iter()
                .filter(|m| m.raw_change_id == rc.id)
                .flat_map(|m| m.matched_tags.clone())
                .collect();

            let status = if rc.change_type.is_breaking() {
                CapabilityStatus::Modified
            } else {
                CapabilityStatus::New
            };

            Capability {
                id: format!("cap_{}", rc.id),
                name: format!("{}: {}", rc.change_type.label(), rc.symbol_name),
                description: format!("{} in {}", rc.change_type.label(), rc.file_path),
                status,
                module: rc.module.clone(),
                sub_modules: vec![],
                confidence: Some(rc.confidence),
                evidence: vec![format!("{}:{}", rc.file_path, rc.line_start)],
                tags: matched_tags,
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
            }
        }).collect()
    }

    // ─── Snapshot Create (LLM only, 兼容旧版) ────────────────────────

    /// 核心流程：从 git diff 创建 Behavior Snapshot（纯 LLM 模式）
    pub async fn create_snapshot(
        &self,
        git_diff: &str,
        message: &str,
        prd_content: Option<&str>,
        prev_snapshot_summary: Option<&str>,
    ) -> Result<BehaviorSnapshot> {
        println!("  [Agent] Extracting behaviors via LLM...");

        let diff = self.truncate_diff(git_diff);
        let system = prompts::SYSTEM_PROMPT_BEHAVIOR_EXTRACTOR;
        let user = prompts::build_extraction_prompt(
            &diff,
            None,
            prev_snapshot_summary,
            prd_content,
        );

        let response = self
            .llm
            .chat_with_retry(system, &user)
            .await
            .context("LLM behavior extraction failed")?;

        let mut snapshot: BehaviorSnapshot = serde_json::from_str(&response)
            .context("Failed to parse LLM response as BehaviorSnapshot")?;

        snapshot.schema_version = 3;
        snapshot.message = message.to_string();
        snapshot.timestamp = chrono::Utc::now().to_rfc3339();
        if snapshot.metadata.is_none() {
            snapshot.metadata = Some(serde_json::json!({}));
        }
        if let Some(obj) = snapshot.metadata.as_mut().and_then(|v| v.as_object_mut()) {
            obj.insert("diff_length".into(), serde_json::json!(git_diff.len()));
        }

        println!("  [Agent] Extracted {} capabilities", snapshot.capabilities.len());
        Ok(snapshot)
    }

    // ─── Diff ───────────────────────────────────────────────────────────

    /// 计算两个 snapshot 之间的行为差异
    /// 先做本地结构化 diff，再调 LLM 生成可读摘要
    pub fn compute_diff(&self, from: &BehaviorSnapshot, to: &BehaviorSnapshot) -> BehaviorDiff {
        let mut added = vec![];
        let mut modified = vec![];
        let mut deleted = vec![];
        let mut unchanged = vec![];

        let from_set: std::collections::HashSet<_> = from.capabilities.iter().map(|c| &c.id).collect();
        let to_set: std::collections::HashSet<_> = to.capabilities.iter().map(|c| &c.id).collect();

        for cap in &to.capabilities {
            if !from_set.contains(&cap.id) {
                added.push(cap.clone());
            } else if cap.status == CapabilityStatus::Modified {
                modified.push(cap.clone());
            } else {
                unchanged.push(cap.clone());
            }
        }

        for cap in &from.capabilities {
            if !to_set.contains(&cap.id) {
                deleted.push(cap.clone());
            }
        }

        // 构建风险与注意事项
        let mut risks = vec![];
        let breaking: Vec<_> = modified.iter().chain(deleted.iter()).collect();
        if !breaking.is_empty() {
            risks.push(format!(
                "{} 项修改/删除的能力可能引入兼容性问题",
                breaking.len()
            ));
        }
        if !added.is_empty() {
            risks.push(format!("{} 项新增能力需确认测试覆盖", added.len()));
        }

        let impact = format!(
            "新增 {} 项, 修改 {} 项, 删除 {} 项, 未变化 {} 项",
            added.len(),
            modified.len(),
            deleted.len(),
            unchanged.len()
        );

        BehaviorDiff {
            from_version: from.version_id.clone(),
            to_version: to.version_id.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            added,
            modified,
            deleted,
            unchanged,
            impact_summary: impact,
            risks_and_notes: risks,
        }
    }

    // ─── Coverage ───────────────────────────────────────────────────────

    /// 通过 LLM 计算 PRD 覆盖率
    pub async fn compute_coverage(
        &self,
        prd_content: &str,
        capabilities: &[Capability],
    ) -> Result<PrdCoverage> {
        println!("  [Agent] Computing PRD coverage via LLM...");

        let caps_json = serde_json::to_string_pretty(capabilities)
            .unwrap_or_else(|_| "[]".to_string());

        let system = prompts::SYSTEM_PROMPT_COVERAGE;
        let user = prompts::build_coverage_prompt(prd_content, &caps_json);

        let response = self
            .llm
            .chat_with_retry(system, &user)
            .await
            .context("LLM coverage computation failed")?;

        let coverage: PrdCoverage = serde_json::from_str(&response)
            .context("Failed to parse LLM coverage response")?;

        println!("  [Agent] PRD coverage: {:.1}%", coverage.percentage);
        Ok(coverage)
    }

    // ─── Regression ─────────────────────────────────────────────────────

    /// 通过 LLM 检测回归
    pub async fn detect_regressions(
        &self,
        prev_snapshot: &BehaviorSnapshot,
        curr_snapshot: &BehaviorSnapshot,
    ) -> Result<Regression> {
        println!("  [Agent] Detecting regressions via LLM...");

        let prev_json = serde_json::to_string_pretty(prev_snapshot).unwrap_or_default();
        let curr_json = serde_json::to_string_pretty(curr_snapshot).unwrap_or_default();

        let system = prompts::SYSTEM_PROMPT_REGRESSION_RISK;
        let user = prompts::build_regression_risk_prompt(&prev_json, &curr_json);

        let response = self
            .llm
            .chat_with_retry(system, &user)
            .await
            .context("LLM regression detection failed")?;

        // 尝试从复合响应中分别提取 regression 和 risk
        let parsed: serde_json::Value = serde_json::from_str(&response)?;

        let regression: Regression = if let Some(r) = parsed.get("regression") {
            serde_json::from_value(r.clone())?
        } else {
            serde_json::from_value(parsed.clone())?
        };

        println!(
            "  [Agent] Regression status: {:?}, {} item(s)",
            regression.status,
            regression.detected_regressions.len()
        );
        Ok(regression)
    }

    // ─── Risk ───────────────────────────────────────────────────────────

    /// 通过 LLM 评估风险
    pub async fn assess_risk(
        &self,
        snapshot: &BehaviorSnapshot,
        prev_snapshot: Option<&BehaviorSnapshot>,
    ) -> Result<RiskAssessment> {
        println!("  [Agent] Assessing risk via LLM...");

        let curr_json = serde_json::to_string_pretty(snapshot).unwrap_or_default();

        let prev_json = prev_snapshot
            .map(|s| serde_json::to_string_pretty(s).unwrap_or_default())
            .unwrap_or_else(|| "null".to_string());

        let system = prompts::SYSTEM_PROMPT_REGRESSION_RISK;
        let user = prompts::build_regression_risk_prompt(&prev_json, &curr_json);

        let response = self
            .llm
            .chat_with_retry(system, &user)
            .await
            .context("LLM risk assessment failed")?;

        let parsed: serde_json::Value = serde_json::from_str(&response)?;

        let risk: RiskAssessment = if let Some(r) = parsed.get("risk") {
            serde_json::from_value(r.clone())?
        } else {
            serde_json::from_value(parsed.clone())?
        };

        println!(
            "  [Agent] Risk: {:?} (score: {}/100)",
            risk.level, risk.score
        );
        Ok(risk)
    }

    // ─── Review Pipeline ────────────────────────────────────────────────

    /// 一体化审查流水线
    pub async fn review_pipeline(
        &self,
        git_diff: &str,
        message: &str,
        prd_content: Option<&str>,
    ) -> Result<BehaviorSnapshot> {
        println!("═══ Paporot Review Pipeline ═══\n");

        // [1/5] Snapshot Create
        println!("[1/5] Extracting behaviors...");
        let prev_summary = self.storage.list_versions_sorted().ok()
            .and_then(|v| v.last().cloned());
        let mut snapshot = self
            .create_snapshot(git_diff, message, prd_content, prev_summary.as_deref())
            .await?;
        let version_id = self.storage.next_version_id()?;
        snapshot.version_id = version_id.clone();
        self.storage.save(&snapshot)?;
        println!("  ✓ Snapshot {} saved\n", version_id);

        // [2/5] Diff
        println!("[2/5] Computing behavior diff...");
        if let Some(ref prev_id) = prev_summary {
            match self.storage.load_by_version(&prev_id) {
                Ok(prev_snap) => {
                    let diff = self.compute_diff(&prev_snap, &snapshot);
                    print_markdown_diff(&diff);
                }
                Err(e) => println!("  ! Cannot load previous snapshot: {}\n", e),
            }
        } else {
            println!("  - First snapshot, no diff to compute\n");
        }

        // [3/5] Coverage
        println!("[3/5] Computing PRD coverage...");
        if let Some(prd) = prd_content {
            match self.compute_coverage(prd, &snapshot.capabilities).await {
                Ok(coverage) => {
                    snapshot.prd_coverage = coverage;
                    // Update saved snapshot with coverage info
                    self.storage.save(&snapshot)?;
                    println!("  ✓ Coverage: {:.1}%\n", snapshot.prd_coverage.percentage);
                }
                Err(e) => println!("  ! Coverage failed: {}\n", e),
            }
        } else {
            println!("  - No PRD provided\n");
        }

        // [4/5] Regression
        println!("[4/5] Detecting regressions...");
        if let Some(prev_id) = &prev_summary {
            match self.storage.load_by_version(prev_id) {
                Ok(prev_snap) => {
                    match self.detect_regressions(&prev_snap, &snapshot).await {
                        Ok(regression) => {
                            snapshot.regression = Some(regression);
                            self.storage.save(&snapshot)?;
                            println!("  ✓ Regression analysis complete\n");
                        }
                        Err(e) => println!("  ! Regression detection failed: {}\n", e),
                    }
                }
                Err(e) => println!("  ! Cannot load previous: {}\n", e),
            }
        } else {
            println!("  - First snapshot, no regression analysis\n");
        }

        // [5/5] Risk
        println!("[5/5] Assessing risk...");
        let prev_for_risk = prev_summary
            .as_deref()
            .and_then(|id| self.storage.load_by_version(id).ok());
        match self.assess_risk(&snapshot, prev_for_risk.as_ref()).await {
            Ok(risk) => {
                snapshot.risk = Some(risk);
                self.storage.save(&snapshot)?;
                println!("  ✓ Risk assessment complete\n");
            }
            Err(e) => println!("  ! Risk assessment failed: {}\n", e),
        }

        // 更新依赖图索引
        let _ = self.graph_storage.init();
        match self.graph_storage.load().and_then(|mut graph| {
            self.graph_storage.update_from_snapshot(&mut graph, &snapshot)?;
            self.graph_storage.save(&graph)
        }) {
            Ok(()) => println!("  ✓ Dependency graph updated\n"),
            Err(e) => eprintln!("  ! Graph update failed: {}\n", e),
        }

        println!("═══ Review Complete ═══");
        Ok(snapshot)
    }

    // ─── Helpers ────────────────────────────────────────────────────────

    /// 截断过大的 diff
    fn truncate_diff(&self, diff: &str) -> String {
        let threshold = self.config.agent.diff_truncate_threshold;
        if diff.len() > threshold {
            eprintln!(
                "  [Agent] Diff too large ({} bytes), truncating to first {} bytes",
                diff.len(),
                threshold
            );
            diff[..threshold].to_string()
        } else {
            diff.to_string()
        }
    }
}

/// 以 Markdown 格式输出 BehaviorDiff
pub fn print_markdown_diff(diff: &BehaviorDiff) {
    println!("# Behavior Diff: {} → {}", diff.from_version, diff.to_version);
    println!();

    if !diff.added.is_empty() {
        println!("## 新增能力 ({})", diff.added.len());
        for cap in &diff.added {
            println!("- **{}**: {}", cap.name, cap.description);
        }
        println!();
    }

    if !diff.modified.is_empty() {
        println!("## 修改能力 ({})", diff.modified.len());
        for cap in &diff.modified {
            println!("- **{}**: {}", cap.name, cap.description);
        }
        println!();
    }

    if !diff.deleted.is_empty() {
        println!("## 删除能力 ({})", diff.deleted.len());
        for cap in &diff.deleted {
            println!("- **{}**: {}", cap.name, cap.description);
        }
        println!();
    }

    if !diff.unchanged.is_empty() {
        println!("## 未变化能力 ({})", diff.unchanged.len());
        for cap in &diff.unchanged {
            println!("- **{}**: {}", cap.name, cap.description);
        }
        println!();
    }

    println!("## 影响范围");
    println!("{}", diff.impact_summary);
    println!();

    if !diff.risks_and_notes.is_empty() {
        println!("## 风险与注意事项");
        for note in &diff.risks_and_notes {
            println!("- {}", note);
        }
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::types::*;

    // ─── compute_diff 接口测试 ─────────────────────────────────────

    fn make_cap(id: &str, name: &str, status: CapabilityStatus) -> Capability {
        Capability {
            id: id.into(), name: name.into(), description: String::new(),
            status, module: None, sub_modules: vec![], confidence: Some(1.0),
            evidence: vec![], tags: vec![], contract: None,
            preconditions: vec![], postconditions: vec![], invariants: vec![],
            categories: vec![], depends_on: vec![], depended_by: vec![],
            evolved_from: None, evidence_trace_ids: vec![], verified_by: None, verified_at: None,
        }
    }

    fn make_snap(version: &str, caps: Vec<Capability>) -> BehaviorSnapshot {
        BehaviorSnapshot {
            schema_version: 3,
            version_id: version.into(),
            git_commit: None, git_ref: None,
            timestamp: "2026-01-01T00:00:00Z".into(),
            message: String::new(),
            capabilities: caps,
            prd_coverage: PrdCoverage { percentage: 0.0, total_items: 0, covered_items: None, details: vec![] },
            regression: None, risk: None, metadata: None,
        }
    }

    /// 测试项: compute_diff 新增能力识别
    /// 输入: from 无此能力, to 有此能力 (status=New)
    /// 预期: diff.added 包含该能力
    #[test]
    fn test_compute_diff_detects_added() {
        let config = Config::default();
        let agent = Agent::new(config);
        let from = make_snap("v1", vec![]);
        let to = make_snap("v2", vec![make_cap("cap_001", "NewFeature", CapabilityStatus::New)]);
        let diff = agent.compute_diff(&from, &to);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].name, "NewFeature");
        assert!(diff.deleted.is_empty());
        assert!(diff.modified.is_empty());
    }

    /// 测试项: compute_diff 删除能力识别
    /// 输入: from 有, to 无
    /// 预期: diff.deleted 包含该能力
    #[test]
    fn test_compute_diff_detects_deleted() {
        let config = Config::default();
        let agent = Agent::new(config);
        let from = make_snap("v1", vec![make_cap("cap_001", "OldFeature", CapabilityStatus::New)]);
        let to = make_snap("v2", vec![]);
        let diff = agent.compute_diff(&from, &to);
        assert_eq!(diff.deleted.len(), 1);
        assert_eq!(diff.deleted[0].id, "cap_001");
        assert!(diff.added.is_empty());
    }

    /// 测试项: compute_diff 修改能力识别
    /// 输入: from 和 to 都有同一 id, to 状态为 Modified
    /// 预期: diff.modified 包含该能力
    #[test]
    fn test_compute_diff_detects_modified() {
        let config = Config::default();
        let agent = Agent::new(config);
        let from = make_snap("v1", vec![make_cap("cap_001", "Feature", CapabilityStatus::New)]);
        let to = make_snap("v2", vec![make_cap("cap_001", "Feature", CapabilityStatus::Modified)]);
        let diff = agent.compute_diff(&from, &to);
        assert_eq!(diff.modified.len(), 1);
        assert!(diff.added.is_empty());
        assert!(diff.deleted.is_empty());
    }

    /// 测试项: compute_diff 未变化能力识别
    /// 输入: from 和 to 都有同一 id, to 状态为 Unchanged
    /// 预期: diff.unchanged 包含该能力
    #[test]
    fn test_compute_diff_unchanged() {
        let config = Config::default();
        let agent = Agent::new(config);
        let cap = make_cap("cap_001", "StableFeature", CapabilityStatus::Unchanged);
        let from = make_snap("v1", vec![cap.clone()]);
        let to = make_snap("v2", vec![cap]);
        let diff = agent.compute_diff(&from, &to);
        assert_eq!(diff.unchanged.len(), 1);
    }

    /// 测试项: compute_diff 混合场景（新增+删除+修改+未变化）
    /// 输入: 4 个能力覆盖全部状态
    /// 预期: 各分类计数精确
    #[test]
    fn test_compute_diff_mixed_scenario() {
        let config = Config::default();
        let agent = Agent::new(config);
        let from = make_snap("v1", vec![
            make_cap("cap_keep", "Keep", CapabilityStatus::New),
            make_cap("cap_mod", "Modify", CapabilityStatus::New),
            make_cap("cap_del", "Delete", CapabilityStatus::New),
        ]);
        let to = make_snap("v2", vec![
            make_cap("cap_keep", "Keep", CapabilityStatus::Unchanged),
            make_cap("cap_mod", "Modify", CapabilityStatus::Modified),
            make_cap("cap_new", "NewOne", CapabilityStatus::New),
        ]);
        let diff = agent.compute_diff(&from, &to);
        assert_eq!(diff.added.len(), 1, "应新增 1 个");
        assert_eq!(diff.modified.len(), 1, "应修改 1 个");
        assert_eq!(diff.deleted.len(), 1, "应删除 1 个");
        assert_eq!(diff.unchanged.len(), 1, "应有 1 个未变");
        assert!(diff.impact_summary.contains("新增 1"));
        assert!(diff.impact_summary.contains("修改 1"));
        assert!(diff.impact_summary.contains("删除 1"));
    }

    /// 测试项: compute_diff 产出风险提示
    /// 输入: 含删除和新增的场景
    /// 预期: risks_and_notes 包含兼容性和测试覆盖提示
    #[test]
    fn test_compute_diff_produces_risks() {
        let config = Config::default();
        let agent = Agent::new(config);
        let from = make_snap("v1", vec![make_cap("cap_del", "Deleted", CapabilityStatus::New)]);
        let to = make_snap("v2", vec![make_cap("cap_new", "New", CapabilityStatus::New)]);
        let diff = agent.compute_diff(&from, &to);
        assert!(diff.risks_and_notes.iter().any(|r| r.contains("兼容性")));
        assert!(diff.risks_and_notes.iter().any(|r| r.contains("测试覆盖")));
    }

    // ─── truncate_diff 接口测试 ───────────────────────────────────

    /// 测试项: 短 diff 不截断
    /// 输入: 小于阈值的 diff
    /// 预期: 原样返回
    #[test]
    fn test_truncate_diff_short() {
        let mut config = Config::default();
        config.agent.diff_truncate_threshold = 1000;
        let agent = Agent::new(config);
        let result = agent.truncate_diff("short diff content");
        assert_eq!(result, "short diff content");
    }

    /// 测试项: 长 diff 截断
    /// 输入: 超过阈值的 diff
    /// 预期: 截断到阈值长度
    #[test]
    fn test_truncate_diff_long() {
        let mut config = Config::default();
        config.agent.diff_truncate_threshold = 10;
        let agent = Agent::new(config);
        let long = "a".repeat(50);
        let result = agent.truncate_diff(&long);
        assert_eq!(result.len(), 10);
        assert!(result.starts_with('a'));
    }

    /// 测试项: 正好等于阈值
    /// 输入: 长度 = threshold
    /// 预期: 不截断
    #[test]
    fn test_truncate_diff_exact_threshold() {
        let mut config = Config::default();
        config.agent.diff_truncate_threshold = 8;
        let agent = Agent::new(config);
        let result = agent.truncate_diff("12345678");
        assert_eq!(result, "12345678");
    }

    // ─── l1_changes_to_capabilities 接口测试 ──────────────────────

    fn make_raw_change(id: &str, name: &str, ct: ChangeType, conf: f32) -> RawChange {
        RawChange {
            id: id.into(), source: ChangeSource::Ast, change_type: ct,
            file_path: "src/lib.rs".into(), language: Language::Rust,
            line_start: 1, line_end: 1, symbol_name: name.into(),
            old_signature: None, new_signature: None,
            confidence: conf, module: Some("test".into()), tags: vec![],
        }
    }

    /// 测试项: 新增函数转为 New 状态 Capability
    /// 输入: FunctionAdded 类型 RawChange
    /// 预期: status=New
    #[test]
    fn test_l1_changes_to_cap_new_fn() {
        let raw = vec![make_raw_change("rc1", "new_func", ChangeType::FunctionAdded, 0.9)];
        let refs: Vec<&RawChange> = raw.iter().collect();
        let caps = Agent::l1_changes_to_capabilities(&refs, &[]);
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].status, CapabilityStatus::New);
        assert_eq!(caps[0].name, "函数新增: new_func");
        assert_eq!(caps[0].confidence, Some(0.9));
    }

    /// 测试项: 破坏性变更为 Modified 状态
    /// 输入: FunctionRemoved 类型
    /// 预期: status=Modified
    #[test]
    fn test_l1_changes_to_cap_breaking() {
        let raw = vec![make_raw_change("rc1", "old_fn", ChangeType::FunctionRemoved, 0.8)];
        let refs: Vec<&RawChange> = raw.iter().collect();
        let caps = Agent::l1_changes_to_capabilities(&refs, &[]);
        assert_eq!(caps[0].status, CapabilityStatus::Modified);
    }

    /// 测试项: L2 规则标签附加到 Capability.tags
    /// 输入: RawChange + 对应的 RuleMatch（含 matched_tags）
    /// 预期: Capability.tags 包含规则标签
    #[test]
    fn test_l1_changes_tags_from_l2() {
        let raw = vec![make_raw_change("rc1", "login", ChangeType::FunctionAdded, 1.0)];
        let matches = vec![RuleMatch {
            rule_id: "sec_auth_001".into(),
            raw_change_id: "rc1".into(),
            matched_tags: vec!["authentication".into(), "security".into()],
            severity: Severity::High,
            category: RuleCategory::Security,
            description: String::new(),
        }];
        let refs: Vec<&RawChange> = raw.iter().collect();
        let caps = Agent::l1_changes_to_capabilities(&refs, &matches);
        assert!(caps[0].tags.contains(&"authentication".to_string()));
        assert!(caps[0].tags.contains(&"security".to_string()));
    }

    /// 测试项: 空输入不崩溃
    /// 输入: 空 changes + 空 matches
    /// 预期: 返回空 Vec
    #[test]
    fn test_l1_changes_empty_input() {
        let caps = Agent::l1_changes_to_capabilities(&[], &[]);
        assert!(caps.is_empty());
    }

    /// 测试项: 多变更批量转换
    /// 输入: 3 个 RawChange
    /// 预期: 返回 3 个 Capability
    #[test]
    fn test_l1_changes_multiple() {
        let raw = vec![
            make_raw_change("r1", "fn_a", ChangeType::FunctionAdded, 1.0),
            make_raw_change("r2", "fn_b", ChangeType::StructAdded, 0.9),
            make_raw_change("r3", "fn_c", ChangeType::ConstantAdded, 0.8),
        ];
        let refs: Vec<&RawChange> = raw.iter().collect();
        let caps = Agent::l1_changes_to_capabilities(&refs, &[]);
        assert_eq!(caps.len(), 3);
        // 每个有唯一 id
        assert_eq!(caps[0].id, "cap_r1");
        assert_eq!(caps[1].id, "cap_r2");
        assert_eq!(caps[2].id, "cap_r3");
    }
}
