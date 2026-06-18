//! Skill Registry
//!
//! 扫描 `.paporot/skills/*/skill.toml`，
//! 校验语义版本兼容性，返回已安装的 Skill 列表。

use anyhow::{Context, Result};
use semver::VersionReq;
use std::path::{Path, PathBuf};

use super::types::{InstalledSkill, SkillManifest, SkillRegistryInfo};

/// Paporot Skill 注册表
///
/// 扫描本地 `.paporot/skills/` 目录，加载并校验所有 Skill。
pub struct SkillRegistry {
    skills_dir: PathBuf,
    paporot_version: String,
}

impl SkillRegistry {
    /// 创建新的 Skill Registry
    ///
    /// `skills_dir` 通常是 `.paporot/skills/`
    /// `paporot_version` 是当前 Paporot Core 的版本号
    pub fn new(skills_dir: impl AsRef<Path>, paporot_version: &str) -> Self {
        Self {
            skills_dir: skills_dir.as_ref().to_path_buf(),
            paporot_version: paporot_version.to_string(),
        }
    }

    /// 列出所有已安装（且语义版本兼容）的 Skill 元信息
    pub fn list(&self) -> Result<Vec<SkillRegistryInfo>> {
        let installed = self.load_all()?;
        let infos = installed
            .into_iter()
            .map(|s| {
                let compatible = self.check_compat(&s.manifest);
                SkillRegistryInfo {
                    name: s.manifest.skill.name,
                    version: s.manifest.skill.version,
                    description: s.manifest.skill.description,
                    requires_paporot: s.manifest.skill.requires_paporot.clone(),
                    compatible,
                }
            })
            .collect();
        Ok(infos)
    }

    /// 加载所有兼容的 Skill（过滤掉不兼容的）
    pub fn load_compatible(&self) -> Result<Vec<InstalledSkill>> {
        let all = self.load_all()?;
        let mut compat = Vec::new();
        let mut skipped = Vec::new();

        for s in all {
            if self.check_compat(&s.manifest) {
                compat.push(s);
            } else {
                skipped.push(s.manifest.skill.name);
            }
        }

        if !skipped.is_empty() {
            eprintln!(
                "  [registry] Skipped {} incompatible skill(s): {}",
                skipped.len(),
                skipped.join(", ")
            );
        }

        Ok(compat)
    }

    // ─── 内部方法 ───────────────────────────────────────────────────

    /// 扫描 skills 目录，加载所有 skill.toml
    fn load_all(&self) -> Result<Vec<InstalledSkill>> {
        if !self.skills_dir.exists() {
            return Ok(Vec::new());
        }

        let mut skills = Vec::new();

        for entry in std::fs::read_dir(&self.skills_dir)
            .with_context(|| format!("Failed to read skills dir: {:?}", self.skills_dir))?
        {
            let entry = entry?;
            let skill_dir = entry.path();

            if !skill_dir.is_dir() {
                continue;
            }

            let toml_path = skill_dir.join("skill.toml");
            let wasm_path = skill_dir.join("skill.wasm");

            if !toml_path.exists() {
                eprintln!(
                    "  [registry] Skipping {}: skill.toml not found",
                    skill_dir.display()
                );
                continue;
            }

            let toml_content = std::fs::read_to_string(&toml_path)
                .with_context(|| format!("Failed to read {:?}", toml_path))?;

            let manifest: SkillManifest = toml::from_str(&toml_content)
                .with_context(|| format!("Failed to parse {:?}", toml_path))?;

            skills.push(InstalledSkill {
                manifest,
                dir: skill_dir,
                wasm_path,
            });
        }

        Ok(skills)
    }

    /// 检查 Skill 的 requires_paporot 是否与当前 Core 版本兼容
    fn check_compat(&self, manifest: &SkillManifest) -> bool {
        let req_str = &manifest.skill.requires_paporot;

        // 解析 semver 约束
        let req = match VersionReq::parse(req_str) {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "  [registry] Invalid semver constraint '{}' in skill '{}': {}",
                    req_str, manifest.skill.name, e
                );
                return false;
            }
        };

        // 解析 Core 版本
        let core_ver = match semver::Version::parse(&self.paporot_version) {
            Ok(v) => v,
            Err(_) => {
                // 如果 Core 版本不是标准 semver（如 "0.1.0-dev"），
                // 尝试宽松匹配
                return true;
            }
        };

        req.matches(&core_ver)
    }
}

// ─── 测试 ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_skill(dir: &Path, name: &str, version: &str, requires: &str) {
        let skill_dir = dir.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();

        let toml_content = format!(
            r#"
[skill]
name = "{}"
version = "{}"
requires_paporot = "{}"
description = "Test skill"
timeout_secs = 30

[inputs]
required = ["repo_tree"]

[outputs]
schema = "{}_output"

[dependencies]
"#,
            name, version, requires, name
        );

        let mut file = std::fs::File::create(skill_dir.join("skill.toml")).unwrap();
        file.write_all(toml_content.as_bytes()).unwrap();
        std::fs::File::create(skill_dir.join("skill.wasm")).unwrap();
    }

    #[test]
    fn test_registry_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let registry = SkillRegistry::new(tmp.path(), "0.2.0");
        let skills = registry.list().unwrap();
        assert!(skills.is_empty());
    }

    #[test]
    fn test_registry_load_and_list() {
        let tmp = TempDir::new().unwrap();
        create_test_skill(tmp.path(), "test-skill", "0.1.0", ">=0.2.0");

        let registry = SkillRegistry::new(tmp.path(), "0.2.0");
        let skills = registry.list().unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "test-skill");
        assert!(skills[0].compatible);
    }

    #[test]
    fn test_registry_incompatible_version() {
        let tmp = TempDir::new().unwrap();
        create_test_skill(tmp.path(), "new-skill", "0.1.0", ">=0.5.0");

        let registry = SkillRegistry::new(tmp.path(), "0.2.0");
        let skills = registry.list().unwrap();
        assert_eq!(skills.len(), 1);
        assert!(!skills[0].compatible, "Skill requiring >=0.5.0 should be incompatible with Core 0.2.0");
    }

    #[test]
    fn test_registry_compatible_range() {
        let tmp = TempDir::new().unwrap();
        create_test_skill(tmp.path(), "range-skill", "0.1.0", ">=0.2.0, <0.3.0");

        let registry = SkillRegistry::new(tmp.path(), "0.2.5");
        let infos = registry.list().unwrap();
        assert!(infos[0].compatible);

        let registry2 = SkillRegistry::new(tmp.path(), "0.3.0");
        let infos2 = registry2.list().unwrap();
        assert!(!infos2[0].compatible, "0.3.0 should not match >=0.2.0, <0.3.0");
    }

    #[test]
    fn test_load_compatible_filters() {
        let tmp = TempDir::new().unwrap();
        create_test_skill(tmp.path(), "compat", "0.1.0", ">=0.2.0");
        create_test_skill(tmp.path(), "incompat", "0.1.0", ">=1.0.0");

        let registry = SkillRegistry::new(tmp.path(), "0.3.0");
        let compat = registry.load_compatible().unwrap();
        assert_eq!(compat.len(), 1);
        assert_eq!(compat[0].manifest.skill.name, "compat");
    }
}
