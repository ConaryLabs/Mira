// src/tools/documents.rs
// Document storage and search tools with semantic search

use sqlx::sqlite::SqlitePool;

use super::semantic::{SemanticSearch, COLLECTION_DOCS};

// === Parameter structs for consolidated document tool ===

pub struct ListDocumentsParams {
    pub doc_type: Option<String>,
    pub limit: Option<i64>,
}

/// List documents
pub async fn list_documents(db: &SqlitePool, req: ListDocumentsParams) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(20);

    let query = r#"
        SELECT id, name, file_path, doc_type, summary, chunk_count, total_tokens,
               datetime(created_at, 'unixepoch', 'localtime') as created_at
        FROM documents
        WHERE ($1 IS NULL OR doc_type = $1)
        ORDER BY created_at DESC
        LIMIT $2
    "#;

    let rows = sqlx::query_as::<_, (String, String, Option<String>, String, Option<String>, i64, i64, String)>(query)
        .bind(&req.doc_type)
        .bind(limit)
        .fetch_all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(id, name, file_path, doc_type, summary, chunk_count, total_tokens, created_at)| {
            serde_json::json!({
                "id": id,
                "name": name,
                "file_path": file_path,
                "doc_type": doc_type,
                "summary": summary,
                "chunk_count": chunk_count,
                "total_tokens": total_tokens,
                "created_at": created_at,
            })
        })
        .collect())
}

/// Search documents - uses semantic search if available
pub async fn search_documents(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    query_str: &str,
    limit: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = limit.unwrap_or(10) as usize;

    // Try semantic search first if available
    if semantic.is_available() {
        match semantic.search(COLLECTION_DOCS, query_str, limit, None).await {
            Ok(results) if !results.is_empty() => {
                return Ok(results.into_iter().map(|r| {
                    serde_json::json!({
                        "content": r.content,
                        "score": r.score,
                        "search_type": "semantic",
                        "document_id": r.metadata.get("document_id"),
                        "document_name": r.metadata.get("document_name"),
                        "doc_type": r.metadata.get("doc_type"),
                        "chunk_index": r.metadata.get("chunk_index"),
                    })
                }).collect());
            }
            Ok(_) => {
                tracing::debug!("No semantic results for document query: {}", query_str);
            }
            Err(e) => {
                tracing::warn!("Semantic document search failed, falling back to text: {}", e);
            }
        }
    }

    // Fallback to SQLite text search
    let search_pattern = format!("%{}%", query_str);

    let sql_query = r#"
        SELECT dc.id, dc.document_id, dc.chunk_index, dc.content,
               d.name, d.doc_type
        FROM document_chunks dc
        JOIN documents d ON dc.document_id = d.id
        WHERE dc.content LIKE $1
        ORDER BY d.created_at DESC, dc.chunk_index
        LIMIT $2
    "#;

    let rows = sqlx::query_as::<_, (String, String, i64, String, String, String)>(sql_query)
        .bind(&search_pattern)
        .bind(limit as i64)
        .fetch_all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(chunk_id, doc_id, chunk_idx, content, doc_name, doc_type)| {
            let display_content = if content.len() > 500 {
                if let Some(pos) = content.to_lowercase().find(&query_str.to_lowercase()) {
                    let start = pos.saturating_sub(100);
                    let end = (pos + query_str.len() + 100).min(content.len());
                    let snippet = &content[start..end];
                    if start > 0 { format!("...{}", snippet) } else { snippet.to_string() }
                } else {
                    format!("{}...", &content[..500])
                }
            } else {
                content
            };

            serde_json::json!({
                "chunk_id": chunk_id,
                "document_id": doc_id,
                "document_name": doc_name,
                "doc_type": doc_type,
                "chunk_index": chunk_idx,
                "content": display_content,
                "search_type": "text",
            })
        })
        .collect())
}

/// Get a specific document
pub async fn get_document(db: &SqlitePool, document_id: &str, include_content: bool) -> anyhow::Result<Option<serde_json::Value>> {
    let doc_query = r#"
        SELECT id, name, file_path, doc_type, content, summary, chunk_count, total_tokens, metadata,
               datetime(created_at, 'unixepoch', 'localtime') as created_at
        FROM documents
        WHERE id = $1
    "#;

    let doc = sqlx::query_as::<_, (String, String, Option<String>, String, Option<String>, Option<String>, i64, i64, Option<String>, String)>(doc_query)
        .bind(document_id)
        .fetch_optional(db)
        .await?;

    match doc {
        Some((id, name, file_path, doc_type, content, summary, chunk_count, total_tokens, metadata, created_at)) => {
            let mut result = serde_json::json!({
                "id": id,
                "name": name,
                "file_path": file_path,
                "doc_type": doc_type,
                "summary": summary,
                "chunk_count": chunk_count,
                "total_tokens": total_tokens,
                "metadata": metadata.and_then(|m| serde_json::from_str::<serde_json::Value>(&m).ok()),
                "created_at": created_at,
            });

            if include_content {
                if let Some(doc_content) = content {
                    result["content"] = serde_json::json!(doc_content);
                } else {
                    let chunks_query = r#"
                        SELECT chunk_index, content, token_count
                        FROM document_chunks
                        WHERE document_id = $1
                        ORDER BY chunk_index
                    "#;

                    let chunk_rows = sqlx::query_as::<_, (i64, String, Option<i64>)>(chunks_query)
                        .bind(document_id)
                        .fetch_all(db)
                        .await
                        .unwrap_or_default();

                    let chunks: Vec<serde_json::Value> = chunk_rows
                        .into_iter()
                        .map(|(idx, content, tokens)| {
                            serde_json::json!({
                                "index": idx,
                                "content": content,
                                "token_count": tokens,
                            })
                        })
                        .collect();

                    result["chunks"] = serde_json::json!(chunks);
                }
            }

            Ok(Some(result))
        }
        None => Ok(None),
    }
}
