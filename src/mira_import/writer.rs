// backend/src/tools/mira_import/writer.rs

use super::openai::MemoryEvalResult;
use super::schema::MiraMessage;
use anyhow::Result;
use chrono::Utc;

use crate::memory::storage::qdrant::store::QdrantMemoryStore;
use crate::memory::storage::sqlite::store::SqliteMemoryStore;
use crate::memory::core::traits::MemoryStore;
use crate::memory::core::types::{MemoryEntry, MemoryType};

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

                // Robust memory extras
                head: None,
                is_code: None,
                lang: None,
                topics: None,

                // Phase 4 additions
                pinned: Some(false),
                subject_tag: None,                 // could derive from eval if you add it
                last_accessed: Some(Utc::now()),
            };

            // The `save` method now returns the entry, so we capture it.
            let saved_memory = sqlite_store.save(&memory).await?;
            if let Some(s) = saved_memory.salience {
                if s >= 3.0 && saved_memory.embedding.is_some() {
                    // Qdrant also needs the updated entry
                    qdrant_store.save(&saved_memory).await?;
                }
            }
        }
    }
    Ok(())
}
