//! Core document operations - shared by MCP and Chat
//!
//! Document storage, search, and retrieval.

use crate::core::primitives::semantic::COLLECTION_DOCS;

use super::super::{CoreResult, OpContext};

// ============================================================================
// Input/Output Types
// ============================================================================

pub struct ListDocumentsInput {
    pub doc_type: Option<String>,
    pub limit: i64,
}

pub struct DocumentInfo {
    pub id: String,
    pub name: String,
    pub file_path: Option<String>,
    pub doc_type: String,
    pub summary: Option<String>,
    pub chunk_count: i64,
    pub total_tokens: i64,
    pub created_at: String,
}

pub struct SearchDocumentsInput {
    pub query: String,
    pub limit: usize,
}

pub struct DocumentSearchResult {
    pub content: String,
    pub score: f32,
    pub search_type: String,
    pub document_id: Option<String>,
    pub document_name: Option<String>,
    pub doc_type: Option<String>,
    pub chunk_index: Option<i64>,
}

pub struct GetDocumentInput {
    pub document_id: String,
    pub include_content: bool,
}

pub struct DocumentDetail {
    pub id: String,
    pub name: String,
    pub file_path: Option<String>,
    pub doc_type: String,
    pub summary: Option<String>,
    pub chunk_count: i64,
    pub total_tokens: i64,
    pub metadata: Option<serde_json::Value>,
    pub created_at: String,
    pub content: Option<String>,
    pub chunks: Option<Vec<DocumentChunk>>,
}

pub struct DocumentChunk {
    pub index: i64,
    pub content: String,
    pub token_count: Option<i64>,
}

// ============================================================================
// Operations
// ============================================================================

/// List documents
pub async fn list_documents(ctx: &OpContext, input: ListDocumentsInput) -> CoreResult<Vec<DocumentInfo>> {
    let db = ctx.require_db()?;

    let query = r#"
        SELECT id, name, file_path, doc_type, summary, chunk_count, total_tokens,
               datetime(created_at, 'unixepoch', 'localtime') as created_at
        FROM documents
        WHERE ($1 IS NULL OR doc_type = $1)
        ORDER BY created_at DESC
        LIMIT $2
    "#;

    let rows = sqlx::query_as::<_, (String, String, Option<String>, String, Option<String>, i64, i64, String)>(query)
        .bind(&input.doc_type)
        .bind(input.limit)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(id, name, file_path, doc_type, summary, chunk_count, total_tokens, created_at)| {
        DocumentInfo {
            id,
            name,
            file_path,
            doc_type,
            summary,
            chunk_count,
            total_tokens,
            created_at,
        }
    }).collect())
}

/// Search documents - uses semantic search if available
/// Checks for cancellation before making external API calls
pub async fn search_documents(ctx: &OpContext, input: SearchDocumentsInput) -> CoreResult<Vec<DocumentSearchResult>> {
    let db = ctx.require_db()?;

    // Check cancellation before potentially slow operations
    ctx.check_cancelled()?;

    // Try semantic search first if available
    if let Some(semantic) = &ctx.semantic {
        if semantic.is_available() {
            match semantic.search(COLLECTION_DOCS, &input.query, input.limit, None).await {
                Ok(results) if !results.is_empty() => {
                    return Ok(results.into_iter().map(|r| {
                        DocumentSearchResult {
                            content: r.content,
                            score: r.score,
                            search_type: "semantic".to_string(),
                            document_id: r.metadata.get("document_id").and_then(|v| v.as_str()).map(String::from),
                            document_name: r.metadata.get("document_name").and_then(|v| v.as_str()).map(String::from),
                            doc_type: r.metadata.get("doc_type").and_then(|v| v.as_str()).map(String::from),
                            chunk_index: r.metadata.get("chunk_index").and_then(|v| v.as_i64()),
                        }
                    }).collect());
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("Semantic document search failed, falling back to text: {}", e);
                }
            }
        }
    }

    // Fallback to SQLite text search
    let search_pattern = format!("%{}%", input.query);

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
        .bind(input.limit as i64)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(_chunk_id, doc_id, chunk_idx, content, doc_name, doc_type)| {
        // Truncate content for display
        let display_content = if content.len() > 500 {
            if let Some(pos) = content.to_lowercase().find(&input.query.to_lowercase()) {
                let start = pos.saturating_sub(100);
                let end = (pos + input.query.len() + 100).min(content.len());
                let snippet = &content[start..end];
                if start > 0 { format!("...{}", snippet) } else { snippet.to_string() }
            } else {
                format!("{}...", &content[..500])
            }
        } else {
            content
        };

        DocumentSearchResult {
            content: display_content,
            score: 1.0,
            search_type: "text".to_string(),
            document_id: Some(doc_id),
            document_name: Some(doc_name),
            doc_type: Some(doc_type),
            chunk_index: Some(chunk_idx),
        }
    }).collect())
}

/// Get a specific document
pub async fn get_document(ctx: &OpContext, input: GetDocumentInput) -> CoreResult<Option<DocumentDetail>> {
    let db = ctx.require_db()?;

    let doc_query = r#"
        SELECT id, name, file_path, doc_type, content, summary, chunk_count, total_tokens, metadata,
               datetime(created_at, 'unixepoch', 'localtime') as created_at
        FROM documents
        WHERE id = $1
    "#;

    let doc = sqlx::query_as::<_, (String, String, Option<String>, String, Option<String>, Option<String>, i64, i64, Option<String>, String)>(doc_query)
        .bind(&input.document_id)
        .fetch_optional(db)
        .await?;

    match doc {
        Some((id, name, file_path, doc_type, content, summary, chunk_count, total_tokens, metadata, created_at)) => {
            let mut detail = DocumentDetail {
                id,
                name,
                file_path,
                doc_type,
                summary,
                chunk_count,
                total_tokens,
                metadata: metadata.and_then(|m| serde_json::from_str(&m).ok()),
                created_at,
                content: None,
                chunks: None,
            };

            if input.include_content {
                if let Some(doc_content) = content {
                    detail.content = Some(doc_content);
                } else {
                    // Fetch chunks
                    let chunks_query = r#"
                        SELECT chunk_index, content, token_count
                        FROM document_chunks
                        WHERE document_id = $1
                        ORDER BY chunk_index
                    "#;

                    let chunk_rows = sqlx::query_as::<_, (i64, String, Option<i64>)>(chunks_query)
                        .bind(&input.document_id)
                        .fetch_all(db)
                        .await
                        .unwrap_or_default();

                    detail.chunks = Some(chunk_rows.into_iter().map(|(index, content, token_count)| {
                        DocumentChunk { index, content, token_count }
                    }).collect());
                }
            }

            Ok(Some(detail))
        }
        None => Ok(None),
    }
}

/// Delete a document
pub async fn delete_document(ctx: &OpContext, document_id: &str) -> CoreResult<bool> {
    let db = ctx.require_db()?;

    // Delete chunks first
    sqlx::query("DELETE FROM document_chunks WHERE document_id = $1")
        .bind(document_id)
        .execute(db)
        .await?;

    // Delete document
    let result = sqlx::query("DELETE FROM documents WHERE id = $1")
        .bind(document_id)
        .execute(db)
        .await?;

    // TODO: Also remove from Qdrant if semantic search is available

    Ok(result.rows_affected() > 0)
}
