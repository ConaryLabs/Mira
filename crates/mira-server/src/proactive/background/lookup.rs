// background/proactive/lookup.rs
// Fast O(1) lookup and feedback tracking for proactive suggestions

use rusqlite::params;

/// Get pre-generated suggestions for a trigger key (fast O(1) lookup)
pub fn get_pre_generated_suggestions(
    conn: &rusqlite::Connection,
    project_id: i64,
    trigger_key: &str,
) -> Result<Vec<(String, f64)>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached(
        r#"
        SELECT suggestion_text, confidence
        FROM proactive_suggestions
        WHERE project_id = ?
          AND trigger_key = ?
          AND (expires_at IS NULL OR expires_at > datetime('now'))
          AND created_at > datetime('now', '-4 hours')
        ORDER BY confidence DESC
        LIMIT 3
    "#,
    )?;

    let rows = stmt.query_map(params![project_id, trigger_key], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
    })?;

    rows.collect()
}

/// Get top pre-generated suggestions regardless of trigger key (for general context injection)
pub fn get_top_pre_generated_suggestions(
    conn: &rusqlite::Connection,
    project_id: i64,
    limit: usize,
) -> Result<Vec<(i64, String, f64)>, rusqlite::Error> {
    let mut stmt = conn.prepare_cached(
        r#"
        SELECT id, suggestion_text, confidence
        FROM proactive_suggestions
        WHERE project_id = ?
          AND (expires_at IS NULL OR expires_at > datetime('now'))
          AND created_at > datetime('now', '-4 hours')
        ORDER BY confidence DESC
        LIMIT ?
    "#,
    )?;

    let rows = stmt.query_map(params![project_id, limit as i64], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, f64>(2)?,
        ))
    })?;

    rows.collect()
}

/// Mark a suggestion as shown (for feedback tracking)
pub fn mark_suggestion_shown(
    conn: &rusqlite::Connection,
    project_id: i64,
    suggestion_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        UPDATE proactive_suggestions
        SET shown_count = shown_count + 1
        WHERE id = ? AND project_id = ?
    "#,
        params![suggestion_id, project_id],
    )?;
    Ok(())
}

/// Mark a suggestion as accepted (for feedback tracking)
pub fn mark_suggestion_accepted(
    conn: &rusqlite::Connection,
    project_id: i64,
    suggestion_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        UPDATE proactive_suggestions
        SET accepted_count = accepted_count + 1
        WHERE id = ? AND project_id = ?
    "#,
        params![suggestion_id, project_id],
    )?;
    Ok(())
}
