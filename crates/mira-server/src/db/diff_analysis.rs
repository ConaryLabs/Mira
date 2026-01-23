// crates/mira-server/src/db/diff_analysis.rs
// Database operations for semantic diff analysis

use anyhow::Result;
use rusqlite::params;

use super::Database;

/// A stored diff analysis result
#[derive(Debug, Clone)]
pub struct DiffAnalysis {
    pub id: i64,
    pub project_id: Option<i64>,
    pub from_commit: String,
    pub to_commit: String,
    pub analysis_type: String,
    pub changes_json: Option<String>,
    pub impact_json: Option<String>,
    pub risk_json: Option<String>,
    pub summary: Option<String>,
    pub files_changed: Option<i64>,
    pub lines_added: Option<i64>,
    pub lines_removed: Option<i64>,
    pub status: String,
    pub created_at: String,
}

/// Parse DiffAnalysis from a rusqlite Row
pub fn parse_diff_analysis_row(row: &rusqlite::Row) -> rusqlite::Result<DiffAnalysis> {
    Ok(DiffAnalysis {
        id: row.get(0)?,
        project_id: row.get(1)?,
        from_commit: row.get(2)?,
        to_commit: row.get(3)?,
        analysis_type: row.get(4)?,
        changes_json: row.get(5)?,
        impact_json: row.get(6)?,
        risk_json: row.get(7)?,
        summary: row.get(8)?,
        files_changed: row.get(9)?,
        lines_added: row.get(10)?,
        lines_removed: row.get(11)?,
        status: row.get(12)?,
        created_at: row.get(13)?,
    })
}

impl Database {
    /// Store a new diff analysis
    pub fn store_diff_analysis(
        &self,
        project_id: Option<i64>,
        from_commit: &str,
        to_commit: &str,
        analysis_type: &str,
        changes_json: Option<&str>,
        impact_json: Option<&str>,
        risk_json: Option<&str>,
        summary: Option<&str>,
        files_changed: Option<i64>,
        lines_added: Option<i64>,
        lines_removed: Option<i64>,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO diff_analyses (
                project_id, from_commit, to_commit, analysis_type,
                changes_json, impact_json, risk_json, summary,
                files_changed, lines_added, lines_removed
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                project_id,
                from_commit,
                to_commit,
                analysis_type,
                changes_json,
                impact_json,
                risk_json,
                summary,
                files_changed,
                lines_added,
                lines_removed
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get a cached diff analysis if it exists
    pub fn get_cached_diff_analysis(
        &self,
        project_id: Option<i64>,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<Option<DiffAnalysis>> {
        let conn = self.conn();
        let sql = "SELECT id, project_id, from_commit, to_commit, analysis_type,
                          changes_json, impact_json, risk_json, summary,
                          files_changed, lines_added, lines_removed, status, created_at
                   FROM diff_analyses
                   WHERE (project_id = ? OR (project_id IS NULL AND ? IS NULL))
                         AND from_commit = ? AND to_commit = ?
                   ORDER BY created_at DESC
                   LIMIT 1";
        let mut stmt = conn.prepare(sql)?;
        let mut rows = stmt.query_map(
            params![project_id, project_id, from_commit, to_commit],
            parse_diff_analysis_row,
        )?;
        match rows.next() {
            Some(Ok(analysis)) => Ok(Some(analysis)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Get recent diff analyses for a project
    pub fn get_recent_diff_analyses(
        &self,
        project_id: Option<i64>,
        limit: usize,
    ) -> Result<Vec<DiffAnalysis>> {
        let conn = self.conn();
        let sql = "SELECT id, project_id, from_commit, to_commit, analysis_type,
                          changes_json, impact_json, risk_json, summary,
                          files_changed, lines_added, lines_removed, status, created_at
                   FROM diff_analyses
                   WHERE project_id = ? OR project_id IS NULL
                   ORDER BY created_at DESC
                   LIMIT ?";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![project_id, limit as i64], parse_diff_analysis_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get a diff analysis by ID
    pub fn get_diff_analysis(&self, analysis_id: i64) -> Result<Option<DiffAnalysis>> {
        let conn = self.conn();
        let sql = "SELECT id, project_id, from_commit, to_commit, analysis_type,
                          changes_json, impact_json, risk_json, summary,
                          files_changed, lines_added, lines_removed, status, created_at
                   FROM diff_analyses
                   WHERE id = ?";
        let mut stmt = conn.prepare(sql)?;
        let mut rows = stmt.query_map([analysis_id], parse_diff_analysis_row)?;
        match rows.next() {
            Some(Ok(analysis)) => Ok(Some(analysis)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Delete old diff analyses (keep only recent N per project)
    pub fn cleanup_old_diff_analyses(&self, project_id: Option<i64>, keep_count: usize) -> Result<usize> {
        let conn = self.conn();
        let sql = "DELETE FROM diff_analyses
                   WHERE (project_id = ? OR (project_id IS NULL AND ? IS NULL))
                         AND id NOT IN (
                             SELECT id FROM diff_analyses
                             WHERE project_id = ? OR (project_id IS NULL AND ? IS NULL)
                             ORDER BY created_at DESC
                             LIMIT ?
                         )";
        let deleted = conn.execute(
            sql,
            params![project_id, project_id, project_id, project_id, keep_count as i64],
        )?;
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_retrieve_diff_analysis() {
        let db = Database::open_in_memory().unwrap();
        let (project_id, _) = db.get_or_create_project("/test", None).unwrap();

        let id = db.store_diff_analysis(
            Some(project_id),
            "abc123",
            "def456",
            "commit",
            Some(r#"[{"change_type": "NewFunction", "file_path": "src/main.rs"}]"#),
            Some(r#"{"affected_functions": []}"#),
            Some(r#"{"overall": "Low", "flags": []}"#),
            Some("Added new function for handling errors"),
            Some(2),
            Some(50),
            Some(10),
        ).unwrap();

        assert!(id > 0);

        let analysis = db.get_diff_analysis(id).unwrap().unwrap();
        assert_eq!(analysis.from_commit, "abc123");
        assert_eq!(analysis.to_commit, "def456");
        assert_eq!(analysis.files_changed, Some(2));
    }

    #[test]
    fn test_cached_diff_analysis() {
        let db = Database::open_in_memory().unwrap();
        let (project_id, _) = db.get_or_create_project("/test", None).unwrap();

        // No cached analysis initially
        let cached = db.get_cached_diff_analysis(Some(project_id), "abc123", "def456").unwrap();
        assert!(cached.is_none());

        // Store one
        db.store_diff_analysis(
            Some(project_id),
            "abc123",
            "def456",
            "commit",
            None,
            None,
            None,
            Some("Test summary"),
            Some(1),
            Some(10),
            Some(5),
        ).unwrap();

        // Now should find it
        let cached = db.get_cached_diff_analysis(Some(project_id), "abc123", "def456").unwrap();
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().summary, Some("Test summary".to_string()));
    }
}
