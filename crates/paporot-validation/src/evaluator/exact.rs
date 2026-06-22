//! 精确匹配评估器
//!
//! 按 (name, status, categories) 三元组匹配。
//! categories 为空时不校验。

use crate::types::{Actual, ActualDiffSummary, Expected, ExpectedCapability};
use anyhow::{bail, Result};

pub fn evaluate(expected: &Expected, actual: &Actual) -> Result<()> {
    // 1. Capability 匹配
    if !expected.capabilities.is_empty() {
        evaluate_capabilities(&expected.capabilities, &actual.capabilities)?;
    }

    // 2. Diff 匹配
    if let Some(ref exp_diff) = expected.diff {
        if let Some(ref act_diff) = actual.diff_summary {
            evaluate_diff(exp_diff, act_diff)?;
        } else {
            bail!("Expected diff summary but actual has none");
        }
    }

    Ok(())
}

fn evaluate_capabilities(expected: &[ExpectedCapability], actual: &[crate::types::ActualCapability]) -> Result<()> {
    for exp in expected {
        let found = actual.iter().find(|a| {
            // name 精确匹配
            if a.name != exp.name {
                return false;
            }
            // status 精确匹配
            if a.status != exp.status {
                return false;
            }
            // categories 非空时精确匹配（忽略顺序）
            if !exp.categories.is_empty() {
                let mut exp_cats = exp.categories.clone();
                exp_cats.sort();
                let mut act_cats = a.categories.clone();
                act_cats.sort();
                if exp_cats != act_cats {
                    return false;
                }
            }
            true
        });

        if found.is_none() {
            let act_names: Vec<String> = actual.iter().map(|a| a.name.clone()).collect();
            bail!(
                "Expected capability (name='{}', status={:?}, cats={:?}) not found. Actual capabilities: {:?}",
                exp.name,
                exp.status,
                exp.categories,
                act_names
            );
        }
    }

    // 检查多余的 capability（expected 不包含的）
    let expected_len = expected.len();
    let actual_len = actual.len();
    if actual_len > expected_len {
        let exp_names: Vec<&str> = expected.iter().map(|e| e.name.as_str()).collect();
        let extra: Vec<&str> = actual
            .iter()
            .filter(|a| !exp_names.contains(&a.name.as_str()))
            .map(|a| a.name.as_str())
            .collect();
        if !extra.is_empty() {
            bail!(
                "Unexpected extra capabilities in actual: {:?} (expected {} caps, got {})",
                extra,
                expected_len,
                actual_len
            );
        }
    }

    Ok(())
}

fn evaluate_diff(expected: &crate::types::ExpectedDiff, actual: &ActualDiffSummary) -> Result<()> {
    if let Some(added) = expected.added_count {
        if added != actual.added_count {
            bail!(
                "Diff added_count mismatch: expected {}, got {}",
                added,
                actual.added_count
            );
        }
    }
    if let Some(removed) = expected.removed_count {
        if removed != actual.removed_count {
            bail!(
                "Diff removed_count mismatch: expected {}, got {}",
                removed,
                actual.removed_count
            );
        }
    }
    if let Some(modified) = expected.modified_count {
        if modified != actual.modified_count {
            bail!(
                "Diff modified_count mismatch: expected {}, got {}",
                modified,
                actual.modified_count
            );
        }
    }
    for name in &expected.added_names {
        if !actual.added_names.contains(name) {
            bail!("Expected added name '{}' not found in actual", name);
        }
    }
    for name in &expected.removed_names {
        if !actual.removed_names.contains(name) {
            bail!("Expected removed name '{}' not found in actual", name);
        }
    }
    Ok(())
}
