//! File Search MCP tools for Gemini RAG functionality
//!
//! Provides per-project semantic document search via Gemini's FileSearch stores.

use anyhow::{bail, Result};
use sqlx::SqlitePool;
use std::path::Path;

use crate::chat::provider::{CustomMetadata, FileSearchClient};

/// Get or create the FileSearch store for a project
pub async fn get_or_create_store(
    db: &SqlitePool,
    client: &FileSearchClient,
    project_path: &str,
) -> Result<String> {
    // Check if store already exists
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT fs.store_name FROM file_search_stores fs
         JOIN projects p ON fs.project_id = p.id
         WHERE p.path = ?"
    )
    .bind(project_path)
    .fetch_optional(db)
    .await?;

    if let Some(store_name) = existing {
        return Ok(store_name);
    }

    // Get project ID
    let project_id = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM projects WHERE path = ?"
    )
    .bind(project_path)
    .fetch_optional(db)
    .await?;

    let project_id = match project_id {
        Some(id) => id,
        None => bail!("Project not found: {}", project_path),
    };

    // Create new store via Gemini API
    let project_name = Path::new(project_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    let display_name = format!("mira-{}", project_name);
    let store = client.create_store(&display_name).await?;

    // Store in database
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT INTO file_search_stores (project_id, store_name, display_name, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?)"
    )
    .bind(project_id)
    .bind(&store.name)
    .bind(&store.display_name)
    .bind(now)
    .bind(now)
    .execute(db)
    .await?;

    tracing::info!("Created FileSearch store for project: {} -> {}", project_path, store.name);
    Ok(store.name)
}

/// Get the store name for a project (if it exists)
pub async fn get_store_name(db: &SqlitePool, project_path: &str) -> Result<Option<String>> {
    let store_name = sqlx::query_scalar::<_, String>(
        "SELECT fs.store_name FROM file_search_stores fs
         JOIN projects p ON fs.project_id = p.id
         WHERE p.path = ?"
    )
    .bind(project_path)
    .fetch_optional(db)
    .await?;

    Ok(store_name)
}

/// Index a file into the project's FileSearch store
pub async fn index_file(
    db: &SqlitePool,
    client: &FileSearchClient,
    project_path: &str,
    file_path: &str,
    display_name: Option<&str>,
    metadata: Option<Vec<CustomMetadata>>,
    wait: bool,
) -> Result<IndexResult> {
    // Get or create store
    let store_name = get_or_create_store(db, client, project_path).await?;

    // Get store ID
    let store_id = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM file_search_stores WHERE store_name = ?"
    )
    .bind(&store_name)
    .fetch_one(db)
    .await?;

    // Upload file
    let path = Path::new(file_path);
    if !path.exists() {
        bail!("File not found: {}", file_path);
    }

    let operation = client.upload_file(&store_name, path, display_name, metadata).await?;

    // Record in database
    let now = chrono::Utc::now().timestamp();
    let mime_type = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();
    let file_size = tokio::fs::metadata(path).await?.len() as i64;

    sqlx::query(
        "INSERT OR REPLACE INTO file_search_documents
         (store_id, file_name, display_name, file_path, mime_type, size_bytes, status, indexed_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?)"
    )
    .bind(store_id)
    .bind(&operation.name)
    .bind(display_name.unwrap_or_else(|| path.file_name().and_then(|n| n.to_str()).unwrap_or("file")))
    .bind(file_path)
    .bind(&mime_type)
    .bind(file_size)
    .bind(now)
    .bind(now)
    .execute(db)
    .await?;

    // Wait for completion if requested
    if wait {
        let op = client.wait_for_operation(&operation.name, 120).await?;

        // Update status
        let status = if op.error.is_some() { "failed" } else { "active" };
        sqlx::query(
            "UPDATE file_search_documents SET status = ?, updated_at = ? WHERE file_name = ?"
        )
        .bind(status)
        .bind(chrono::Utc::now().timestamp())
        .bind(&operation.name)
        .execute(db)
        .await?;

        // Update store stats
        update_store_stats(db, client, &store_name).await?;

        return Ok(IndexResult {
            operation_name: operation.name,
            status: status.to_string(),
            file_path: file_path.to_string(),
        });
    }

    Ok(IndexResult {
        operation_name: operation.name,
        status: "pending".to_string(),
        file_path: file_path.to_string(),
    })
}

/// List indexed files for a project
pub async fn list_indexed_files(db: &SqlitePool, project_path: &str) -> Result<Vec<IndexedFile>> {
    let files = sqlx::query_as::<_, IndexedFile>(
        "SELECT fsd.file_path, fsd.display_name, fsd.mime_type, fsd.size_bytes, fsd.status, fsd.indexed_at
         FROM file_search_documents fsd
         JOIN file_search_stores fs ON fsd.store_id = fs.id
         JOIN projects p ON fs.project_id = p.id
         WHERE p.path = ?
         ORDER BY fsd.indexed_at DESC"
    )
    .bind(project_path)
    .fetch_all(db)
    .await?;

    Ok(files)
}

/// Remove a file from the index
pub async fn remove_file(
    db: &SqlitePool,
    project_path: &str,
    file_path: &str,
) -> Result<bool> {
    let result = sqlx::query(
        "DELETE FROM file_search_documents
         WHERE file_path = ? AND store_id IN (
             SELECT fs.id FROM file_search_stores fs
             JOIN projects p ON fs.project_id = p.id
             WHERE p.path = ?
         )"
    )
    .bind(file_path)
    .bind(project_path)
    .execute(db)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Get store status for a project
pub async fn get_store_status(db: &SqlitePool, project_path: &str) -> Result<Option<StoreStatus>> {
    let status = sqlx::query_as::<_, StoreStatus>(
        "SELECT fs.store_name, fs.display_name, fs.active_documents, fs.pending_documents,
                fs.failed_documents, fs.size_bytes, fs.created_at
         FROM file_search_stores fs
         JOIN projects p ON fs.project_id = p.id
         WHERE p.path = ?"
    )
    .bind(project_path)
    .fetch_optional(db)
    .await?;

    Ok(status)
}

/// Update store statistics from Gemini API
async fn update_store_stats(db: &SqlitePool, client: &FileSearchClient, store_name: &str) -> Result<()> {
    let store = client.get_store(store_name).await?;

    sqlx::query(
        "UPDATE file_search_stores
         SET active_documents = ?, pending_documents = ?, failed_documents = ?,
             size_bytes = ?, updated_at = ?
         WHERE store_name = ?"
    )
    .bind(store.active_documents_count as i64)
    .bind(store.pending_documents_count as i64)
    .bind(store.failed_documents_count as i64)
    .bind(store.size_bytes as i64)
    .bind(chrono::Utc::now().timestamp())
    .bind(store_name)
    .execute(db)
    .await?;

    Ok(())
}

// ============================================================================
// Result Types
// ============================================================================

#[derive(Debug, serde::Serialize)]
pub struct IndexResult {
    pub operation_name: String,
    pub status: String,
    pub file_path: String,
}

#[derive(Debug, sqlx::FromRow, serde::Serialize)]
pub struct IndexedFile {
    pub file_path: String,
    pub display_name: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub status: String,
    pub indexed_at: i64,
}

#[derive(Debug, sqlx::FromRow, serde::Serialize)]
pub struct StoreStatus {
    pub store_name: String,
    pub display_name: Option<String>,
    pub active_documents: i64,
    pub pending_documents: i64,
    pub failed_documents: i64,
    pub size_bytes: i64,
    pub created_at: i64,
}
