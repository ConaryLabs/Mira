// crates/mira-server/src/db/schema/entities.rs
// Entity tables migration for memory entity linking

use crate::db::migration_helpers::{add_column_if_missing, create_table_if_missing, table_exists};
use anyhow::Result;
use rusqlite::Connection;

/// Create entity tables and add has_entities column to memory_facts.
///
/// Fully idempotent — safe to run on existing databases.
pub fn migrate_entity_tables(conn: &Connection) -> Result<()> {
    // Table 1: memory_entities — canonical entity registry
    create_table_if_missing(
        conn,
        "memory_entities",
        r#"
        CREATE TABLE IF NOT EXISTS memory_entities (
            id INTEGER PRIMARY KEY,
            project_id INTEGER REFERENCES projects(id),
            canonical_name TEXT NOT NULL,
            entity_type TEXT NOT NULL,
            display_name TEXT,
            occurrence_count INTEGER DEFAULT 1,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(project_id, canonical_name, entity_type)
        );
    "#,
    )?;

    // Table 2: memory_entity_links — many-to-many between facts and entities
    create_table_if_missing(
        conn,
        "memory_entity_links",
        r#"
        CREATE TABLE IF NOT EXISTS memory_entity_links (
            id INTEGER PRIMARY KEY,
            fact_id INTEGER NOT NULL REFERENCES memory_facts(id) ON DELETE CASCADE,
            entity_id INTEGER NOT NULL REFERENCES memory_entities(id) ON DELETE CASCADE,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(fact_id, entity_id)
        );
        CREATE INDEX IF NOT EXISTS idx_entity_links_fact ON memory_entity_links(fact_id);
        CREATE INDEX IF NOT EXISTS idx_entity_links_entity ON memory_entity_links(entity_id);
    "#,
    )?;

    // Column: has_entities on memory_facts (1 = extraction has been run)
    if table_exists(conn, "memory_facts") {
        add_column_if_missing(conn, "memory_facts", "has_entities", "INTEGER DEFAULT 0")?;
    }

    Ok(())
}
