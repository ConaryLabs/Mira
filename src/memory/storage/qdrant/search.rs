// src/memory/storage/qdrant/search.rs

use crate::memory::core::types::MemoryEntry;
use chrono::{DateTime, Utc};

/// Builds the JSON filter block for a session
pub fn build_session_filter(session_id: &str) -> serde_json::Value {
    serde_json::json!({
        "must": [{
            "key": "session_id",
            "match": { "value": session_id }
        }]
    })
}

/// Build an advanced filter with optional tags and salience
pub fn build_advanced_filter(
    session_id: &str,
    tags: Option<&[String]>,
    min_salience: Option<f32>,
) -> serde_json::Value {
    let mut must = vec![serde_json::json!({
        "key": "session_id",
        "match": { "value": session_id }
    })];

    if let Some(tags) = tags {
        must.push(serde_json::json!({
            "key": "tags",
            "match": { "any": tags }
        }));
    }

    if let Some(salience) = min_salience {
        must.push(serde_json::json!({
            "key": "salience",
            "range": { "gte": salience }
        }));
    }

    serde_json::json!({ "must": must })
}

/// Safely convert milliseconds to DateTime<Utc>
fn millis_to_datetime(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms)
        .unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap())
}

/// Parses a single Qdrant point/payload result into a MemoryEntry
pub fn parse_memory_entry_from_qdrant(point: &serde_json::Value) -> Option<MemoryEntry> {
    let payload = point.get("payload")?;
    let vector = point.get("vector");

    let timestamp = payload
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .map(millis_to_datetime)
        .unwrap_or_else(Utc::now);

    let analyzed_at = payload
        .get("analyzed_at")
        .and_then(|v| v.as_i64())
        .map(millis_to_datetime);

    let last_recalled = payload
        .get("last_recalled")
        .and_then(|v| v.as_i64())
        .map(millis_to_datetime);

    Some(MemoryEntry {
        id: payload.get("id").and_then(|id| id.as_i64()),
        session_id: payload.get("session_id")?.as_str()?.to_string(),
        response_id: payload
            .get("response_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        parent_id: payload.get("parent_id").and_then(|v| v.as_i64()),
        role: payload.get("role")?.as_str()?.to_string(),
        content: payload.get("content")?.as_str()?.to_string(),
        timestamp,
        tags: payload
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
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
        reasoning_effort: payload
            .get("reasoning_effort")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        verbosity: payload
            .get("verbosity")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        embedding: vector.and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|val| val.as_f64().map(|f| f as f32))
                    .collect()
            })
        }),
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
    })
}
