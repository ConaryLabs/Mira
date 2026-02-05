// crates/mira-server/src/db/entities.rs
// CRUD operations for memory entities and entity-fact links

use rusqlite::Connection;
use std::collections::HashMap;

/// Upsert an entity, incrementing occurrence_count only for genuinely new links.
///
/// Returns the entity ID.
pub fn upsert_entity_sync(
    conn: &Connection,
    project_id: Option<i64>,
    canonical_name: &str,
    entity_type: &str,
    display_name: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO memory_entities (project_id, canonical_name, entity_type, display_name, occurrence_count)
         VALUES (?1, ?2, ?3, ?4, 0)
         ON CONFLICT(project_id, canonical_name, entity_type)
         DO UPDATE SET display_name = COALESCE(display_name, ?4)",
        rusqlite::params![project_id, canonical_name, entity_type, display_name],
    )?;

    conn.query_row(
        "SELECT id FROM memory_entities WHERE project_id IS ?1 AND canonical_name = ?2 AND entity_type = ?3",
        rusqlite::params![project_id, canonical_name, entity_type],
        |row| row.get(0),
    )
}

/// Link an entity to a fact. Returns true if a new link was created (false if already existed).
///
/// When a new link is created, also increments the entity's occurrence_count.
pub fn link_entity_to_fact_sync(
    conn: &Connection,
    fact_id: i64,
    entity_id: i64,
) -> rusqlite::Result<bool> {
    let result = conn.execute(
        "INSERT OR IGNORE INTO memory_entity_links (fact_id, entity_id) VALUES (?1, ?2)",
        rusqlite::params![fact_id, entity_id],
    )?;

    if result > 0 {
        // New link — increment occurrence_count
        conn.execute(
            "UPDATE memory_entities SET occurrence_count = occurrence_count + 1 WHERE id = ?1",
            [entity_id],
        )?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Mark a fact as having had entity extraction run (even if zero entities found).
pub fn mark_fact_has_entities_sync(conn: &Connection, fact_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE memory_facts SET has_entities = 1 WHERE id = ?1",
        [fact_id],
    )?;
    Ok(())
}

/// Get entity match counts for a set of canonical names within a project.
///
/// Returns a map of fact_id → number of distinct matching canonical entities.
/// Caps the input at 30 canonical names to stay within SQLite param limits.
pub fn get_entity_match_counts_sync(
    conn: &Connection,
    project_id: Option<i64>,
    canonical_names: &[String],
) -> rusqlite::Result<HashMap<i64, u32>> {
    if canonical_names.is_empty() {
        return Ok(HashMap::new());
    }

    // Cap at 30 to avoid SQLite parameter overflow
    let capped = if canonical_names.len() > 30 {
        &canonical_names[..30]
    } else {
        canonical_names
    };

    // Build parameterized IN clause
    let placeholders: Vec<String> = (0..capped.len()).map(|i| format!("?{}", i + 2)).collect();
    let sql = format!(
        "SELECT mel.fact_id, COUNT(DISTINCT me.canonical_name) as match_count
         FROM memory_entity_links mel
         JOIN memory_entities me ON mel.entity_id = me.id
         WHERE me.project_id IS ?1 AND me.canonical_name IN ({})
         GROUP BY mel.fact_id",
        placeholders.join(", ")
    );

    let mut stmt = conn.prepare(&sql)?;

    // Build parameter list: project_id + canonical names
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(capped.len() + 1);
    params.push(Box::new(project_id));
    for name in capped {
        params.push(Box::new(name.clone()));
    }

    let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, u32>(1)?))
    })?;

    let mut map = HashMap::new();
    for row in rows {
        let (fact_id, count) = row?;
        map.insert(fact_id, count);
    }

    Ok(map)
}

/// Find fact IDs that haven't had entity extraction run yet.
pub fn find_facts_without_entities_sync(
    conn: &Connection,
    limit: usize,
) -> rusqlite::Result<Vec<(i64, Option<i64>, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, content FROM memory_facts
         WHERE has_entities = 0
         ORDER BY created_at ASC
         LIMIT ?1",
    )?;

    let rows = stmt.query_map([limit as i64], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;

    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        crate::db::test_support::setup_test_connection()
    }

    fn insert_project(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT INTO projects (path, name) VALUES ('/test', 'test')",
            [],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn insert_fact(conn: &Connection, project_id: i64, content: &str) -> i64 {
        conn.execute(
            "INSERT INTO memory_facts (project_id, content, fact_type, confidence)
             VALUES (?1, ?2, 'general', 0.5)",
            rusqlite::params![project_id, content],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn test_upsert_entity_creates_new() {
        let conn = setup_db();
        let pid = insert_project(&conn);

        let eid = upsert_entity_sync(
            &conn,
            Some(pid),
            "database_pool",
            "code_ident",
            "DatabasePool",
        )
        .unwrap();
        assert!(eid > 0);

        // Verify occurrence_count starts at 0 (incremented on link, not on upsert)
        let count: i64 = conn
            .query_row(
                "SELECT occurrence_count FROM memory_entities WHERE id = ?1",
                [eid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_upsert_entity_idempotent() {
        let conn = setup_db();
        let pid = insert_project(&conn);

        let eid1 = upsert_entity_sync(
            &conn,
            Some(pid),
            "database_pool",
            "code_ident",
            "DatabasePool",
        )
        .unwrap();
        let eid2 = upsert_entity_sync(
            &conn,
            Some(pid),
            "database_pool",
            "code_ident",
            "DatabasePool",
        )
        .unwrap();
        assert_eq!(eid1, eid2);
    }

    #[test]
    fn test_link_entity_to_fact() {
        let conn = setup_db();
        let pid = insert_project(&conn);
        let fid = insert_fact(&conn, pid, "test content");
        let eid = upsert_entity_sync(
            &conn,
            Some(pid),
            "database_pool",
            "code_ident",
            "DatabasePool",
        )
        .unwrap();

        // First link should succeed
        let created = link_entity_to_fact_sync(&conn, fid, eid).unwrap();
        assert!(created);

        // Second link should be a no-op
        let created2 = link_entity_to_fact_sync(&conn, fid, eid).unwrap();
        assert!(!created2);

        // occurrence_count should be 1 (only incremented once)
        let count: i64 = conn
            .query_row(
                "SELECT occurrence_count FROM memory_entities WHERE id = ?1",
                [eid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_link_multiple_facts_increments_occurrence() {
        let conn = setup_db();
        let pid = insert_project(&conn);
        let fid1 = insert_fact(&conn, pid, "content 1");
        let fid2 = insert_fact(&conn, pid, "content 2");
        let eid = upsert_entity_sync(
            &conn,
            Some(pid),
            "database_pool",
            "code_ident",
            "DatabasePool",
        )
        .unwrap();

        link_entity_to_fact_sync(&conn, fid1, eid).unwrap();
        link_entity_to_fact_sync(&conn, fid2, eid).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT occurrence_count FROM memory_entities WHERE id = ?1",
                [eid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_mark_fact_has_entities() {
        let conn = setup_db();
        let pid = insert_project(&conn);
        let fid = insert_fact(&conn, pid, "test content");

        // Initially has_entities = 0
        let has: i64 = conn
            .query_row(
                "SELECT has_entities FROM memory_facts WHERE id = ?1",
                [fid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(has, 0);

        mark_fact_has_entities_sync(&conn, fid).unwrap();

        let has: i64 = conn
            .query_row(
                "SELECT has_entities FROM memory_facts WHERE id = ?1",
                [fid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(has, 1);
    }

    #[test]
    fn test_get_entity_match_counts() {
        let conn = setup_db();
        let pid = insert_project(&conn);

        let fid1 = insert_fact(&conn, pid, "content about DatabasePool");
        let fid2 = insert_fact(&conn, pid, "content about store_memory_sync");
        let fid3 = insert_fact(&conn, pid, "content about both");

        let eid_pool = upsert_entity_sync(
            &conn,
            Some(pid),
            "database_pool",
            "code_ident",
            "DatabasePool",
        )
        .unwrap();
        let eid_store = upsert_entity_sync(
            &conn,
            Some(pid),
            "store_memory_sync",
            "code_ident",
            "store_memory_sync",
        )
        .unwrap();

        link_entity_to_fact_sync(&conn, fid1, eid_pool).unwrap();
        link_entity_to_fact_sync(&conn, fid2, eid_store).unwrap();
        link_entity_to_fact_sync(&conn, fid3, eid_pool).unwrap();
        link_entity_to_fact_sync(&conn, fid3, eid_store).unwrap();

        let names = vec!["database_pool".to_string(), "store_memory_sync".to_string()];
        let counts = get_entity_match_counts_sync(&conn, Some(pid), &names).unwrap();

        assert_eq!(*counts.get(&fid1).unwrap_or(&0), 1); // only database_pool
        assert_eq!(*counts.get(&fid2).unwrap_or(&0), 1); // only store_memory_sync
        assert_eq!(*counts.get(&fid3).unwrap_or(&0), 2); // both
    }

    #[test]
    fn test_get_entity_match_counts_empty_names() {
        let conn = setup_db();
        let counts = get_entity_match_counts_sync(&conn, Some(1), &[]).unwrap();
        assert!(counts.is_empty());
    }

    #[test]
    fn test_find_facts_without_entities() {
        let conn = setup_db();
        let pid = insert_project(&conn);
        let fid1 = insert_fact(&conn, pid, "no entities yet");
        insert_fact(&conn, pid, "also no entities");

        mark_fact_has_entities_sync(&conn, fid1).unwrap();

        let unprocessed = find_facts_without_entities_sync(&conn, 10).unwrap();
        assert_eq!(unprocessed.len(), 1);
        assert_eq!(unprocessed[0].2, "also no entities");
    }

    #[test]
    fn test_cascade_delete_fact_removes_links() {
        let conn = setup_db();
        let pid = insert_project(&conn);
        let fid = insert_fact(&conn, pid, "will be deleted");
        let eid = upsert_entity_sync(&conn, Some(pid), "test_entity", "code_ident", "test_entity")
            .unwrap();
        link_entity_to_fact_sync(&conn, fid, eid).unwrap();

        // Delete the fact
        conn.execute("DELETE FROM memory_facts WHERE id = ?1", [fid])
            .unwrap();

        // Links should be cascaded
        let link_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memory_entity_links WHERE fact_id = ?1",
                [fid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(link_count, 0);
    }
}
