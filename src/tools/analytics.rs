// src/tools/analytics.rs
// Analytics and introspection tools - simplified for Claude Code

use sqlx::sqlite::SqlitePool;
use sqlx::{Column, Row};

use super::types::*;

/// List all tables with row counts
pub async fn list_tables(db: &SqlitePool) -> anyhow::Result<Vec<serde_json::Value>> {
    let query = r#"
        SELECT name FROM sqlite_master
        WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_sqlx%'
        ORDER BY name
    "#;

    let tables: Vec<String> = sqlx::query_scalar(query)
        .fetch_all(db)
        .await?;

    let mut results = Vec::new();
    for table in &tables {
        let count_query = format!("SELECT COUNT(*) FROM \"{}\"", table);
        let count: i64 = sqlx::query_scalar(&count_query)
            .fetch_one(db)
            .await
            .unwrap_or(0);
        results.push(serde_json::json!({
            "table": table,
            "row_count": count
        }));
    }

    Ok(results)
}

/// Execute a read-only query
pub async fn query(db: &SqlitePool, req: QueryRequest) -> anyhow::Result<serde_json::Value> {
    // Security: Only allow SELECT statements
    let sql_upper = req.sql.trim().to_uppercase();
    if !sql_upper.starts_with("SELECT") {
        anyhow::bail!("Only SELECT queries are allowed for safety");
    }

    // Prevent dangerous operations - check for whole words only
    let forbidden = ["DROP", "DELETE", "INSERT", "UPDATE", "ALTER", "CREATE", "TRUNCATE", "EXEC", "EXECUTE"];
    // Split on non-alphanumeric chars to check whole words (avoids matching CREATE in CREATED_AT)
    let words: Vec<&str> = sql_upper.split(|c: char| !c.is_alphanumeric() && c != '_').collect();
    for word in forbidden {
        if words.contains(&word) {
            anyhow::bail!("Query contains forbidden keyword: {}", word);
        }
    }

    let limit = req.limit.unwrap_or(100);
    let final_sql = if sql_upper.contains("LIMIT") {
        req.sql
    } else {
        format!("{} LIMIT {}", req.sql, limit)
    };

    let rows = sqlx::query(&final_sql)
        .fetch_all(db)
        .await?;

    // Extract column names from first row (if any)
    let columns: Vec<String> = if let Some(first_row) = rows.first() {
        first_row.columns().iter().map(|c| c.name().to_string()).collect()
    } else {
        vec![]
    };

    // Convert rows to JSON values
    // SQLite types can be tricky (especially for aggregates), so try multiple approaches
    let row_data: Vec<Vec<serde_json::Value>> = rows.iter().map(|row| {
        row.columns().iter().enumerate().map(|(i, _col)| {
            // Try integer first (most common for counts, IDs)
            if let Ok(v) = row.try_get::<i64, _>(i) {
                return serde_json::Value::from(v);
            }
            // Try float
            if let Ok(v) = row.try_get::<f64, _>(i) {
                return serde_json::Value::from(v);
            }
            // Try bool
            if let Ok(v) = row.try_get::<bool, _>(i) {
                return serde_json::Value::from(v);
            }
            // Try string (catches TEXT and most other types)
            if let Ok(v) = row.try_get::<String, _>(i) {
                return serde_json::Value::from(v);
            }
            // Try Option<String> for nullable columns
            if let Ok(Some(v)) = row.try_get::<Option<String>, _>(i) {
                return serde_json::Value::from(v);
            }
            serde_json::Value::Null
        }).collect()
    }).collect();

    Ok(serde_json::json!({
        "query": final_sql,
        "row_count": rows.len(),
        "columns": columns,
        "rows": row_data,
    }))
}
