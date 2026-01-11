// crates/mira-server/src/mcp/extraction.rs
// Extract meaningful outcomes from tool calls and store as project memories

use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::db::Database;
use crate::embeddings::Embeddings;
use crate::web::deepseek::{DeepSeekClient, Message};

/// Tools that produce outcomes worth remembering
const EXTRACTABLE_TOOLS: &[&str] = &[
    "task",              // Task completions and updates
    "goal",              // Goal progress and milestones
    "semantic_code_search", // Code discoveries
    "find_callers",      // Call graph insights
    "find_callees",      // Call graph insights
    "check_capability",  // Feature existence checks
];

/// Prompt for extracting actionable outcomes from tool results
const TOOL_EXTRACTION_PROMPT: &str = r#"Extract any significant outcomes from this tool call that would be useful to recall later. Focus on:
- Decisions made (chose X over Y, will use Z approach)
- Important discoveries (found that X is in Y, discovered Z pattern)
- Task/goal completions with context
- Architectural insights about the codebase

Return ONLY a JSON array of outcomes. Each should have:
- "content": clear statement of what was decided/found/completed
- "category": one of "decision", "discovery", "completion", "architecture"
- "key": optional unique identifier for deduplication

Example output:
[
  {"content": "Decided to use tokio channels for background processing", "category": "decision", "key": "bg_processing_approach"},
  {"content": "Found authentication logic in src/auth/middleware.rs", "category": "discovery"}
]

Rules:
- Only extract NEW information, not routine queries
- Skip if tool just listed/searched without actionable outcome
- Skip trivial operations (listing tasks, checking status)
- Focus on things the user would want to recall in future sessions

If nothing worth remembering, return: []

Respond with ONLY the JSON array, no other text."#;

/// Spawn background extraction for a tool call
pub fn spawn_tool_extraction(
    db: Arc<Database>,
    embeddings: Option<Arc<Embeddings>>,
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
            &db,
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
    db: &Database,
    embeddings: Option<&Arc<Embeddings>>,
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

    let messages = vec![
        Message::system(TOOL_EXTRACTION_PROMPT.to_string()),
        Message::user(tool_context),
    ];

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

        let id = db.store_memory(
            project_id,
            Some(&key),
            &outcome.content,
            &outcome.category,
            Some("tool_outcome"),
            0.85, // Slightly lower than manual, higher than chat extraction
        )?;

        // Store embedding if available (also marks fact as having embedding)
        if let Some(embeddings) = embeddings {
            if let Ok(embedding) = embeddings.embed(&outcome.content).await {
                if let Err(e) = db.store_fact_embedding(id, &outcome.content, &embedding) {
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
