// crates/mira-server/src/background/code_health/cargo.rs
// Cargo check integration for detecting compiler warnings

use crate::db::{StoreObservationParams, store_observation_sync};
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
///
/// WARNING: This is a blocking function. Use `collect_cargo_warnings_async()` in async contexts.
pub(super) fn collect_cargo_warnings(project_path: &str) -> Result<Vec<CargoFinding>, String> {
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
    Ok(parse_cargo_output(&stdout))
}

/// Async version of collect_cargo_warnings using tokio::process::Command.
/// The tokio Child handle is held for the duration of the await, so if this
/// future is dropped (e.g. outer timeout fires), tokio kills the child process
/// automatically — preventing orphaned cargo processes.
pub async fn collect_cargo_warnings_async(project_path: &str) -> Result<Vec<CargoFinding>, String> {
    use tokio::process::Command as TokioCommand;

    // Check if it's a Rust project
    let cargo_toml = Path::new(project_path).join("Cargo.toml");
    if !cargo_toml.exists() {
        return Ok(Vec::new());
    }

    let output = TokioCommand::new("cargo")
        .args(["check", "--message-format=json", "--quiet"])
        .current_dir(project_path)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("Failed to run cargo check: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_cargo_output(&stdout))
}

/// Store collected cargo findings in the database (batch write).
pub fn store_cargo_findings(
    conn: &Connection,
    project_id: i64,
    findings: &[CargoFinding],
) -> Result<usize, String> {
    for finding in findings {
        store_observation_sync(
            conn,
            StoreObservationParams {
                project_id: Some(project_id),
                key: Some(&finding.key),
                content: &finding.content,
                observation_type: "health",
                category: Some("warning"),
                confidence: 0.9,
                source: "code_health",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .str_err()?;
    }
    Ok(findings.len())
}

/// Parse cargo JSON output lines into findings (extracted for testability).
/// Same logic as collect_cargo_warnings but operates on raw stdout string.
fn parse_cargo_output(stdout: &str) -> Vec<CargoFinding> {
    let mut findings = Vec::new();
    let mut seen_warnings = HashSet::new();

    for line in stdout.lines() {
        if let Ok(msg) = serde_json::from_str::<CargoMessage>(line)
            && msg.reason == "compiler-message"
            && let Some(compiler_msg) = msg.message
            && compiler_msg.level == "warning"
        {
            let location = compiler_msg
                .spans
                .first()
                .map(|s| format!("{}:{}", s.file_name, s.line_start))
                .unwrap_or_default();

            let dedup_key = format!("{}:{}", location, compiler_msg.message);
            if seen_warnings.contains(&dedup_key) {
                continue;
            }
            seen_warnings.insert(dedup_key);

            let idx = findings.len();
            let content = if location.is_empty() {
                format!("[warning] {}", compiler_msg.message)
            } else {
                format!("[warning] {} at {}", compiler_msg.message, location)
            };

            let key = format!("health:warning:{}:{}", location, idx);
            findings.push(CargoFinding { key, content });
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real cargo JSON output samples
    const WARNING_LINE: &str = r#"{"reason":"compiler-message","package_id":"mypkg 0.1.0","manifest_path":"/tmp/Cargo.toml","target":{"name":"mypkg"},"message":{"rendered":"warning: unused variable","children":[],"code":null,"level":"warning","message":"unused variable: `x`","spans":[{"file_name":"src/main.rs","byte_end":100,"byte_start":90,"column_end":10,"column_start":5,"is_primary":true,"line_end":5,"line_start":5,"text":[]}]}}"#;

    const ERROR_LINE: &str = r#"{"reason":"compiler-message","package_id":"mypkg 0.1.0","manifest_path":"/tmp/Cargo.toml","target":{"name":"mypkg"},"message":{"rendered":"error[E0308]","children":[],"code":null,"level":"error","message":"mismatched types","spans":[{"file_name":"src/lib.rs","byte_end":50,"byte_start":40,"column_end":8,"column_start":1,"is_primary":true,"line_end":3,"line_start":3,"text":[]}]}}"#;

    const BUILD_FINISHED: &str = r#"{"reason":"build-finished","success":true}"#;

    // ═══════════════════════════════════════════════════════════════════════════
    // CargoMessage deserialization
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_deserialize_warning() {
        let msg: CargoMessage = serde_json::from_str(WARNING_LINE).unwrap();
        assert_eq!(msg.reason, "compiler-message");
        let cm = msg.message.unwrap();
        assert_eq!(cm.level, "warning");
        assert_eq!(cm.message, "unused variable: `x`");
        assert_eq!(cm.spans.len(), 1);
        assert_eq!(cm.spans[0].file_name, "src/main.rs");
        assert_eq!(cm.spans[0].line_start, 5);
    }

    #[test]
    fn test_deserialize_error() {
        let msg: CargoMessage = serde_json::from_str(ERROR_LINE).unwrap();
        let cm = msg.message.unwrap();
        assert_eq!(cm.level, "error");
    }

    #[test]
    fn test_deserialize_build_finished() {
        let msg: CargoMessage = serde_json::from_str(BUILD_FINISHED).unwrap();
        assert_eq!(msg.reason, "build-finished");
        assert!(msg.message.is_none());
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // parse_cargo_output
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_parse_empty() {
        assert!(parse_cargo_output("").is_empty());
    }

    #[test]
    fn test_parse_single_warning() {
        let findings = parse_cargo_output(WARNING_LINE);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].content.contains("[warning]"));
        assert!(findings[0].content.contains("unused variable: `x`"));
        assert!(findings[0].content.contains("src/main.rs:5"));
    }

    #[test]
    fn test_parse_skips_errors() {
        let findings = parse_cargo_output(ERROR_LINE);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_parse_skips_build_finished() {
        let findings = parse_cargo_output(BUILD_FINISHED);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_parse_mixed_output() {
        let input = format!("{}\n{}\n{}", WARNING_LINE, ERROR_LINE, BUILD_FINISHED);
        let findings = parse_cargo_output(&input);
        assert_eq!(findings.len(), 1); // Only the warning
    }

    #[test]
    fn test_parse_dedup_identical_warnings() {
        let input = format!("{}\n{}", WARNING_LINE, WARNING_LINE);
        let findings = parse_cargo_output(&input);
        assert_eq!(findings.len(), 1, "Duplicate warnings should be deduped");
    }

    #[test]
    fn test_parse_no_spans_produces_empty_location() {
        let no_span = r#"{"reason":"compiler-message","package_id":"p","manifest_path":"/tmp/Cargo.toml","target":{"name":"p"},"message":{"rendered":"","children":[],"code":null,"level":"warning","message":"crate-level warning","spans":[]}}"#;
        let findings = parse_cargo_output(no_span);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].content, "[warning] crate-level warning");
        // No "at" since location is empty
        assert!(!findings[0].content.contains(" at "));
    }

    #[test]
    fn test_parse_invalid_json_skipped() {
        let input = "not valid json\n{}\n";
        let findings = parse_cargo_output(input);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_parse_key_format() {
        let findings = parse_cargo_output(WARNING_LINE);
        assert!(findings[0].key.starts_with("health:warning:"));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // store_cargo_findings
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_store_empty() {
        let conn = crate::db::test_support::setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/cargo", Some("test")).unwrap();
        let count = store_cargo_findings(&conn, pid, &[]).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_store_persists() {
        let conn = crate::db::test_support::setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/cargo", Some("test")).unwrap();

        let findings = vec![
            CargoFinding {
                key: "health:warning:src/main.rs:5:0".to_string(),
                content: "[warning] unused variable at src/main.rs:5".to_string(),
            },
            CargoFinding {
                key: "health:warning:src/lib.rs:10:1".to_string(),
                content: "[warning] dead code at src/lib.rs:10".to_string(),
            },
        ];

        let count = store_cargo_findings(&conn, pid, &findings).unwrap();
        assert_eq!(count, 2);

        let stored: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM system_observations WHERE project_id = ? AND category = 'warning'",
                [pid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, 2);
    }
}
