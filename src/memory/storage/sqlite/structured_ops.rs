// src/memory/storage/sqlite/structured_ops.rs
// FIXED: Smart embedding skip logic to avoid waste and double-embedding
// PHASE 1 UPDATE: Removed llm_metadata table operations (table deleted)

use anyhow::Result;
use sqlx::{SqlitePool, Transaction, Sqlite};
use tracing::{debug, info, warn};

use crate::config::CONFIG;
use crate::llm::structured::{CompleteResponse, StructuredLLMResponse, LLMMetadata};
use crate::llm::provider::OpenAiEmbeddings;
use crate::llm::embeddings::EmbeddingHead;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::memory::core::types::MemoryEntry;
use chrono::Utc;

pub async fn save_structured_response(
    pool: &SqlitePool,
    session_id: &str,
    response: &CompleteResponse,
    parent_id: Option<i64>,
) -> Result<i64> {
    let mut tx = pool.begin().await?;
    
    let message_id = insert_memory_entry(
        &mut tx,
        session_id,
        &response.structured,
        response.metadata.response_id.as_deref(),
        parent_id,
    ).await?;
    
    insert_message_analysis(
        &mut tx,
        message_id,
        &response.structured.analysis,
    ).await?;
    
    // REMOVED: insert_llm_metadata (table deleted in Phase 1)
    // LLM metadata will be tracked in operations table for coding operations
    
    tx.commit().await?;
    
    info!("Saved complete response {} with analysis", message_id);
    
    Ok(message_id)
}

/// New function that handles embedding generation and storage
/// Called separately AFTER save_structured_response to keep transactions clean
/// 
/// SMART SKIP LOGIC:
/// - Large code responses (>30k AND is_code): Skip - code intelligence handles function-level embeddings
/// - Large non-code responses (>30k AND NOT is_code): Truncate with warning (future: chunk)
/// - Small responses (<30k): Embed normally
pub async fn process_embeddings(
    pool: &SqlitePool,
    message_id: i64,
    session_id: &str,
    response: &StructuredLLMResponse,
    embedding_client: &OpenAiEmbeddings,
    multi_store: &QdrantMultiStore,
) -> Result<()> {
    // Skip if no heads to route to
    if response.analysis.routed_to_heads.is_empty() {
        debug!("No embedding heads specified for message {}, skipping embeddings", message_id);
        return Ok(());
    }
    
    info!("Processing embeddings for message {} -> heads: {:?}", message_id, response.analysis.routed_to_heads);
    
    // Check salience threshold
    let min_salience = CONFIG.salience_min_for_embed;
    if response.analysis.salience < min_salience as f64 {
        debug!(
            "Message {} salience ({}) below threshold ({}), skipping embeddings",
            message_id, response.analysis.salience, min_salience
        );
        return Ok(());
    }

    // Gate: skip code-head embeddings for chat responses unless explicitly enabled
    let mut routed_heads: Vec<String> = response.analysis.routed_to_heads.clone();
    if !CONFIG.embed_code_from_chat {
        let before = routed_heads.len();
        routed_heads.retain(|h| h != "code");
        if routed_heads.len() != before {
            debug!(
                "Message {}: skipping 'code' head per config (embed_code_from_chat=false)",
                message_id
            );
        }
    }
    if routed_heads.is_empty() {
        debug!(
            "Message {}: no eligible heads after gating (possibly only 'code'), skipping embeddings",
            message_id
        );
        return Ok(());
    }
    
    // ====================================================================
    // SMART SKIP LOGIC: Avoid double-embedding and wasted API calls
    // ====================================================================
    let content_len = response.output.len();
    let is_code = response.analysis.contains_code;
    
    // Case 1: Large code response - code intelligence handles it better
    if content_len > 30000 && is_code {
        info!(
            "Message {} is large code response ({} chars) - skipping semantic embedding (code intelligence provides function-level embeddings)",
            message_id, content_len
        );
        return Ok(());
    }
    
    // Case 2: Content fits within token limit - embed normally
    // Case 3: Large non-code content - truncate with warning (future: implement chunking)
    let content_to_embed = if content_len > 30000 {
        warn!(
            "Message {} content too long ({} chars), truncating to 30000 for embedding. Consider implementing chunking for non-code content.",
            message_id, content_len
        );
        &response.output[..30000]
    } else {
        &response.output
    };
    
    // Generate embedding for the message content
    let embedding = match embedding_client.embed(content_to_embed).await {
        Ok(emb) => emb,
        Err(e) => {
            warn!("Failed to generate embedding for message {}: {}", message_id, e);
            return Err(e);
        }
    };
    
    info!(
        "Generated embedding for message {} (dimension: {}, content length: {})",
        message_id,
        embedding.len(),
        content_to_embed.len()
    );
    
    // Store in each routed head
    let mut stored_count = 0;
    for head_str in &routed_heads {
        // Parse the head string to EmbeddingHead enum
        let head = match head_str.parse::<EmbeddingHead>() {
            Ok(h) => h,
            Err(e) => {
                warn!("Invalid embedding head '{}' for message {}: {}", head_str, message_id, e);
                continue;
            }
        };
        
        // Check if this head is enabled in config
        if !CONFIG.embed_heads.contains(head_str) {
            debug!("Head '{}' not enabled in config, skipping", head_str);
            continue;
        }
        
        // Create memory entry for Qdrant
        let qdrant_entry = create_qdrant_entry(
            message_id,
            session_id,
            response,
            embedding.clone(),
        );
        
        // Save to appropriate Qdrant collection and get the point_id
        match multi_store.save(head, &qdrant_entry).await {
            Ok(point_id) => {
                info!("Stored embedding for message {} in {} collection (point_id: {})", 
                    message_id, head.as_str(), point_id);
                
                // Track the embedding in message_embeddings table
                let collection_name = multi_store.get_collection_name(head)
                    .unwrap_or_else(|| format!("unknown-{}", head.as_str()));
                
                if let Err(e) = track_embedding_in_db(
                    pool,
                    message_id,
                    &point_id,
                    &collection_name,
                    head_str,
                ).await {
                    warn!(
                        "Failed to track embedding for message {} in message_embeddings table: {}",
                        message_id, e
                    );
                } else {
                    debug!("Tracked embedding {} in message_embeddings table", point_id);
                }
                
                stored_count += 1;
            }
            Err(e) => {
                warn!(
                    "Failed to store embedding for message {} in {} collection: {}",
                    message_id, head.as_str(), e
                );
            }
        }
    }
    
    if stored_count > 0 {
        info!(
            "Successfully processed embeddings for message {} -> stored in {} collections",
            message_id,
            stored_count
        );
    }
    
    Ok(())
}

/// Track an embedding in the message_embeddings table
pub async fn track_embedding_in_db(
    pool: &SqlitePool,
    message_id: i64,
    qdrant_point_id: &str,
    collection_name: &str,
    embedding_head: &str,
) -> Result<()> {
    sqlx::query!(
        r#"
        INSERT INTO message_embeddings (
            message_id, qdrant_point_id, collection_name, embedding_head
        ) VALUES (?, ?, ?, ?)
        "#,
        message_id,
        qdrant_point_id,
        collection_name,
        embedding_head
    )
    .execute(pool)
    .await?;
    
    Ok(())
}

/// Delete all embeddings for a message from both Qdrant and the tracking table
pub async fn delete_message_embeddings(
    pool: &SqlitePool,
    multi_store: &QdrantMultiStore,
    message_id: i64,
) -> Result<()> {
    // Get all embeddings for this message from the tracking table
    let embeddings = sqlx::query!(
        r#"
        SELECT qdrant_point_id, collection_name, embedding_head
        FROM message_embeddings
        WHERE message_id = ?
        "#,
        message_id
    )
    .fetch_all(pool)
    .await?;
    
    // Delete from Qdrant collections
    for embedding in &embeddings {
        if let Ok(head) = embedding.embedding_head.parse::<EmbeddingHead>() {
            if let Err(e) = multi_store.delete(head, message_id).await {
                warn!(
                    "Failed to delete message {} from {} collection: {}",
                    message_id, embedding.embedding_head, e
                );
            }
        }
    }
    
    // Delete from tracking table
    sqlx::query!(
        "DELETE FROM message_embeddings WHERE message_id = ?",
        message_id
    )
    .execute(pool)
    .await?;
    
    info!("Deleted {} embeddings for message {}", embeddings.len(), message_id);
    Ok(())
}

/// Get all Qdrant point IDs for a message
pub async fn get_message_point_ids(
    pool: &SqlitePool,
    message_id: i64,
) -> Result<Vec<(String, String, String)>> {
    let rows = sqlx::query!(
        r#"
        SELECT qdrant_point_id, collection_name, embedding_head
        FROM message_embeddings
        WHERE message_id = ?
        "#,
        message_id
    )
    .fetch_all(pool)
    .await?;
    
    Ok(rows.into_iter()
        .map(|r| (r.qdrant_point_id, r.collection_name, r.embedding_head))
        .collect())
}

/// Create a MemoryEntry for Qdrant storage
fn create_qdrant_entry(
    message_id: i64,
    session_id: &str,
    response: &StructuredLLMResponse,
    embedding: Vec<f32>,
) -> MemoryEntry {
    // Also sanitize programming_lang for consistency
    let sanitized_lang = sanitize_programming_lang(&response.analysis.programming_lang);

    MemoryEntry {
        id: Some(message_id),
        session_id: session_id.to_string(),
        response_id: None,
        parent_id: None,
        role: "assistant".to_string(),
        content: response.output.clone(),
        timestamp: Utc::now(),
        tags: Some(response.analysis.topics.clone()),
        mood: response.analysis.mood.clone(),
        intensity: response.analysis.intensity.map(|i| i as f32),
        salience: Some(response.analysis.salience as f32),
        original_salience: Some(response.analysis.salience as f32),
        intent: response.analysis.intent.clone(),
        topics: Some(response.analysis.topics.clone()),
        summary: response.analysis.summary.clone(),
        relationship_impact: response.analysis.relationship_impact.clone(),
        contains_code: Some(response.analysis.contains_code),
        language: Some(response.analysis.language.clone()),
        programming_lang: sanitized_lang,
        analyzed_at: Some(Utc::now()),
        analysis_version: Some("structured_v1".to_string()),
        routed_to_heads: Some(response.analysis.routed_to_heads.clone()),
        last_recalled: Some(Utc::now()),
        recall_count: Some(0),
        contains_error: Some(response.analysis.contains_error),
        error_type: response.analysis.error_type.clone(),
        error_severity: response.analysis.error_severity.clone(),
        error_file: response.analysis.error_file.clone(),
        model_version: None,
        prompt_tokens: None,
        completion_tokens: None,
        reasoning_tokens: None,
        total_tokens: None,
        latency_ms: None,
        generation_time_ms: None,
        finish_reason: None,
        tool_calls: None,
        temperature: None,
        max_tokens: None,
        embedding: Some(embedding),
        embedding_heads: Some(response.analysis.routed_to_heads.clone()),
        qdrant_point_ids: None,
    }
}

/// Limit programming_lang to the DB-allowed set or None
fn sanitize_programming_lang(lang_opt: &Option<String>) -> Option<String> {
    let Some(raw) = lang_opt.as_ref().map(|s| s.to_lowercase()) else { return None };
    match raw.as_str() {
        // Allowed as-is
        "rust" | "typescript" | "javascript" | "python" | "go" | "java" => Some(raw),
        // Common aliases
        "ts" | "tsx" => Some("typescript".to_string()),
        "js" | "jsx" | "node" => Some("javascript".to_string()),
        "py" => Some("python".to_string()),
        "golang" => Some("go".to_string()),
        // Everything else (css, html, json, yaml, bash, etc.) -> None
        _ => None,
    }
}

async fn insert_memory_entry(
    tx: &mut Transaction<'_, Sqlite>,
    session_id: &str,
    response: &StructuredLLMResponse,
    response_id: Option<&str>,
    parent_id: Option<i64>,
) -> Result<i64> {
    let tags_json = serde_json::to_string(&response.analysis.topics)?;
    
    let result = sqlx::query!(
        r#"
        INSERT INTO memory_entries (
            session_id, response_id, parent_id, role, content, timestamp, tags
        ) VALUES (?, ?, ?, 'assistant', ?, CURRENT_TIMESTAMP, ?)
        RETURNING id
        "#,
        session_id,
        response_id,
        parent_id,
        response.output,
        tags_json
    )
    .fetch_one(&mut **tx)
    .await?;
    
    debug!("Inserted memory_entries id={}", result.id);
    Ok(result.id)
}

async fn insert_message_analysis(
    tx: &mut Transaction<'_, Sqlite>,
    message_id: i64,
    analysis: &crate::llm::structured::types::MessageAnalysis,
) -> Result<()> {
    let topics_json = serde_json::to_string(&analysis.topics)?;
    let heads_json = serde_json::to_string(&analysis.routed_to_heads)?;

    // Sanitize programming_lang to satisfy SQLite CHECK constraint
    let db_lang = sanitize_programming_lang(&analysis.programming_lang);
    if analysis.programming_lang.is_some() && db_lang.is_none() {
        warn!(
            "Coercing unsupported programming_lang='{:?}' to NULL for message {}",
            analysis.programming_lang, message_id
        );
    }
    
    sqlx::query!(
        r#"
        INSERT INTO message_analysis (
            message_id, mood, intensity, salience, original_salience, intent, topics, summary,
            relationship_impact, contains_code, language, programming_lang,
            contains_error, error_type, error_severity, error_file,
            analyzed_at, analysis_version, routed_to_heads, recall_count
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, 'structured_v1', ?, 0)
        "#,
        message_id,
        analysis.mood,
        analysis.intensity,
        analysis.salience,
        analysis.salience,
        analysis.intent,
        topics_json,
        analysis.summary,
        analysis.relationship_impact,
        analysis.contains_code,
        analysis.language,
        db_lang,
        analysis.contains_error,
        analysis.error_type,
        analysis.error_severity,
        analysis.error_file,
        heads_json
    )
    .execute(&mut **tx)
    .await?;
    
    debug!("Inserted message_analysis for message {} with original_salience={:?}", message_id, analysis.salience);
    Ok(())
}

pub async fn load_structured_response(pool: &SqlitePool, message_id: i64) -> Result<Option<CompleteResponse>> {
    let memory_row = match sqlx::query!(
        r#"
        SELECT id, session_id, response_id, role, content, timestamp
        FROM memory_entries
        WHERE id = ?
        "#,
        message_id
    )
    .fetch_optional(pool)
    .await? {
        Some(row) => row,
        None => return Ok(None),
    };

    let analysis_row = match sqlx::query!(
        r#"
        SELECT mood, intensity, salience, original_salience, intent, topics, summary,
               relationship_impact, contains_code, language, programming_lang, routed_to_heads,
               contains_error, error_type, error_severity, error_file
        FROM message_analysis
        WHERE message_id = ?
        "#,
        message_id
    )
    .fetch_optional(pool)
    .await? {
        Some(row) => row,
        None => return Ok(None),
    };

    let topics: Vec<String> = serde_json::from_str(&analysis_row.topics)?;
    let routed_to_heads: Vec<String> = serde_json::from_str(&analysis_row.routed_to_heads)?;

    let structured = StructuredLLMResponse {
        output: memory_row.content,
        analysis: crate::llm::structured::types::MessageAnalysis {
            salience: analysis_row.salience.unwrap_or(5.0) as f64,
            topics,
            contains_code: analysis_row.contains_code.unwrap_or(false),
            routed_to_heads,
            language: analysis_row.language.unwrap_or_else(|| "en".to_string()),
            mood: analysis_row.mood,
            intensity: analysis_row.intensity.map(|v| v as f64),
            intent: analysis_row.intent,
            summary: analysis_row.summary,
            relationship_impact: analysis_row.relationship_impact,
            programming_lang: analysis_row.programming_lang,
            contains_error: analysis_row.contains_error.unwrap_or(false),
            error_type: analysis_row.error_type,
            error_severity: analysis_row.error_severity,
            error_file: analysis_row.error_file,
        },
        reasoning: None,
        schema_name: Some("retrieved".to_string()),
        validation_status: Some("valid".to_string()),
    };

    // Create minimal metadata (since llm_metadata table is gone)
    let metadata = LLMMetadata {
        response_id: memory_row.response_id,
        prompt_tokens: None,
        completion_tokens: None,
        thinking_tokens: None,
        total_tokens: None,
        latency_ms: 0,
        finish_reason: None,
        model_version: "unknown".to_string(),
        temperature: 0.0,
        max_tokens: 4096,
    };

    let raw_response = serde_json::json!({
        "reconstructed": true,
        "message_id": message_id
    });

    Ok(Some(CompleteResponse {
        structured,
        metadata,
        raw_response,
        artifacts: None,
    }))
}

#[derive(Debug, Clone)]
pub struct ResponseStatistics {
    pub total_responses: i64,
    pub avg_tokens: f64,
    pub avg_latency_ms: f64,
    pub max_tokens: i64,
    pub min_tokens: i64,
}

// Backwards compatibility alias
pub type StructuredResponseStats = ResponseStatistics;
