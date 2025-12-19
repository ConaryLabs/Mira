//! Memory tools: remember and recall
//!
//! Uses mira_core::memory for shared logic, keeps only mira-chat specific wrappers.

use anyhow::Result;
use mira_core::{make_memory_key, recall_memory_facts, upsert_memory_fact, MemoryScope, RecallConfig};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;

use mira_core::semantic::SemanticSearch;

use crate::chat::COLLECTION_MEMORY;

/// Memory tool implementations
pub struct MemoryTools<'a> {
    pub semantic: &'a Option<Arc<SemanticSearch>>,
    pub db: &'a Option<SqlitePool>,
}

impl<'a> MemoryTools<'a> {
    pub async fn remember(&self, args: &Value) -> Result<String> {
        let content = args["content"].as_str().unwrap_or("");
        let fact_type = args["fact_type"].as_str().unwrap_or("general");
        let category = args["category"].as_str();

        if content.is_empty() {
            return Ok("Error: content is required".into());
        }

        // Generate key using shared function
        let key = make_memory_key(content);

        // Store in SQLite using shared upsert
        let mut sqlite_stored = false;
        if let Some(db) = self.db {
            match upsert_memory_fact(
                db,
                MemoryScope::Global, // mira-chat doesn't use project scoping
                &key,
                content,
                fact_type,
                category,
                "mira-chat",
            )
            .await
            {
                Ok(_id) => {
                    sqlite_stored = true;
                }
                Err(e) => {
                    tracing::warn!("Failed to store memory in SQLite: {}", e);
                }
            }
        }

        // Store in Qdrant for semantic search
        let mut semantic_stored = false;
        if let Some(semantic) = self.semantic {
            if semantic.is_available() {
                let mut metadata = HashMap::new();
                metadata.insert("fact_type".into(), json!(fact_type));
                metadata.insert("key".into(), json!(key));
                if let Some(cat) = category {
                    metadata.insert("category".into(), json!(cat));
                }

                // Generate ID for Qdrant storage
                let id = uuid::Uuid::new_v4().to_string();
                if let Err(e) = semantic.store(COLLECTION_MEMORY, &id, content, metadata).await {
                    tracing::warn!("Failed to store in Qdrant: {}", e);
                } else {
                    semantic_stored = true;
                }
            }
        }

        Ok(json!({
            "status": "remembered",
            "key": key,
            "fact_type": fact_type,
            "category": category,
            "sqlite": sqlite_stored,
            "semantic_search": semantic_stored,
        })
        .to_string())
    }

    pub async fn recall(&self, args: &Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;
        let fact_type = args["fact_type"].as_str();
        let category = args["category"].as_str();

        if query.is_empty() {
            return Ok("Error: query is required".into());
        }

        // Use shared recall function if we have a database
        if let Some(db) = self.db {
            let cfg = RecallConfig {
                collection: COLLECTION_MEMORY,
                fact_type,
                category,
            };

            // Get semantic reference if available
            let semantic_ref = self.semantic.as_ref().map(|arc| arc.as_ref());

            match recall_memory_facts(db, semantic_ref, cfg, query, limit, None).await {
                Ok(facts) if !facts.is_empty() => {
                    let items: Vec<Value> = facts
                        .iter()
                        .map(|f| {
                            json!({
                                "content": f.value,
                                "key": f.key,
                                "fact_type": f.fact_type,
                                "category": f.category,
                                "score": f.score,
                                "search_type": f.search_type.as_str(),
                            })
                        })
                        .collect();

                    return Ok(json!({
                        "results": items,
                        "search_type": facts[0].search_type.as_str(),
                        "count": items.len(),
                    })
                    .to_string());
                }
                Ok(_) => {
                    // No results
                }
                Err(e) => {
                    tracing::warn!("Recall failed: {}", e);
                }
            }
        }

        Ok(json!({
            "results": [],
            "search_type": "none",
            "count": 0,
            "message": "No memories found matching query",
        })
        .to_string())
    }
}
