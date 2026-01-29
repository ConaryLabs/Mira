// db/chat.rs
// Chat message and summary storage operations


use rusqlite::Connection;


// ═══════════════════════════════════════════════════════════════════════════════
// Sync functions for pool.interact() usage
// ═══════════════════════════════════════════════════════════════════════════════

/// Get timestamp of the most recent chat message (sync version for pool.interact)
pub fn get_last_chat_time_sync(conn: &Connection) -> rusqlite::Result<Option<String>> {
    let timestamp: Option<String> = conn
        .query_row(
            "SELECT created_at FROM chat_messages ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();
    Ok(timestamp)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Database impl methods
// ═══════════════════════════════════════════════════════════════════════════════

