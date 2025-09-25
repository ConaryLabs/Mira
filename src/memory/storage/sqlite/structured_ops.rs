// src/memory/storage/sqlite/structured_ops.rs

use anyhow::{anyhow, Result};
use sqlx::{SqlitePool, Transaction, Sqlite};
use tracing::{debug, info};

use crate::llm::structured::{CompleteResponse, StructuredGPT5Response, GPT5Metadata};

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
    
    insert_gpt5_metadata(
        &mut tx,
        message_id,
        &response.metadata,
    ).await?;
    
    tx.commit().await?;
    
    info!("Saved complete response {} with all metadata", message_id);
    
    trigger_conditional_operations(message_id, &response.structured).await?;
    
    Ok(message_id)
}

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

async fn insert_message_analysis(
    tx: &mut Transaction<'_, Sqlite>,
    message_id: i64,
    analysis: &crate::llm::structured::types::MessageAnalysis,
) -> Result<()> {
    let topics_json = serde_json::to_string(&analysis.topics)?;
    let heads_json = serde_json::to_string(&analysis.routed_to_heads)?;
    
    sqlx::query!(
        r#"
        INSERT INTO message_analysis (
            message_id, mood, intensity, salience, original_salience, intent, topics, summary,
            relationship_impact, contains_code, language, programming_lang,
            analyzed_at, analysis_version, routed_to_heads, recall_count
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, 'structured_v1', ?, 0)
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
        analysis.programming_lang,
        heads_json
    )
    .execute(&mut **tx)
    .await?;
    
    debug!("Inserted message_analysis for message {} with original_salience={:?}", message_id, analysis.salience);
    Ok(())
}

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
        metadata.latency_ms,
        metadata.finish_reason,
        metadata.temperature,
        metadata.max_tokens,
        metadata.reasoning_effort,
        metadata.verbosity
    )
    .execute(&mut **tx)
    .await?;
    
    debug!("Inserted gpt5_metadata for message {}", message_id);
    Ok(())
}

async fn trigger_conditional_operations(
    _message_id: i64,
    response: &StructuredGPT5Response,
) -> Result<()> {
    if !response.analysis.routed_to_heads.is_empty() {
        info!("Queuing embeddings for heads: {:?}", response.analysis.routed_to_heads);
    }
    
    if response.analysis.contains_code {
        if let Some(ref lang) = response.analysis.programming_lang {
            info!("Triggering code analysis for {} code", lang);
        }
    }
    
    Ok(())
}

pub async fn load_structured_response(
    pool: &SqlitePool,
    message_id: i64,
) -> Result<Option<CompleteResponse>> {
    let memory_row = match sqlx::query!(
        "SELECT session_id, response_id, content, timestamp, tags FROM memory_entries WHERE id = ?",
        message_id
    )
    .fetch_optional(pool)
    .await? {
        Some(row) => row,
        None => return Ok(None),
    };

    let analysis_row = match sqlx::query!(
        r#"
        SELECT mood, intensity, salience, intent, topics, summary, relationship_impact,
               contains_code, language, programming_lang, routed_to_heads
        FROM message_analysis WHERE message_id = ?
        "#,
        message_id
    )
    .fetch_optional(pool)
    .await? {
        Some(row) => row,
        None => return Ok(None),
    };

    let metadata_row = match sqlx::query!(
        r#"
        SELECT model_version, prompt_tokens, completion_tokens, reasoning_tokens,
               total_tokens, latency_ms, finish_reason, temperature, max_tokens,
               reasoning_effort, verbosity
        FROM gpt5_metadata WHERE message_id = ?
        "#,
        message_id
    )
    .fetch_optional(pool)
    .await? {
        Some(row) => row,
        None => return Ok(None),
    };

    let topics: Vec<String> = serde_json::from_str(
        &analysis_row.topics.ok_or_else(|| anyhow!("Missing topics field"))?
    )?;
    
    let routed_to_heads: Vec<String> = serde_json::from_str(
        &analysis_row.routed_to_heads.ok_or_else(|| anyhow!("Missing routed_to_heads field"))?
    )?;

    let structured = StructuredGPT5Response {
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
        },
        reasoning: None,
        schema_name: Some("retrieved".to_string()),  // FIXED: Added missing field
        validation_status: Some("valid".to_string()),  // FIXED: Added missing field
    };

    let metadata = GPT5Metadata {
        response_id: memory_row.response_id,
        prompt_tokens: metadata_row.prompt_tokens.map(|v| v as i64),
        completion_tokens: metadata_row.completion_tokens.map(|v| v as i64),
        reasoning_tokens: metadata_row.reasoning_tokens.map(|v| v as i64),
        total_tokens: metadata_row.total_tokens.map(|v| v as i64),
        latency_ms: metadata_row.latency_ms.unwrap_or(0) as i64,
        finish_reason: metadata_row.finish_reason,
        model_version: metadata_row.model_version.unwrap_or_else(|| "gpt-5".to_string()),
        temperature: metadata_row.temperature.unwrap_or(0.0),
        max_tokens: metadata_row.max_tokens.unwrap_or(4096) as i64,
        reasoning_effort: metadata_row.reasoning_effort.unwrap_or_else(|| "medium".to_string()),
        verbosity: metadata_row.verbosity.unwrap_or_else(|| "medium".to_string()),
    };

    let raw_response = serde_json::json!({
        "reconstructed": true,
        "message_id": message_id
    });

    Ok(Some(CompleteResponse {
        structured,
        metadata,
        raw_response,
        artifacts: None,  // FIXED: Added missing field
    }))
}

pub async fn get_response_statistics(pool: &SqlitePool) -> Result<ResponseStatistics> {
    let stats_row = sqlx::query!(
        r#"
        SELECT 
            COUNT(*) as total_responses,
            AVG(CAST(total_tokens AS REAL)) as avg_tokens,
            AVG(CAST(latency_ms AS REAL)) as avg_latency_ms,
            MAX(total_tokens) as max_tokens,
            MIN(total_tokens) as min_tokens
        FROM gpt5_metadata
        WHERE total_tokens IS NOT NULL
        "#
    )
    .fetch_one(pool)
    .await?;

    Ok(ResponseStatistics {
        total_responses: stats_row.total_responses as u64,
        avg_tokens: stats_row.avg_tokens.unwrap_or(0.0),
        avg_latency_ms: stats_row.avg_latency_ms.unwrap_or(0.0),
        max_tokens: stats_row.max_tokens.unwrap_or(0) as u32,
        min_tokens: stats_row.min_tokens.unwrap_or(0) as u32,
    })
}

#[derive(Debug)]
pub struct ResponseStatistics {
    pub total_responses: u64,
    pub avg_tokens: f64,
    pub avg_latency_ms: f64,
    pub max_tokens: u32,
    pub min_tokens: u32,
}

pub type StructuredResponseStats = ResponseStatistics;
pub use get_response_statistics as get_structured_response_stats;
