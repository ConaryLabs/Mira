// src/memory/sqlite/query.rs

//! All raw SQL query strings and helper functions for the SQLite memory store.

/// Inserts a new memory entry into the chat_history table.
pub const INSERT_MEMORY: &str = r#"
    INSERT INTO chat_history (
        session_id, role, content, timestamp,
        embedding, salience, tags, summary, memory_type,
        logprobs, moderation_flag, system_fingerprint
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
"#;

/// Loads the last N messages for a given session, most recent first.
pub const LOAD_RECENT: &str = r#"
    SELECT id, session_id, role, content, timestamp, embedding, salience, tags, summary, memory_type,
           logprobs, moderation_flag, system_fingerprint
    FROM chat_history
    WHERE session_id = ?
    ORDER BY timestamp DESC
    LIMIT ?
"#;

/// Updates memory metadata for an entry by id.
pub const UPDATE_METADATA: &str = r#"
    UPDATE chat_history
    SET embedding = ?, salience = ?, tags = ?, summary = ?, memory_type = ?,
        logprobs = ?, moderation_flag = ?, system_fingerprint = ?
    WHERE id = ?
"#;

/// Deletes a memory entry by id.
pub const DELETE_MEMORY: &str = r#"
    DELETE FROM chat_history WHERE id = ?
"#;

/// (Optional) Selects all flagged/unsafe messages for moderation.
pub const LOAD_FLAGGED: &str = r#"
    SELECT id, session_id, role, content, timestamp, embedding, salience, tags, summary, memory_type,
           logprobs, moderation_flag, system_fingerprint
    FROM chat_history
    WHERE moderation_flag = 1
    ORDER BY timestamp DESC
"#;

/// (Helper) Converts a Vec<f32> embedding into a Vec<u8> for SQLite blob storage.
pub fn embedding_f32_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// (Helper) Converts a SQLite blob (Vec<u8>) back into Vec<f32>.
pub fn embedding_bytes_to_f32(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}
