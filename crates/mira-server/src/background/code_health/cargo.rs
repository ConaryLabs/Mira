// crates/mira-server/src/background/code_health/cargo.rs
// Cargo check integration for detecting compiler warnings

use crate::db::{StoreMemoryParams, store_memory_sync};
use crate::utils::ResultExt;
use rusqlite::Connection;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

/// Cargo message format
#[derive(Deserialize)]
pub(super) struct CargoMessage {
    reason: String,
    message: Option<CompilerMessage>,
}

#[derive(Deserialize)]
pub(super) struct CompilerMessage {
    level: String,
    message: String,
    spans: Vec<Span>,
}

#[derive(Deserialize)]
pub(super) struct Span {
    file_name: String,
    line_start: u32,
}

/// A collected cargo warning finding, ready for batch storage
pub struct CargoFinding {
    pub key: String,
    pub content: String,
}

/// Run cargo check and collect warnings (no DB writes).
/// Returns findings to be batch-stored by the caller.
pub fn collect_cargo_warnings(project_path: &str) -> Result<Vec<CargoFinding>, String> {
    // Check if it's a Rust project
    let cargo_toml = Path::new(project_path).join("Cargo.toml");
    if !cargo_toml.exists() {
        return Ok(Vec::new());
    }

    let output = Command::new("cargo")
        .args(["check", "--message-format=json", "--quiet"])
        .current_dir(project_path)
        .output()
        .map_err(|e| format!("Failed to run cargo check: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut findings = Vec::new();
    let mut seen_warnings = HashSet::new();

    for line in stdout.lines() {
        if let Ok(msg) = serde_json::from_str::<CargoMessage>(line)
            && msg.reason == "compiler-message"
            && let Some(compiler_msg) = msg.message
            && compiler_msg.level == "warning"
        {
            // Get location from first span
            let location = compiler_msg
                .spans
                .first()
                .map(|s| format!("{}:{}", s.file_name, s.line_start))
                .unwrap_or_default();

            // Deduplicate by location + message
            let dedup_key = format!("{}:{}", location, compiler_msg.message);
            if seen_warnings.contains(&dedup_key) {
                continue;
            }
            seen_warnings.insert(dedup_key);

            let idx = findings.len();

            // Format the issue
            let content = if location.is_empty() {
                format!("[warning] {}", compiler_msg.message)
            } else {
                format!("[warning] {} at {}", compiler_msg.message, location)
            };

            let key = format!("health:warning:{}:{}", location, idx);
            findings.push(CargoFinding { key, content });
        }
    }

    Ok(findings)
}

/// Store collected cargo findings in the database (batch write).
pub fn store_cargo_findings(
    conn: &Connection,
    project_id: i64,
    findings: &[CargoFinding],
) -> Result<usize, String> {
    for finding in findings {
        store_memory_sync(
            conn,
            StoreMemoryParams {
                project_id: Some(project_id),
                key: Some(&finding.key),
                content: &finding.content,
                fact_type: "health",
                category: Some("warning"),
                confidence: 0.9,
                session_id: None,
                user_id: None,
                scope: "project",
                branch: None,
            },
        )
        .str_err()?;
    }
    Ok(findings.len())
}
