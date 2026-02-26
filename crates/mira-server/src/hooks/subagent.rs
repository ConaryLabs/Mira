// crates/mira-server/src/hooks/subagent.rs
// SubagentStart and SubagentStop hook handlers

use crate::hooks::{HookTimer, read_hook_input, write_hook_output};
use crate::utils::truncate_at_boundary;
use anyhow::{Context, Result};
use std::path::PathBuf;

/// Maximum total characters for full-capability subagents (Plan, general-purpose)
const MAX_CONTEXT_CHARS_FULL: usize = 5000;

/// Budget allocated to the code bundle portion of context
const BUNDLE_BUDGET: i64 = 3000;

/// Minimum entities to consider subagent output significant
const MIN_SIGNIFICANT_ENTITIES: usize = 3;

/// Check if a subagent type is narrow/exploratory (smaller context budget, skip goals).
fn is_narrow_subagent(subagent_type: &str) -> bool {
    matches!(
        subagent_type.to_lowercase().as_str(),
        "explore" | "code-reviewer" | "code-simplifier" | "haiku"
    )
}

/// SubagentStart hook input
#[derive(Debug)]
struct SubagentStartInput {
    subagent_type: String,
    task_description: Option<String>,
    session_id: String,
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
            session_id: json
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
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
    session_id: String,
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
            session_id: json
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
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

    tracing::debug!(
        subagent_type = %start_input.subagent_type,
        task = ?start_input
            .task_description
            .as_deref()
            .map(|s| if s.len() > 50 {
                format!("{}...", truncate_at_boundary(s, 50))
            } else {
                s.to_string()
            }),
        "SubagentStart hook triggered"
    );

    // Connect to MCP server via IPC (falls back to direct DB if server unavailable)
    let mut client = crate::ipc::client::HookClient::connect().await;

    // Get current project
    let sid = Some(start_input.session_id.as_str()).filter(|s| !s.is_empty());
    let Some((project_id, project_path)) = client.resolve_project(None, sid).await else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Derive a short project label from the path (last component)
    let project_label = std::path::Path::new(&project_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("");

    let mut context_parts: Vec<String> = Vec::new();
    let narrow = is_narrow_subagent(&start_input.subagent_type);
    let context_cap = MAX_CONTEXT_CHARS_FULL;

    // Get active goals -- skip for narrow/exploratory subagents (goals are
    // strategic context, not useful for focused search/review tasks)
    if !narrow {
        let goal_lines = client.get_active_goals(project_id, 3).await;
        if !goal_lines.is_empty() {
            let label = if project_label.is_empty() {
                "[Mira/goals]".to_string()
            } else {
                format!("[Mira/goals ({})]", project_label)
            };
            context_parts.push(format!(
                "{} Active goals:\n{}",
                label,
                goal_lines.join("\n")
            ));
        }
    }

    // Auto-bundle: inject code context based on the task description.
    // Extract file path scopes from the prompt, then generate a lightweight bundle.
    // Falls back to keyword-based scopes when no file paths are found.
    let mut bundle_injected = false;
    if !narrow && let Some(ref task_desc) = start_input.task_description {
        let mut scopes = extract_scopes_from_prompt(task_desc);

        // Semantic fallback: if no file paths found, extract keywords and use
        // them as scope patterns (matched via LIKE against module paths in the index)
        if scopes.is_empty() {
            scopes = extract_keyword_scopes(task_desc);
        }

        for scope in scopes.iter().take(2) {
            if let Some(bundle) = client
                .generate_bundle(project_id, scope, BUNDLE_BUDGET, "overview")
                .await
            {
                context_parts.push(bundle);
                bundle_injected = true;
                break; // One bundle is enough
            }
        }
    }

    // Track whether goals were actually injected into context_parts.
    // Narrow subagents skip goals entirely, and even full subagents may have
    // no active goals -- only log "goals" when content was actually added.
    let goals_injected = !narrow
        && context_parts
            .first()
            .map(|p| p.contains("[Mira/goals"))
            .unwrap_or(false);

    // Build output, truncating to stay under token budget
    let mut sources_kept = Vec::new();
    if goals_injected {
        sources_kept.push("goals".to_string());
    }
    if bundle_injected {
        sources_kept.push("bundle".to_string());
    }

    let output = if context_parts.is_empty() {
        serde_json::json!({})
    } else {
        let mut context = format!(
            "[Mira/context] Subagent context:\n\n{}",
            context_parts.join("\n\n")
        );
        if context.len() > context_cap {
            // UTF-8 safe truncation
            context = truncate_at_boundary(&context, context_cap).to_string();
            // Find last newline to avoid mid-line truncation
            if let Some(pos) = context.rfind('\n') {
                context.truncate(pos);
            }
            context.push_str("\n...");
        }

        let db_path = crate::hooks::get_db_path();
        crate::db::injection::record_injection_fire_and_forget(
            &db_path,
            &crate::db::injection::InjectionRecord {
                hook_name: "SubagentStart".to_string(),
                session_id: Some(start_input.session_id.clone()),
                project_id: Some(project_id),
                chars_injected: context.len(),
                sources_kept,
                sources_dropped: vec![],
                latency_ms: None,
                was_deduped: false,
                was_cached: false,
            },
        );

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
        tracing::debug!("SubagentStop hook already active, skipping");
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    }

    tracing::debug!(
        subagent_type = %stop_input.subagent_type,
        "SubagentStop hook triggered"
    );

    let subagent_output = match &stop_input.subagent_output {
        Some(output) if !output.trim().is_empty() => output.clone(),
        _ => {
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };

    // Connect to MCP server via IPC (falls back to direct DB if server unavailable)
    let mut client = crate::ipc::client::HookClient::connect().await;

    // Get current project
    let sid = Some(stop_input.session_id.as_str()).filter(|s| !s.is_empty());
    let Some((project_id, _)) = client.resolve_project(None, sid).await else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Extract structured findings (tables, headers, severity markers) from output.
    // These survive compaction via the observations table with a longer TTL.
    let findings = extract_findings_from_output(&subagent_output);
    if !findings.is_empty() {
        tracing::debug!(
            count = findings.len(),
            "SubagentStop: storing structured findings"
        );
        let findings_content = format!(
            "[Mira/findings] Subagent:{} results:\n{}",
            stop_input.subagent_type,
            findings.join("\n---\n")
        );
        // Longer TTL for findings (30 days vs 7 for entity discoveries)
        client
            .store_observation(
                Some(project_id),
                &findings_content,
                "subagent_findings",
                Some("subagent_findings"),
                0.8,
                "subagent",
                "project",
                Some("+30 days"),
            )
            .await;
    }

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

    if entities.len() >= MIN_SIGNIFICANT_ENTITIES {
        tracing::debug!(
            count = entities.len(),
            "SubagentStop: significant entities found, storing discovery"
        );

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
    } else {
        tracing::debug!(
            count = entities.len(),
            "SubagentStop: below entity threshold, skipping entity storage"
        );
    }

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
            tracing::warn!(
                path = %path_str,
                "SubagentStop rejected transcript_path (canonicalize failed)"
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
    tracing::warn!(
        path = %path_str,
        "SubagentStop rejected transcript_path outside home directory"
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
            tracing::warn!(error = %e, "SubagentStop failed to read transcript");
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

    tracing::debug!(
        count = entities.len(),
        "SubagentStop: extracted additional entities from transcript"
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

/// Maximum characters per extracted finding
const MAX_FINDING_LEN: usize = 600;

/// Maximum findings to extract from a single subagent output
const MAX_FINDINGS: usize = 5;

/// Extract structured findings from subagent output.
///
/// Detects markdown-structured content: summary tables, priority/severity headers,
/// recommendation sections, and finding entries. Returns condensed findings that
/// can be stored as observations and survive compaction.
fn extract_findings_from_output(output: &str) -> Vec<String> {
    let mut findings: Vec<String> = Vec::new();
    let lines: Vec<&str> = output.lines().collect();
    let mut i = 0;

    while i < lines.len() && findings.len() < MAX_FINDINGS {
        let line = lines[i].trim();
        let lower = line.to_lowercase();

        // Detect summary/priority tables: a header line followed by table rows
        let is_findings_header = lower.starts_with("## summary")
            || lower.starts_with("### summary")
            || lower.starts_with("## priority summary")
            || lower.starts_with("## top ")
            || lower.starts_with("### top ")
            || lower.starts_with("## immediate wins")
            || lower.starts_with("## high-priority")
            || lower.starts_with("## consensus")
            || lower.starts_with("## key tensions")
            || lower.starts_with("## high impact")
            || lower.starts_with("## actionable");

        if is_findings_header {
            // Capture this header + following content until next ## header or blank gap
            let mut block = String::from(line);
            i += 1;
            while i < lines.len() {
                let next = lines[i];
                let next_trimmed = next.trim();
                // Stop at the next section header
                if next_trimmed.starts_with("## ") {
                    break;
                }
                block.push('\n');
                block.push_str(next);
                if block.len() > MAX_FINDING_LEN {
                    break;
                }
                i += 1;
            }
            let truncated = if block.len() > MAX_FINDING_LEN {
                crate::utils::truncate_at_boundary(&block, MAX_FINDING_LEN).to_string()
            } else {
                block
            };
            if truncated.len() > 20 {
                findings.push(truncated);
            }
            continue;
        }

        // Detect individual findings: "### Finding N" or "**Finding N:**"
        // The starts_with check is sufficient -- requiring ':' or '--' was too
        // restrictive and silently dropped headers like "### Finding 1".
        let is_individual_finding = lower.starts_with("### finding")
            || lower.starts_with("## finding")
            || lower.starts_with("**finding");

        if is_individual_finding {
            let mut block = String::from(line);
            i += 1;
            // Capture until next heading or empty line gap (2+ blank lines)
            let mut blanks = 0;
            while i < lines.len() {
                let next = lines[i].trim();
                if next.is_empty() {
                    blanks += 1;
                    if blanks >= 2 {
                        break;
                    }
                } else {
                    blanks = 0;
                    if next.starts_with("### ") || next.starts_with("## ") {
                        break;
                    }
                }
                block.push('\n');
                block.push_str(lines[i]);
                if block.len() > MAX_FINDING_LEN {
                    break;
                }
                i += 1;
            }
            let truncated = if block.len() > MAX_FINDING_LEN {
                crate::utils::truncate_at_boundary(&block, MAX_FINDING_LEN).to_string()
            } else {
                block
            };
            if truncated.len() > 20 {
                findings.push(truncated);
            }
            continue;
        }

        i += 1;
    }

    findings
}

/// Common words to exclude from keyword scope extraction.
const SCOPE_STOP_WORDS: &[&str] = &[
    "the",
    "this",
    "that",
    "with",
    "from",
    "into",
    "about",
    "after",
    "before",
    "should",
    "could",
    "would",
    "does",
    "have",
    "been",
    "being",
    "will",
    "when",
    "where",
    "what",
    "which",
    "their",
    "there",
    "here",
    "then",
    "than",
    "them",
    "they",
    "some",
    "more",
    "most",
    "also",
    "just",
    "like",
    "make",
    "find",
    "look",
    "check",
    "review",
    "analyze",
    "implement",
    "create",
    "update",
    "delete",
    "remove",
    "code",
    "file",
    "function",
    "class",
    "module",
    "system",
    "using",
    "used",
    "need",
    "want",
    "help",
    "please",
    "each",
    "every",
    "other",
    "only",
    "both",
    "same",
];

/// Extract keyword-based scopes from a task description when no file paths are present.
///
/// Looks for domain-specific identifiers: CamelCase words, snake_case words, and
/// longer lowercase words that might match module or directory names in the code index.
/// Returns scopes suitable for `generate_bundle`'s LIKE-based pattern matching.
fn extract_keyword_scopes(prompt: &str) -> Vec<String> {
    let mut scopes = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for word in prompt.split_whitespace() {
        let word = word.trim_matches(|c: char| {
            c == '"'
                || c == '\''
                || c == '`'
                || c == '('
                || c == ')'
                || c == ','
                || c == ';'
                || c == '.'
                || c == ':'
                || c == '?'
                || c == '!'
        });

        if word.len() < 4 {
            continue;
        }

        let lower = word.to_lowercase();
        if SCOPE_STOP_WORDS.contains(&lower.as_str()) {
            continue;
        }

        // CamelCase identifiers (e.g., DatabasePool, HookClient)
        let has_mixed_case = word.chars().any(|c| c.is_uppercase())
            && word.chars().any(|c| c.is_lowercase())
            && word.chars().all(|c| c.is_alphanumeric() || c == '_');

        // snake_case identifiers (e.g., extract_scopes, hook_client)
        let is_snake_case = word.contains('_')
            && word.chars().all(|c| c.is_alphanumeric() || c == '_')
            && word.len() >= 5;

        if (has_mixed_case || is_snake_case) && seen.insert(lower.clone()) {
            scopes.push(lower);
        }
    }

    // Take at most 3 keyword scopes
    scopes.truncate(3);
    scopes
}

/// Extract directory scopes from a task description prompt.
///
/// Looks for file paths (e.g. "src/hooks/subagent.rs") and returns their
/// parent directory as a scope suitable for bundle generation.
/// Falls back to extracting module-like identifiers if no paths found.
fn extract_scopes_from_prompt(prompt: &str) -> Vec<String> {
    let mut scopes = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Match file paths: word chars, slashes, dots, hyphens ending in a known extension
    // or containing at least one slash
    for word in prompt.split_whitespace() {
        // Strip surrounding punctuation (quotes, parens, backticks, commas)
        let word = word.trim_matches(|c: char| {
            c == '"' || c == '\'' || c == '`' || c == '(' || c == ')' || c == ',' || c == ';'
        });

        // Must contain a slash to look like a path
        if !word.contains('/') {
            continue;
        }

        // Extract the directory portion
        if let Some(idx) = word.rfind('/') {
            let dir = &word[..idx + 1];
            // Sanity: directory must be reasonable (no spaces, not too long)
            if dir.len() <= 200
                && !dir.contains(' ')
                && dir
                    .chars()
                    .all(|c| c.is_alphanumeric() || "/_-./".contains(c))
                && seen.insert(dir.to_string())
            {
                scopes.push(dir.to_string());
            }
        }
    }

    scopes
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

    #[test]
    fn test_narrow_subagent_types() {
        assert!(is_narrow_subagent("explore"));
        assert!(is_narrow_subagent("code-reviewer"));
        assert!(is_narrow_subagent("code-simplifier"));
        assert!(is_narrow_subagent("haiku"));
    }

    #[test]
    fn test_full_subagent_types() {
        assert!(!is_narrow_subagent("plan"));
        assert!(!is_narrow_subagent("general-purpose"));
        assert!(!is_narrow_subagent("Bash"));
        assert!(!is_narrow_subagent(""));
    }

    #[test]
    fn test_narrow_subagent_case_insensitive() {
        // is_narrow_subagent calls .to_lowercase() so it is case-insensitive.
        assert!(is_narrow_subagent("Explore"));
        assert!(is_narrow_subagent("EXPLORE"));
        assert!(is_narrow_subagent("Code-Reviewer"));
        assert!(is_narrow_subagent("HAIKU"));
    }

    #[test]
    fn extract_scopes_finds_file_paths() {
        let prompt = "Look at src/hooks/subagent.rs and crates/mira-server/src/ipc/ops.rs";
        let scopes = extract_scopes_from_prompt(prompt);
        assert_eq!(scopes, vec!["src/hooks/", "crates/mira-server/src/ipc/"]);
    }

    #[test]
    fn extract_scopes_handles_backtick_paths() {
        let prompt = "Check `src/tools/core/code/bundle.rs` for the implementation";
        let scopes = extract_scopes_from_prompt(prompt);
        assert_eq!(scopes, vec!["src/tools/core/code/"]);
    }

    #[test]
    fn extract_scopes_deduplicates() {
        let prompt = "Read src/hooks/subagent.rs and src/hooks/session.rs";
        let scopes = extract_scopes_from_prompt(prompt);
        assert_eq!(scopes, vec!["src/hooks/"]);
    }

    #[test]
    fn extract_scopes_empty_for_no_paths() {
        let prompt = "Find where authentication is handled";
        let scopes = extract_scopes_from_prompt(prompt);
        assert!(scopes.is_empty());
    }

    // ── extract_keyword_scopes ─────────────────────────────────────────

    #[test]
    fn keyword_scopes_finds_camel_case() {
        let prompt = "Refactor the DatabasePool to use connection pooling";
        let scopes = extract_keyword_scopes(prompt);
        assert!(scopes.contains(&"databasepool".to_string()));
    }

    #[test]
    fn keyword_scopes_finds_snake_case() {
        let prompt = "Look at how hook_client handles IPC connections";
        let scopes = extract_keyword_scopes(prompt);
        assert!(scopes.contains(&"hook_client".to_string()));
    }

    #[test]
    fn keyword_scopes_skips_stop_words() {
        let prompt = "Please review this code and check for issues";
        let scopes = extract_keyword_scopes(prompt);
        assert!(scopes.is_empty());
    }

    #[test]
    fn keyword_scopes_limits_to_three() {
        let prompt = "Check DatabasePool HookClient EmbeddingClient FuzzyCache SessionManager";
        let scopes = extract_keyword_scopes(prompt);
        assert!(scopes.len() <= 3);
    }

    #[test]
    fn keyword_scopes_skips_short_words() {
        let prompt = "The API key was missing";
        let scopes = extract_keyword_scopes(prompt);
        // "API" is only 3 chars, "key" is 3 chars, "was" is stop word
        assert!(scopes.is_empty());
    }

    // ── extract_findings_from_output ───────────────────────────────────

    #[test]
    fn findings_extracts_summary_table() {
        let output = "Some intro text\n\n## Summary Table\n\n| Finding | Priority |\n|---------|----------|\n| Missing auth | High |\n| Slow query | Low |\n\n## Next Section\n\nMore text";
        let findings = extract_findings_from_output(output);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].contains("Summary Table"));
        assert!(findings[0].contains("Missing auth"));
    }

    #[test]
    fn findings_extracts_individual_finding() {
        let output = "## Overview\n\nSome text\n\n### Finding 1 -- Missing Authentication\n\nThe auth module lacks input validation.\nThis could allow unauthorized access.\n\n### Finding 2 -- Slow Query\n\nThe query takes too long.";
        let findings = extract_findings_from_output(output);
        assert!(!findings.is_empty());
        assert!(findings[0].contains("Missing Authentication"));
    }

    #[test]
    fn findings_respects_max_limit() {
        let mut output = String::new();
        for i in 0..10 {
            output.push_str(&format!("## Top {} items\n\nContent for item {}\n\n", i, i));
        }
        let findings = extract_findings_from_output(&output);
        assert!(findings.len() <= MAX_FINDINGS);
    }

    #[test]
    fn findings_empty_for_normal_prose() {
        let output = "I looked at the code and it seems fine. The function handles errors correctly and the tests pass.";
        let findings = extract_findings_from_output(output);
        assert!(findings.is_empty());
    }

    #[test]
    fn findings_extracts_consensus_section() {
        let output = "## Consensus Items\n\n- All experts agree on X\n- Y is also important\n\n## Tensions\n\nSome disagreement on Z";
        let findings = extract_findings_from_output(output);
        assert!(!findings.is_empty());
        assert!(findings[0].contains("Consensus"));
    }

    #[test]
    fn findings_extracts_key_tensions_section() {
        let output = "## Introduction\n\nSome preamble.\n\n## Key Tensions\n\n- Expert A favors approach X\n- Expert B prefers approach Y\n- Trade-off between speed and safety\n\n## Recommendations\n\nFinal notes.";
        let findings = extract_findings_from_output(output);
        assert!(!findings.is_empty());
        let tensions = findings
            .iter()
            .find(|f| f.contains("Key Tensions"))
            .expect("should extract Key Tensions section");
        assert!(tensions.contains("Expert A"));
        assert!(tensions.contains("Expert B"));
    }

    #[test]
    fn findings_extracts_actionable_section() {
        let output = "## Summary\n\nOverview here.\n\n## Actionable Items\n\n1. Fix the auth bypass\n2. Add rate limiting\n3. Update dependencies\n\n## Appendix\n\nExtra details.";
        let findings = extract_findings_from_output(output);
        assert!(
            findings.len() >= 2,
            "should extract both Summary and Actionable"
        );
        let actionable = findings
            .iter()
            .find(|f| f.contains("Actionable"))
            .expect("should extract Actionable section");
        assert!(actionable.contains("Fix the auth bypass"));
        assert!(actionable.contains("rate limiting"));
    }

    #[test]
    fn findings_extracts_individual_finding_without_colon_or_dashes() {
        let output = "## Overview\n\nSome text\n\n### Finding 1\n\nThis finding has no colon or dashes in its header.\nIt should still be extracted.\n\n### Finding 2\n\nAnother finding without punctuation.";
        let findings = extract_findings_from_output(output);
        assert!(
            !findings.is_empty(),
            "findings with plain headers should be extracted"
        );
        assert!(findings[0].contains("Finding 1"));
    }
}
