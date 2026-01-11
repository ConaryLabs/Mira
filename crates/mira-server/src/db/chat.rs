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

    /// Mark messages as summarized and link to the summary for reversibility
    pub fn mark_messages_summarized(&self, start_id: i64, end_id: i64, summary_id: i64) -> Result<usize> {
        let conn = self.conn();
        let updated = conn.execute(
            "UPDATE chat_messages SET summarized = 1, summary_id = ? WHERE id >= ? AND id <= ?",
            params![summary_id, start_id, end_id],
        )?;
        Ok(updated)
    }

    /// Unroll a summary: restore original messages and delete the summary
    /// Returns the number of messages restored
    pub fn unroll_summary(&self, summary_id: i64) -> Result<usize> {
        let conn = self.conn();

        // Restore messages linked to this summary
        let restored = conn.execute(
            "UPDATE chat_messages SET summarized = 0, summary_id = NULL WHERE summary_id = ?",
            [summary_id],
        )?;

        // Delete the summary
        conn.execute("DELETE FROM chat_summaries WHERE id = ?", [summary_id])?;

        Ok(restored)
    }

    /// Get messages that belong to a specific summary (for preview before unrolling)
    pub fn get_messages_for_summary(&self, summary_id: i64) -> Result<Vec<ChatMessage>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, role, content, reasoning_content, created_at
             FROM chat_messages
             WHERE summary_id = ?
             ORDER BY id ASC"
        )?;

        let rows = stmt.query_map([summary_id], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                content: row.get(2)?,
                reasoning_content: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Store a chat summary
    pub fn store_chat_summary(
        &self,
        project_id: Option<i64>,
        summary: &str,
        range_start: i64,
        range_end: i64,
        level: i32,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO chat_summaries (project_id, summary, message_range_start, message_range_end, summary_level)
             VALUES (?, ?, ?, ?, ?)",
            params![project_id, summary, range_start, range_end, level],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get recent summaries for a project (or global if project_id is None)
    pub fn get_recent_summaries(
        &self,
        project_id: Option<i64>,
        level: i32,
        limit: usize,
    ) -> Result<Vec<ChatSummary>> {
        let conn = self.conn();

        let (sql, has_project) = match project_id {
            Some(_) => (
                "SELECT id, project_id, summary, message_range_start, message_range_end, summary_level, created_at
                 FROM chat_summaries
                 WHERE project_id = ? AND summary_level = ?
                 ORDER BY id DESC
                 LIMIT ?",
                true,
            ),
            None => (
                "SELECT id, project_id, summary, message_range_start, message_range_end, summary_level, created_at
                 FROM chat_summaries
                 WHERE project_id IS NULL AND summary_level = ?
                 ORDER BY id DESC
                 LIMIT ?",
                false,
            ),
        };

        let mut stmt = conn.prepare(sql)?;
        let rows: Box<dyn Iterator<Item = Result<ChatSummary, _>>> = if has_project {
            Box::new(stmt.query_map(params![project_id, level, limit as i64], |row| {
                Ok(ChatSummary {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    summary: row.get(2)?,
                    message_range_start: row.get(3)?,
                    message_range_end: row.get(4)?,
                    summary_level: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?)
        } else {
            Box::new(stmt.query_map(params![level, limit as i64], |row| {
                Ok(ChatSummary {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    summary: row.get(2)?,
                    message_range_start: row.get(3)?,
                    message_range_end: row.get(4)?,
                    summary_level: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?)
        };

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

    /// Count summaries at a given level for a project
    pub fn count_summaries_at_level(&self, project_id: Option<i64>, level: i32) -> Result<i64> {
        let conn = self.conn();
        let count: i64 = match project_id {
            Some(pid) => conn.query_row(
                "SELECT COUNT(*) FROM chat_summaries WHERE project_id = ? AND summary_level = ?",
                params![pid, level],
                |row| row.get(0),
            )?,
            None => conn.query_row(
                "SELECT COUNT(*) FROM chat_summaries WHERE project_id IS NULL AND summary_level = ?",
                [level],
                |row| row.get(0),
            )?,
        };
        Ok(count)
    }

    /// Get oldest summaries at a level for a project (for promotion to next level)
    pub fn get_oldest_summaries(
        &self,
        project_id: Option<i64>,
        level: i32,
        limit: usize,
    ) -> Result<Vec<ChatSummary>> {
        let conn = self.conn();

        let (sql, has_project) = match project_id {
            Some(_) => (
                "SELECT id, project_id, summary, message_range_start, message_range_end, summary_level, created_at
                 FROM chat_summaries
                 WHERE project_id = ? AND summary_level = ?
                 ORDER BY id ASC
                 LIMIT ?",
                true,
            ),
            None => (
                "SELECT id, project_id, summary, message_range_start, message_range_end, summary_level, created_at
                 FROM chat_summaries
                 WHERE project_id IS NULL AND summary_level = ?
                 ORDER BY id ASC
                 LIMIT ?",
                false,
            ),
        };

        let mut stmt = conn.prepare(sql)?;
        let rows: Box<dyn Iterator<Item = Result<ChatSummary, _>>> = if has_project {
            Box::new(stmt.query_map(params![project_id, level, limit as i64], |row| {
                Ok(ChatSummary {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    summary: row.get(2)?,
                    message_range_start: row.get(3)?,
                    message_range_end: row.get(4)?,
                    summary_level: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?)
        } else {
            Box::new(stmt.query_map(params![level, limit as i64], |row| {
                Ok(ChatSummary {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    summary: row.get(2)?,
                    message_range_start: row.get(3)?,
                    message_range_end: row.get(4)?,
                    summary_level: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?)
        };

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
    /// Get timestamp of the most recent chat message
    pub fn get_last_chat_time(&self) -> Result<Option<String>> {
        let conn = self.conn();
        let timestamp: Option<String> = conn.query_row(
            "SELECT created_at FROM chat_messages ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        ).ok();
        Ok(timestamp)
    }
}
