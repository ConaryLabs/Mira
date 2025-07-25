// backend/src/tools/mira_import/mod.rs

pub mod schema;
pub mod openai;
pub mod writer;

use schema::{ChatExport, MiraMessage};
use openai::batch_memory_eval;
use writer::insert_messages;
use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::qdrant::store::QdrantMemoryStore;

use anyhow::Result;

/// Imports all threads/messages from export using real store instances.
/// - Pass SqliteMemoryStore and QdrantMemoryStore when calling!
pub async fn import_conversations(
    export: ChatExport,
    sqlite_store: &SqliteMemoryStore,
    qdrant_store: &QdrantMemoryStore,
) -> Result<()> {
    for thread in &export.0 {
        // Use conversation_id or fallback to thread.title as session_id
        let thread_id = thread
            .conversation_id
            .clone()
            .or_else(|| thread.title.clone())
            .unwrap_or_else(|| "unknown-session".to_string());

        let messages = thread.flatten();
        // Batch process all messages for memory_eval
        let api_key = std::env::var("OPENAI_API_KEY")?;
        let mira_msgs: Vec<MiraMessage> = messages.clone();
        let evals = batch_memory_eval(&mira_msgs, &api_key).await?;
        // Insert into SQLite/Qdrant
        insert_messages(sqlite_store, qdrant_store, &thread_id, &messages, &evals).await?;
    }
    Ok(())
}
