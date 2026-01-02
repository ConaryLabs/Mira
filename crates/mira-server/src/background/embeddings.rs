// crates/mira-server/src/background/embeddings.rs
// OpenAI Batch API for embeddings (50% cheaper)

use super::scanner;
use crate::db::Database;
use crate::embeddings::EmbeddingClient;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// OpenAI API base URL
const OPENAI_API_URL: &str = "https://api.openai.com/v1";

/// Maximum items per batch file
const MAX_BATCH_SIZE: usize = 1000;

/// Model to use for embeddings
const MODEL: &str = "text-embedding-3-small";

/// Batch request in JSONL format
#[derive(Debug, Serialize)]
struct BatchRequest {
    custom_id: String,
    method: String,
    url: String,
    body: BatchBody,
}

#[derive(Debug, Serialize)]
struct BatchBody {
    model: String,
    input: String,
}

/// Batch status response (serde ignores unknown fields)
#[derive(Debug, Deserialize)]
struct BatchStatus {
    status: String,
    output_file_id: Option<String>,
    request_counts: Option<RequestCounts>,
}

#[derive(Debug, Deserialize)]
struct RequestCounts {
    total: i64,
    completed: i64,
}

/// File upload response
#[derive(Debug, Deserialize)]
struct FileUploadResponse {
    id: String,
}

/// Batch creation response
#[derive(Debug, Deserialize)]
struct BatchCreateResponse {
    id: String,
}

/// Batch result line
#[derive(Debug, Deserialize)]
struct BatchResultLine {
    custom_id: String,
    response: BatchResultResponse,
}

#[derive(Debug, Deserialize)]
struct BatchResultResponse {
    status_code: i32,
    body: Option<EmbeddingResponse>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

/// Process a batch of pending embeddings
pub async fn process_batch(db: &Arc<Database>, embeddings: &Arc<EmbeddingClient>) -> Result<usize, String> {
    // Check for in-progress batch first
    if let Some(batch_id) = get_active_batch(db)? {
        return check_batch_status(db, embeddings, &batch_id).await;
    }

    // Find pending embeddings
    let pending = scanner::find_pending_embeddings(db, MAX_BATCH_SIZE)?;
    if pending.is_empty() {
        return Ok(0);
    }

    tracing::info!("Found {} pending embeddings for batch processing", pending.len());

    // Create batch file content
    let mut jsonl_lines = Vec::new();
    let ids: Vec<i64> = pending.iter().map(|p| p.id).collect();

    for item in &pending {
        let request = BatchRequest {
            custom_id: format!("emb_{}", item.id),
            method: "POST".to_string(),
            url: "/v1/embeddings".to_string(),
            body: BatchBody {
                model: MODEL.to_string(),
                input: truncate_text(&item.chunk_content, 8000),
            },
        };
        jsonl_lines.push(serde_json::to_string(&request).map_err(|e| e.to_string())?);
    }

    let jsonl_content = jsonl_lines.join("\n");

    // Mark as processing
    scanner::mark_embeddings_processing(db, &ids)?;

    // Upload file and create batch
    let api_key = embeddings.api_key();
    let batch_id = create_batch(&api_key, &jsonl_content).await?;

    // Store batch ID for tracking
    store_active_batch(db, &batch_id, &ids)?;

    tracing::info!("Created batch {} with {} items", batch_id, pending.len());
    Ok(0) // Will process results on next iteration
}

/// Create a batch job with OpenAI
async fn create_batch(api_key: &str, jsonl_content: &str) -> Result<String, String> {
    let client = reqwest::Client::new();

    // Step 1: Upload the JSONL file
    let form = reqwest::multipart::Form::new()
        .text("purpose", "batch")
        .part(
            "file",
            reqwest::multipart::Part::text(jsonl_content.to_string())
                .file_name("embeddings.jsonl")
                .mime_str("application/jsonl")
                .map_err(|e| e.to_string())?,
        );

    let upload_response = client
        .post(format!("{}/files", OPENAI_API_URL))
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Failed to upload batch file: {}", e))?;

    if !upload_response.status().is_success() {
        let error_text = upload_response.text().await.unwrap_or_default();
        return Err(format!("File upload failed: {}", error_text));
    }

    let file_response: FileUploadResponse = upload_response
        .json()
        .await
        .map_err(|e| format!("Failed to parse file upload response: {}", e))?;

    tracing::debug!("Uploaded batch file: {}", file_response.id);

    // Step 2: Create the batch
    let batch_body = serde_json::json!({
        "input_file_id": file_response.id,
        "endpoint": "/v1/embeddings",
        "completion_window": "24h"
    });

    let batch_response = client
        .post(format!("{}/batches", OPENAI_API_URL))
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&batch_body)
        .send()
        .await
        .map_err(|e| format!("Failed to create batch: {}", e))?;

    if !batch_response.status().is_success() {
        let error_text = batch_response.text().await.unwrap_or_default();
        return Err(format!("Batch creation failed: {}", error_text));
    }

    let batch: BatchCreateResponse = batch_response
        .json()
        .await
        .map_err(|e| format!("Failed to parse batch response: {}", e))?;

    Ok(batch.id)
}

/// Check status of an active batch and process results if complete
async fn check_batch_status(
    db: &Arc<Database>,
    embeddings: &Arc<EmbeddingClient>,
    batch_id: &str,
) -> Result<usize, String> {
    let client = reqwest::Client::new();
    let api_key = embeddings.api_key();

    let response = client
        .get(format!("{}/batches/{}", OPENAI_API_URL, batch_id))
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .map_err(|e| format!("Failed to check batch status: {}", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("Batch status check failed: {}", error_text));
    }

    let status: BatchStatus = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse batch status: {}", e))?;

    match status.status.as_str() {
        "completed" => {
            tracing::info!("Batch {} completed", batch_id);
            if let Some(output_file_id) = status.output_file_id {
                let processed = process_batch_results(db, embeddings, &output_file_id).await?;
                clear_active_batch(db)?;
                return Ok(processed);
            }
            clear_active_batch(db)?;
            Ok(0)
        }
        "failed" | "expired" | "cancelled" => {
            tracing::warn!("Batch {} failed with status: {}", batch_id, status.status);
            // Reset pending items so they can be retried
            reset_batch_items(db)?;
            clear_active_batch(db)?;
            Ok(0)
        }
        "in_progress" | "validating" | "finalizing" => {
            if let Some(counts) = status.request_counts {
                tracing::debug!(
                    "Batch {} in progress: {}/{} completed",
                    batch_id,
                    counts.completed,
                    counts.total
                );
            }
            Ok(0)
        }
        _ => {
            tracing::debug!("Batch {} status: {}", batch_id, status.status);
            Ok(0)
        }
    }
}

/// Download and process batch results
async fn process_batch_results(
    db: &Arc<Database>,
    embeddings: &Arc<EmbeddingClient>,
    output_file_id: &str,
) -> Result<usize, String> {
    let client = reqwest::Client::new();
    let api_key = embeddings.api_key();

    // Download the results file
    let response = client
        .get(format!("{}/files/{}/content", OPENAI_API_URL, output_file_id))
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .map_err(|e| format!("Failed to download results: {}", e))?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("Results download failed: {}", error_text));
    }

    let content = response.text().await.map_err(|e| e.to_string())?;
    let mut processed = 0;
    let mut completed_ids = Vec::new();

    for line in content.lines() {
        if line.is_empty() {
            continue;
        }

        let result: BatchResultLine = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Failed to parse result line: {}", e);
                continue;
            }
        };

        // Extract ID from custom_id (format: "emb_123")
        let id: i64 = match result.custom_id.strip_prefix("emb_").and_then(|s| s.parse().ok()) {
            Some(id) => id,
            None => continue,
        };

        if result.response.status_code == 200 {
            if let Some(body) = result.response.body {
                if let Some(data) = body.data.first() {
                    // Store the embedding
                    if let Err(e) = store_embedding(db, id, &data.embedding) {
                        tracing::warn!("Failed to store embedding {}: {}", id, e);
                    } else {
                        completed_ids.push(id);
                        processed += 1;
                    }
                }
            }
        }
    }

    // Mark completed
    scanner::mark_embeddings_completed(db, &completed_ids)?;

    tracing::info!("Processed {} embeddings from batch", processed);
    Ok(processed)
}

/// Store an embedding in vec_code
fn store_embedding(db: &Arc<Database>, pending_id: i64, embedding: &[f32]) -> Result<(), String> {
    let conn = db.conn();

    // Get the pending item details
    let (project_id, file_path, chunk_content): (i64, String, String) = conn
        .query_row(
            "SELECT project_id, file_path, chunk_content FROM pending_embeddings WHERE id = ?",
            params![pending_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| e.to_string())?;

    // Convert embedding to bytes
    let embedding_bytes: Vec<u8> = embedding
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();

    // Insert into vec_code
    conn.execute(
        "INSERT INTO vec_code (embedding, file_path, chunk_content, project_id)
         VALUES (?, ?, ?, ?)",
        params![embedding_bytes, file_path, chunk_content, project_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

// ═══════════════════════════════════════
// BATCH TRACKING
// ═══════════════════════════════════════

fn get_active_batch(db: &Arc<Database>) -> Result<Option<String>, String> {
    let conn = db.conn();
    let result: Option<String> = conn
        .query_row(
            "SELECT batch_id FROM background_batches WHERE status = 'active' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();
    Ok(result)
}

fn store_active_batch(db: &Arc<Database>, batch_id: &str, item_ids: &[i64]) -> Result<(), String> {
    let conn = db.conn();

    // Create table if needed
    conn.execute(
        "CREATE TABLE IF NOT EXISTS background_batches (
            id INTEGER PRIMARY KEY,
            batch_id TEXT NOT NULL,
            item_ids TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .map_err(|e| e.to_string())?;

    let ids_json = serde_json::to_string(item_ids).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO background_batches (batch_id, item_ids, status) VALUES (?, ?, 'active')",
        params![batch_id, ids_json],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

fn clear_active_batch(db: &Arc<Database>) -> Result<(), String> {
    let conn = db.conn();
    conn.execute("UPDATE background_batches SET status = 'completed' WHERE status = 'active'", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn reset_batch_items(db: &Arc<Database>) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "UPDATE pending_embeddings SET status = 'pending' WHERE status = 'processing'",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        text.chars().take(max_chars).collect()
    }
}
