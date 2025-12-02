// src/memory/features/document_processing/storage.rs
//! Storage layer for documents in SQLite and Qdrant

use crate::config::CONFIG;
use crate::llm::provider::GeminiEmbeddings;
use anyhow::Result;
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    Condition, DeletePointsBuilder, Filter, PointStruct, SearchPoints, UpsertPointsBuilder,
    Vectors, WithPayloadSelector, with_payload_selector::SelectorOptions,
};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;

/// Document record from database
#[derive(Debug, Clone)]
pub struct DocumentRecord {
    pub id: String,
    pub project_id: String,
    pub file_name: String, // Maps to original_name in DB
    pub file_path: String,
    pub file_type: String,
    pub size_bytes: i64,
    pub created_at: i64, // Maps to uploaded_at in DB
    pub file_hash: Option<String>,
}

/// Search result with relevance score
#[derive(Debug, Clone, serde::Serialize)]
pub struct DocumentSearchResult {
    pub document_id: String,
    pub chunk_id: String,
    pub file_name: String,
    pub content: String,
    pub score: f32,
    pub chunk_index: usize,
    pub page_number: Option<usize>,
}

/// Document storage handler for SQLite and Qdrant
pub struct DocumentStorage {
    sqlite_pool: SqlitePool,
    qdrant_client: Qdrant,
    embedding_client: Arc<GeminiEmbeddings>,
}

impl DocumentStorage {
    /// Create a new document storage handler
    pub fn new(sqlite_pool: SqlitePool, qdrant_client: Qdrant) -> Self {
        // Create the embedding client - simple and clean
        let embedding_client = Arc::new(GeminiEmbeddings::new(
            CONFIG.google_api_key.clone(),
            CONFIG.gemini_embedding_model.clone(),
        ));

        Self {
            sqlite_pool,
            qdrant_client,
            embedding_client,
        }
    }

    /// Find document by hash to detect duplicates
    pub async fn find_by_hash(
        &self,
        file_hash: &str,
        project_id: &str,
    ) -> Result<Option<DocumentRecord>> {
        #[derive(sqlx::FromRow)]
        struct DbRecord {
            id: String,
            project_id: String,
            original_name: String,
            file_path: String,
            file_type: Option<String>,
            size_bytes: i64,
            uploaded_at: i64,
            file_hash: Option<String>,
        }

        let record = sqlx::query_as::<_, DbRecord>(
            r#"
            SELECT 
                id,
                project_id,
                original_name,
                file_path,
                file_type,
                size_bytes,
                uploaded_at,
                file_hash
            FROM documents 
            WHERE file_hash = ? AND project_id = ?
            "#,
        )
        .bind(file_hash)
        .bind(project_id)
        .fetch_optional(&self.sqlite_pool)
        .await?;

        Ok(record.map(|r| DocumentRecord {
            id: r.id,
            project_id: r.project_id,
            file_name: r.original_name,
            file_path: r.file_path,
            file_type: r
                .file_type
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            size_bytes: r.size_bytes,
            created_at: r.uploaded_at,
            file_hash: r.file_hash,
        }))
    }

    /// Store a processed document with all chunks
    pub async fn store_document(&self, document: &super::ProcessedDocument) -> Result<()> {
        // Start transaction
        let mut tx = self.sqlite_pool.begin().await?;

        // 1. Store document metadata in SQLite
        let uploaded_at = document.created_at.timestamp();

        sqlx::query!(
            r#"
            INSERT INTO documents (
                id, project_id, original_name, file_path, file_type,
                size_bytes, uploaded_at, file_hash, content_hash
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            document.id,
            document.project_id,
            document.file_name,
            document.file_path,
            document.file_type,
            document.size_bytes,
            uploaded_at,
            document.file_hash,
            document.file_hash // Using file_hash as content_hash for now
        )
        .execute(&mut *tx)
        .await?;

        // 2. Store document chunks in SQLite
        for (idx, chunk) in document.chunks.iter().enumerate() {
            let chunk_index = idx as i64;
            let char_start = chunk.char_start as i64;
            let char_end = chunk.char_end as i64;

            sqlx::query!(
                r#"
                INSERT INTO document_chunks (
                    document_id, chunk_index, qdrant_point_id, content,
                    char_start, char_end
                ) VALUES (?, ?, ?, ?, ?, ?)
                "#,
                document.id,
                chunk_index,
                chunk.id, // Use chunk.id as qdrant_point_id
                chunk.content,
                char_start,
                char_end
            )
            .execute(&mut *tx)
            .await?;
        }

        // Commit transaction
        tx.commit().await?;

        // 3. Generate embeddings and store in Qdrant
        self.store_embeddings(document).await?;

        Ok(())
    }

    /// Store document embeddings in Qdrant
    async fn store_embeddings(&self, document: &super::ProcessedDocument) -> Result<()> {
        let mut points = Vec::new();

        // Generate embeddings for each chunk
        for (idx, chunk) in document.chunks.iter().enumerate() {
            // Generate embedding for chunk content
            let embedding = self.embedding_client.embed(&chunk.content).await?;

            // Create payload with document and chunk metadata
            let mut payload = HashMap::new();
            payload.insert("document_id".to_string(), document.id.clone().into());
            payload.insert("project_id".to_string(), document.project_id.clone().into());
            payload.insert("chunk_id".to_string(), chunk.id.clone().into());
            payload.insert("chunk_index".to_string(), (idx as i64).into());
            payload.insert("file_name".to_string(), document.file_name.clone().into());
            payload.insert("file_type".to_string(), document.file_type.clone().into());
            payload.insert("content".to_string(), chunk.content.clone().into());

            if let Some(page) = chunk.page_number {
                payload.insert("page_number".to_string(), (page as i64).into());
            }

            // Create point for Qdrant with matching chunk ID
            let point = PointStruct {
                id: Some(chunk.id.clone().into()),
                vectors: Some(Vectors::from(embedding)),
                payload,
                ..Default::default()
            };

            points.push(point);
        }

        // Batch insert points into Qdrant "documents" collection using builder
        if !points.is_empty() {
            let upsert_operation = UpsertPointsBuilder::new("documents", points).build();
            self.qdrant_client.upsert_points(upsert_operation).await?;
        }

        Ok(())
    }

    /// Search documents using vector similarity
    pub async fn search_documents(
        &self,
        project_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<DocumentSearchResult>> {
        // Generate query embedding
        let query_embedding = self.embedding_client.embed(query).await?;

        // Create filter for project_id
        let filter = Filter::must([Condition::matches("project_id", project_id.to_string())]);

        // Search in Qdrant
        let search_result = self
            .qdrant_client
            .search_points(SearchPoints {
                collection_name: "documents".to_string(),
                vector: query_embedding.into(),
                filter: Some(filter),
                limit: limit as u64,
                with_payload: Some(WithPayloadSelector {
                    selector_options: Some(SelectorOptions::Enable(true)),
                }),
                ..Default::default()
            })
            .await?;

        // Convert to search results
        let results: Vec<DocumentSearchResult> = search_result
            .result
            .into_iter()
            .map(|point| {
                let payload = point.payload;
                DocumentSearchResult {
                    document_id: payload
                        .get("document_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| String::new()),
                    chunk_id: payload
                        .get("chunk_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| String::new()),
                    file_name: payload
                        .get("file_name")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| String::new()),
                    content: payload
                        .get("content")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| String::new()),
                    score: point.score,
                    chunk_index: payload
                        .get("chunk_index")
                        .and_then(|v| v.as_integer())
                        .unwrap_or(0) as usize,
                    page_number: payload
                        .get("page_number")
                        .and_then(|v| v.as_integer())
                        .map(|p| p as usize),
                }
            })
            .collect();

        Ok(results)
    }

    /// Retrieve a specific document by ID
    pub async fn retrieve_document(&self, document_id: &str) -> Result<Option<DocumentRecord>> {
        #[derive(sqlx::FromRow)]
        struct DbRecord {
            id: String,
            project_id: String,
            original_name: String,
            file_path: String,
            file_type: Option<String>,
            size_bytes: i64,
            uploaded_at: i64,
            file_hash: Option<String>,
        }

        let record = sqlx::query_as::<_, DbRecord>(
            r#"
            SELECT 
                id,
                project_id,
                original_name,
                file_path,
                file_type,
                size_bytes,
                uploaded_at,
                file_hash
            FROM documents 
            WHERE id = ?
            "#,
        )
        .bind(document_id)
        .fetch_optional(&self.sqlite_pool)
        .await?;

        Ok(record.map(|r| DocumentRecord {
            id: r.id,
            project_id: r.project_id,
            file_name: r.original_name,
            file_path: r.file_path,
            file_type: r
                .file_type
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            size_bytes: r.size_bytes,
            created_at: r.uploaded_at,
            file_hash: r.file_hash,
        }))
    }

    /// Get all chunks for a document
    pub async fn get_document_chunks(
        &self,
        document_id: &str,
    ) -> Result<Vec<super::DocumentChunk>> {
        #[derive(sqlx::FromRow)]
        struct ChunkRow {
            document_id: String,
            chunk_index: i64,
            qdrant_point_id: String,
            content: String,
            char_start: i64,
            char_end: i64,
        }

        let rows = sqlx::query_as::<_, ChunkRow>(
            r#"
            SELECT 
                document_id,
                chunk_index,
                qdrant_point_id,
                content,
                char_start,
                char_end
            FROM document_chunks 
            WHERE document_id = ?
            ORDER BY chunk_index
            "#,
        )
        .bind(document_id)
        .fetch_all(&self.sqlite_pool)
        .await?;

        let chunks = rows
            .into_iter()
            .map(|row| {
                super::DocumentChunk {
                    id: row.qdrant_point_id, // Use qdrant_point_id as the chunk ID
                    document_id: row.document_id,
                    content: row.content,
                    chunk_index: row.chunk_index as usize,
                    page_number: None, // Not stored in DB
                    section_title: None,
                    char_start: row.char_start as usize,
                    char_end: row.char_end as usize,
                }
            })
            .collect();

        Ok(chunks)
    }

    /// Delete a document and all its chunks
    pub async fn delete_document(&self, document_id: &str) -> Result<()> {
        // CRITICAL FIX: Get chunk IDs BEFORE deleting from SQLite
        let chunk_ids: Vec<String> =
            sqlx::query_scalar("SELECT qdrant_point_id FROM document_chunks WHERE document_id = ?")
                .bind(document_id)
                .fetch_all(&self.sqlite_pool)
                .await?;

        // Delete from Qdrant first if there are chunks
        if !chunk_ids.is_empty() {
            tracing::info!(
                "Deleting {} chunk embeddings from Qdrant for document {}",
                chunk_ids.len(),
                document_id
            );

            // Convert chunk IDs to PointId format
            let point_ids: Vec<qdrant_client::qdrant::PointId> =
                chunk_ids.iter().map(|id| id.clone().into()).collect();

            // Delete from Qdrant "documents" collection
            let delete_operation = DeletePointsBuilder::new("documents")
                .points(point_ids)
                .build();

            match self.qdrant_client.delete_points(delete_operation).await {
                Ok(_) => {
                    tracing::info!(
                        "Successfully deleted {} points from Qdrant",
                        chunk_ids.len()
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to delete points from Qdrant for document {}: {}",
                        document_id,
                        e
                    );
                    // Continue with SQLite deletion even if Qdrant fails
                }
            }
        }

        // Start transaction for SQLite cleanup
        let mut tx = self.sqlite_pool.begin().await?;

        // Delete chunks from SQLite (foreign key constraint)
        sqlx::query!(
            "DELETE FROM document_chunks WHERE document_id = ?",
            document_id
        )
        .execute(&mut *tx)
        .await?;

        // Delete document from SQLite
        sqlx::query!("DELETE FROM documents WHERE id = ?", document_id)
            .execute(&mut *tx)
            .await?;

        // Commit transaction
        tx.commit().await?;

        tracing::info!("Document {} deleted from database", document_id);

        Ok(())
    }

    /// List all documents for a project
    pub async fn list_documents(&self, project_id: &str) -> Result<Vec<DocumentRecord>> {
        #[derive(sqlx::FromRow)]
        struct DbRecord {
            id: String,
            project_id: String,
            original_name: String,
            file_path: String,
            file_type: Option<String>,
            size_bytes: i64,
            uploaded_at: i64,
            file_hash: Option<String>,
        }

        let records = sqlx::query_as::<_, DbRecord>(
            r#"
            SELECT 
                id,
                project_id,
                original_name,
                file_path,
                file_type,
                size_bytes,
                uploaded_at,
                file_hash
            FROM documents 
            WHERE project_id = ?
            ORDER BY uploaded_at DESC
            "#,
        )
        .bind(project_id)
        .fetch_all(&self.sqlite_pool)
        .await?;

        Ok(records
            .into_iter()
            .map(|r| DocumentRecord {
                id: r.id,
                project_id: r.project_id,
                file_name: r.original_name,
                file_path: r.file_path,
                file_type: r
                    .file_type
                    .unwrap_or_else(|| "application/octet-stream".to_string()),
                size_bytes: r.size_bytes,
                created_at: r.uploaded_at,
                file_hash: r.file_hash,
            })
            .collect())
    }
}
