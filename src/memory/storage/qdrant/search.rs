// src/memory/qdrant/search.rs

//! Qdrant search and filter helpers for semantic recall.

use crate::memory::core::types::{MemoryEntry, MemoryType, MemoryTag};
use chrono::{DateTime, Utc};
use std::str::FromStr; // Needed for MemoryType parsing

/// Builds the JSON filter block for a session (and, optionally, tags/salience).
pub fn build_session_filter(session_id: &str) -> serde_json::Value {
    serde_json::json!({
        "must": [{
            "key": "session_id",
            "match": { "value": session_id }
        }]
    })
}

/// Optionally, build an advanced filter.
pub fn build_advanced_filter(
    session_id: &str,
    tags: Option<&[MemoryTag]>,
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

/// Parses a single Qdrant point/payload result into a MemoryEntry.
/// Used after semantic search. Assumes field names match your schema.
pub fn parse_memory_entry_from_qdrant(point: &serde_json::Value) -> Option<MemoryEntry> {
    let payload = point.get("payload")?;
    let vector = point.get("vector"); // optional

    let timestamp = payload
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .map(millis_to_datetime)
        .unwrap_or_else(Utc::now);

    // Phase 4: try to read last_accessed (ms), else default to now
    let last_accessed = payload
        .get("last_accessed")
        .and_then(|v| v.as_i64())
        .map(millis_to_datetime)
        .unwrap_or_else(Utc::now);

    Some(MemoryEntry {
        id: payload.get("id").and_then(|id| id.as_i64()),
        session_id: payload.get("session_id")?.as_str()?.to_string(),
        role: payload.get("role")?.as_str()?.to_string(),
        content: payload.get("content")?.as_str()?.to_string(),
        timestamp,

        embedding: vector.and_then(|v| {
            v.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|val| val.as_f64().map(|f| f as f32))
                    .collect()
            })
        }),
        salience: payload
            .get("salience")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32),
        tags: payload
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|tag| tag.as_str().map(|s| s.to_string()))
                    .collect()
            }),
        summary: payload
            .get("summary")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        memory_type: payload
            .get("memory_type")
            .and_then(|v| v.as_str())
            .and_then(|s| MemoryType::from_str(s).ok()),
        logprobs: payload.get("logprobs").cloned(),
        moderation_flag: payload.get("moderation_flag").and_then(|v| v.as_bool()),
        system_fingerprint: payload
            .get("system_fingerprint")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),

        // Robust memory (Phase 3)
        head: payload.get("head").and_then(|v| v.as_str()).map(String::from),
        is_code: payload.get("is_code").and_then(|v| v.as_bool()),
        lang: payload.get("lang").and_then(|v| v.as_str()).map(String::from),
        topics: payload
            .get("topics")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|topic| topic.as_str().map(String::from))
                    .collect()
            }),

        // Phase 4 additions
        pinned: payload.get("pinned").and_then(|v| v.as_bool()).or(Some(false)),
        subject_tag: payload
            .get("subject_tag")
            .and_then(|v| v.as_str())
            .map(String::from),
        last_accessed: Some(last_accessed),
    })
}
