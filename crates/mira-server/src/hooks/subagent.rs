// crates/mira-server/src/hooks/subagent.rs
// SubagentStart and SubagentStop hook handlers

use crate::hooks::{HookTimer, read_hook_input, write_hook_output};
use crate::utils::truncate_at_boundary;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// Maximum total characters for injected context (~500 tokens)
const MAX_CONTEXT_CHARS: usize = 2000;

/// Minimum entities to consider subagent output significant
const MIN_SIGNIFICANT_ENTITIES: usize = 3;

/// SubagentStart hook input
#[derive(Debug)]
struct SubagentStartInput {
    subagent_type: String,
    task_description: Option<String>,
}

impl SubagentStartInput {
    fn from_json(json: &serde_json::Value) -> Self {
        Self {
            subagent_type: json
                .get("subagent_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            task_description: json
                .get("task_description")
                .or_else(|| json.get("prompt"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        }
    }
}

/// SubagentStop hook input
#[derive(Debug)]
struct SubagentStopInput {
    subagent_type: String,
    subagent_output: Option<String>,
    stop_hook_active: bool,
    agent_transcript_path: Option<String>,
}

impl SubagentStopInput {
    fn from_json(json: &serde_json::Value) -> Self {
        Self {
            subagent_type: json
                .get("subagent_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            subagent_output: json
                .get("subagent_output")
                .or_else(|| json.get("output"))
                .or_else(|| json.get("result"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            stop_hook_active: json
                .get("stop_hook_active")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            agent_transcript_path: json
                .get("agent_transcript_path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        }
    }
}

/// Run SubagentStart hook
///
/// Injects relevant Mira context when a subagent spawns:
/// 1. Active goals related to current work
/// 2. Recent decisions about relevant code areas (via embeddings or keyword fallback)
/// 3. Key memories that might help the subagent
pub async fn run_start() -> Result<()> {
    let _timer = HookTimer::start("SubagentStart");
    let input = read_hook_input().context("Failed to parse hook input from stdin")?;
    let start_input = SubagentStartInput::from_json(&input);

    eprintln!(
        "[mira] SubagentStart hook triggered (type: {}, task: {:?})",
        start_input.subagent_type,
        start_input
            .task_description
            .as_deref()
            .map(|s| if s.len() > 50 {
                format!("{}...", truncate_at_boundary(s, 50))
            } else {
                s.to_string()
            })
    );

    // Connect to MCP server via IPC (falls back to direct DB if server unavailable)
    let mut client = crate::ipc::client::HookClient::connect().await;

    // Get current project
    let Some((project_id, _)) = client.resolve_project(None).await else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    let mut context_parts: Vec<String> = Vec::new();

    // Get active goals
    let goal_lines = client.get_active_goals(project_id, 3).await;
    if !goal_lines.is_empty() {
        context_parts.push(format!(
            "[Mira/goals] Active goals:\n{}",
            goal_lines.join("\n")
        ));
    }

    // Get relevant memories based on task description (semantic with keyword fallback)
    if let Some(task) = &start_input.task_description {
        let memories = client.recall_memories(project_id, task).await;
        if !memories.is_empty() {
            context_parts.push(format!(
                "[Mira/memory] Relevant context:\n{}",
                memories.join("\n")
            ));
        }
    }

    // Build output, truncating to stay under token budget
    let output = if context_parts.is_empty() {
        serde_json::json!({})
    } else {
        let mut context = format!(
            "[Mira/context] Subagent context:\n\n{}",
            context_parts.join("\n\n")
        );
        if context.len() > MAX_CONTEXT_CHARS {
            // UTF-8 safe truncation
            context = truncate_at_boundary(&context, MAX_CONTEXT_CHARS).to_string();
            // Find last newline to avoid mid-line truncation
            if let Some(pos) = context.rfind('\n') {
                context.truncate(pos);
            }
            context.push_str("\n...");
        }
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "SubagentStart",
                "additionalContext": context
            }
        })
    };

    write_hook_output(&output);
    Ok(())
}

/// Run SubagentStop hook
///
/// Captures useful discoveries from subagent work:
/// - Extracts code entities from subagent output using heuristics
/// - Optionally reads agent_transcript_path for richer discovery
/// - If significant entities found (3+), stores a condensed memory
pub async fn run_stop() -> Result<()> {
    let _timer = HookTimer::start("SubagentStop");
    let input = read_hook_input().context("Failed to parse hook input from stdin")?;
    let stop_input = SubagentStopInput::from_json(&input);

    // Prevent infinite loops per CC 2.1.39 protocol
    if stop_input.stop_hook_active {
        eprintln!("[mira] SubagentStop hook already active, skipping");
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    }

    eprintln!(
        "[mira] SubagentStop hook triggered (type: {})",
        stop_input.subagent_type
    );

    let subagent_output = match &stop_input.subagent_output {
        Some(output) if !output.trim().is_empty() => output.clone(),
        _ => {
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };

    // Extract entities from summary output
    let mut entities = crate::entities::extract_entities_heuristic(&subagent_output);

    // Extract additional entities from full transcript if available
    if let Some(transcript_entities) =
        extract_transcript_entities(&stop_input.agent_transcript_path)
    {
        // Merge transcript entities, deduplicating by canonical_name
        let existing: std::collections::HashSet<String> =
            entities.iter().map(|e| e.canonical_name.clone()).collect();
        for entity in transcript_entities {
            if !existing.contains(&entity.canonical_name) {
                entities.push(entity);
            }
        }
    }

    if entities.len() < MIN_SIGNIFICANT_ENTITIES {
        eprintln!(
            "[mira] SubagentStop: only {} entities found, skipping memory storage",
            entities.len()
        );
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    }

    eprintln!(
        "[mira] SubagentStop: {} significant entities found, storing discovery",
        entities.len()
    );

    // Connect to MCP server via IPC (falls back to direct DB if server unavailable)
    let mut client = crate::ipc::client::HookClient::connect().await;

    // Get current project
    let Some((project_id, _)) = client.resolve_project(None).await else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Build condensed summary from entities
    let entity_summary = build_entity_summary(&stop_input.subagent_type, &entities);

    // Store as a subagent discovery observation
    client
        .store_observation(
            Some(project_id),
            &entity_summary,
            "subagent_discovery",
            Some("subagent_discovery"),
            0.6,
            "subagent",
            "project",
            Some("+7 days"),
        )
        .await;

    write_hook_output(&serde_json::json!({}));
    Ok(())
}

/// Validate a transcript path is safe to read (under home dir or /tmp).
/// Uses the same pattern as precompact.rs.
fn validate_transcript_path(path_str: &str) -> Option<PathBuf> {
    let path = PathBuf::from(path_str);
    let canonical = match path.canonicalize() {
        Ok(c) => c,
        Err(_) => {
            eprintln!(
                "[mira] SubagentStop rejected transcript_path (canonicalize failed): {}",
                path_str
            );
            return None;
        }
    };
    // Validate path is under user's home directory
    if let Some(home) = dirs::home_dir()
        && canonical.starts_with(&home)
    {
        return Some(canonical);
    }
    // Also allow /tmp which Claude Code may use
    if canonical.starts_with("/tmp") {
        return Some(canonical);
    }
    eprintln!(
        "[mira] SubagentStop rejected transcript_path outside home directory: {}",
        path_str
    );
    None
}

/// Extract entities from a subagent's JSONL transcript file.
/// Returns None if the path is missing, invalid, or unreadable.
/// Errors are logged but never block the hook.
fn extract_transcript_entities(path: &Option<String>) -> Option<Vec<crate::entities::RawEntity>> {
    let path_str = path.as_deref()?;
    let canonical = validate_transcript_path(path_str)?;

    let content = match std::fs::read_to_string(&canonical) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[mira] SubagentStop failed to read transcript: {}", e);
            return None;
        }
    };

    // Extract text from assistant messages in the JSONL transcript
    let mut assistant_text = String::new();
    for line in content.lines() {
        // Skip empty lines
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Parse each JSONL line
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        // Look for assistant role messages
        let role = entry.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "assistant" {
            continue;
        }
        // Extract text content - may be a string or array of content blocks
        if let Some(content) = entry.get("content") {
            match content {
                serde_json::Value::String(s) => {
                    assistant_text.push_str(s);
                    assistant_text.push('\n');
                }
                serde_json::Value::Array(blocks) => {
                    for block in blocks {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            assistant_text.push_str(text);
                            assistant_text.push('\n');
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if assistant_text.is_empty() {
        return None;
    }

    let entities = crate::entities::extract_entities_heuristic(&assistant_text);
    if entities.is_empty() {
        return None;
    }

    eprintln!(
        "[mira] SubagentStop: extracted {} additional entities from transcript",
        entities.len()
    );
    Some(entities)
}

/// Build a condensed summary from extracted entities
fn build_entity_summary(subagent_type: &str, entities: &[crate::entities::RawEntity]) -> String {
    use crate::entities::EntityType;

    let files: Vec<&str> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::FilePath)
        .take(5)
        .map(|e| e.name.as_str())
        .collect();

    let code_idents: Vec<&str> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::CodeIdent)
        .take(5)
        .map(|e| e.name.as_str())
        .collect();

    let crates: Vec<&str> = entities
        .iter()
        .filter(|e| e.entity_type == EntityType::CrateName)
        .take(3)
        .map(|e| e.name.as_str())
        .collect();

    let mut parts = Vec::new();
    parts.push(format!("[Mira/context] Subagent:{}", subagent_type));

    if !files.is_empty() {
        parts.push(format!("Files: {}", files.join(", ")));
    }
    if !code_idents.is_empty() {
        parts.push(format!("Identifiers: {}", code_idents.join(", ")));
    }
    if !crates.is_empty() {
        parts.push(format!("Crates: {}", crates.join(", ")));
    }

    parts.join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subagent_start_input_parses_basic() {
        let json = serde_json::json!({
            "subagent_type": "Explore",
            "task_description": "Find authentication code"
        });
        let input = SubagentStartInput::from_json(&json);
        assert_eq!(input.subagent_type, "Explore");
        assert_eq!(
            input.task_description.as_deref(),
            Some("Find authentication code")
        );
    }

    #[test]
    fn subagent_start_input_uses_prompt_fallback() {
        let json = serde_json::json!({
            "subagent_type": "Plan",
            "prompt": "Plan the caching layer"
        });
        let input = SubagentStartInput::from_json(&json);
        assert_eq!(
            input.task_description.as_deref(),
            Some("Plan the caching layer")
        );
    }

    #[test]
    fn subagent_start_input_handles_missing_fields() {
        let json = serde_json::json!({});
        let input = SubagentStartInput::from_json(&json);
        assert_eq!(input.subagent_type, "unknown");
        assert!(input.task_description.is_none());
    }

    #[test]
    fn subagent_stop_input_parses_output() {
        let json = serde_json::json!({
            "subagent_type": "Explore",
            "subagent_output": "Found DatabasePool in src/db/pool.rs and EmbeddingClient in src/embeddings/mod.rs"
        });
        let input = SubagentStopInput::from_json(&json);
        assert_eq!(input.subagent_type, "Explore");
        assert!(input.subagent_output.is_some());
        assert!(!input.stop_hook_active);
        assert!(input.agent_transcript_path.is_none());
    }

    #[test]
    fn subagent_stop_input_parses_stop_hook_active() {
        let json = serde_json::json!({
            "subagent_type": "Explore",
            "stop_hook_active": true
        });
        let input = SubagentStopInput::from_json(&json);
        assert!(input.stop_hook_active);
    }

    #[test]
    fn subagent_stop_input_stop_hook_active_defaults_false() {
        let json = serde_json::json!({
            "subagent_type": "Explore"
        });
        let input = SubagentStopInput::from_json(&json);
        assert!(!input.stop_hook_active);
    }

    #[test]
    fn subagent_stop_input_parses_transcript_path() {
        let json = serde_json::json!({
            "subagent_type": "Explore",
            "agent_transcript_path": "/tmp/claude/transcript.jsonl"
        });
        let input = SubagentStopInput::from_json(&json);
        assert_eq!(
            input.agent_transcript_path.as_deref(),
            Some("/tmp/claude/transcript.jsonl")
        );
    }

    #[test]
    fn subagent_stop_input_tries_output_fallback() {
        let json = serde_json::json!({
            "subagent_type": "Explore",
            "output": "some output"
        });
        let input = SubagentStopInput::from_json(&json);
        assert_eq!(input.subagent_output.as_deref(), Some("some output"));
    }

    #[test]
    fn subagent_stop_input_tries_result_fallback() {
        let json = serde_json::json!({
            "subagent_type": "Explore",
            "result": "some result"
        });
        let input = SubagentStopInput::from_json(&json);
        assert_eq!(input.subagent_output.as_deref(), Some("some result"));
    }

    #[test]
    fn extract_transcript_entities_returns_none_for_missing_path() {
        assert!(extract_transcript_entities(&None).is_none());
    }

    #[test]
    fn extract_transcript_entities_returns_none_for_nonexistent_file() {
        let path = Some("/tmp/nonexistent_mira_test_file_12345.jsonl".to_string());
        assert!(extract_transcript_entities(&path).is_none());
    }

    #[test]
    fn validate_transcript_path_rejects_outside_home_and_tmp() {
        assert!(validate_transcript_path("/etc/passwd").is_none());
    }

    #[test]
    fn build_entity_summary_all_types() {
        use crate::entities::{EntityType, RawEntity};

        let entities = vec![
            RawEntity {
                name: "src/db/pool.rs".to_string(),
                canonical_name: "src/db/pool.rs".to_string(),
                entity_type: EntityType::FilePath,
            },
            RawEntity {
                name: "DatabasePool".to_string(),
                canonical_name: "database_pool".to_string(),
                entity_type: EntityType::CodeIdent,
            },
            RawEntity {
                name: "EmbeddingClient".to_string(),
                canonical_name: "embedding_client".to_string(),
                entity_type: EntityType::CodeIdent,
            },
            RawEntity {
                name: "deadpool_sqlite".to_string(),
                canonical_name: "deadpool_sqlite".to_string(),
                entity_type: EntityType::CrateName,
            },
        ];

        let summary = build_entity_summary("Explore", &entities);
        assert!(summary.contains("[Mira/context] Subagent:Explore"));
        assert!(summary.contains("Files: src/db/pool.rs"));
        assert!(summary.contains("DatabasePool"));
        assert!(summary.contains("EmbeddingClient"));
        assert!(summary.contains("Crates: deadpool_sqlite"));
    }

    #[test]
    fn build_entity_summary_no_files() {
        use crate::entities::{EntityType, RawEntity};

        let entities = vec![
            RawEntity {
                name: "DatabasePool".to_string(),
                canonical_name: "database_pool".to_string(),
                entity_type: EntityType::CodeIdent,
            },
            RawEntity {
                name: "store_memory_sync".to_string(),
                canonical_name: "store_memory_sync".to_string(),
                entity_type: EntityType::CodeIdent,
            },
            RawEntity {
                name: "recall_semantic".to_string(),
                canonical_name: "recall_semantic".to_string(),
                entity_type: EntityType::CodeIdent,
            },
        ];

        let summary = build_entity_summary("Plan", &entities);
        assert!(summary.contains("[Mira/context] Subagent:Plan"));
        assert!(!summary.contains("Files:"));
        assert!(summary.contains("Identifiers:"));
    }
}
