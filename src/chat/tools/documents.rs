//! Document management tools for Chat
//!
//! Thin wrapper delegating to core::ops::documents for shared implementation with MCP.

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::Path;
use std::sync::Arc;

use crate::core::ops::documents as core_docs;
use crate::core::{OpContext, SemanticSearch};

/// Document management tool implementations
pub struct DocumentTools<'a> {
    pub cwd: &'a Path,
    pub db: &'a Option<SqlitePool>,
    pub semantic: &'a Option<Arc<SemanticSearch>>,
}

impl<'a> DocumentTools<'a> {
    /// Consolidated document tool - handles list, search, get, delete
    pub async fn document(&self, args: &Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("list");
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        match action {
            "list" => {
                let ctx = OpContext::just_db(db.clone());
                let input = core_docs::ListDocumentsInput {
                    doc_type: args["doc_type"].as_str().map(String::from),
                    limit: args["limit"].as_i64().unwrap_or(20),
                };

                match core_docs::list_documents(&ctx, input).await {
                    Ok(docs) => {
                        let docs_json: Vec<Value> = docs
                            .into_iter()
                            .map(|d| {
                                json!({
                                    "id": d.id,
                                    "name": d.name,
                                    "doc_type": d.doc_type,
                                    "summary": d.summary,
                                    "chunk_count": d.chunk_count,
                                    "created_at": d.created_at,
                                })
                            })
                            .collect();
                        let count = docs_json.len();
                        Ok(json!({
                            "documents": docs_json,
                            "count": count,
                        })
                        .to_string())
                    }
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "search" => {
                let query = args["query"].as_str().unwrap_or("");
                if query.is_empty() {
                    return Ok("Error: query is required".into());
                }

                let limit = args["limit"].as_i64().unwrap_or(10) as usize;

                let ctx = if let Some(sem) = self.semantic.as_ref() {
                    OpContext::with_db_and_semantic(db.clone(), sem.clone())
                } else {
                    OpContext::just_db(db.clone())
                };

                let input = core_docs::SearchDocumentsInput {
                    query: query.to_string(),
                    limit,
                };

                match core_docs::search_documents(&ctx, input).await {
                    Ok(results) => {
                        let results_json: Vec<Value> = results
                            .into_iter()
                            .map(|r| {
                                json!({
                                    "content": r.content,
                                    "score": r.score,
                                    "document_id": r.document_id,
                                    "document_name": r.document_name,
                                    "doc_type": r.doc_type,
                                    "chunk_index": r.chunk_index,
                                })
                            })
                            .collect();
                        let count = results_json.len();
                        Ok(json!({
                            "query": query,
                            "results": results_json,
                            "count": count,
                        })
                        .to_string())
                    }
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "get" => {
                let document_id = args["document_id"].as_str().unwrap_or("");
                if document_id.is_empty() {
                    return Ok("Error: document_id is required".into());
                }

                let include_content = args["include_content"].as_bool().unwrap_or(false);
                let ctx = OpContext::just_db(db.clone());

                let input = core_docs::GetDocumentInput {
                    document_id: document_id.to_string(),
                    include_content,
                };

                match core_docs::get_document(&ctx, input).await {
                    Ok(Some(d)) => {
                        let mut result = json!({
                            "id": d.id,
                            "name": d.name,
                            "file_path": d.file_path,
                            "doc_type": d.doc_type,
                            "summary": d.summary,
                            "chunk_count": d.chunk_count,
                            "total_tokens": d.total_tokens,
                            "created_at": d.created_at,
                        });
                        if let Some(content) = d.content {
                            result["content"] = json!(content);
                        }
                        Ok(result.to_string())
                    }
                    Ok(None) => Ok(json!({"error": "Document not found"}).to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "delete" => {
                let document_id = args["document_id"].as_str().unwrap_or("");
                if document_id.is_empty() {
                    return Ok("Error: document_id is required".into());
                }

                let ctx = OpContext::just_db(db.clone());
                match core_docs::delete_document(&ctx, document_id).await {
                    Ok(true) => Ok(json!({
                        "status": "deleted",
                        "document_id": document_id,
                    })
                    .to_string()),
                    Ok(false) => Ok(json!({
                        "status": "not_found",
                        "document_id": document_id,
                    })
                    .to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            _ => Ok(format!(
                "Unknown action: {}. Use list/search/get/delete",
                action
            )),
        }
    }
}
