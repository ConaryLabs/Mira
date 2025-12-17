//! Memory tools: remember and recall

use anyhow::Result;
use chrono::Utc;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::semantic::{SemanticSearch, COLLECTION_MEMORY};

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

        let now = Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();

        // Generate key from content (first 50 chars, normalized)
        let key: String = content
            .chars()
            .take(50)
            .collect::<String>()
            .to_lowercase()
            .replace(|c: char| !c.is_alphanumeric() && c != ' ', "")
            .trim()
            .to_string();

        // Store in SQLite if available
        if let Some(db) = self.db {
            let _ = sqlx::query(
                r#"
                INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, times_used, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, 'mira-chat', 1.0, 0, $6, $6)
                ON CONFLICT(key) DO UPDATE SET
                    value = excluded.value,
                    fact_type = excluded.fact_type,
                    category = COALESCE(excluded.category, memory_facts.category),
                    updated_at = excluded.updated_at
            "#,
            )
            .bind(&id)
            .bind(fact_type)
            .bind(&key)
            .bind(content)
            .bind(category)
            .bind(now)
            .execute(db)
            .await;
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
            "semantic_search": semantic_stored,
        })
        .to_string())
    }

    pub async fn recall(&self, args: &Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;
        let fact_type = args["fact_type"].as_str();

        if query.is_empty() {
            return Ok("Error: query is required".into());
        }

        // Try semantic search first
        if let Some(semantic) = self.semantic {
            if semantic.is_available() {
                // Build filter for fact_type if specified
                let filter = fact_type.map(|ft| {
                    qdrant_client::qdrant::Filter::must([
                        qdrant_client::qdrant::Condition::matches("fact_type", ft.to_string()),
                    ])
                });

                match semantic.search(COLLECTION_MEMORY, query, limit, filter).await {
                    Ok(results) if !results.is_empty() => {
                        let items: Vec<Value> = results
                            .iter()
                            .map(|r| {
                                json!({
                                    "content": r.content,
                                    "score": r.score,
                                    "search_type": "semantic",
                                    "fact_type": r.metadata.get("fact_type"),
                                    "category": r.metadata.get("category"),
                                })
                            })
                            .collect();

                        return Ok(json!({
                            "results": items,
                            "search_type": "semantic",
                            "count": items.len(),
                        })
                        .to_string());
                    }
                    Ok(_) => {
                        // Fall through to SQLite
                    }
                    Err(e) => {
                        tracing::warn!("Semantic search failed: {}", e);
                    }
                }
            }
        }

        // Fallback to SQLite text search
        if let Some(db) = self.db {
            let pattern = format!("%{}%", query);

            let rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
                r#"
                SELECT fact_type, key, value, category
                FROM memory_facts
                WHERE value LIKE $1 OR key LIKE $1 OR category LIKE $1
                ORDER BY times_used DESC, updated_at DESC
                LIMIT $2
                "#,
            )
            .bind(&pattern)
            .bind(limit as i64)
            .fetch_all(db)
            .await
            .unwrap_or_default();

            if !rows.is_empty() {
                let items: Vec<Value> = rows
                    .iter()
                    .map(|(ft, key, value, cat)| {
                        json!({
                            "content": value,
                            "search_type": "text",
                            "fact_type": ft,
                            "key": key,
                            "category": cat,
                        })
                    })
                    .collect();

                return Ok(json!({
                    "results": items,
                    "search_type": "text",
                    "count": items.len(),
                })
                .to_string());
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
