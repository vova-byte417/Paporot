//! L3 LLM Bridge：将 LLM 调用集成到三层架构中
//!
//! 对应 PRD P0 §3.4。在 Agent 中通过 L3 只处理 L1+L2 无法确定的部分。
//! MVP 阶段：直接复用现有 LLM 调用逻辑，不做分片/缓存优化。

use crate::llm::client::LlmClient;
use crate::prompts;
use crate::types::*;
use super::types::*;

/// L3 LLM 增强桥接器
pub struct LlmBridge {
    client: LlmClient,
}

impl LlmBridge {
    /// 创建桥接器
    pub fn new(client: LlmClient) -> Self {
        Self { client }
    }

    /// 仅对低置信度变更调用 LLM 补充语义描述
    ///
    /// # Arguments
    /// * `low_confidence` - L1 获取到的低置信度（<0.5）变更
    /// * `residual_diff` - L1+L2 未覆盖的残留 diff 片段
    ///
    /// # Returns
    /// LLM 提取的 BehaviorSnapshot
    pub async fn enhance(
        &self,
        low_confidence: &[RawChange],
        residual_diff: &str,
    ) -> anyhow::Result<Vec<LlmFragment>> {
        // MVP: 如果残留 diff 为空且没有低置信度变更，直接返回空
        if low_confidence.is_empty() && residual_diff.trim().is_empty() {
            return Ok(vec![]);
        }

        // 构建 prompt：让 LLM 只关注 L1 未覆盖的部分
        let system = prompts::SYSTEM_PROMPT_BEHAVIOR_EXTRACTOR;
        let user = prompts::build_extraction_prompt(
            residual_diff,
            None,
            None,
            None,
        );

        let response = self.client.chat_with_retry(system, &user).await?;

        // 返回 LLM 片段
        Ok(vec![LlmFragment {
            fragment_id: format!("llm_{}", uuid::Uuid::new_v4()),
            content: response,
            file_paths: vec![],
            raw_json: None,
        }])
    }

    /// 将 L3 输出合并为 Capability 列表
    pub fn merge_fragments(fragments: &[LlmFragment]) -> Vec<Capability> {
        let mut capabilities = Vec::new();

        for fragment in fragments {
            // 尝试解析为 BehaviorSnapshot
            if let Ok(snapshot) = serde_json::from_str::<BehaviorSnapshot>(&fragment.content) {
                capabilities.extend(snapshot.capabilities);
            }
        }

        capabilities
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试项: merge_fragments 空片段
    /// 输入: 空 Vec
    /// 预期: 返回空 Capability 列表
    #[test]
    fn test_merge_fragments_empty() {
        let caps = LlmBridge::merge_fragments(&[]);
        assert!(caps.is_empty());
    }

    /// 测试项: merge_fragments 含非 JSON 内容的片段
    /// 输入: content = "plain text"
    /// 预期: 不可解析，跳过，返回空列表
    #[test]
    fn test_merge_fragments_non_json() {
        let fragments = vec![LlmFragment {
            fragment_id: "f1".into(),
            content: "just some plain text".into(),
            file_paths: vec![],
            raw_json: None,
        }];
        let caps = LlmBridge::merge_fragments(&fragments);
        assert!(caps.is_empty());
    }

    /// 测试项: merge_fragments 含有效 BehaviorSnapshot JSON
    /// 输入: content 为合法 BehaviorSnapshot JSON（含 1 个 Capability）
    /// 预期: 解析出 1 个 Capability
    #[test]
    fn test_merge_fragments_valid_snapshot() {
        let snap_json = serde_json::json!({
            "version_id": "v1",
            "timestamp": "2026-01-01T00:00:00Z",
            "message": "test",
            "capabilities": [{
                "id": "cap_001",
                "name": "LLM Cap",
                "description": "from LLM",
                "status": "new",
                "prd_coverage": { "percentage": 0.0, "total_items": 0, "details": [] }
            }],
            "prd_coverage": { "percentage": 0.0, "total_items": 0, "details": [] }
        });
        let fragments = vec![LlmFragment {
            fragment_id: "f1".into(),
            content: snap_json.to_string(),
            file_paths: vec![],
            raw_json: None,
        }];
        let caps = LlmBridge::merge_fragments(&fragments);
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].name, "LLM Cap");
        assert_eq!(caps[0].id, "cap_001");
    }

    /// 测试项: merge_fragments 多片段合并
    /// 输入: 2 个片段各含 1 个 Capability
    /// 预期: 合并得到 2 个 Capability
    #[test]
    fn test_merge_fragments_multiple() {
        let snap1 = serde_json::json!({ "version_id":"v1","timestamp":"t","message":"","capabilities":[{"id":"c1","name":"Cap1","description":"","status":"new","prd_coverage":{"percentage":0,"total_items":0,"details":[]}}],"prd_coverage":{"percentage":0,"total_items":0,"details":[]} });
        let snap2 = serde_json::json!({ "version_id":"v2","timestamp":"t","message":"","capabilities":[{"id":"c2","name":"Cap2","description":"","status":"new","prd_coverage":{"percentage":0,"total_items":0,"details":[]}}],"prd_coverage":{"percentage":0,"total_items":0,"details":[]} });
        let fragments = vec![
            LlmFragment { fragment_id: "f1".into(), content: snap1.to_string(), file_paths: vec![], raw_json: None },
            LlmFragment { fragment_id: "f2".into(), content: snap2.to_string(), file_paths: vec![], raw_json: None },
        ];
        let caps = LlmBridge::merge_fragments(&fragments);
        assert_eq!(caps.len(), 2);
    }
}
