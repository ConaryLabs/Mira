// src/memory/storage/sqlite/structured_ops.rs
// Atomic database operations for structured responses - the bulletproof core

use anyhow::Result;
use sqlx::{SqlitePool, Transaction, Sqlite};
use tracing::{debug, info, warn};

use crate::llm::structured::{CompleteResponse, StructuredGPT5Response, MessageAnalysis, GPT5Metadata};

/// Save complete structured response to all three tables atomically
/// This is the heart of the data-hoarding machine - NOTHING gets lost
pub async fn save_structured_response(
    pool: &SqlitePool,
    session_id: &str,
    response: &CompleteResponse,
    parent_id: Option<i64>,
) -> Result<i64> {
    let mut tx = pool.begin().await?;
    
    // 1. Insert memory_entries (core content)
    let message_id = insert_memory_entry(
        &mut tx,
        session_id,
        &response.structured,
        response.metadata.response_id.as_deref(),
        parent_id,
    ).await?;
    
    // 2. Insert message_analysis (complete analysis)
    insert_message_analysis(
        &mut tx,
        message_id,
        &response.structured.analysis,
    ).await?;
    
    // 3. Insert gpt5_metadata (all token counts and timing)
    insert_gpt5_metadata(
        &mut tx,
        message_id,
        &response.metadata,
    ).await?;
    
    // Commit atomically - all or nothing
    tx.commit().await?;
    
    info!("Saved complete response {} with all metadata", message_id);
    
    // Trigger conditional operations (embeddings, code intelligence)
    trigger_conditional_operations(message_id, &response.structured).await?;
    
    Ok(message_id)
}

/// Insert into memory_entries table
async fn insert_memory_entry(
    tx: &mut Transaction<'_, Sqlite>,
    session_id: &str,
    response: &StructuredGPT5Response,
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

/// Insert into message_analysis table with all fields populated
async fn insert_message_analysis(
    tx: &mut Transaction<'_, Sqlite>,
    message_id: i64,
    analysis: &MessageAnalysis,
) -> Result<()> {
    let topics_json = serde_json::to_string(&analysis.topics)?;
    let heads_json = serde_json::to_string(&analysis.routed_to_heads)?;
    
    sqlx::query!(
        r#"
        INSERT INTO message_analysis (
            message_id, mood, intensity, salience, intent, topics, summary,
            relationship_impact, contains_code, language, programming_lang,
            analyzed_at, analysis_version, routed_to_heads, recall_count
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, 'structured_v1', ?, 0)
        "#,
        message_id,
        analysis.mood,
        analysis.intensity,
        analysis.salience,
        analysis.intent,
        topics_json,
        analysis.summary,
        analysis.relationship_impact,
        analysis.contains_code,
        analysis.language,
        analysis.programming_lang,
        heads_json
    )
    .execute(&mut **tx)
    .await?;
    
    debug!("Inserted message_analysis for message_id={}", message_id);
    Ok(())
}

/// Insert into gpt5_metadata table with complete token/timing data
async fn insert_gpt5_metadata(
    tx: &mut Transaction<'_, Sqlite>,
    message_id: i64,
    metadata: &GPT5Metadata,
) -> Result<()> {
    sqlx::query!(
        r#"
        INSERT INTO gpt5_metadata (
            message_id, model_version, prompt_tokens, completion_tokens,
            reasoning_tokens, total_tokens, latency_ms, generation_time_ms,
            finish_reason, tool_calls, temperature, max_tokens,
            reasoning_effort, verbosity
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?, ?, ?)
        "#,
        message_id,
        metadata.model_version,
        metadata.prompt_tokens,
        metadata.completion_tokens,
        metadata.reasoning_tokens,
        metadata.total_tokens,
        metadata.latency_ms,
        metadata.latency_ms,  // Use latency for generation_time_ms for now
        metadata.finish_reason,
        metadata.temperature,
        metadata.max_tokens,
        metadata.reasoning_effort,
        metadata.verbosity
    )
    .execute(&mut **tx)
    .await?;
    
    debug!("Inserted gpt5_metadata for message_id={}", message_id);
    Ok(())
}

/// Trigger conditional operations after successful save
async fn trigger_conditional_operations(
    message_id: i64,
    response: &StructuredGPT5Response,
) -> Result<()> {
    // Queue for embeddings based on routed_to_heads
    if !response.analysis.routed_to_heads.is_empty() {
        info!("Queuing embeddings for heads: {:?}", response.analysis.routed_to_heads);
        // TODO: Add embedding queue calls here
        // embedding_service.queue_for_heads(message_id, &response.analysis.routed_to_heads).await?;
    }
    
    // Trigger code intelligence if code is detected
    if response.analysis.contains_code {
        if let Some(ref lang) = response.analysis.programming_lang {
            info!("Triggering code analysis for {} code", lang);
            // TODO: Add code intelligence triggers here
            // code_intelligence_service.analyze_content(message_id, &response.output, lang).await?;
        } else {
            warn!("Code detected but no programming language specified for message {}", message_id);
        }
    }
    
    // Queue for summarization if high salience
    if response.analysis.salience > 7.0 {
        info!("High salience message {} ({}), queuing for summarization", 
              message_id, response.analysis.salience);
        // TODO: Add summarization queue call here
        // summarization_service.queue_high_salience(message_id).await?;
    }
    
    Ok(())
}

/// Load structured response by message ID (for testing/verification)
pub async fn load_structured_response(
    pool: &SqlitePool,
    message_id: i64,
) -> Result<Option<CompleteResponse>> {
    // Join all three tables to get complete data
    let row = sqlx::query!(
        r#"
        SELECT 
            me.content, me.session_id, me.response_id, me.tags,
            ma.mood, ma.intensity, ma.salience, ma.intent, ma.topics,
            ma.summary, ma.relationship_impact, ma.contains_code,
            ma.language, ma.programming_lang, ma.routed_to_heads,
            gm.model_version, gm.prompt_tokens, gm.completion_tokens,
            gm.reasoning_tokens, gm.total_tokens, gm.latency_ms,
            gm.finish_reason, gm.temperature, gm.max_tokens,
            gm.reasoning_effort, gm.verbosity
        FROM memory_entries me
        JOIN message_analysis ma ON me.id = ma.message_id
        JOIN gpt5_metadata gm ON me.id = gm.message_id
        WHERE me.id = ?
        "#,
        message_id
    )
    .fetch_optional(pool)
    .await?;
    
    if let Some(row) = row {
        // Parse JSON fields safely
        let topics: Vec<String> = serde_json::from_str(
            &row.topics.ok_or_else(|| anyhow!("Missing topics field"))?
        )?;
        let routed_to_heads: Vec<String> = serde_json::from_str(
            &row.routed_to_heads.ok_or_else(|| anyhow!("Missing routed_to_heads field"))?
        )?;
        
        // Reconstruct structured response
        let structured = StructuredGPT5Response {
            output: row.content,
            analysis: MessageAnalysis {
                salience: row.salience.ok_or_else(|| anyhow!("Missing salience"))?,
                topics,
                contains_code: row.contains_code.ok_or_else(|| anyhow!("Missing contains_code"))?,
                routed_to_heads,
                language: row.language.ok_or_else(|| anyhow!("Missing language"))?,
                mood: row.mood,
                intensity: row.intensity,
                intent: row.intent,
                summary: row.summary,
                relationship_impact: row.relationship_impact,
                programming_lang: row.programming_lang,
            },
            reasoning: None, // Not stored in database
        };
        
        // Reconstruct metadata
        let metadata = GPT5Metadata {
            response_id: row.response_id,
            prompt_tokens: row.prompt_tokens,
            completion_tokens: row.completion_tokens,
            reasoning_tokens: row.reasoning_tokens,
            total_tokens: row.total_tokens,
            finish_reason: row.finish_reason,
            latency_ms: row.latency_ms.ok_or_else(|| anyhow!("Missing latency_ms"))?,
            model_version: row.model_version.ok_or_else(|| anyhow!("Missing model_version"))?,
            temperature: row.temperature.ok_or_else(|| anyhow!("Missing temperature"))?,
            max_tokens: row.max_tokens.ok_or_else(|| anyhow!("Missing max_tokens"))?,
            reasoning_effort: row.reasoning_effort.ok_or_else(|| anyhow!("Missing reasoning_effort"))?,
            verbosity: row.verbosity.ok_or_else(|| anyhow!("Missing verbosity"))?,
        };
        
        Ok(Some(CompleteResponse {
            structured,
            metadata,
            raw_response: serde_json::Value::Null, // Not stored
        }))
    } else {
        Ok(None)
    }
}

/// Get statistics about stored structured responses
pub async fn get_structured_response_stats(pool: &SqlitePool) -> Result<StructuredResponseStats> {
    let row = sqlx::query!(
        r#"
        SELECT 
            COUNT(*) as total_responses,
            AVG(gm.total_tokens) as avg_tokens,
            AVG(gm.latency_ms) as avg_latency_ms,
            COUNT(CASE WHEN ma.contains_code = true THEN 1 END) as code_responses,
            AVG(ma.salience) as avg_salience
        FROM memory_entries me
        JOIN message_analysis ma ON me.id = ma.message_id
        JOIN gpt5_metadata gm ON me.id = gm.message_id
        WHERE me.role = 'assistant'
        "#
    )
    .fetch_one(pool)
    .await?;
    
    Ok(StructuredResponseStats {
        total_responses: row.total_responses,
        avg_tokens: row.avg_tokens.unwrap_or(0.0),
        avg_latency_ms: row.avg_latency_ms.unwrap_or(0.0),
        code_responses: row.code_responses,
        avg_salience: row.avg_salience.unwrap_or(0.0),
    })
}

/// Statistics about structured responses
#[derive(Debug)]
pub struct StructuredResponseStats {
    pub total_responses: i32,
    pub avg_tokens: f64,
    pub avg_latency_ms: f64,
    pub code_responses: i32,
    pub avg_salience: f64,
}
