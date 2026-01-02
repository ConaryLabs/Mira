// db/chat.rs
// Chat message and summary storage operations

use anyhow::Result;
use rusqlite::params;

use super::types::{ChatMessage, ChatSummary};
use super::Database;

impl Database {
    /// Store a chat message
    pub fn store_chat_message(
        &self,
        role: &str,
        content: &str,
        reasoning_content: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO chat_messages (role, content, reasoning_content) VALUES (?, ?, ?)",
            params![role, content, reasoning_content],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get recent chat messages (for context window)
    pub fn get_recent_messages(&self, limit: usize) -> Result<Vec<ChatMessage>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, role, content, reasoning_content, created_at
             FROM chat_messages
             WHERE summarized = 0
             ORDER BY id DESC
             LIMIT ?"
        )?;

        let rows = stmt.query_map([limit as i64], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                reasoning_content: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;

        // Collect and reverse to get chronological order
        let mut messages: Vec<ChatMessage> = rows.filter_map(|r| r.ok()).collect();
        messages.reverse();
        Ok(messages)
    }

    /// Get messages older than a certain ID (for summarization)
    pub fn get_messages_before(&self, before_id: i64, limit: usize) -> Result<Vec<ChatMessage>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, role, content, reasoning_content, created_at
             FROM chat_messages
             WHERE id < ? AND summarized = 0
             ORDER BY id DESC
             LIMIT ?"
        )?;

        let rows = stmt.query_map(params![before_id, limit as i64], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                reasoning_content: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;

        let mut messages: Vec<ChatMessage> = rows.filter_map(|r| r.ok()).collect();
        messages.reverse();
        Ok(messages)
    }

    /// Mark messages as summarized
    pub fn mark_messages_summarized(&self, start_id: i64, end_id: i64) -> Result<usize> {
        let conn = self.conn();
        let updated = conn.execute(
            "UPDATE chat_messages SET summarized = 1 WHERE id >= ? AND id <= ?",
            params![start_id, end_id],
        )?;
        Ok(updated)
    }

    /// Store a chat summary
    pub fn store_chat_summary(
        &self,
        summary: &str,
        range_start: i64,
        range_end: i64,
        level: i32,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO chat_summaries (summary, message_range_start, message_range_end, summary_level)
             VALUES (?, ?, ?, ?)",
            params![summary, range_start, range_end, level],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get recent summaries
    pub fn get_recent_summaries(&self, level: i32, limit: usize) -> Result<Vec<ChatSummary>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, summary, message_range_start, message_range_end, summary_level, created_at
             FROM chat_summaries
             WHERE summary_level = ?
             ORDER BY id DESC
             LIMIT ?"
        )?;

        let rows = stmt.query_map(params![level, limit as i64], |row| {
            Ok(ChatSummary {
                id: row.get(0)?,
                summary: row.get(1)?,
                message_range_start: row.get(2)?,
                message_range_end: row.get(3)?,
                summary_level: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;

        let mut summaries: Vec<ChatSummary> = rows.filter_map(|r| r.ok()).collect();
        summaries.reverse();
        Ok(summaries)
    }

    /// Get count of unsummarized messages
    pub fn count_unsummarized_messages(&self) -> Result<i64> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM chat_messages WHERE summarized = 0",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Count summaries at a given level
    pub fn count_summaries_at_level(&self, level: i32) -> Result<i64> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM chat_summaries WHERE summary_level = ?",
            [level],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Get oldest summaries at a level (for promotion to next level)
    pub fn get_oldest_summaries(&self, level: i32, limit: usize) -> Result<Vec<ChatSummary>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, summary, message_range_start, message_range_end, summary_level, created_at
             FROM chat_summaries
             WHERE summary_level = ?
             ORDER BY id ASC
             LIMIT ?"
        )?;

        let rows = stmt.query_map(params![level, limit as i64], |row| {
            Ok(ChatSummary {
                id: row.get(0)?,
                summary: row.get(1)?,
                message_range_start: row.get(2)?,
                message_range_end: row.get(3)?,
                summary_level: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Delete summaries by IDs (after promotion)
    pub fn delete_summaries(&self, ids: &[i64]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let conn = self.conn();
        let placeholders: Vec<_> = ids.iter().map(|_| "?").collect();
        let sql = format!(
            "DELETE FROM chat_summaries WHERE id IN ({})",
            placeholders.join(",")
        );

        let params: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let deleted = conn.execute(&sql, params.as_slice())?;
        Ok(deleted)
    }
}
