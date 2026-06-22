/// Skill: Verification Runner
///
/// Goal: 对所有上游 Skill 产出 Artifact 执行 Contract 验证、收集 Evidence、
///       在 FAIL 时保存 Replay Case。纯过程式 Skill，不调用 LLM。
///
/// Inputs:  上游 6 个分析 Skill 的 JSON 输出
/// Output:  verification_result JSON (overall_status + per-artifact results)

use paporot_skill_sdk::prelude::*;

/// 上游 Skill 名 → artifact_type 映射
fn skill_to_artifact_type(skill_name: &str) -> &str {
    match skill_name {
        "architecture-doc-generator"
        | "dependency-analysis"
        | "runtime-flow-analysis"
        | "module-discovery"
        | "behavior-boundary-discovery"
        | "repository-understanding" => "json",
        _ => "json",
    }
}

#[no_mangle]
pub extern "C" fn paporot_skill_execute() -> i32 {
    let upstream_names = [
        "repository-understanding",
        "module-discovery",
        "dependency-analysis",
        "runtime-flow-analysis",
        "behavior-boundary-discovery",
        "architecture-doc-generator",
    ];

    let mut results: Vec<Value> = Vec::new();
    let mut replay_count: u32 = 0;
    let mut overall_pass = true;

    for skill_name in &upstream_names {
        // 读取上游 Skill 输出
        let input_key = format!("skill_output__{}", skill_name);
        let output = match read_input(&input_key) {
            Some(o) => o,
            None => continue, // 上游未产出则跳过
        };

        if output.is_empty() || output == "{}" {
            continue;
        }

        let artifact_type = skill_to_artifact_type(skill_name);

        // 1. Contract 验证
        let verify_result_json = verify_contract(artifact_type, &output)
            .unwrap_or_else(|| {
                format!(
                    r#"{{"status":"ERROR","error":"host_verify_contract failed for {}"}}"#,
                    skill_name
                )
            });

        let verify_result: Value = serde_json::from_str(&verify_result_json)
            .unwrap_or_else(|_| {
                json!({
                    "artifact_id": skill_name,
                    "artifact_type": artifact_type,
                    "status": "ERROR",
                    "rule_results": [],
                    "suggestions": ["Failed to parse verification result"]
                })
            });

        let status = verify_result["status"].as_str().unwrap_or("ERROR");

        // 2. Evidence 收集
        let prev_outputs = upstream_names
            .iter()
            .filter(|n| **n != *skill_name)
            .filter_map(|n| {
                let key = format!("skill_output__{}", n);
                read_input(&key).map(|v| (n.to_string(), v))
            })
            .collect::<Vec<_>>();

        let prev_json: Value = prev_outputs
            .iter()
            .map(|(n, v)| (n.clone(), Value::String(v.clone())))
            .collect::<serde_json::Map<_, _>>()
            .into();

        capture_evidence(
            skill_name,
            &prev_json.to_string(),
            &output,
            "{}",
        );

        // 3. FAIL → 保存 Replay Case
        if status != "PASS" {
            overall_pass = false;

            let case = json!({
                "case_id": format!("replay-{:x}", output.as_bytes().as_ptr() as usize),
                "created_at": "wasm-sandbox",
                "artifact_type": artifact_type,
                "artifact_id": skill_name,
                "upstream_input": prev_json,
                "failed_artifact": output,
                "contract_result": verify_result,
                "suggestions": verify_result["suggestions"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            });

            save_replay_case(&case.to_string());
            replay_count += 1;
        }

        results.push(json!({
            "artifact_id": skill_name,
            "artifact_type": artifact_type,
            "status": status,
            "rule_results": verify_result["rule_results"],
            "suggestions": verify_result["suggestions"],
        }));
    }

    let overall_status = if overall_pass { "PASS" } else { "FAIL" };
    let output = json!({
        "overall_status": overall_status,
        "results": results,
        "replay_cases_saved": replay_count,
    });

    if !overall_pass {
        write_error(&format!(
            "Verification FAILED: {}/{} artifacts failed",
            results.iter().filter(|r| r["status"] != "PASS").count(),
            results.len(),
        ));
    }

    write_output(&output);
    0
}

fn main() {}
