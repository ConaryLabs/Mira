// src/memory/features/code_intelligence/invalidation.rs
//
// Embedding invalidation for code files when they change
// Deletes Qdrant embeddings using code_element IDs as point IDs

use anyhow::Result;
use sqlx::SqlitePool;
use tracing::{debug, info};

use crate::llm::EmbeddingHead;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;

/// Delete all code embeddings for a specific file_id
pub async fn invalidate_file_embeddings(
    pool: &SqlitePool,
    multi_store: &QdrantMultiStore,
    file_id: i64,
) -> Result<u64> {
    info!("Invalidating code embeddings for file_id: {}", file_id);

    // Get all code_element IDs for this file
    let elements = sqlx::query!(
        r#"
        SELECT id
        FROM code_elements
        WHERE file_id = ?
        "#,
        file_id
    )
    .fetch_all(pool)
    .await?;

    if elements.is_empty() {
        debug!("No code elements found for file_id {}", file_id);
        return Ok(0);
    }

    let mut deleted_count = 0u64;

    // Delete each element's embedding from Qdrant
    for element in &elements {
        // Handle Option<i64> from query result
        let element_id = match element.id {
            Some(id) => id,
            None => continue,
        };

        // Delete from code collection
        match multi_store.delete(EmbeddingHead::Code, element_id).await {
            Ok(_) => {
                deleted_count += 1;
                debug!(
                    "Deleted embedding for code_element {} from code collection",
                    element_id
                );
            }
            Err(e) => {
                debug!(
                    "Could not delete code_element {} from code collection: {}",
                    element_id, e
                );
            }
        }
    }

    if deleted_count > 0 {
        info!(
            "Invalidated {} code embeddings for file_id {}",
            deleted_count, file_id
        );
    }

    Ok(deleted_count)
}

/// Delete all code embeddings for an entire project
pub async fn invalidate_project_embeddings(
    pool: &SqlitePool,
    multi_store: &QdrantMultiStore,
    project_id: &str,
) -> Result<u64> {
    info!(
        "Invalidating all code embeddings for project: {}",
        project_id
    );

    // Get all file_ids for this project
    let files = sqlx::query!(
        r#"
        SELECT DISTINCT rf.id
        FROM repository_files rf
        JOIN git_repo_attachments gra ON rf.attachment_id = gra.id
        WHERE gra.project_id = ?
        "#,
        project_id
    )
    .fetch_all(pool)
    .await?;

    let mut total_deleted = 0u64;

    for file in files {
        if let Some(file_id) = file.id {
            total_deleted += invalidate_file_embeddings(pool, multi_store, file_id).await?;
        }
    }

    info!(
        "Invalidated {} embeddings for project {}",
        total_deleted, project_id
    );
    Ok(total_deleted)
}
