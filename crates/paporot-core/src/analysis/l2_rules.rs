//! L2 规则引擎：对 L1 产出的 RawChange 做语义规则匹配
//!
//! 检测安全影响、破坏性变更、性能风险等。全确定性，不调用 LLM。
//!
//! 对应 PRD P0 §3.3

use crate::types::*;
use crate::types::Severity;

/// 规则引擎
pub struct RuleEngine {
    rules: Vec<Rule>,
}

impl RuleEngine {
    /// 创建包含内置规则集的引擎
    pub fn new() -> Self {
        Self {
            rules: Self::builtin_rules(),
        }
    }

    /// 评估所有规则，返回命中列表
    pub fn evaluate(&self, raw_changes: &[RawChange]) -> Vec<RuleMatch> {
        let mut matches = Vec::new();

        for rule in &self.rules {
            for rc in raw_changes {
                if Self::trigger_matches(&rule.trigger, rc) {
                    matches.push(RuleMatch {
                        rule_id: rule.id.clone(),
                        raw_change_id: rc.id.clone(),
                        matched_tags: rule.tags.clone(),
                        severity: rule.severity.clone(),
                        category: rule.category.clone(),
                        description: rule.description.clone(),
                    });
                }
            }
        }

        matches
    }

    /// 检查单个 RuleTrigger 是否命中 RawChange
    fn trigger_matches(trigger: &RuleTrigger, rc: &RawChange) -> bool {
        match trigger {
            RuleTrigger::SymbolMatches { pattern } => {
                rc.symbol_name.to_lowercase().contains(&pattern.to_lowercase())
            }
            RuleTrigger::ChangeTypeIn(types) => {
                types.contains(&rc.change_type)
            }
            RuleTrigger::FilePathMatches { pattern } => {
                let path = rc.file_path.to_lowercase();
                let p = pattern.to_lowercase();
                if p.contains('*') {
                    let re_pattern = format!("^{}$", regex::escape(&p).replace("\\*", ".*"));
                    regex::Regex::new(&re_pattern)
                        .map(|re| re.is_match(&path))
                        .unwrap_or(false)
                } else {
                    path.contains(&p)
                }
            }
            RuleTrigger::ContentContains { pattern } => {
                let sig = rc.new_signature.as_deref().unwrap_or(&rc.symbol_name);
                sig.to_lowercase().contains(&pattern.to_lowercase())
            }
            RuleTrigger::And(a, b) => {
                Self::trigger_matches(a, rc) && Self::trigger_matches(b, rc)
            }
            RuleTrigger::Or(a, b) => {
                Self::trigger_matches(a, rc) || Self::trigger_matches(b, rc)
            }
            RuleTrigger::Not(inner) => {
                !Self::trigger_matches(inner, rc)
            }
        }
    }

    /// 内置规则集
    fn builtin_rules() -> Vec<Rule> {
        vec![
            // ── 安全规则 ──
            Rule {
                id: "sec_auth_001".into(),
                name: "认证逻辑变更".into(),
                category: RuleCategory::Security,
                severity: Severity::High,
                trigger: RuleTrigger::Or(
                    Box::new(RuleTrigger::SymbolMatches { pattern: "auth".into() }),
                    Box::new(RuleTrigger::SymbolMatches { pattern: "login".into() }),
                ),
                tags: vec!["security".into(), "authentication".into()],
                description: "认证相关函数被修改，可能影响登录安全".into(),
            },
            Rule {
                id: "sec_auth_002".into(),
                name: "权限/guard 变更".into(),
                category: RuleCategory::Security,
                severity: Severity::High,
                trigger: RuleTrigger::Or(
                    Box::new(RuleTrigger::SymbolMatches { pattern: "guard".into() }),
                    Box::new(RuleTrigger::SymbolMatches { pattern: "permission".into() }),
                ),
                tags: vec!["security".into(), "authorization".into()],
                description: "权限检查逻辑被修改，可能引入越权风险".into(),
            },
            Rule {
                id: "sec_crypto_001".into(),
                name: "加密/哈希相关变更".into(),
                category: RuleCategory::Security,
                severity: Severity::High,
                trigger: RuleTrigger::Or(
                    Box::new(RuleTrigger::SymbolMatches { pattern: "hash".into() }),
                    Box::new(RuleTrigger::Or(
                        Box::new(RuleTrigger::SymbolMatches { pattern: "encrypt".into() }),
                        Box::new(RuleTrigger::SymbolMatches { pattern: "decrypt".into() }),
                    )),
                ),
                tags: vec!["security".into(), "crypto".into()],
                description: "加密或哈希函数被修改".into(),
            },
            Rule {
                id: "sec_sql_001".into(),
                name: "SQL 查询变更".into(),
                category: RuleCategory::Security,
                severity: Severity::High,
                trigger: RuleTrigger::ContentContains { pattern: "sql".into() },
                tags: vec!["security".into(), "sql".into()],
                description: "SQL 相关代码变更，需检查是否有注入风险".into(),
            },
            Rule {
                id: "sec_token_001".into(),
                name: "Token/JWT/Session 变更".into(),
                category: RuleCategory::Security,
                severity: Severity::High,
                trigger: RuleTrigger::Or(
                    Box::new(RuleTrigger::SymbolMatches { pattern: "token".into() }),
                    Box::new(RuleTrigger::Or(
                        Box::new(RuleTrigger::SymbolMatches { pattern: "jwt".into() }),
                        Box::new(RuleTrigger::SymbolMatches { pattern: "session".into() }),
                    )),
                ),
                tags: vec!["security".into(), "session".into()],
                description: "Token 或会话管理逻辑变更".into(),
            },

            // ── 破坏性变更规则 ──
            Rule {
                id: "breaking_001".into(),
                name: "公开 API 删除".into(),
                category: RuleCategory::Breaking,
                severity: Severity::High,
                trigger: RuleTrigger::And(
                    Box::new(RuleTrigger::ChangeTypeIn(vec![
                        ChangeType::FunctionRemoved,
                        ChangeType::HttpRouteRemoved,
                    ])),
                    Box::new(RuleTrigger::Not(Box::new(RuleTrigger::SymbolMatches { pattern: "test".into() }))),
                ),
                tags: vec!["breaking".into()],
                description: "公开接口被删除，下游可能崩溃".into(),
            },
            Rule {
                id: "breaking_002".into(),
                name: "参数签名变更".into(),
                category: RuleCategory::Breaking,
                severity: Severity::High,
                trigger: RuleTrigger::ChangeTypeIn(vec![ChangeType::FunctionSignatureChanged]),
                tags: vec!["breaking".into(), "api".into()],
                description: "函数签名改变，调用方需要同步更新".into(),
            },
            Rule {
                id: "breaking_003".into(),
                name: "数据结构字段删除".into(),
                category: RuleCategory::Breaking,
                severity: Severity::Medium,
                trigger: RuleTrigger::ChangeTypeIn(vec![ChangeType::StructFieldRemoved]),
                tags: vec!["breaking".into(), "schema".into()],
                description: "结构体/类字段被删除，序列化可能不兼容".into(),
            },
            Rule {
                id: "breaking_004".into(),
                name: "枚举变体删除".into(),
                category: RuleCategory::Breaking,
                severity: Severity::High,
                trigger: RuleTrigger::ChangeTypeIn(vec![ChangeType::EnumVariantRemoved]),
                tags: vec!["breaking".into(), "exhaustiveness".into()],
                description: "枚举变体被删除会导致 match 穷尽性破坏".into(),
            },
            Rule {
                id: "breaking_005".into(),
                name: "常量/配置删除".into(),
                category: RuleCategory::Breaking,
                severity: Severity::Medium,
                trigger: RuleTrigger::ChangeTypeIn(vec![ChangeType::ConstantRemoved]),
                tags: vec!["breaking".into(), "config".into()],
                description: "常量或配置项被删除".into(),
            },

            // ── 性能规则 ──
            Rule {
                id: "perf_001".into(),
                name: "文件路径:数据库/SQL".into(),
                category: RuleCategory::Performance,
                severity: Severity::Medium,
                trigger: RuleTrigger::FilePathMatches { pattern: "*.sql".into() },
                tags: vec!["performance".into(), "database".into()],
                description: "数据库查询发生变更，检查是否需要添加索引".into(),
            },
            Rule {
                id: "perf_002".into(),
                name: "数据库 schema 变更".into(),
                category: RuleCategory::Performance,
                severity: Severity::Medium,
                trigger: RuleTrigger::Or(
                    Box::new(RuleTrigger::FilePathMatches { pattern: "*migration*".into() }),
                    Box::new(RuleTrigger::FilePathMatches { pattern: "*schema*".into() }),
                ),
                tags: vec!["performance".into(), "database".into(), "migration".into()],
                description: "数据库 schema 迁移，检查是否需要数据迁移脚本".into(),
            },

            // ── 弃用/维护规则 ──
            Rule {
                id: "deprec_001".into(),
                name: "deprecated 标注".into(),
                category: RuleCategory::Deprecation,
                severity: Severity::Low,
                trigger: RuleTrigger::ContentContains { pattern: "deprecat".into() },
                tags: vec!["deprecation".into()],
                description: "包含 deprecated 标注".into(),
            },
            Rule {
                id: "deprec_002".into(),
                name: "TODO/FIXME 标注".into(),
                category: RuleCategory::Deprecation,
                severity: Severity::Low,
                trigger: RuleTrigger::Or(
                    Box::new(RuleTrigger::ContentContains { pattern: "todo".into() }),
                    Box::new(RuleTrigger::ContentContains { pattern: "fixme".into() }),
                ),
                tags: vec!["tech_debt".into()],
                description: "包含 TODO/FIXME 标注".into(),
            },

            // ── 测试规则 ──
            Rule {
                id: "misc_test_001".into(),
                name: "测试代码变更".into(),
                category: RuleCategory::Domain,
                severity: Severity::Low,
                trigger: RuleTrigger::Or(
                    Box::new(RuleTrigger::FilePathMatches { pattern: "*test*".into() }),
                    Box::new(RuleTrigger::FilePathMatches { pattern: "*_test.*".into() }),
                ),
                tags: vec!["test".into(), "non-functional".into()],
                description: "测试代码变更".into(),
            },
        ]
    }
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rc(id: &str, name: &str, ct: ChangeType, path: &str) -> RawChange {
        RawChange {
            id: id.into(),
            source: ChangeSource::Ast,
            change_type: ct,
            file_path: path.into(),
            language: Language::Rust,
            line_start: 1,
            line_end: 1,
            symbol_name: name.into(),
            old_signature: None,
            new_signature: Some(format!("fn {}()", name)),
            confidence: 1.0,
            module: None,
            tags: vec![],
        }
    }

    #[test]
    fn test_auth_rule_hits() {
        let engine = RuleEngine::new();
        let rc = make_rc("rc_1", "login", ChangeType::FunctionAdded, "src/auth.rs");
        let matches = engine.evaluate(&[rc]);
        assert!(matches.iter().any(|m| m.rule_id == "sec_auth_001"));
    }

    #[test]
    fn test_breaking_rule_hits() {
        let engine = RuleEngine::new();
        let rc = make_rc("rc_2", "old_function", ChangeType::FunctionRemoved, "src/lib.rs");
        let matches = engine.evaluate(&[rc]);
        assert!(matches.iter().any(|m| m.rule_id == "breaking_001"));
    }

    #[test]
    fn test_test_file_is_tagged() {
        let engine = RuleEngine::new();
        let rc = make_rc("rc_3", "test_something", ChangeType::FunctionAdded, "src/lib_test.rs");
        let matches = engine.evaluate(&[rc]);
        assert!(matches.iter().any(|m| m.rule_id == "misc_test_001"));
    }

    #[test]
    fn test_normal_fn_not_flagged() {
        let engine = RuleEngine::new();
        let rc = make_rc("rc_4", "calculate_total", ChangeType::FunctionAdded, "src/utils.rs");
        let matches = engine.evaluate(&[rc]);
        // 不应命中 auth 规则
        assert!(!matches.iter().any(|m| m.rule_id == "sec_auth_001"));
    }
}
