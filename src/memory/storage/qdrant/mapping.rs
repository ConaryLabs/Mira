// src/memory/storage/qdrant/mapping.rs

use crate::memory::core::types::MemoryEntry;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

/// Converts a MemoryEntry to Qdrant payload (serde_json::Value)
pub fn memory_entry_to_payload(entry: &MemoryEntry) -> Value {
    json!({
        // Core fields
        "id": entry.id,
        "session_id": entry.session_id,
        "response_id": entry.response_id,
        "parent_id": entry.parent_id,
        "role": entry.role,
        "content": entry.content,
        "timestamp": entry.timestamp.timestamp_millis(),
        "tags": entry.tags,
        
        // Analysis fields
        "mood": entry.mood,
        "intensity": entry.intensity,
        "salience": entry.salience,
        "original_salience": entry.original_salience,
        "intent": entry.intent,
        "topics": entry.topics,
        "summary": entry.summary,
        "relationship_impact": entry.relationship_impact,
        "contains_code": entry.contains_code,
        "language": entry.language,
        "programming_lang": entry.programming_lang,
        "analyzed_at": entry.analyzed_at.map(|t| t.timestamp_millis()),
        "analysis_version": entry.analysis_version,
        "routed_to_heads": entry.routed_to_heads,
        "last_recalled": entry.last_recalled.map(|t| t.timestamp_millis()),
        "recall_count": entry.recall_count,
        
        // LLM metadata fields
        "model_version": entry.model_version,
        "prompt_tokens": entry.prompt_tokens,
        "completion_tokens": entry.completion_tokens,
        "reasoning_tokens": entry.reasoning_tokens,
        "total_tokens": entry.total_tokens,
        "latency_ms": entry.latency_ms,
        "generation_time_ms": entry.generation_time_ms,
        "finish_reason": entry.finish_reason,
        "tool_calls": entry.tool_calls,
        "temperature": entry.temperature,
        "max_tokens": entry.max_tokens,
        
        // Embedding metadata
        "embedding_heads": entry.embedding_heads,
        "qdrant_point_ids": entry.qdrant_point_ids,
    })
}

/// Converts Qdrant payload JSON + vector to a MemoryEntry
/// Note: All string allocations here are necessary - Qdrant returns borrowed JSON
/// that doesn't live long enough, so we must own the strings.
pub fn payload_to_memory_entry(payload: &Value, vector: &[f32], id: Option<i64>) -> MemoryEntry {
    let timestamp = payload
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .and_then(DateTime::from_timestamp_millis)
        .unwrap_or_else(Utc::now);

    let analyzed_at = payload
        .get("analyzed_at")
        .and_then(|v| v.as_i64())
        .and_then(DateTime::from_timestamp_millis);

    let last_recalled = payload
        .get("last_recalled")
        .and_then(|v| v.as_i64())
        .and_then(DateTime::from_timestamp_millis);

    MemoryEntry {
        id,
        session_id: payload
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        response_id: payload
            .get("response_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        parent_id: payload
            .get("parent_id")
            .and_then(|v| v.as_i64()),
        role: payload
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        content: payload
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        timestamp,
        tags: payload.get("tags").and_then(|v| v.as_array()).map(|arr| {
            arr.iter()
                .filter_map(|tag| tag.as_str().map(|s| s.to_string()))
                .collect()
        }),
        mood: payload
            .get("mood")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        intensity: payload
            .get("intensity")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32),
        salience: payload
            .get("salience")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32),
        original_salience: payload
            .get("original_salience")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32),
        intent: payload
            .get("intent")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        topics: payload
            .get("topics")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|topic| topic.as_str().map(String::from))
                    .collect()
            }),
        summary: payload
            .get("summary")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        relationship_impact: payload
            .get("relationship_impact")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        contains_code: payload.get("contains_code").and_then(|v| v.as_bool()),
        language: payload
            .get("language")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        programming_lang: payload
            .get("programming_lang")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        analyzed_at,
        analysis_version: payload
            .get("analysis_version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        routed_to_heads: payload
            .get("routed_to_heads")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|h| h.as_str().map(String::from))
                    .collect()
            }),
        last_recalled,
        recall_count: payload
            .get("recall_count")
            .and_then(|v| v.as_i64())
            .map(|i| i as i32),
        model_version: payload
            .get("model_version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        prompt_tokens: payload
            .get("prompt_tokens")
            .and_then(|v| v.as_i64())
            .map(|i| i as i32),
        completion_tokens: payload
            .get("completion_tokens")
            .and_then(|v| v.as_i64())
            .map(|i| i as i32),
        reasoning_tokens: payload
            .get("reasoning_tokens")
            .and_then(|v| v.as_i64())
            .map(|i| i as i32),
        total_tokens: payload
            .get("total_tokens")
            .and_then(|v| v.as_i64())
            .map(|i| i as i32),
        latency_ms: payload
            .get("latency_ms")
            .and_then(|v| v.as_i64())
            .map(|i| i as i32),
        generation_time_ms: payload
            .get("generation_time_ms")
            .and_then(|v| v.as_i64())
            .map(|i| i as i32),
        finish_reason: payload
            .get("finish_reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        tool_calls: payload
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| tc.as_str().map(String::from))
                    .collect()
            }),
        temperature: payload
            .get("temperature")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32),
        max_tokens: payload
            .get("max_tokens")
            .and_then(|v| v.as_i64())
            .map(|i| i as i32),
        embedding: Some(vector.to_vec()),
        embedding_heads: payload
            .get("embedding_heads")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|h| h.as_str().map(String::from))
                    .collect()
            }),
        qdrant_point_ids: payload
            .get("qdrant_point_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|id| id.as_str().map(String::from))
                    .collect()
            }),
    }
}
