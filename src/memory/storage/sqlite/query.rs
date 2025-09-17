// src/memory/storage/sqlite/query.rs

//! SQL query strings and helper functions for the SQLite memory store.
//! Updated for the new memory_entries and message_analysis schema.

/// Inserts a new memory entry into the memory_entries table
pub const INSERT_MEMORY: &str = r#"
    INSERT INTO memory_entries (
        session_id, response_id, parent_id, role, content, timestamp, tags
    ) VALUES (?, ?, ?, ?, ?, ?, ?)
    RETURNING id
"#;

/// Loads the last N messages with analysis data for a session
pub const LOAD_RECENT: &str = r#"
    SELECT 
        m.id, m.session_id, m.role, m.content, m.timestamp, m.tags,
        m.response_id, m.parent_id,
        a.mood, a.intensity, a.salience, a.intent, a.topics, a.summary,
        a.contains_code, a.programming_lang, a.last_recalled
    FROM memory_entries m
    LEFT JOIN message_analysis a ON m.id = a.message_id
    WHERE m.session_id = ?
    ORDER BY m.timestamp DESC
    LIMIT ?
"#;

/// Updates message analysis for an entry
pub const UPDATE_ANALYSIS: &str = r#"
    INSERT INTO message_analysis (
        message_id, mood, intensity, salience, intent, topics, summary,
        contains_code, programming_lang, routed_to_heads, analysis_version
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(message_id) DO UPDATE SET
        mood = excluded.mood,
        intensity = excluded.intensity,
        salience = excluded.salience,
        intent = excluded.intent,
        topics = excluded.topics,
        summary = excluded.summary,
        contains_code = excluded.contains_code,
        programming_lang = excluded.programming_lang,
        routed_to_heads = excluded.routed_to_heads,
        analysis_version = excluded.analysis_version,
        analyzed_at = CURRENT_TIMESTAMP
"#;

/// Updates tags for a memory entry
pub const UPDATE_TAGS: &str = r#"
    UPDATE memory_entries
    SET tags = ?
    WHERE id = ?
"#;

/// Deletes a memory entry and its analysis
pub const DELETE_MEMORY: &str = r#"
    DELETE FROM memory_entries WHERE id = ?
"#;

/// Updates recall metadata when a memory is accessed
pub const UPDATE_RECALL: &str = r#"
    UPDATE message_analysis 
    SET last_recalled = CURRENT_TIMESTAMP,
        recall_count = COALESCE(recall_count, 0) + 1
    WHERE message_id = ?
"#;

/// Gets messages in a conversation thread
pub const GET_THREAD: &str = r#"
    WITH RECURSIVE thread AS (
        SELECT id, parent_id, session_id, role, content, timestamp, tags
        FROM memory_entries 
        WHERE response_id = ?
        
        UNION ALL
        
        SELECT m.id, m.parent_id, m.session_id, m.role, m.content, m.timestamp, m.tags
        FROM memory_entries m
        JOIN thread t ON m.id = t.parent_id
    )
    SELECT * FROM thread ORDER BY timestamp ASC
"#;

/// Helper: Convert Vec<f32> embedding to bytes (for compatibility, though embeddings are in Qdrant now)
pub fn embedding_f32_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Helper: Convert bytes back to Vec<f32> (for compatibility)
pub fn embedding_bytes_to_f32(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}
