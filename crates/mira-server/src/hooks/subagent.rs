// crates/mira-server/src/hooks/subagent.rs
// SubagentStart and SubagentStop hook handlers

use crate::db::pool::DatabasePool;
use crate::hooks::{
    HookTimer, get_db_path, read_hook_input, resolve_project_id, write_hook_output,
};
use crate::utils::truncate_at_boundary;
use anyhow::Result;
use std::sync::Arc;

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
    let input = read_hook_input()?;
    let start_input = SubagentStartInput::from_json(&input);

    eprintln!(
        "[mira] SubagentStart hook triggered (type: {}, task: {:?})",
        start_input.subagent_type,
        start_input
            .task_description
            .as_deref()
            .map(|s| if s.len() > 50 {
                format!("{}...", &s[..50])
            } else {
                s.to_string()
            })
    );

    // Open database
    let db_path = get_db_path();
    let pool = match DatabasePool::open(&db_path).await {
        Ok(p) => Arc::new(p),
        Err(_) => {
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };

    // Get current project
    let Some(project_id) = resolve_project_id(&pool).await else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    let mut context_parts: Vec<String> = Vec::new();

    // Get active goals
    let goals = get_active_goals(&pool, project_id).await;
    if !goals.is_empty() {
        context_parts.push(format!("**Active Goals:**\n{}", goals.join("\n")));
    }

    // Get relevant memories based on task description
    if let Some(task) = &start_input.task_description {
        let memories = get_relevant_memories(&pool, project_id, task).await;
        if !memories.is_empty() {
            context_parts.push(format!("**Relevant Context:**\n{}", memories.join("\n")));
        }
    }

    // Build output, truncating to stay under token budget
    let output = if context_parts.is_empty() {
        serde_json::json!({})
    } else {
        let mut context = format!(
            "Mira context for this subagent:\n\n{}",
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
/// - If significant entities found (3+), stores a condensed memory
pub async fn run_stop() -> Result<()> {
    let _timer = HookTimer::start("SubagentStop");
    let input = read_hook_input()?;
    let stop_input = SubagentStopInput::from_json(&input);

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

    // Extract entities heuristically (no LLM calls)
    let entities = crate::entities::extract_entities_heuristic(&subagent_output);

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

    // Open database
    let db_path = get_db_path();
    let pool = match DatabasePool::open(&db_path).await {
        Ok(p) => Arc::new(p),
        Err(_) => {
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };

    // Get current project
    let Some(project_id) = resolve_project_id(&pool).await else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Build condensed summary from entities
    let entity_summary = build_entity_summary(&stop_input.subagent_type, &entities);

    // Store as a subagent discovery memory
    pool.try_interact("subagent discovery", move |conn| {
        crate::db::store_memory_sync(
            conn,
            crate::db::StoreMemoryParams {
                project_id: Some(project_id),
                key: None,
                content: &entity_summary,
                fact_type: "context",
                category: Some("subagent_discovery"),
                confidence: 0.6,
                session_id: None,
                user_id: None,
                scope: "project",
                branch: None,
                team_id: None,
            },
        )?;
        Ok(())
    })
    .await;

    write_hook_output(&serde_json::json!({}));
    Ok(())
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
    parts.push(format!("[Subagent:{}]", subagent_type));

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

/// Get active goals for context injection
async fn get_active_goals(pool: &Arc<DatabasePool>, project_id: i64) -> Vec<String> {
    let pool_clone = pool.clone();

    let result = pool_clone
        .interact(move |conn| {
            let sql = r#"
                SELECT title, status, progress_percent
                FROM goals
                WHERE project_id = ?
                  AND status IN ('planning', 'in_progress')
                ORDER BY
                    CASE priority
                        WHEN 'critical' THEN 1
                        WHEN 'high' THEN 2
                        WHEN 'medium' THEN 3
                        ELSE 4
                    END,
                    created_at DESC
                LIMIT 3
            "#;

            let mut stmt = conn.prepare(sql)?;
            let goals: Vec<String> = stmt
                .query_map(rusqlite::params![project_id], |row| {
                    let title: String = row.get(0)?;
                    let status: String = row.get(1)?;
                    let progress: i32 = row.get(2)?;
                    Ok(format!("- {} [{}] ({}%)", title, status, progress))
                })?
                .filter_map(crate::db::log_and_discard)
                .collect();

            Ok::<_, anyhow::Error>(goals)
        })
        .await;

    result.unwrap_or_default()
}

/// Get memories relevant to the task using embedding search with keyword fallback.
///
/// Tries semantic search first if an embedding client can be created, then falls
/// back to the original keyword-based LIKE matching.
async fn get_relevant_memories(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    task: &str,
) -> Vec<String> {
    // Try embedding-based recall first
    if let Some(memories) = try_semantic_recall(pool, project_id, task).await
        && !memories.is_empty()
    {
        return memories;
    }

    // Fall back to keyword matching
    get_keyword_memories(pool, project_id, task).await
}

/// Attempt semantic recall using embeddings. Returns None if embeddings unavailable.
async fn try_semantic_recall(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    task: &str,
) -> Option<Vec<String>> {
    use crate::config::{ApiKeys, EmbeddingsConfig};
    use crate::embeddings::EmbeddingClient;
    use crate::entities::extract_entities_heuristic;
    use crate::search::embedding_to_bytes;

    // Create embedding client directly (hooks don't have ToolContext)
    let emb =
        EmbeddingClient::from_config(&ApiKeys::from_env(), &EmbeddingsConfig::from_env(), None)?;

    // Embed the task description
    let query_embedding = emb.embed(task).await.ok()?;
    let embedding_bytes = embedding_to_bytes(&query_embedding);

    // Extract entities from query for boosting
    let query_entity_names: Vec<String> = extract_entities_heuristic(task)
        .into_iter()
        .map(|e| e.canonical_name)
        .collect();

    let pool_clone = pool.clone();
    let result: Vec<crate::db::RecallRow> = pool_clone
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(crate::db::recall_semantic_with_entity_boost_sync(
                conn,
                &embedding_bytes,
                Some(project_id),
                None, // user_id - not available in hook context
                None, // team_id
                None, // current_branch
                &query_entity_names,
                5, // fetch 5, we'll filter and take top 3
            )?)
        })
        .await
        .ok()?;

    // Filter low-quality results and format
    let memories: Vec<String> = result
        .into_iter()
        .filter(|(_, _, distance, _, _)| *distance < 0.7)
        .take(3)
        .map(|(_, ref content, _, _, _)| format_memory_line(content))
        .collect();

    Some(memories)
}

/// Format a memory line with fact_type prefix and truncation.
/// Since semantic search returns content without fact_type, we infer from content.
fn format_memory_line(content: &str) -> String {
    let truncated = if content.len() > 150 {
        format!("{}...", &content[..150])
    } else {
        content.to_string()
    };
    format!("- {}", truncated)
}

/// Original keyword-based memory matching (fallback)
async fn get_keyword_memories(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    task: &str,
) -> Vec<String> {
    let pool_clone = pool.clone();
    let task = task.to_string();

    // Extract keywords from task for matching
    let keywords: Vec<String> = task
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .take(5)
        .map(|s| s.to_string())
        .collect();

    if keywords.is_empty() {
        return Vec::new();
    }

    let result = pool_clone
        .interact(move |conn| {
            // Build a simple keyword match query
            let like_clauses: Vec<String> = keywords
                .iter()
                .map(|_| "content LIKE '%' || ? || '%'".to_string())
                .collect();
            let where_clause = like_clauses.join(" OR ");

            let sql = format!(
                r#"
                SELECT content, fact_type
                FROM memory_facts
                WHERE project_id = ?
                  AND ({})
                ORDER BY
                    CASE fact_type
                        WHEN 'decision' THEN 1
                        WHEN 'preference' THEN 2
                        ELSE 3
                    END,
                    created_at DESC
                LIMIT 3
            "#,
                where_clause
            );

            let mut stmt = conn.prepare(&sql)?;

            // Build params: project_id + all keywords
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(project_id)];
            for kw in &keywords {
                params.push(Box::new(kw.clone()));
            }
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params.iter().map(|b| b.as_ref()).collect();

            let memories: Vec<String> = stmt
                .query_map(params_refs.as_slice(), |row| {
                    let content: String = row.get(0)?;
                    let fact_type: Option<String> = row.get(1)?;
                    let prefix = match fact_type.as_deref() {
                        Some("decision") => "[Decision]",
                        Some("preference") => "[Preference]",
                        _ => "[Context]",
                    };
                    // Truncate long content
                    let content = if content.len() > 150 {
                        format!("{}...", &content[..150])
                    } else {
                        content
                    };
                    Ok(format!("{} {}", prefix, content))
                })?
                .filter_map(crate::db::log_and_discard)
                .collect();

            Ok::<_, anyhow::Error>(memories)
        })
        .await;

    result.unwrap_or_default()
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
        assert!(summary.contains("[Subagent:Explore]"));
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
        assert!(summary.contains("[Subagent:Plan]"));
        assert!(!summary.contains("Files:"));
        assert!(summary.contains("Identifiers:"));
    }

    #[test]
    fn format_memory_line_truncates_long_content() {
        let short = "Short memory";
        assert_eq!(format_memory_line(short), "- Short memory");

        let long = "A".repeat(200);
        let result = format_memory_line(&long);
        assert!(result.ends_with("..."));
        // "- " prefix (2) + 150 chars + "..." (3) = 155
        assert_eq!(result.len(), 155);
    }
}
