// src/memory/qdrant/search.rs

//! Qdrant search and filter helpers for semantic recall.

use crate::memory::types::{MemoryEntry, MemoryType, MemoryTag};
use chrono::{DateTime, Utc, NaiveDateTime};

/// Builds the JSON filter block for a session (and, optionally, tags/salience).
pub fn build_session_filter(session_id: &str) -> serde_json::Value {
    serde_json::json!({
        "must": [{
            "key": "session_id",
            "match": { "value": session_id }
        }]
    })
}

/// Optionally, build an advanced filter (for future use).
pub fn build_advanced_filter(session_id: &str, tags: Option<&[MemoryTag]>, min_salience: Option<f32>) -> serde_json::Value {
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

/// Safely convert milliseconds to DateTime<Utc> (with fallback to Unix epoch).
fn millis_to_datetime(ms: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_utc(
        NaiveDateTime::from_timestamp_millis(ms)
            .unwrap_or_else(|| NaiveDateTime::from_timestamp(0, 0)),
        Utc,
    )
}

/// Parses a single Qdrant point/payload result into a MemoryEntry.
/// Used after semantic search. Assumes field names match your schema.
pub fn parse_memory_entry_from_qdrant(point: &serde_json::Value) -> Option<MemoryEntry> {
    let payload = point.get("payload")?;
    let vector = point.get("vector")?;

    // Fix: Remove use of `?` in closure for timestamp parsing
    let timestamp = payload
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .map(millis_to_datetime)
        .unwrap_or_else(|| Utc::now());

    Some(MemoryEntry {
        id: point.get("id").and_then(|id| id.as_i64()),
        session_id: payload.get("session_id")?.as_str()?.to_string(),
        role: payload.get("role")?.as_str()?.to_string(),
        content: payload.get("content")?.as_str()?.to_string(),
        timestamp,
        embedding: Some(
            vector
                .as_array()?
                .iter()
                .filter_map(|val| val.as_f64().map(|f| f as f32))
                .collect(),
        ),
        salience: payload.get("salience").and_then(|v| v.as_f64()).map(|f| f as f32),
        tags: payload
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|tag| tag.as_str().map(|s| s.to_string()))
                    .collect()
            }),
        summary: payload.get("summary").and_then(|v| v.as_str()).map(|s| s.to_string()),
        memory_type: payload
            .get("memory_type")
            .and_then(|v| v.as_str())
            .and_then(|s| match s {
                "Feeling" => Some(MemoryType::Feeling),
                "Fact" => Some(MemoryType::Fact),
                "Joke" => Some(MemoryType::Joke),
                "Promise" => Some(MemoryType::Promise),
                "Event" => Some(MemoryType::Event),
                _ => Some(MemoryType::Other),
            }),
        logprobs: payload.get("logprobs").and_then(|v| serde_json::to_value(v).ok()),
        moderation_flag: payload.get("moderation_flag").and_then(|v| v.as_bool()),
        system_fingerprint: payload
            .get("system_fingerprint")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}
