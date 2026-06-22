//! Evidence Engine — collects and manages verification evidence.
//!
//! Evidence lives in WASM sandbox memory during execution.
//! On FAIL, relevant evidence is persisted as a Replay Case.

use crate::verification::types::*;
use std::path::PathBuf;

/// In-memory evidence store (lives in SandboxHost state).
#[derive(Debug, Default)]
pub struct EvidenceBuffer {
    pub records: Vec<EvidenceRecord>,
}

impl EvidenceBuffer {
    pub fn new() -> Self {
        Self { records: Vec::new() }
    }

    pub fn capture(&mut self, artifact_id: &str, input: &str, output: &str, intermediate: &str) {
        self.records.push(EvidenceRecord {
            artifact_id: artifact_id.to_string(),
            input: input.to_string(),
            output: output.to_string(),
            intermediate: intermediate.to_string(),
        });
    }

    /// Get the evidence record for a specific artifact. Returns the LAST match.
    pub fn get(&self, artifact_id: &str) -> Option<&EvidenceRecord> {
        self.records.iter().rev().find(|r| r.artifact_id == artifact_id)
    }
}

/// Save a replay case to disk (called by host on FAIL).
pub fn save_replay_case(case: &ReplayCase, paporot_dir: &PathBuf) -> Result<(), String> {
    let dir = paporot_dir.join("regression").join("cases");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create regression dir: {}", e))?;

    let filename = format!("{}.json", case.case_id);
    let path = dir.join(filename);
    let json = serde_json::to_string_pretty(case)
        .map_err(|e| format!("Failed to serialize replay case: {}", e))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("Failed to write replay case: {}", e))?;

    Ok(())
}

/// Load all replay cases from disk.
pub fn load_replay_cases(paporot_dir: &PathBuf) -> Vec<ReplayCase> {
    let dir = paporot_dir.join("regression").join("cases");
    if !dir.exists() {
        return Vec::new();
    }

    let mut cases = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(case) = serde_json::from_str::<ReplayCase>(&content) {
                        cases.push(case);
                    }
                }
            }
        }
    }
    cases
}
