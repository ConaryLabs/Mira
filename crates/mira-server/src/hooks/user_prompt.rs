// src/hooks/user_prompt.rs
// UserPromptSubmit hook handler for proactive context injection

use anyhow::Result;
use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::hooks::{read_hook_input, write_hook_output};
use std::path::PathBuf;
use std::sync::Arc;

/// Get database path (same as other hooks)
fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}

/// Get embeddings client if available (with pool for usage tracking)
fn get_embeddings(pool: Option<Arc<DatabasePool>>) -> Option<Arc<EmbeddingClient>> {
    EmbeddingClient::from_env(pool).map(Arc::new)
}

/// Run UserPromptSubmit hook
pub async fn run() -> Result<()> {
    let input = read_hook_input()?;

    // Extract user message and session ID
    let user_message = input
        .get("prompt")
        .or_else(|| input.get("user_message"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let session_id = input
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    eprintln!("[mira] UserPromptSubmit hook triggered (session: {}, message length: {})",
        &session_id[..session_id.len().min(8)],
        user_message.len()
    );
    eprintln!("[mira] Hook input keys: {:?}", input.as_object().map(|obj| obj.keys().collect::<Vec<_>>()));

    // Open database and create context injection manager
    let db_path = get_db_path();
    let pool = Arc::new(DatabasePool::open(std::path::Path::new(&db_path)).await?);
    let embeddings = get_embeddings(Some(pool.clone()));
    let manager = crate::context::ContextInjectionManager::new(pool, embeddings).await;

    // Get relevant context with metadata
    let result = manager.get_context_for_message(user_message, session_id).await;

    if result.has_context() {
        eprintln!("[mira] {}", result.summary());
        write_hook_output(&serde_json::json!({
            "systemMessage": result.context,
            "metadata": {
                "sources": result.sources,
                "from_cache": result.from_cache
            }
        }));
    } else {
        if let Some(reason) = &result.skip_reason {
            eprintln!("[mira] Context injection skipped: {}", reason);
        }
        // No context to inject - output empty object
        write_hook_output(&serde_json::json!({}));
    }

    Ok(())
}