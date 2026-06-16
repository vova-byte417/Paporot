//! `Paporot version` / `Paporot status` — 版本与状态信息

use anyhow::Result;
use crate::agent::Agent;

/// 执行 version 命令
pub async fn run(agent: &Agent) -> Result<()> {
    let _ = agent; // 保持接口一致
    let pkg_version = env!("CARGO_PKG_VERSION");
    let pkg_name = env!("CARGO_PKG_NAME");
    let pkg_description = env!("CARGO_PKG_DESCRIPTION");

    println!("{} v{}", pkg_name, pkg_version);
    println!("{}", pkg_description);
    println!();
    println!("Commands:");
    println!("  snapshot create  Create a new behavior snapshot via LLM");
    println!("  diff             Compare behavior snapshots");
    println!("  coverage         Compute PRD coverage via LLM");
    println!("  regression       Detect regressions via LLM");
    println!("  risk             Assess risk level via LLM");
    println!("  review           Full review pipeline");
    println!("  version          Show version info");
    println!("  status           Show current status");

    Ok(())
}

/// 执行 status 命令
pub async fn status(agent: &Agent) -> Result<()> {
    println!("Paporot Status");
    println!("-------------");

    // Git 信息
    if let Ok(branch) = get_git_branch() {
        println!("  Git branch : {}", branch);
    }
    if let Ok(commit) = get_git_commit() {
        println!("  Git commit : {}", commit);
    }

    // Snapshot 信息
    match agent.storage.list_versions_sorted() {
        Ok(versions) if !versions.is_empty() => {
            println!("  Snapshots  : {} stored ({})", versions.len(), versions.join(", "));
        }
        _ => {
            println!("  Snapshots  : none (run 'Paporot snapshot create')");
        }
    }

    // LLM 配置
    println!("  LLM model  : {}", agent.config.llm.model);
    if agent.config.llm.api_key.is_empty() {
        println!("  LLM key    : not set (use Paporot_API_KEY env var or .Paporot/config.toml)");
    } else {
        let masked: String = agent.config.llm.api_key
            .chars()
            .enumerate()
            .map(|(i, c)| if i < 8 || i >= agent.config.llm.api_key.len().saturating_sub(4) { c } else { '*' })
            .collect();
        println!("  LLM key    : {}", masked);
    }

    Ok(())
}

fn get_git_branch() -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()?;

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn get_git_commit() -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()?;

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// 遮蔽 API key 中间部分
pub fn mask_api_key(key: &str) -> String {
    if key.len() <= 12 {
        return "***".into();
    }
    let visible_head = &key[..8];
    let visible_tail = &key[key.len().saturating_sub(4)..];
    format!("{}***{}", visible_head, visible_tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试项: mask_api_key 标准长 key
    /// 输入: 32 字符 key
    /// 预期: 前 8 + *** + 后 4
    #[test]
    fn test_mask_api_key_standard() {
        let masked = mask_api_key("sk-1234567890abcdef1234567890");
        assert!(masked.starts_with("sk-12345"), "应以 sk-12345 开头");
        assert!(masked.ends_with("7890"), "应以 7890 结尾");
        assert!(masked.contains("***"), "应包含 ***");
    }

    /// 测试项: mask_api_key 短 key
    /// 输入: 不到 12 字符的 key
    /// 预期: 返回 ***
    #[test]
    fn test_mask_api_key_short() {
        assert_eq!(mask_api_key("abc"), "***");
        assert_eq!(mask_api_key(""), "***");
    }

    /// 测试项: mask_api_key 正好 12 字符
    /// 输入: 12 字符 key
    /// 预期: 仍然返回 ***
    #[test]
    fn test_mask_api_key_exact_12() {
        let masked = mask_api_key("123456789012");
        assert_eq!(masked, "***");
    }

    /// 测试项: mask_api_key 13 字符
    /// 输入: 13 字符
    /// 预期: 前 8 + *** + 后 4
    #[test]
    fn test_mask_api_key_13_chars() {
        let masked = mask_api_key("1234567890abc");
        assert_eq!(masked.len(), 15); // 8 + 3 + 4
    }

    /// 测试项: CARGO_PKG_VERSION 编译期常量存在
    #[test]
    fn test_cargo_pkg_version_exists() {
        let ver = env!("CARGO_PKG_VERSION");
        assert!(!ver.is_empty());
        // 语义化版本号格式
        assert!(ver.chars().next().unwrap().is_ascii_digit());
    }

    /// 测试项: CARGO_PKG_NAME 编译期常量
    #[test]
    fn test_cargo_pkg_name() {
        let name = env!("CARGO_PKG_NAME");
        assert_eq!(name, "Paporot");
    }
}
