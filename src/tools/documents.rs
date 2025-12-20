// src/tools/documents.rs
// Document storage and search tools - thin wrapper over core::ops::documents

use sqlx::sqlite::SqlitePool;
use std::sync::Arc;

use crate::core::ops::documents as core_docs;
use crate::core::OpContext;
use super::semantic::SemanticSearch;

// === Parameter structs for consolidated document tool ===

pub struct ListDocumentsParams {
    pub doc_type: Option<String>,
    pub limit: Option<i64>,
}

/// List documents
pub async fn list_documents(db: &SqlitePool, req: ListDocumentsParams) -> anyhow::Result<Vec<serde_json::Value>> {
    let ctx = OpContext::just_db(db.clone());

    let input = core_docs::ListDocumentsInput {
        doc_type: req.doc_type,
        limit: req.limit.unwrap_or(20),
    };

    let docs = core_docs::list_documents(&ctx, input).await?;

    Ok(docs.into_iter().map(|d| {
        serde_json::json!({
            "id": d.id,
            "name": d.name,
            "file_path": d.file_path,
            "doc_type": d.doc_type,
            "summary": d.summary,
            "chunk_count": d.chunk_count,
            "total_tokens": d.total_tokens,
            "created_at": d.created_at,
        })
    }).collect())
}

/// Search documents - uses semantic search if available
pub async fn search_documents(
    db: &SqlitePool,
    semantic: Arc<SemanticSearch>,
    query_str: &str,
    limit: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let ctx = OpContext::with_db_and_semantic(db.clone(), semantic);
    let limit = limit.unwrap_or(10) as usize;

    let input = core_docs::SearchDocumentsInput {
        query: query_str.to_string(),
        limit,
    };

    let results = core_docs::search_documents(&ctx, input).await?;

    Ok(results.into_iter().map(|r| {
        serde_json::json!({
            "content": r.content,
            "score": r.score,
            "search_type": r.search_type,
            "document_id": r.document_id,
            "document_name": r.document_name,
            "doc_type": r.doc_type,
            "chunk_index": r.chunk_index,
        })
    }).collect())
}

/// Get a specific document
pub async fn get_document(db: &SqlitePool, document_id: &str, include_content: bool) -> anyhow::Result<Option<serde_json::Value>> {
    let ctx = OpContext::just_db(db.clone());

    let input = core_docs::GetDocumentInput {
        document_id: document_id.to_string(),
        include_content,
    };

    let doc = core_docs::get_document(&ctx, input).await?;

    Ok(doc.map(|d| {
        let mut result = serde_json::json!({
            "id": d.id,
            "name": d.name,
            "file_path": d.file_path,
            "doc_type": d.doc_type,
            "summary": d.summary,
            "chunk_count": d.chunk_count,
            "total_tokens": d.total_tokens,
            "metadata": d.metadata,
            "created_at": d.created_at,
        });

        if let Some(content) = d.content {
            result["content"] = serde_json::json!(content);
        }

        if let Some(chunks) = d.chunks {
            let chunks_json: Vec<serde_json::Value> = chunks.into_iter().map(|c| {
                serde_json::json!({
                    "index": c.index,
                    "content": c.content,
                    "token_count": c.token_count,
                })
            }).collect();
            result["chunks"] = serde_json::json!(chunks_json);
        }

        result
    }))
}

/// Delete a document
pub async fn delete_document(db: &SqlitePool, document_id: &str) -> anyhow::Result<bool> {
    let ctx = OpContext::just_db(db.clone());
    let deleted = core_docs::delete_document(&ctx, document_id).await?;
    Ok(deleted)
}
