//! Capability Evidence 数据类型。
//!
//! 定义 L1 AST → L2 Rules → L3 LLM 证据链的数据结构。

use serde::{Deserialize, Serialize};

// ─── Evidence ──────────────────────────────────────────────────────

/// 一个 Capability 的完整推断证据。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Evidence {
    /// 关联的 Capability ID
    pub capability_id: String,
    /// 关联的 Snapshot version ID
    pub snapshot_version: String,
    /// L1 AST 证据
    pub l1: Vec<L1Evidence>,
    /// L2 规则匹配证据
    pub l2: Vec<L2Evidence>,
    /// L3 LLM 证据（None 表示未配置 L3 provider）
    pub l3: Option<L3Evidence>,
    /// 三层置信度评分
    pub confidence: EvidenceConfidence,
    /// 证据生成时间
    pub generated_at: String,
}

// ─── L1Evidence ────────────────────────────────────────────────────

/// L1 AST 层面的符号提取证据。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct L1Evidence {
    /// 符号名称
    pub symbol: String,
    /// 所在文件
    pub file_path: String,
    /// 行号
    pub line: usize,
    /// 符号类型
    pub kind: SymbolKind,
    /// 可见性
    pub visibility: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Implementation,
    Module,
}

// ─── L2Evidence ────────────────────────────────────────────────────

/// L2 规则匹配证据。
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct L2Evidence {
    /// 匹配的规则 ID
    pub rule_id: String,
    /// 规则名称
    pub rule_name: String,
    /// 被匹配的 L1 符号
    pub matched_symbol: String,
    /// 触发的文件变更
    pub file_change: String,
    /// 匹配原因描述
    pub reason: String,
    /// 严重程度: critical / high / medium / low
    pub severity: String,
}

// ─── L3Evidence ────────────────────────────────────────────────────

/// L3 LLM 推断证据。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct L3Evidence {
    /// LLM prompt 的 hash
    pub prompt_hash: String,
    /// LLM 输出的推断片段
    pub fragment: String,
    /// LLM 模型名称
    pub model: String,
    /// LLM 调用时间
    pub timestamp: String,
}

// ─── EvidenceConfidence ────────────────────────────────────────────

/// 三层独立置信度评分。
///
/// 初期：各层独立评分（0.0–1.0）。
/// 后期：积累数据后切换为自适应加权公式。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvidenceConfidence {
    /// L1 AST 证据可信度
    pub l1_score: f64,
    /// L2 规则匹配可信度
    pub l2_score: f64,
    /// L3 LLM 推断可信度（None 表示无 L3）
    pub l3_score: Option<f64>,
}

impl Default for EvidenceConfidence {
    fn default() -> Self {
        Self {
            l1_score: 0.0,
            l2_score: 0.0,
            l3_score: None,
        }
    }
}

// ─── EvidenceHash ──────────────────────────────────────────────────

/// Snapshot 创建时记录的轻量证据 hash。
///
/// 用于事后 `evidence generate` 时校验证据是否与 snapshot 创建时一致。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvidenceHash {
    pub l1_hash: String,
    pub l2_hash: String,
    pub l3_hash: Option<String>,
}

// ─── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evidence_serde_roundtrip() {
        let evidence = Evidence {
            capability_id: "cap_001".into(),
            snapshot_version: "v1".into(),
            l1: vec![L1Evidence {
                symbol: "login".into(),
                file_path: "src/auth.rs".into(),
                line: 42,
                kind: SymbolKind::Function,
                visibility: "pub".into(),
            }],
            l2: vec![L2Evidence {
                rule_id: "r001".into(),
                rule_name: "auth_pattern".into(),
                matched_symbol: "login".into(),
                file_change: "src/auth.rs".into(),
                reason: "detects auth entry point".into(),
                severity: "high".into(),
            }],
            l3: Some(L3Evidence {
                prompt_hash: "abc123".into(),
                fragment: "email/password authentication".into(),
                model: "deepseek-chat".into(),
                timestamp: "2026-06-12T14:00:00Z".into(),
            }),
            confidence: EvidenceConfidence {
                l1_score: 0.85,
                l2_score: 0.72,
                l3_score: Some(0.90),
            },
            generated_at: "2026-06-12T14:00:00Z".into(),
        };

        let json = serde_json::to_string(&evidence).unwrap();
        let decoded: Evidence = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.capability_id, "cap_001");
        assert_eq!(decoded.l1.len(), 1);
        assert_eq!(decoded.l2.len(), 1);
        assert!(decoded.l3.is_some());
        assert_eq!(decoded.confidence.l1_score, 0.85);
    }

    #[test]
    fn test_evidence_without_l3() {
        let evidence = Evidence {
            capability_id: "cap_002".into(),
            snapshot_version: "v1".into(),
            l1: vec![],
            l2: vec![],
            l3: None,
            confidence: EvidenceConfidence::default(),
            generated_at: "2026-06-12T14:00:00Z".into(),
        };

        let json = serde_json::to_string(&evidence).unwrap();
        let decoded: Evidence = serde_json::from_str(&json).unwrap();
        assert!(decoded.l3.is_none());
        assert!(decoded.confidence.l3_score.is_none());
    }

    #[test]
    fn test_l1_evidence_symbol_kinds() {
        let kinds = vec![
            SymbolKind::Function,
            SymbolKind::Struct,
            SymbolKind::Enum,
            SymbolKind::Trait,
            SymbolKind::Implementation,
            SymbolKind::Module,
        ];

        for kind in kinds {
            let evidence = L1Evidence {
                symbol: "test".into(),
                file_path: "src/test.rs".into(),
                line: 1,
                kind: kind.clone(),
                visibility: "pub".into(),
            };
            let json = serde_json::to_string(&evidence).unwrap();
            let decoded: L1Evidence = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded.kind, kind);
        }
    }

    #[test]
    fn test_evidence_hash() {
        let hash = EvidenceHash {
            l1_hash: "abc123def456".into(),
            l2_hash: "789ghi".into(),
            l3_hash: None,
        };
        let json = serde_json::to_string(&hash).unwrap();
        let decoded: EvidenceHash = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.l1_hash, "abc123def456");
        assert!(decoded.l3_hash.is_none());
    }
}
