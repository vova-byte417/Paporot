//! Contract Engine — loads YAML contracts and validates artifacts.
//!
//! Pure Rust, no LLM. All checks complete in <10ms.

use crate::verification::types::*;

/// Verify an artifact against its contract.
/// `contract_yaml` is the raw YAML string loaded from .paporot/contracts/.
pub fn verify_artifact(
    artifact_id: &str,
    artifact_type: &str,
    artifact_content: &str,
    contract_yaml: &str,
) -> Result<VerificationResult, String> {
    let contract: ContractConfig = serde_yaml::from_str(contract_yaml)
        .map_err(|e| format!("Failed to parse contract YAML: {}", e))?;

    let mut rule_results = Vec::new();
    let mut suggestions = Vec::new();

    // ── Syntax Rules ──────────────────────────────────────────
    if let serde_yaml::Value::Mapping(ref syntax) = contract.rules.syntax {
        for (key, value) in syntax {
            let rule_name = key.as_str().unwrap_or("?");
            match rule_name {
                "valid_json" if is_bool_true(value) => {
                    let (pass, detail) = check_valid_json(artifact_content);
                    rule_results.push(RuleResult { rule: "valid_json".into(), pass, detail });
                }
                "conforms_to_schema" => {
                    let schema_path = value.as_str().unwrap_or("");
                    if !schema_path.is_empty() {
                        rule_results.push(RuleResult {
                            rule: "conforms_to_schema".into(), pass: true,
                            detail: Some(format!("schema file: {} (validated by host)", schema_path)),
                        });
                    }
                }
                "valid_excalidraw_schema" if is_bool_true(value) => {
                    let (pass, detail) = check_excalidraw_schema(artifact_content);
                    rule_results.push(RuleResult { rule: "valid_excalidraw_schema".into(), pass, detail });
                }
                _ => {}
            }
        }
    }

    // ── Structure Rules ───────────────────────────────────────
    if let serde_yaml::Value::Mapping(ref structure) = contract.rules.structure {
        // Parse artifact content as JSON for structure analysis
        let parsed: Option<serde_json::Value> = serde_json::from_str(artifact_content).ok();

        for (key, value) in structure {
            let rule_name = key.as_str().unwrap_or("?");
            match rule_name {
                "required_fields" => {
                    if let Some(ref v) = parsed {
                        let fields: Vec<String> = value
                            .as_sequence()
                            .map(|s| s.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                            .unwrap_or_default();
                        for field in &fields {
                            let pass = v.get(field).is_some();
                            rule_results.push(RuleResult {
                                rule: format!("required_field:{}", field),
                                pass,
                                detail: if !pass {
                                    Some(format!("missing required field '{}'", field))
                                } else { None },
                            });
                            if !pass {
                                suggestions.push(format!(
                                    "输出缺少必填字段 '{}'，请检查上游 Skill 是否正确生成了该字段", field
                                ));
                            }
                        }
                    }
                }
                "no_empty_arrays" if is_bool_true(value) => {
                    if let Some(ref v) = parsed {
                        let empties = find_empty_arrays(v, "");
                        for (path, _) in &empties {
                            rule_results.push(RuleResult {
                                rule: "no_empty_arrays".into(),
                                pass: false,
                                detail: Some(format!("empty array at '{}'", path)),
                            });
                            suggestions.push(format!("字段 '{}' 是空数组，请检查数据源", path));
                        }
                        if empties.is_empty() {
                            rule_results.push(RuleResult {
                                rule: "no_empty_arrays".into(), pass: true, detail: None,
                            });
                        }
                    }
                }
                "min_elements" => {
                    if let Some(ref v) = parsed {
                        if let Some(elements) = v.get("elements").and_then(|e| e.as_array()) {
                            let min = value.as_u64().unwrap_or(1) as usize;
                            let count = elements.len();
                            let pass = count >= min;
                            rule_results.push(RuleResult {
                                rule: "min_elements".into(),
                                pass,
                                detail: Some(format!("elements count: {}, minimum: {}", count, min)),
                            });
                            if !pass {
                                suggestions.push(format!(
                                    "元素数量 {} 低于最低要求 {}，请检查生成逻辑", count, min
                                ));
                            }
                        }
                    }
                }
                "max_elements" => {
                    if let Some(ref v) = parsed {
                        if let Some(elements) = v.get("elements").and_then(|e| e.as_array()) {
                            let max = value.as_u64().unwrap_or(500) as usize;
                            let count = elements.len();
                            let pass = count <= max;
                            rule_results.push(RuleResult {
                                rule: "max_elements".into(),
                                pass,
                                detail: Some(format!("elements count: {}, maximum: {}", count, max)),
                            });
                            if !pass {
                                suggestions.push(format!(
                                    "元素数量 {} 超过上限 {}，请精简", count, max
                                ));
                            }
                        }
                    }
                }
                "no_deleted_only" if is_bool_true(value) => {
                    if let Some(ref v) = parsed {
                        if let Some(elements) = v.get("elements").and_then(|e| e.as_array()) {
                            if !elements.is_empty() {
                                let all_deleted = elements.iter().all(|el| {
                                    el.get("isDeleted")
                                        .and_then(|d| d.as_bool())
                                        .unwrap_or(false)
                                });
                                let pass = !all_deleted;
                                rule_results.push(RuleResult {
                                    rule: "no_deleted_only".into(),
                                    pass,
                                    detail: if !pass {
                                        Some("all elements are marked isDeleted=true".into())
                                    } else { None },
                                });
                                if !pass {
                                    suggestions.push(
                                        "所有元素都被标记为删除，请检查 isDeleted 状态".into()
                                    );
                                }
                            }
                        }
                    }
                }
                "allowed_element_types" => {
                    if let Some(ref v) = parsed {
                        if let Some(elements) = v.get("elements").and_then(|e| e.as_array()) {
                            let allowed: Vec<String> = value
                                .as_sequence()
                                .map(|s| s.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                                .unwrap_or_default();
                            let mut invalid_types = Vec::new();
                            for el in elements {
                                if let Some(t) = el.get("type").and_then(|t| t.as_str()) {
                                    if !allowed.iter().any(|a| a == t) {
                                        invalid_types.push(t.to_string());
                                    }
                                }
                            }
                            let pass = invalid_types.is_empty();
                            rule_results.push(RuleResult {
                                rule: "allowed_element_types".into(),
                                pass,
                                detail: if !pass {
                                    Some(format!("invalid types: {:?}", invalid_types))
                                } else { None },
                            });
                            if !pass {
                                suggestions.push(format!(
                                    "发现不支持的元素类型: {:?}，允许的类型: {:?}",
                                    invalid_types, allowed
                                ));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let all_pass = rule_results.iter().all(|r| r.pass);
    let status = if all_pass { "PASS".to_string() } else { "FAIL".to_string() };

    Ok(VerificationResult {
        artifact_id: artifact_id.to_string(),
        artifact_type: artifact_type.to_string(),
        status,
        rule_results,
        suggestions,
    })
}

// ─── Helpers ────────────────────────────────────────────────────

fn is_bool_true(v: &serde_yaml::Value) -> bool {
    v.as_bool().unwrap_or(false)
}

fn check_valid_json(content: &str) -> (bool, Option<String>) {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(_) => (true, None),
        Err(e) => (false, Some(format!("invalid JSON: {}", e))),
    }
}

fn check_excalidraw_schema(content: &str) -> (bool, Option<String>) {
    let v: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(e) => return (false, Some(format!("not valid JSON: {}", e))),
    };

    let obj = match v.as_object() {
        Some(o) => o,
        None => return (false, Some("not a JSON object".into())),
    };

    // Must have "type" field
    if !obj.contains_key("type") {
        return (false, Some("missing 'type' field".into()));
    }
    // Must have "version" field
    if !obj.contains_key("version") {
        return (false, Some("missing 'version' field".into()));
    }
    // Must have "elements" array
    match obj.get("elements").and_then(|e| e.as_array()) {
        Some(_) => (true, None),
        None => (false, Some("missing or invalid 'elements' array".into())),
    }
}

fn find_empty_arrays(v: &serde_json::Value, prefix: &str) -> Vec<(String, usize)> {
    let mut empties = Vec::new();
    match v {
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                empties.push((if prefix.is_empty() { "(root)".into() } else { prefix.to_string() }, 0));
            }
            for (i, item) in arr.iter().enumerate() {
                empties.extend(find_empty_arrays(item, &format!("{}[{}]", prefix, i)));
            }
        }
        serde_json::Value::Object(map) => {
            for (k, val) in map {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{}.{}", prefix, k)
                };
                empties.extend(find_empty_arrays(val, &path));
            }
        }
        _ => {}
    }
    empties
}

#[allow(dead_code)]
fn find_empty_objects(v: &serde_json::Value) -> Vec<String> {
    let mut empties = Vec::new();
    if let serde_json::Value::Object(map) = v {
        for (k, val) in map {
            if val.is_object() && val.as_object().map(|o| o.is_empty()).unwrap_or(false) {
                empties.push(k.clone());
            } else if val.is_object() {
                empties.extend(find_empty_objects(val));
            }
        }
    }
    empties
}
