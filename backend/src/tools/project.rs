// src/tools/project.rs
// Project guidelines tools

use chrono::Utc;
use sqlx::sqlite::SqlitePool;

use super::types::*;

/// Get coding guidelines
pub async fn get_guidelines(db: &SqlitePool, req: GetGuidelinesRequest) -> anyhow::Result<Vec<serde_json::Value>> {
    let query = r#"
        SELECT id, content, category, project_path, priority,
               datetime(created_at, 'unixepoch', 'localtime') as created_at
        FROM coding_guidelines
        WHERE ($1 IS NULL OR project_path = $1 OR project_path IS NULL)
          AND ($2 IS NULL OR category = $2)
        ORDER BY priority DESC, category, created_at
    "#;

    let rows = sqlx::query_as::<_, (i64, String, String, Option<String>, i64, String)>(query)
        .bind(&req.project_path)
        .bind(&req.category)
        .fetch_all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(id, content, category, project_path, priority, created_at)| {
            serde_json::json!({
                "id": id,
                "content": content,
                "category": category,
                "project_path": project_path,
                "priority": priority,
                "created_at": created_at,
            })
        })
        .collect())
}

/// Add a coding guideline
pub async fn add_guideline(db: &SqlitePool, req: AddGuidelineRequest) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let priority = req.priority.unwrap_or(0);

    let result = sqlx::query(r#"
        INSERT INTO coding_guidelines (content, category, project_path, priority, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $5)
    "#)
    .bind(&req.content)
    .bind(&req.category)
    .bind(&req.project_path)
    .bind(priority)
    .bind(now)
    .execute(db)
    .await?;

    Ok(serde_json::json!({
        "status": "added",
        "id": result.last_insert_rowid(),
        "category": req.category,
        "content": req.content,
        "project_path": req.project_path,
        "priority": priority,
    }))
}
