// src/services/context.rs

use std::sync::Arc;
use anyhow::Result;
use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::recall::{build_context, RecallContext};

#[derive(Clone)]
pub struct ContextService {
    sqlite_store: Arc<SqliteMemoryStore>,
    qdrant_store: Arc<QdrantMemoryStore>,
}

impl ContextService {
    pub fn new(
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
    ) -> Self {
        Self {
            sqlite_store,
            qdrant_store,
        }
    }
    
    pub async fn build_context(
        &self,
        session_id: &str,
        embedding: Option<&[f32]>,
        _project_id: Option<&str>,
    ) -> Result<RecallContext> {
        // Get limits from environment
        let recent_messages = std::env::var("MIRA_CONTEXT_RECENT_MESSAGES")
            .ok().and_then(|s| s.parse().ok()).unwrap_or(30);
        let semantic_matches = std::env::var("MIRA_CONTEXT_SEMANTIC_MATCHES")
            .ok().and_then(|s| s.parse().ok()).unwrap_or(15);
        
        let context = build_context(
            session_id,
            embedding,
            recent_messages,
            semantic_matches,
            self.sqlite_store.as_ref(),
            self.qdrant_store.as_ref(),
        )
        .await
        .unwrap_or_else(|e| {
            eprintln!("⚠️ Failed to build recall context: {:?}", e);
            RecallContext::new(vec![], vec![])
        });
        
        // Log semantic matches if any
        if !context.semantic.is_empty() {
            eprintln!("🔍 Semantic matches:");
            for (i, msg) in context.semantic.iter().take(5).enumerate() {
                eprintln!("  {}. [salience: {}] {}", 
                    i+1, 
                    msg.salience.unwrap_or(0.0),
                    msg.content.chars().take(80).collect::<String>()
                );
            }
        }
        
        Ok(context)
    }
}
