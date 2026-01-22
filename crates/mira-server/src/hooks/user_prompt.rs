// src/hooks/user_prompt.rs
// UserPromptSubmit hook handler for proactive context injection

use anyhow::Result;
use crate::db::Database;
use crate::embeddings::Embeddings;
use crate::hooks::{read_hook_input, write_hook_output};
use std::path::PathBuf;
use std::sync::Arc;

/// Get database path (same as other hooks)
fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}

/// Get embeddings client if available
fn get_embeddings() -> Option<Arc<Embeddings>> {
    std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .map(|key| Arc::new(Embeddings::new(key)))
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
    let db = Arc::new(Database::open(&db_path)?);
    let embeddings = get_embeddings();
    let manager = crate::context::ContextInjectionManager::new(db, embeddings);

    // Get relevant context
    let context = manager.get_context_for_message(user_message, session_id).await;

    if !context.is_empty() {
        eprintln!("[mira] Injecting context ({} chars)", context.len());
        write_hook_output(&serde_json::json!({
            "systemMessage": context
        }));
    } else {
        // No context to inject - output empty object
        write_hook_output(&serde_json::json!({}));
    }

    Ok(())
}