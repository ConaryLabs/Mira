// backend/src/tools/mira_import/writer.rs

use super::schema::MiraMessage;
use super::openai::MemoryEvalResult;
use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::types::{MemoryEntry, MemoryType};
use crate::memory::traits::MemoryStore; // <-- Add this for trait methods!
use chrono::Utc;
use anyhow::Result;

/// Import a batch of messages into both SQLite and Qdrant stores as needed.
/// - `sqlite_store`: the real SqliteMemoryStore instance
/// - `qdrant_store`: the real QdrantMemoryStore instance
/// - `thread_id`: session/conversation id for this batch
/// - `messages`: all messages to import
/// - `evals`: evaluated memory metadata (indexed by message_id)
pub async fn insert_messages(
    sqlite_store: &SqliteMemoryStore,
    qdrant_store: &QdrantMemoryStore,
    thread_id: &str,
    messages: &[MiraMessage],
    evals: &std::collections::HashMap<String, MemoryEvalResult>,
) -> Result<()> {
    for msg in messages {
        if let Some(eval) = evals.get(&msg.message_id) {
            let memory = MemoryEntry {
                id: None,
                session_id: thread_id.to_string(),
                role: msg.role.clone(),
                content: msg.content.clone(),
                timestamp: msg.create_time.unwrap_or_else(Utc::now),
                embedding: Some(eval.embedding.clone()),
                salience: Some(eval.salience),
                tags: Some(eval.tags.clone()),
                summary: None,
                memory_type: Some(match eval.memory_type.to_lowercase().as_str() {
                    "feeling" => MemoryType::Feeling,
                    "fact" => MemoryType::Fact,
                    "joke" => MemoryType::Joke,
                    "promise" => MemoryType::Promise,
                    "event" => MemoryType::Event,
                    _ => MemoryType::Other,
                }),
                logprobs: None,
                moderation_flag: None,
                system_fingerprint: None,
            };
            sqlite_store.save(&memory).await?;
            if let Some(s) = memory.salience {
                if s >= 3.0 && memory.embedding.is_some() {
                    qdrant_store.save(&memory).await?;
                }
            }
        }
    }
    Ok(())
}
