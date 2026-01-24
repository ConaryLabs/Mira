// crates/mira-server/src/db/migration_helpers.rs
// Database migration helper utilities

use anyhow::Result;
use rusqlite::Connection;
use tracing::info;

/// Check if a table exists in the database
pub fn table_exists(conn: &Connection, table_name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?",
        [table_name],
        |_| Ok(true),
    ).unwrap_or(false)
}

/// Check if a column exists in a table
pub fn column_exists(conn: &Connection, table_name: &str, column_name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM pragma_table_info(?) WHERE name=?",
        [table_name, column_name],
        |_| Ok(true),
    ).unwrap_or(false)
}

/// Add a column to a table if it doesn't already exist
pub fn add_column_if_missing(
    conn: &Connection,
    table_name: &str,
    column_name: &str,
    column_def: &str,
) -> Result<()> {
    if column_exists(conn, table_name, column_name) {
        return Ok(());
    }

    info!("Migrating {} to add {} column", table_name, column_name);
    let sql = format!("ALTER TABLE {} ADD COLUMN {} {}", table_name, column_name, column_def);
    conn.execute(&sql, [])?;
    Ok(())
}

/// Create a table if it doesn't exist (with logging)
pub fn create_table_if_missing(
    conn: &Connection,
    table_name: &str,
    sql: &str,
) -> Result<()> {
    if table_exists(conn, table_name) {
        return Ok(());
    }

    info!("Creating {} table", table_name);
    conn.execute_batch(sql)?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_exists_in_memory() {
        let conn = Connection::open_in_memory().unwrap();

        // Table doesn't exist yet
        assert!(!table_exists(&conn, "test_table"));

        // Create table
        conn.execute("CREATE TABLE test_table (id INTEGER)", []).unwrap();

        // Now it exists
        assert!(table_exists(&conn, "test_table"));
    }

    #[test]
    fn test_column_exists_in_memory() {
        let conn = Connection::open_in_memory().unwrap();

        // Create table with specific column
        conn.execute("CREATE TABLE test_table (id INTEGER, name TEXT)", []).unwrap();

        // Column exists
        assert!(column_exists(&conn, "test_table", "id"));
        assert!(column_exists(&conn, "test_table", "name"));

        // Column doesn't exist
        assert!(!column_exists(&conn, "test_table", "email"));
    }

    #[test]
    fn test_add_column_if_missing() {
        let conn = Connection::open_in_memory().unwrap();

        // Create table without the column
        conn.execute("CREATE TABLE test_table (id INTEGER)", []).unwrap();

        // Add missing column
        add_column_if_missing(&conn, "test_table", "name", "TEXT").unwrap();

        // Column now exists
        assert!(column_exists(&conn, "test_table", "name"));

        // Adding again should be idempotent
        add_column_if_missing(&conn, "test_table", "name", "TEXT").unwrap();
    }

    #[test]
    fn test_create_table_if_missing() {
        let conn = Connection::open_in_memory().unwrap();

        // Create table if missing
        create_table_if_missing(&conn, "new_table", "CREATE TABLE new_table (id INTEGER)").unwrap();

        // Table now exists
        assert!(table_exists(&conn, "new_table"));

        // Creating again should be idempotent
        create_table_if_missing(&conn, "new_table", "CREATE TABLE new_table (id INTEGER)").unwrap();
    }
}
