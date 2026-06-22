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

// ═══════════════════════════════════════════════════════════════════════
// Unit Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers ────────────────────────────────────────────────────

    fn json_contract_yaml() -> String {
        r#"artifact_type: json
version: "1.0"
severity: error

rules:
  syntax:
    valid_json: true

  structure:
    required_fields:
      - project_name
      - summary
    no_empty_arrays: true
"#
        .to_string()
    }

    fn excalidraw_contract_yaml() -> String {
        r#"artifact_type: excalidraw
version: "1.0"
severity: error

rules:
  syntax:
    valid_json: true
    valid_excalidraw_schema: true

  structure:
    min_elements: 1
    no_deleted_only: true
    allowed_element_types:
      - rectangle
      - ellipse
      - diamond
      - text
      - arrow
      - line
      - freedraw
      - image
    max_elements: 500
"#
        .to_string()
    }

    // ═══════════════════════════════════════════════════════════════
    // valid_json rule
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_valid_json_passes() {
        let result = verify_artifact(
            "test-artifact",
            "json",
            r#"{"project_name":"Test","summary":"ok"}"#,
            &json_contract_yaml(),
        )
        .unwrap();

        assert_eq!(result.status, "PASS");
        assert!(result.rule_results.iter().any(|r| r.rule == "valid_json" && r.pass));
    }

    #[test]
    fn test_valid_json_fails_on_invalid() {
        let result = verify_artifact(
            "test-artifact",
            "json",
            r#"not json at all"#,
            &json_contract_yaml(),
        )
        .unwrap();

        assert_eq!(result.status, "FAIL");
        let json_rule = result.rule_results.iter().find(|r| r.rule == "valid_json").unwrap();
        assert!(!json_rule.pass);
        assert!(json_rule.detail.as_ref().unwrap().contains("invalid JSON"));
    }

    #[test]
    fn test_valid_json_passes_with_null() {
        let result = verify_artifact(
            "test-artifact",
            "json",
            "null",
            &json_contract_yaml(),
        )
        .unwrap();

        // null is valid JSON but fails required_fields (no project_name)
        let json_rule = result.rule_results.iter().find(|r| r.rule == "valid_json").unwrap();
        assert!(json_rule.pass);
    }

    // ═══════════════════════════════════════════════════════════════
    // Excalidraw schema
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_excalidraw_schema_passes() {
        let content = r#"{"type":"excalidraw","version":2,"elements":[{"type":"rectangle","x":0,"y":0}]}"#;
        let result = verify_artifact(
            "test-excalidraw",
            "excalidraw",
            content,
            &excalidraw_contract_yaml(),
        )
        .unwrap();

        let schema_rule = result.rule_results.iter().find(|r| r.rule == "valid_excalidraw_schema").unwrap();
        assert!(schema_rule.pass);
    }

    #[test]
    fn test_excalidraw_schema_fails_missing_type() {
        let content = r#"{"version":2,"elements":[{"type":"rectangle"}]}"#;
        let result = verify_artifact(
            "test-excalidraw",
            "excalidraw",
            content,
            &excalidraw_contract_yaml(),
        )
        .unwrap();

        let schema_rule = result.rule_results.iter().find(|r| r.rule == "valid_excalidraw_schema").unwrap();
        assert!(!schema_rule.pass);
        assert!(schema_rule.detail.as_ref().unwrap().contains("missing 'type' field"));
    }

    #[test]
    fn test_excalidraw_schema_fails_missing_version() {
        let content = r#"{"type":"excalidraw","elements":[]}"#;
        let result = verify_artifact(
            "test-excalidraw",
            "excalidraw",
            content,
            &excalidraw_contract_yaml(),
        )
        .unwrap();

        let schema_rule = result.rule_results.iter().find(|r| r.rule == "valid_excalidraw_schema").unwrap();
        assert!(!schema_rule.pass);
        assert!(schema_rule.detail.as_ref().unwrap().contains("missing 'version' field"));
    }

    #[test]
    fn test_excalidraw_schema_fails_missing_elements() {
        let content = r#"{"type":"excalidraw","version":2}"#;
        let result = verify_artifact(
            "test-excalidraw",
            "excalidraw",
            content,
            &excalidraw_contract_yaml(),
        )
        .unwrap();

        let schema_rule = result.rule_results.iter().find(|r| r.rule == "valid_excalidraw_schema").unwrap();
        assert!(!schema_rule.pass);
        assert!(schema_rule.detail.as_ref().unwrap().contains("missing or invalid 'elements'"));
    }

    #[test]
    fn test_excalidraw_schema_fails_elements_not_array() {
        let content = r#"{"type":"excalidraw","version":2,"elements":"not-an-array"}"#;
        let result = verify_artifact(
            "test-excalidraw",
            "excalidraw",
            content,
            &excalidraw_contract_yaml(),
        )
        .unwrap();

        let schema_rule = result.rule_results.iter().find(|r| r.rule == "valid_excalidraw_schema").unwrap();
        assert!(!schema_rule.pass);
    }

    #[test]
    fn test_excalidraw_schema_elements_is_array_passes() {
        let content = r#"{"type":"excalidraw","version":2,"elements":[]}"#;
        let result = verify_artifact(
            "test-excalidraw",
            "excalidraw",
            content,
            &excalidraw_contract_yaml(),
        )
        .unwrap();

        let schema_rule = result.rule_results.iter().find(|r| r.rule == "valid_excalidraw_schema").unwrap();
        assert!(schema_rule.pass, "Elements array (even empty) should pass excalidraw schema check");
    }

    // ═══════════════════════════════════════════════════════════════
    // required_fields rule
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_required_fields_all_present() {
        let content = r#"{"project_name":"Test","summary":"A summary","extra":"ignored"}"#;
        let result = verify_artifact(
            "test-artifact", "json", content, &json_contract_yaml(),
        )
        .unwrap();

        assert!(result.rule_results.iter().any(|r| r.rule == "required_field:project_name" && r.pass));
        assert!(result.rule_results.iter().any(|r| r.rule == "required_field:summary" && r.pass));
    }

    #[test]
    fn test_required_fields_missing_one() {
        let content = r#"{"summary":"only summary"}"#;
        let result = verify_artifact(
            "test-artifact", "json", content, &json_contract_yaml(),
        )
        .unwrap();

        let field_rule = result.rule_results.iter().find(|r| r.rule == "required_field:project_name").unwrap();
        assert!(!field_rule.pass);
        assert!(field_rule.detail.as_ref().unwrap().contains("missing required field"));
        assert!(!result.suggestions.is_empty());
    }

    #[test]
    fn test_required_fields_on_array_input_all_fail() {
        let content = r#"[1, 2, 3]"#;
        let result = verify_artifact(
            "test-artifact", "json", content, &json_contract_yaml(),
        )
        .unwrap();

        // Arrays don't have string-keyed fields, so all required_fields fail
        let field_rules: Vec<_> = result.rule_results.iter()
            .filter(|r| r.rule.starts_with("required_field:"))
            .collect();
        assert!(!field_rules.is_empty(), "required_fields should still run on array input");
        assert!(field_rules.iter().all(|r| !r.pass), "all required_fields should fail on array");
    }

    // ═══════════════════════════════════════════════════════════════
    // no_empty_arrays rule
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_no_empty_arrays_passes() {
        let content = r#"{"project_name":"Test","summary":"ok","items":[1,2,3]}"#;
        let result = verify_artifact(
            "test-artifact", "json", content, &json_contract_yaml(),
        )
        .unwrap();

        let rule = result.rule_results.iter().find(|r| r.rule == "no_empty_arrays").unwrap();
        assert!(rule.pass);
    }

    #[test]
    fn test_no_empty_arrays_detects_empty() {
        let content = r#"{"project_name":"Test","summary":"ok","items":[]}"#;
        let result = verify_artifact(
            "test-artifact", "json", content, &json_contract_yaml(),
        )
        .unwrap();

        let empty_rules: Vec<_> = result.rule_results.iter()
            .filter(|r| r.rule == "no_empty_arrays" && !r.pass)
            .collect();
        assert!(!empty_rules.is_empty());
    }

    #[test]
    fn test_no_empty_arrays_nested() {
        let content = r#"{"project_name":"Test","summary":"ok","data":{"items":[]}}"#;
        let result = verify_artifact(
            "test-artifact", "json", content, &json_contract_yaml(),
        )
        .unwrap();

        let empty_rules: Vec<_> = result.rule_results.iter()
            .filter(|r| r.rule == "no_empty_arrays" && !r.pass)
            .collect();
        assert!(!empty_rules.is_empty());
        let first_fail = empty_rules.first().unwrap();
        assert!(first_fail.detail.as_ref().unwrap().contains("data.items"));
    }

    // ═══════════════════════════════════════════════════════════════
    // min_elements / max_elements rules
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_min_elements_passes() {
        let content = r#"{"type":"excalidraw","version":2,"elements":[
            {"type":"rectangle","x":0,"y":0},
            {"type":"ellipse","x":1,"y":1}
        ]}"#;
        let result = verify_artifact(
            "test-excalidraw", "excalidraw", content, &excalidraw_contract_yaml(),
        )
        .unwrap();

        let rule = result.rule_results.iter().find(|r| r.rule == "min_elements").unwrap();
        assert!(rule.pass);
    }

    #[test]
    fn test_min_elements_fails_with_zero() {
        let content = r#"{"type":"excalidraw","version":2,"elements":[]}"#;
        let result = verify_artifact(
            "test-excalidraw", "excalidraw", content, &excalidraw_contract_yaml(),
        )
        .unwrap();

        let rule = result.rule_results.iter().find(|r| r.rule == "min_elements").unwrap();
        assert!(!rule.pass);
        assert!(rule.detail.as_ref().unwrap().contains("elements count: 0"));
    }

    #[test]
    fn test_max_elements_passes() {
        let elements: Vec<_> = (0..5).map(|i| {
            serde_json::json!({"type":"rectangle","x":i,"y":0})
        }).collect();
        let content = serde_json::json!({
            "type": "excalidraw", "version": 2, "elements": elements
        }).to_string();

        let result = verify_artifact(
            "test-excalidraw", "excalidraw", &content, &excalidraw_contract_yaml(),
        )
        .unwrap();

        let rule = result.rule_results.iter().find(|r| r.rule == "max_elements").unwrap();
        assert!(rule.pass);
    }

    #[test]
    fn test_max_elements_fails_when_exceeded() {
        let elements: Vec<_> = (0..501).map(|i| {
            serde_json::json!({"type":"rectangle","x":i,"y":0})
        }).collect();
        let content = serde_json::json!({
            "type": "excalidraw", "version": 2, "elements": elements
        }).to_string();

        let result = verify_artifact(
            "test-excalidraw", "excalidraw", &content, &excalidraw_contract_yaml(),
        )
        .unwrap();

        let rule = result.rule_results.iter().find(|r| r.rule == "max_elements").unwrap();
        assert!(!rule.pass);
    }

    // ═══════════════════════════════════════════════════════════════
    // no_deleted_only rule
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_no_deleted_only_passes_when_mixed() {
        let content = r#"{"type":"excalidraw","version":2,"elements":[
            {"type":"rectangle","isDeleted":true},
            {"type":"ellipse","isDeleted":false}
        ]}"#;
        let result = verify_artifact(
            "test-excalidraw", "excalidraw", content, &excalidraw_contract_yaml(),
        )
        .unwrap();

        let rule = result.rule_results.iter().find(|r| r.rule == "no_deleted_only").unwrap();
        assert!(rule.pass);
    }

    #[test]
    fn test_no_deleted_only_fails_all_deleted() {
        let content = r#"{"type":"excalidraw","version":2,"elements":[
            {"type":"rectangle","isDeleted":true},
            {"type":"ellipse","isDeleted":true}
        ]}"#;
        let result = verify_artifact(
            "test-excalidraw", "excalidraw", content, &excalidraw_contract_yaml(),
        )
        .unwrap();

        let rule = result.rule_results.iter().find(|r| r.rule == "no_deleted_only").unwrap();
        assert!(!rule.pass);
        assert!(!result.suggestions.is_empty());
    }

    #[test]
    fn test_no_deleted_only_skips_on_non_excalidraw() {
        let content = r#"{"project_name":"Test","summary":"ok","elements":[{"isDeleted":true}]}"#;
        let result = verify_artifact(
            "test-artifact", "json", content, &json_contract_yaml(),
        )
        .unwrap();

        assert!(result.rule_results.iter().all(|r| r.rule != "no_deleted_only"));
    }

    // ═══════════════════════════════════════════════════════════════
    // allowed_element_types rule
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_allowed_element_types_passes() {
        let content = r#"{"type":"excalidraw","version":2,"elements":[
            {"type":"rectangle"},
            {"type":"ellipse"},
            {"type":"arrow"}
        ]}"#;
        let result = verify_artifact(
            "test-excalidraw", "excalidraw", content, &excalidraw_contract_yaml(),
        )
        .unwrap();

        let rule = result.rule_results.iter().find(|r| r.rule == "allowed_element_types").unwrap();
        assert!(rule.pass);
    }

    #[test]
    fn test_allowed_element_types_fails_invalid_type() {
        let content = r#"{"type":"excalidraw","version":2,"elements":[
            {"type":"rectangle"},
            {"type":"foobar_invalid_type"},
            {"type":"arrow"}
        ]}"#;
        let result = verify_artifact(
            "test-excalidraw", "excalidraw", content, &excalidraw_contract_yaml(),
        )
        .unwrap();

        let rule = result.rule_results.iter().find(|r| r.rule == "allowed_element_types").unwrap();
        assert!(!rule.pass);
        assert!(rule.detail.as_ref().unwrap().contains("foobar_invalid_type"));
    }

    #[test]
    fn test_allowed_element_types_all_invalid() {
        let content = r#"{"type":"excalidraw","version":2,"elements":[
            {"type":"bad1"},
            {"type":"bad2"}
        ]}"#;
        let result = verify_artifact(
            "test-excalidraw", "excalidraw", content, &excalidraw_contract_yaml(),
        )
        .unwrap();

        let rule = result.rule_results.iter().find(|r| r.rule == "allowed_element_types").unwrap();
        assert!(!rule.pass);
    }

    // ═══════════════════════════════════════════════════════════════
    // Overall status logic
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_overall_pass_when_all_rules_pass() {
        let content = r#"{"type":"excalidraw","version":2,"elements":[
            {"type":"rectangle","x":0,"y":0}
        ]}"#;
        let result = verify_artifact(
            "test-excalidraw", "excalidraw", content, &excalidraw_contract_yaml(),
        )
        .unwrap();

        assert_eq!(result.status, "PASS");
        assert!(result.suggestions.is_empty());
    }

    #[test]
    fn test_overall_fail_when_any_rule_fails() {
        let content = r#"not json"#;
        let result = verify_artifact(
            "test-artifact", "json", content, &json_contract_yaml(),
        )
        .unwrap();

        assert_eq!(result.status, "FAIL");
    }

    #[test]
    fn test_partial_fail_collects_all_failures() {
        let content = r#"{"summary":"ok","items":[],"more":[]}"#;
        let result = verify_artifact(
            "test-artifact", "json", content, &json_contract_yaml(),
        )
        .unwrap();

        assert_eq!(result.status, "FAIL");
        let fail_count = result.rule_results.iter().filter(|r| !r.pass).count();
        assert!(fail_count >= 2, "Expected at least 2 failures, got {}", fail_count);
    }

    // ═══════════════════════════════════════════════════════════════
    // Edge cases
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_bad_contract_yaml_returns_error() {
        let result = verify_artifact(
            "test-artifact", "json", "{}", "definitely: not: valid: yaml: [[[",
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_contract_structure_produces_no_structure_rules() {
        let contract = r#"artifact_type: json
version: "1.0"
severity: error
rules:
  syntax:
    valid_json: true
  structure: {}
"#;
        let result = verify_artifact(
            "test-artifact", "json", "{}", contract,
        )
        .unwrap();

        assert_eq!(result.rule_results.len(), 1);
        assert!(result.rule_results[0].pass);
    }

    #[test]
    fn test_syntax_rule_not_enabled_does_not_check() {
        let contract = r#"artifact_type: json
version: "1.0"
severity: error
rules:
  syntax: {}
  structure:
    required_fields:
      - name
"#;
        let result = verify_artifact(
            "test-artifact", "json", "not json!!!!", contract,
        )
        .unwrap();

        assert!(result.rule_results.iter().all(|r| r.rule != "valid_json"));
        let name_rule = result.rule_results.iter().find(|r| r.rule == "required_field:name");
        assert!(name_rule.is_none());
    }

    #[test]
    fn test_conforms_to_schema_rule() {
        let contract = r#"artifact_type: json
version: "1.0"
severity: error
rules:
  syntax:
    conforms_to_schema: "some_schema.json"
  structure: {}
"#;
        let result = verify_artifact(
            "test-artifact", "json", "{}", contract,
        )
        .unwrap();

        let rule = result.rule_results.iter().find(|r| r.rule == "conforms_to_schema").unwrap();
        assert!(rule.pass);
        assert!(rule.detail.as_ref().unwrap().contains("some_schema.json"));
    }

    #[test]
    fn test_artifact_id_and_type_are_preserved() {
        let result = verify_artifact(
            "custom-id", "excalidraw", "{}", &json_contract_yaml(),
        )
        .unwrap();

        assert_eq!(result.artifact_id, "custom-id");
        assert_eq!(result.artifact_type, "excalidraw");
    }

    #[test]
    fn test_excalidraw_valid_with_no_elements_still_fails_min_elements() {
        let content = r#"{"type":"excalidraw","version":2,"elements":[]}"#;
        let result = verify_artifact(
            "test-excalidraw", "excalidraw", content, &excalidraw_contract_yaml(),
        )
        .unwrap();

        assert_eq!(result.status, "FAIL");
        assert!(result.rule_results.iter().any(|r| r.rule == "min_elements" && !r.pass));
    }
}
