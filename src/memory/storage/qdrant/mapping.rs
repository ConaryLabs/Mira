// src/memory/qdrant/mapping.rs

//! Maps between MemoryEntry structs and Qdrant payload JSON for point upserts/search.

use crate::memory::core::types::{MemoryEntry, MemoryType};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::str::FromStr;

/// Converts a MemoryEntry to Qdrant payload (serde_json::Value)
pub fn memory_entry_to_payload(entry: &MemoryEntry) -> Value {
    json!({
        "id": entry.id, // also include the sqlite ID for easier lookup
        "session_id": entry.session_id,
        "role": entry.role,
        "content": entry.content,
        "timestamp": entry.timestamp.timestamp_millis(),
        "salience": entry.salience,
        "tags": entry.tags,
        "summary": entry.summary,
        "memory_type": entry.memory_type.as_ref().map(|mt| format!("{mt:?}")),
        "logprobs": entry.logprobs,
        "moderation_flag": entry.moderation_flag,
        "system_fingerprint": entry.system_fingerprint,

        // Robust memory (Phase 3)
        "head": entry.head,
        "is_code": entry.is_code,
        "lang": entry.lang,
        "topics": entry.topics,

        // Phase 4 additions
        "pinned": entry.pinned, // store as boolean in payload
        "subject_tag": entry.subject_tag,
        "last_accessed": entry.last_accessed.map(|t| t.timestamp_millis()),
    })
}

/// Converts Qdrant payload JSON + vector to a MemoryEntry.
/// (Vector is required—Qdrant always returns it.)
pub fn payload_to_memory_entry(payload: &Value, vector: &[f32], id: Option<i64>) -> MemoryEntry {
    let timestamp = payload
        .get("timestamp")
        .and_then(|v| v.as_i64())
        .and_then(DateTime::from_timestamp_millis)
        .unwrap_or_else(Utc::now);

    // Phase 4: try to read last_accessed (ms), else default to now
    let last_accessed = payload
        .get("last_accessed")
        .and_then(|v| v.as_i64())
        .and_then(DateTime::from_timestamp_millis)
        .unwrap_or_else(Utc::now);

    MemoryEntry {
        id,
        session_id: payload
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
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
        embedding: Some(vector.to_vec()),
        salience: payload
            .get("salience")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32),
        tags: payload.get("tags").and_then(|v| v.as_array()).map(|arr| {
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
    }
}
