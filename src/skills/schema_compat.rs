//! Schema 兼容层
//!
//! 当 Paporot Core 升级导致数据结构变更时，
//! 将 Core 提供的新版本数据转换为 Skill 声明的旧版本格式。
//!
//! 策略：Runtime 端适配，Skill 无感。
//! - 兼容成功 → 透传转换后数据
//! - 兼容失败 → 降级（SKIPPED + 错误日志）

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Schema 版本声明 ────────────────────────────────────────────────

/// Core 中每个数据类型当前的 schema 版本
#[derive(Debug, Clone, Default)]
pub struct CoreSchemaVersions {
    pub versions: HashMap<String, String>,
}

impl CoreSchemaVersions {
    pub fn new() -> Self {
        let mut versions = HashMap::new();
        versions.insert("repo_tree".to_string(), "1.0".to_string());
        versions.insert("repo_files".to_string(), "1.0".to_string());
        versions.insert("git_meta".to_string(), "1.0".to_string());
        versions.insert("ast_symbols".to_string(), "1.0".to_string());
        versions.insert("import_graph".to_string(), "1.0".to_string());
        versions.insert("call_graph".to_string(), "1.0".to_string());
        versions.insert("entry_points".to_string(), "1.0".to_string());
        versions.insert("git_diff".to_string(), "1.0".to_string());
        versions.insert("symbol_references".to_string(), "1.0".to_string());
        versions.insert("prd_content".to_string(), "1.0".to_string());
        versions.insert("language_config".to_string(), "1.0".to_string());
        Self { versions }
    }

    /// 获取某个数据类型的当前 Core 版本
    pub fn get(&self, input_name: &str) -> Option<&str> {
        self.versions.get(input_name).map(|s| s.as_str())
    }
}

// ─── 兼容转换器 ─────────────────────────────────────────────────────

/// Schema 兼容转换器
///
/// 维护 Core 数据版本到 Skill 期望版本之间的转换映射。
/// MVP 阶段所有数据类型都是 v1.0，因此只需做透传兼容。
pub struct SchemaCompat {
    core_versions: CoreSchemaVersions,
}

impl SchemaCompat {
    pub fn new() -> Self {
        Self {
            core_versions: CoreSchemaVersions::new(),
        }
    }

    /// 检查 Skill 声明的 input schema 是否与 Core 兼容
    ///
    /// 返回兼容后的数据版本，如果不兼容则返回 None。
    pub fn check(
        &self,
        input_name: &str,
        skill_requires_version: &str,
    ) -> Option<String> {
        let core_version = self.core_versions.get(input_name)?;

        // MVP: 所有数据类型都是 v1.0，只需精确匹配
        if skill_requires_version == core_version {
            Some(core_version.to_string())
        } else {
            None
        }
    }

    /// 转换为 Skill 期望的版本
    ///
    /// `raw_json` 是 Core 产生的原始 JSON 字节，
    /// 返回转换后可直接注入 WASM 的数据。
    pub fn convert(
        &self,
        input_name: &str,
        skill_requires_version: &str,
        raw_json: &[u8],
    ) -> Result<Vec<u8>> {
        let core_version = self
            .core_versions
            .get(input_name)
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        if skill_requires_version == core_version {
            // 版本一致，直接透传
            Ok(raw_json.to_vec())
        } else {
            bail!(
                "Schema version mismatch for '{}': Core provides '{}', Skill requires '{}', no compat path available",
                input_name, core_version, skill_requires_version
            )
        }
    }

    /// 批量检查 Skill 的 schema_version 声明
    ///
    /// 返回所有不兼容的 input 名称列表
    pub fn check_all(
        &self,
        skill_name: &str,
        schema_versions: &HashMap<String, String>,
    ) -> Vec<SchemaIncompat> {
        let mut incompat = Vec::new();

        for (input_name, skill_version) in schema_versions {
            if self.check(input_name, skill_version).is_none() {
                let core_ver = self
                    .core_versions
                    .get(input_name)
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                incompat.push(SchemaIncompat {
                    skill_name: skill_name.to_string(),
                    input_name: input_name.clone(),
                    skill_requires: skill_version.clone(),
                    core_provides: core_ver,
                });
            }
        }

        incompat
    }
}

impl Default for SchemaCompat {
    fn default() -> Self {
        Self::new()
    }
}

// ─── 辅助类型 ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct SchemaIncompat {
    pub skill_name: String,
    pub input_name: String,
    pub skill_requires: String,
    pub core_provides: String,
}

impl std::fmt::Display for SchemaIncompat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Input '{}': Skill requires '{}', Core provides '{}'",
            self.input_name, self.skill_requires, self.core_provides
        )
    }
}

// ─── 输入数据的通用包装 ─────────────────────────────────────────────

/// Core 提供给 Skill 的所有输入数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInputData {
    pub inputs: HashMap<String, Vec<u8>>,
}

impl SkillInputData {
    pub fn new() -> Self {
        Self {
            inputs: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: impl Into<String>, value: Vec<u8>) {
        self.inputs.insert(key.into(), value);
    }
}

// ─── 测试 ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compat_same_version() {
        let compat = SchemaCompat::new();
        let result = compat.check("repo_tree", "1.0");
        assert!(result.is_some());
    }

    #[test]
    fn test_compat_different_version() {
        let compat = SchemaCompat::new();
        let result = compat.check("repo_tree", "2.0");
        assert!(result.is_none());
    }

    #[test]
    fn test_compat_unknown_input() {
        let compat = SchemaCompat::new();
        let result = compat.check("unknown_data", "1.0");
        assert!(result.is_none());
    }

    #[test]
    fn test_convert_passthrough() {
        let compat = SchemaCompat::new();
        let data = br#"{"test": true}"#;
        let result = compat.convert("repo_tree", "1.0", data).unwrap();
        assert_eq!(result, data);
    }

    #[test]
    fn test_check_all() {
        let compat = SchemaCompat::new();
        let mut versions = HashMap::new();
        versions.insert("repo_tree".to_string(), "1.0".to_string());
        versions.insert("unknown_input".to_string(), "99.0".to_string());

        let incompat = compat.check_all("test-skill", &versions);
        assert_eq!(incompat.len(), 1);
        assert_eq!(incompat[0].input_name, "unknown_input");
    }
}
