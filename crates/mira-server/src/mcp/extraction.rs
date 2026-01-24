// crates/mira-server/src/mcp/extraction.rs
// Extract meaningful outcomes from tool calls and store as project memories

use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::db::pool::DatabasePool;
use crate::db::{store_memory_sync, store_fact_embedding_sync, StoreMemoryParams};
use crate::embeddings::EmbeddingClient;
use crate::llm::{DeepSeekClient, PromptBuilder};
use crate::search::embedding_to_bytes;

/// Tools that produce outcomes worth remembering
const EXTRACTABLE_TOOLS: &[&str] = &[
    "task",              // Task completions and updates
    "goal",              // Goal progress and milestones
    "semantic_code_search", // Code discoveries
    "find_callers",      // Call graph insights
    "find_callees",      // Call graph insights
    "check_capability",  // Feature existence checks
];


/// Spawn background extraction for a tool call
pub fn spawn_tool_extraction(
    pool: Arc<DatabasePool>,
    embeddings: Option<Arc<EmbeddingClient>>,
    deepseek: Option<Arc<DeepSeekClient>>,
    project_id: Option<i64>,
    tool_name: String,
    args: String,
    result: String,
) {
    // Only extract from certain tools
    if !EXTRACTABLE_TOOLS.contains(&tool_name.as_str()) {
        debug!("Tool extraction: skipping {} (not extractable)", tool_name);
        return;
    }

    // Skip if result is too short (likely just a status message)
    if result.len() < 50 {
        debug!("Tool extraction: skipping {} (result too short: {} chars)", tool_name, result.len());
        return;
    }

    // Need DeepSeek for extraction
    let Some(deepseek) = deepseek else {
        debug!("Tool extraction: skipping {} (no DeepSeek configured)", tool_name);
        return;
    };

    info!("Tool extraction: spawning extraction for {} ({} chars)", tool_name, result.len());

    tokio::spawn(async move {
        if let Err(e) = extract_and_store(
            &pool,
            embeddings.as_ref(),
            &deepseek,
            project_id,
            &tool_name,
            &args,
            &result,
        ).await {
            warn!("Tool extraction failed for {}: {}", tool_name, e);
        }
    });
}

/// Perform extraction and store results
async fn extract_and_store(
    pool: &DatabasePool,
    embeddings: Option<&Arc<EmbeddingClient>>,
    deepseek: &DeepSeekClient,
    project_id: Option<i64>,
    tool_name: &str,
    args: &str,
    result: &str,
) -> anyhow::Result<()> {
    // Build context for extraction
    let tool_context = format!(
        "Tool: {}\nArguments: {}\nResult:\n{}",
        tool_name,
        args,
        // Truncate very long results
        if result.len() > 3000 { &result[..3000] } else { result }
    );

    let messages = PromptBuilder::for_tool_extraction()
        .build_messages(tool_context);

    // Call DeepSeek for extraction
    let response = deepseek.chat(messages, None).await?;

    let content = response.content
        .ok_or_else(|| anyhow::anyhow!("No content in extraction response"))?;

    // Parse JSON array
    let outcomes: Vec<ExtractedOutcome> = match serde_json::from_str(&content) {
        Ok(o) => o,
        Err(e) => {
            debug!("Failed to parse tool extraction response: {} - content: {}", e, content);
            return Ok(());
        }
    };

    if outcomes.is_empty() {
        debug!("No outcomes extracted from {} call", tool_name);
        return Ok(());
    }

    info!("Extracted {} outcomes from {} call", outcomes.len(), tool_name);

    // Store each outcome
    for outcome in outcomes {
        let key = outcome.key
            .map(|k| format!("tool:{}:{}", tool_name, k))
            .unwrap_or_else(|| format!("tool:{}:{}", tool_name, uuid::Uuid::new_v4()));

        // Store memory using pool
        let content_clone = outcome.content.clone();
        let key_clone = key.clone();
        let category_clone = outcome.category.clone();
        let id = pool.interact(move |conn| {
            store_memory_sync(conn, StoreMemoryParams {
                project_id,
                key: Some(&key_clone),
                content: &content_clone,
                fact_type: "tool_outcome",
                category: Some(&category_clone),
                confidence: 0.85,
                session_id: None,
                user_id: None,
                scope: "project",
            }).map_err(|e| anyhow::anyhow!("{}", e))
        }).await?;

        // Store embedding if available (also marks fact as having embedding)
        if let Some(embeddings) = embeddings {
            if let Ok(embedding) = embeddings.embed(&outcome.content).await {
                let embedding_bytes = embedding_to_bytes(&embedding);
                let content_for_embed = outcome.content.clone();
                if let Err(e) = pool.interact(move |conn| {
                    store_fact_embedding_sync(conn, id, &content_for_embed, &embedding_bytes)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                }).await {
                    warn!("Failed to store embedding for outcome {}: {}", id, e);
                }
            }
        }

        debug!("Stored tool outcome: {} (category: {})", outcome.content, outcome.category);
    }

    Ok(())
}

/// An outcome extracted from a tool call
#[derive(Debug, serde::Deserialize)]
struct ExtractedOutcome {
    content: String,
    category: String,
    key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // EXTRACTABLE_TOOLS tests
    // ============================================================================

    #[test]
    fn test_extractable_tools_contains_task() {
        assert!(EXTRACTABLE_TOOLS.contains(&"task"));
    }

    #[test]
    fn test_extractable_tools_contains_goal() {
        assert!(EXTRACTABLE_TOOLS.contains(&"goal"));
    }

    #[test]
    fn test_extractable_tools_contains_search() {
        assert!(EXTRACTABLE_TOOLS.contains(&"semantic_code_search"));
    }

    #[test]
    fn test_extractable_tools_contains_call_graph() {
        assert!(EXTRACTABLE_TOOLS.contains(&"find_callers"));
        assert!(EXTRACTABLE_TOOLS.contains(&"find_callees"));
    }

    #[test]
    fn test_extractable_tools_contains_capability() {
        assert!(EXTRACTABLE_TOOLS.contains(&"check_capability"));
    }

    #[test]
    fn test_extractable_tools_excludes_common() {
        // These tools should NOT be extractable
        assert!(!EXTRACTABLE_TOOLS.contains(&"remember"));
        assert!(!EXTRACTABLE_TOOLS.contains(&"recall"));
        assert!(!EXTRACTABLE_TOOLS.contains(&"index"));
        assert!(!EXTRACTABLE_TOOLS.contains(&"session_start"));
    }

    // ============================================================================
    // ExtractedOutcome deserialization tests
    // ============================================================================

    #[test]
    fn test_extracted_outcome_deserialize_full() {
        let json = r#"{"content": "Found auth module in src/auth", "category": "discovery", "key": "auth_location"}"#;
        let outcome: ExtractedOutcome = serde_json::from_str(json).unwrap();
        assert_eq!(outcome.content, "Found auth module in src/auth");
        assert_eq!(outcome.category, "discovery");
        assert_eq!(outcome.key, Some("auth_location".to_string()));
    }

    #[test]
    fn test_extracted_outcome_deserialize_no_key() {
        let json = r#"{"content": "Task completed successfully", "category": "progress"}"#;
        let outcome: ExtractedOutcome = serde_json::from_str(json).unwrap();
        assert_eq!(outcome.content, "Task completed successfully");
        assert_eq!(outcome.category, "progress");
        assert_eq!(outcome.key, None);
    }

    #[test]
    fn test_extracted_outcome_deserialize_null_key() {
        let json = r#"{"content": "Some insight", "category": "insight", "key": null}"#;
        let outcome: ExtractedOutcome = serde_json::from_str(json).unwrap();
        assert_eq!(outcome.key, None);
    }

    #[test]
    fn test_extracted_outcome_array() {
        let json = r#"[
            {"content": "First outcome", "category": "discovery"},
            {"content": "Second outcome", "category": "progress", "key": "task_123"}
        ]"#;
        let outcomes: Vec<ExtractedOutcome> = serde_json::from_str(json).unwrap();
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].content, "First outcome");
        assert_eq!(outcomes[1].key, Some("task_123".to_string()));
    }

    #[test]
    fn test_extracted_outcome_empty_array() {
        let json = "[]";
        let outcomes: Vec<ExtractedOutcome> = serde_json::from_str(json).unwrap();
        assert!(outcomes.is_empty());
    }

    #[test]
    fn test_extracted_outcome_missing_required_field() {
        let json = r#"{"content": "Only content"}"#;
        let result: Result<ExtractedOutcome, _> = serde_json::from_str(json);
        assert!(result.is_err()); // category is required
    }
}
