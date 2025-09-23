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
/// FIXED: Excludes system messages (summaries) from chat history
pub const LOAD_RECENT: &str = r#"
    SELECT 
        m.id, m.session_id, m.role, m.content, m.timestamp, m.tags,
        m.response_id, m.parent_id,
        a.mood, a.intensity, a.salience, a.intent, a.topics, a.summary,
        a.contains_code, a.programming_lang, a.last_recalled
    FROM memory_entries m
    LEFT JOIN message_analysis a ON m.id = a.message_id
    WHERE m.session_id = ? AND m.role != 'system'
    ORDER BY m.timestamp DESC
    LIMIT ?
"#;

/// Load recent messages including system messages (for debugging/admin)
pub const LOAD_RECENT_ALL: &str = r#"
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

/// Get summaries for context building
pub const LOAD_SUMMARIES: &str = r#"
    SELECT 
        id, summary_type, summary_text, message_count, 
        first_message_id, last_message_id, created_at, embedding_generated
    FROM rolling_summaries 
    WHERE session_id = ? 
    ORDER BY created_at DESC 
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
        INNER JOIN thread t ON m.parent_id = t.id
    )
    SELECT * FROM thread
    ORDER BY timestamp ASC
"#;
