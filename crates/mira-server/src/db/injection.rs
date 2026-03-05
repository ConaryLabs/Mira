// crates/mira-server/src/db/injection.rs
// Database operations for context injection tracking

use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{Connection, params};

/// Record for a single context injection event.
#[derive(Debug, Clone)]
pub struct InjectionRecord {
    pub hook_name: String,
    pub session_id: Option<String>,
    pub project_id: Option<i64>,
    pub chars_injected: usize,
    pub sources_kept: Vec<String>,
    pub sources_dropped: Vec<String>,
    pub latency_ms: Option<u64>,
    pub was_deduped: bool,
    pub was_cached: bool,
    pub content: Option<String>,
    pub categories: Vec<String>,
}

/// Fire-and-forget injection recording. Opens a direct connection to the DB,
/// inserts the record, and silently drops errors. Use this from hooks that
/// don't have a long-lived pool.
pub fn record_injection_fire_and_forget(db_path: &std::path::Path, record: &InjectionRecord) {
    match Connection::open(db_path) {
        Ok(conn) => {
            if let Err(e) = insert_injection_sync(&conn, record) {
                tracing::debug!("record injection: {e}");
            }
        }
        Err(e) => tracing::debug!("record injection: failed to open db: {e}"),
    }
}

/// Insert an injection record - sync version for pool.interact() / try_interact()
pub fn insert_injection_sync(conn: &Connection, record: &InjectionRecord) -> Result<i64> {
    let sources_kept = if record.sources_kept.is_empty() {
        None
    } else {
        Some(record.sources_kept.join(","))
    };
    let sources_dropped = if record.sources_dropped.is_empty() {
        None
    } else {
        Some(record.sources_dropped.join(","))
    };
    let content = record
        .content
        .as_deref()
        .map(|c| crate::utils::truncate_at_boundary(c, 2000));
    let categories = if record.categories.is_empty() {
        None
    } else {
        Some(record.categories.join(","))
    };

    conn.execute(
        "INSERT INTO context_injections (
            hook_name, session_id, project_id, chars_injected,
            sources_kept, sources_dropped, latency_ms, was_deduped, was_cached,
            content, categories
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            record.hook_name,
            record.session_id,
            record.project_id,
            record.chars_injected as i64,
            sources_kept,
            sources_dropped,
            record.latency_ms.map(|ms| ms as i64),
            record.was_deduped as i32,
            record.was_cached as i32,
            content,
            categories,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Summary stats for injection activity
#[derive(Debug, Clone)]
pub struct InjectionStats {
    pub total_injections: u64,
    pub total_chars: u64,
    pub total_deduped: u64,
    pub total_cached: u64,
    pub avg_chars: f64,
    pub avg_latency_ms: Option<f64>,
}

/// Query injection stats for a session
pub fn get_injection_stats_for_session(
    conn: &Connection,
    session_id: &str,
) -> Result<InjectionStats> {
    let mut stmt = conn.prepare(
        "SELECT
            COUNT(*) as total_injections,
            COALESCE(SUM(chars_injected), 0) as total_chars,
            COALESCE(SUM(was_deduped), 0) as total_deduped,
            COALESCE(SUM(was_cached), 0) as total_cached,
            COALESCE(AVG(chars_injected), 0) as avg_chars,
            AVG(latency_ms) as avg_latency_ms
        FROM context_injections
        WHERE session_id = ?",
    )?;

    let stats = stmt.query_row(params![session_id], |row| {
        Ok(InjectionStats {
            total_injections: row.get::<_, i64>(0)? as u64,
            total_chars: row.get::<_, i64>(1)? as u64,
            total_deduped: row.get::<_, i64>(2)? as u64,
            total_cached: row.get::<_, i64>(3)? as u64,
            avg_chars: row.get(4)?,
            avg_latency_ms: row.get(5)?,
        })
    })?;

    Ok(stats)
}

/// Query cumulative injection stats (optionally filtered by project)
pub fn get_injection_stats_cumulative(
    conn: &Connection,
    project_id: Option<i64>,
    since_days: Option<u32>,
) -> Result<InjectionStats> {
    let mut sql = String::from(
        "SELECT
            COUNT(*) as total_injections,
            COALESCE(SUM(chars_injected), 0) as total_chars,
            COALESCE(SUM(was_deduped), 0) as total_deduped,
            COALESCE(SUM(was_cached), 0) as total_cached,
            COALESCE(AVG(chars_injected), 0) as avg_chars,
            AVG(latency_ms) as avg_latency_ms
        FROM context_injections
        WHERE 1=1",
    );

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(pid) = project_id {
        sql.push_str(" AND project_id = ?");
        params_vec.push(Box::new(pid));
    }

    if let Some(days) = since_days {
        sql.push_str(" AND created_at >= datetime('now', ? || ' days')");
        params_vec.push(Box::new(-(days as i64)));
    }

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let stats = stmt.query_row(params_refs.as_slice(), |row| {
        Ok(InjectionStats {
            total_injections: row.get::<_, i64>(0)? as u64,
            total_chars: row.get::<_, i64>(1)? as u64,
            total_deduped: row.get::<_, i64>(2)? as u64,
            total_cached: row.get::<_, i64>(3)? as u64,
            avg_chars: row.get(4)?,
            avg_latency_ms: row.get(5)?,
        })
    })?;

    Ok(stats)
}

/// Count distinct sessions that have injection records
pub fn count_tracked_sessions(conn: &Connection, project_id: Option<i64>) -> Result<u64> {
    let mut sql = String::from(
        "SELECT COUNT(DISTINCT session_id) FROM context_injections WHERE session_id IS NOT NULL AND session_id != ''",
    );

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(pid) = project_id {
        sql.push_str(" AND project_id = ?");
        params_vec.push(Box::new(pid));
    }

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let count: i64 = stmt.query_row(params_refs.as_slice(), |row| row.get(0))?;
    Ok(count as u64)
}

/// Get a breakdown of injection categories (how many times each category was injected).
pub fn get_category_breakdown_sync(
    conn: &Connection,
    session_id: Option<&str>,
    project_id: Option<i64>,
) -> Result<HashMap<String, usize>> {
    let mut sql = String::from(
        "SELECT categories FROM context_injections WHERE categories IS NOT NULL AND categories != ''",
    );
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(sid) = session_id {
        sql.push_str(" AND session_id = ?");
        params_vec.push(Box::new(sid.to_string()));
    }
    if let Some(pid) = project_id {
        sql.push_str(" AND project_id = ?");
        params_vec.push(Box::new(pid));
    }

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| row.get::<_, String>(0))?;

    let mut counts: HashMap<String, usize> = HashMap::new();
    for row in rows.flatten() {
        for cat in row.split(',') {
            let cat = cat.trim();
            if !cat.is_empty() {
                *counts.entry(cat.to_string()).or_default() += 1;
            }
        }
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = crate::db::test_support::setup_test_connection();
        crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();
        conn
    }

    fn make_record(hook: &str, session_id: Option<&str>) -> InjectionRecord {
        InjectionRecord {
            hook_name: hook.to_string(),
            session_id: session_id.map(|s| s.to_string()),
            project_id: Some(1),
            chars_injected: 500,
            sources_kept: vec!["semantic".to_string(), "goals".to_string()],
            sources_dropped: vec!["convention".to_string()],
            latency_ms: Some(12),
            was_deduped: false,
            was_cached: false,
            content: None,
            categories: vec![],
        }
    }

    #[test]
    fn test_insert_returns_id() {
        let conn = setup_db();
        let record = make_record("UserPromptSubmit", Some("session-1"));
        let id = insert_injection_sync(&conn, &record).unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_insert_minimal_record() {
        let conn = setup_db();
        let record = InjectionRecord {
            hook_name: "SessionStart".to_string(),
            session_id: None,
            project_id: None,
            chars_injected: 0,
            sources_kept: vec![],
            sources_dropped: vec![],
            latency_ms: None,
            was_deduped: false,
            was_cached: false,
            content: None,
            categories: vec![],
        };
        let id = insert_injection_sync(&conn, &record).unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_session_stats() {
        let conn = setup_db();

        insert_injection_sync(&conn, &make_record("UserPromptSubmit", Some("s1"))).unwrap();
        insert_injection_sync(&conn, &make_record("UserPromptSubmit", Some("s1"))).unwrap();

        let mut deduped = make_record("UserPromptSubmit", Some("s1"));
        deduped.was_deduped = true;
        deduped.chars_injected = 0;
        insert_injection_sync(&conn, &deduped).unwrap();

        let stats = get_injection_stats_for_session(&conn, "s1").unwrap();
        assert_eq!(stats.total_injections, 3);
        assert_eq!(stats.total_chars, 1000); // 500 + 500 + 0
        assert_eq!(stats.total_deduped, 1);
        assert!(stats.avg_latency_ms.is_some());
    }

    #[test]
    fn test_session_stats_empty() {
        let conn = setup_db();
        let stats = get_injection_stats_for_session(&conn, "nonexistent").unwrap();
        assert_eq!(stats.total_injections, 0);
        assert_eq!(stats.total_chars, 0);
    }

    #[test]
    fn test_cumulative_stats() {
        let conn = setup_db();

        insert_injection_sync(&conn, &make_record("UserPromptSubmit", Some("s1"))).unwrap();
        insert_injection_sync(&conn, &make_record("SessionStart", Some("s2"))).unwrap();

        let stats = get_injection_stats_cumulative(&conn, None, None).unwrap();
        assert_eq!(stats.total_injections, 2);
        assert_eq!(stats.total_chars, 1000);
    }

    #[test]
    fn test_cumulative_stats_filtered_by_project() {
        let conn = setup_db();
        crate::db::get_or_create_project_sync(&conn, "/other", Some("other")).unwrap();

        insert_injection_sync(&conn, &make_record("UserPromptSubmit", Some("s1"))).unwrap();

        let mut other = make_record("UserPromptSubmit", Some("s2"));
        other.project_id = Some(2);
        insert_injection_sync(&conn, &other).unwrap();

        let stats = get_injection_stats_cumulative(&conn, Some(1), None).unwrap();
        assert_eq!(stats.total_injections, 1);
    }

    #[test]
    fn test_count_tracked_sessions() {
        let conn = setup_db();

        insert_injection_sync(&conn, &make_record("UserPromptSubmit", Some("s1"))).unwrap();
        insert_injection_sync(&conn, &make_record("UserPromptSubmit", Some("s1"))).unwrap();
        insert_injection_sync(&conn, &make_record("SessionStart", Some("s2"))).unwrap();

        let count = count_tracked_sessions(&conn, None).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_count_tracked_sessions_empty() {
        let conn = setup_db();
        let count = count_tracked_sessions(&conn, None).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_insert_with_content_and_categories() {
        let conn = setup_db();
        let mut record = make_record("UserPromptSubmit", Some("session-1"));
        record.content = Some("goals context here".to_string());
        record.categories = vec!["goals".to_string(), "file_hints".to_string()];
        let id = insert_injection_sync(&conn, &record).unwrap();
        assert!(id > 0);

        let stored: String = conn
            .query_row(
                "SELECT content FROM context_injections WHERE id = ?",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, "goals context here");

        let cats: String = conn
            .query_row(
                "SELECT categories FROM context_injections WHERE id = ?",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cats, "goals,file_hints");
    }

    #[test]
    fn test_content_truncated_at_limit() {
        let conn = setup_db();
        let mut record = make_record("SessionStart", Some("session-1"));
        record.content = Some("x".repeat(3000));
        let id = insert_injection_sync(&conn, &record).unwrap();

        let stored: String = conn
            .query_row(
                "SELECT content FROM context_injections WHERE id = ?",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(stored.len() <= 2000);
    }

    #[test]
    fn test_content_truncated_utf8_safe() {
        let conn = setup_db();
        let mut record = make_record("SessionStart", Some("session-1"));
        // Each char is 2 bytes; 1100 chars = 2200 bytes, exceeds 2000 byte limit
        record.content = Some("\u{00e9}".repeat(1100));
        let id = insert_injection_sync(&conn, &record).unwrap();

        let stored: String = conn
            .query_row(
                "SELECT content FROM context_injections WHERE id = ?",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(stored.len() <= 2000);
        // Verify it's valid UTF-8 (would panic on invalid)
        assert!(stored.chars().all(|c| c == '\u{00e9}'));
    }

    #[test]
    fn test_get_injection_categories_breakdown() {
        let conn = setup_db();
        let mut r1 = make_record("UserPromptSubmit", Some("s1"));
        r1.categories = vec!["goals".into(), "reactive".into()];
        insert_injection_sync(&conn, &r1).unwrap();

        let mut r2 = make_record("SubagentStart", Some("s1"));
        r2.categories = vec!["goals".into()];
        insert_injection_sync(&conn, &r2).unwrap();

        let breakdown = get_category_breakdown_sync(&conn, Some("s1"), None).unwrap();
        assert_eq!(breakdown.get("goals"), Some(&2));
        assert_eq!(breakdown.get("reactive"), Some(&1));
    }

    #[test]
    fn test_category_breakdown_empty() {
        let conn = setup_db();
        let breakdown = get_category_breakdown_sync(&conn, Some("nonexistent"), None).unwrap();
        assert!(breakdown.is_empty());
    }

    #[test]
    fn test_category_breakdown_filtered_by_project() {
        let conn = setup_db();
        crate::db::get_or_create_project_sync(&conn, "/other", Some("other")).unwrap();

        let mut r1 = make_record("UserPromptSubmit", Some("s1"));
        r1.categories = vec!["goals".into()];
        insert_injection_sync(&conn, &r1).unwrap();

        let mut r2 = make_record("UserPromptSubmit", Some("s2"));
        r2.project_id = Some(2);
        r2.categories = vec!["goals".into(), "tasks".into()];
        insert_injection_sync(&conn, &r2).unwrap();

        let breakdown = get_category_breakdown_sync(&conn, None, Some(1)).unwrap();
        assert_eq!(breakdown.get("goals"), Some(&1));
        assert_eq!(breakdown.get("tasks"), None);
    }
}
