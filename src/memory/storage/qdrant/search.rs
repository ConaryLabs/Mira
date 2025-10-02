// src/memory/storage/qdrant/search.rs

use crate::memory::core::types::MemoryEntry;

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

/// Parses a single Qdrant point/payload result into a MemoryEntry
/// 
/// This is a compatibility wrapper - the actual implementation is in mapping.rs
/// to avoid duplicate code. Use payload_to_memory_entry() directly for new code.
pub fn parse_memory_entry_from_qdrant(point: &serde_json::Value) -> Option<MemoryEntry> {
    let payload = point.get("payload")?;
    let vector = point.get("vector")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|val| val.as_f64().map(|f| f as f32))
                .collect::<Vec<f32>>()
        })?;
    
    let id = payload.get("id").and_then(|id| id.as_i64());
    
    Some(super::mapping::payload_to_memory_entry(payload, &vector, id))
}
