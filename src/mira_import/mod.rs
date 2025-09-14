// src/mira_import/mod.rs

pub mod schema;
pub mod openai;
pub mod writer;

use schema::{ChatExport, MiraMessage};
use openai::batch_memory_eval;
use writer::insert_messages;
use crate::memory::storage::sqlite::store::SqliteMemoryStore;
use crate::memory::storage::qdrant::store::QdrantMemoryStore;

use anyhow::Result;

pub async fn import_conversations(
    export: ChatExport,
    sqlite_store: &SqliteMemoryStore,
    qdrant_store: &QdrantMemoryStore,
) -> Result<()> {
    for thread in &export.0 {
        let thread_id = thread
            .conversation_id
            .clone()
            .or_else(|| thread.title.clone())
            .unwrap_or_else(|| "unknown-session".to_string());

        let messages = thread.flatten();
        let api_key = std::env::var("OPENAI_API_KEY")?;
        let mira_msgs: Vec<MiraMessage> = messages.clone();
        let evals = batch_memory_eval(&mira_msgs, &api_key).await?;
        
        insert_messages(sqlite_store, qdrant_store, &thread_id, &messages, &evals).await?;
    }
    Ok(())
}
