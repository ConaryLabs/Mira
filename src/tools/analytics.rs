// src/tools/analytics.rs
// Analytics and introspection tools - simplified for Claude Code

use sqlx::sqlite::SqlitePool;

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

    // Prevent dangerous operations
    let forbidden = ["DROP", "DELETE", "INSERT", "UPDATE", "ALTER", "CREATE", "TRUNCATE", "EXEC", "EXECUTE"];
    for word in forbidden {
        if sql_upper.contains(word) {
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

    Ok(serde_json::json!({
        "query": final_sql,
        "row_count": rows.len(),
        "message": format!("Query returned {} rows", rows.len())
    }))
}
