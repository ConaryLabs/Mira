//! Memory tools: remember and recall
//!
//! Thin wrapper that delegates to core::ops::memory for the actual implementation.
//! This keeps Chat-specific types separate from the shared core.

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::Arc;

use crate::core::SemanticSearch;

use crate::core::ops::memory as core_memory;
use crate::core::OpContext;

/// Memory tool implementations
pub struct MemoryTools<'a> {
    pub semantic: &'a Option<Arc<SemanticSearch>>,
    pub db: &'a Option<SqlitePool>,
}

impl<'a> MemoryTools<'a> {
    pub async fn remember(&self, args: &Value) -> Result<String> {
        let content = args["content"].as_str().unwrap_or("");
        let fact_type = args["fact_type"].as_str();
        let category = args["category"].as_str();

        if content.is_empty() {
            return Ok("Error: content is required".into());
        }

        // Need database for core ops
        let Some(db) = self.db else {
            return Ok("Error: database not available".into());
        };

        // Build OpContext
        let mut ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
            .with_db(db.clone());

        if let Some(semantic) = self.semantic {
            ctx = ctx.with_semantic(semantic.clone());
        }

        // Convert to core input
        let input = core_memory::RememberInput {
            content: content.to_string(),
            fact_type: fact_type.map(|s| s.to_string()),
            category: category.map(|s| s.to_string()),
            key: None,
            project_id: None, // mira-chat doesn't use project scoping
            source: "mira-chat".to_string(),
        };

        // Call core operation
        match core_memory::remember(&ctx, input).await {
            Ok(output) => Ok(json!({
                "status": "remembered",
                "key": output.key,
                "fact_type": output.fact_type,
                "category": output.category,
                "sqlite": true,
                "semantic_search": output.semantic_stored,
            })
            .to_string()),
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }

    pub async fn recall(&self, args: &Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;
        let fact_type = args["fact_type"].as_str();
        let category = args["category"].as_str();

        if query.is_empty() {
            return Ok("Error: query is required".into());
        }

        // Need database for core ops
        let Some(db) = self.db else {
            return Ok(json!({
                "results": [],
                "search_type": "none",
                "count": 0,
                "message": "Database not available",
            })
            .to_string());
        };

        // Build OpContext
        let mut ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
            .with_db(db.clone());

        if let Some(semantic) = self.semantic {
            ctx = ctx.with_semantic(semantic.clone());
        }

        // Convert to core input
        let input = core_memory::RecallInput {
            query: query.to_string(),
            limit: Some(limit),
            fact_type: fact_type.map(|s| s.to_string()),
            category: category.map(|s| s.to_string()),
            project_id: None,
        };

        // Call core operation
        match core_memory::recall(&ctx, input).await {
            Ok(output) if !output.facts.is_empty() => {
                let items: Vec<Value> = output
                    .facts
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

                Ok(json!({
                    "results": items,
                    "search_type": output.search_type,
                    "count": items.len(),
                })
                .to_string())
            }
            Ok(_) => Ok(json!({
                "results": [],
                "search_type": "none",
                "count": 0,
                "message": "No memories found matching query",
            })
            .to_string()),
            Err(e) => {
                tracing::warn!("Recall failed: {}", e);
                Ok(json!({
                    "results": [],
                    "search_type": "none",
                    "count": 0,
                    "message": format!("Recall failed: {}", e),
                })
                .to_string())
            }
        }
    }
}
