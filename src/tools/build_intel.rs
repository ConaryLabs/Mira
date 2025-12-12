// src/tools/build_intel.rs
// Build intelligence tools - track build errors and learn from fixes

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

// === Parameter structs for consolidated build tool ===

pub struct GetBuildErrorsParams {
    pub file_path: Option<String>,
    pub category: Option<String>,
    pub include_resolved: Option<bool>,
    pub limit: Option<i64>,
}

pub struct RecordBuildParams {
    pub command: String,
    pub success: bool,
    pub duration_ms: Option<i64>,
}

pub struct RecordBuildErrorParams {
    pub message: String,
    pub category: Option<String>,
    pub severity: Option<String>,
    pub file_path: Option<String>,
    pub line_number: Option<i32>,
    pub code: Option<String>,
}

fn hash_error(message: &str) -> String {
    let mut hasher = DefaultHasher::new();
    let normalized = message
        .lines()
        .next()
        .unwrap_or(message)
        .to_lowercase()
        .replace(|c: char| c.is_numeric(), "N");
    normalized.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Get recent build errors
pub async fn get_build_errors(db: &SqlitePool, req: GetBuildErrorsParams) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(20);
    let include_resolved = req.include_resolved.unwrap_or(false);

    let query = r#"
        SELECT id, build_run_id, error_hash, category, severity, message,
               file_path, line_number, column_number, code, suggestion,
               resolved,
               datetime(created_at, 'unixepoch', 'localtime') as created_at,
               datetime(resolved_at, 'unixepoch', 'localtime') as resolved_at
        FROM build_errors
        WHERE ($1 IS NULL OR file_path LIKE $1)
          AND ($2 IS NULL OR category = $2)
          AND ($3 = 1 OR resolved = 0)
        ORDER BY created_at DESC
        LIMIT $4
    "#;

    let file_pattern = req.file_path.as_ref().map(|f| format!("%{}%", f));
    let rows = sqlx::query_as::<_, (i64, Option<i64>, String, Option<String>, String, String, Option<String>, Option<i64>, Option<i64>, Option<String>, Option<String>, bool, String, Option<String>)>(query)
        .bind(&file_pattern)
        .bind(&req.category)
        .bind(if include_resolved { 1 } else { 0 })
        .bind(limit)
        .fetch_all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(id, build_run_id, error_hash, category, severity, message, file_path, line, col, code, suggestion, resolved, created_at, resolved_at)| {
            serde_json::json!({
                "id": id,
                "build_run_id": build_run_id,
                "error_hash": error_hash,
                "category": category,
                "severity": severity,
                "message": message,
                "file_path": file_path,
                "line_number": line,
                "column_number": col,
                "code": code,
                "suggestion": suggestion,
                "resolved": resolved,
                "created_at": created_at,
                "resolved_at": resolved_at,
            })
        })
        .collect())
}

/// Record a build run
pub async fn record_build(db: &SqlitePool, req: RecordBuildParams) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();

    let result = sqlx::query(r#"
        INSERT INTO build_runs (command, success, duration_ms, error_count, warning_count, started_at, completed_at)
        VALUES ($1, $2, $3, 0, 0, $4, $4)
    "#)
    .bind(&req.command)
    .bind(req.success)
    .bind(req.duration_ms)
    .bind(now)
    .execute(db)
    .await?;

    Ok(serde_json::json!({
        "status": "recorded",
        "build_run_id": result.last_insert_rowid(),
        "command": req.command,
        "success": req.success,
    }))
}

/// Record a build error
pub async fn record_build_error(db: &SqlitePool, req: RecordBuildErrorParams) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let error_hash = hash_error(&req.message);
    let severity = req.severity.as_deref().unwrap_or("error");

    let result = sqlx::query(r#"
        INSERT INTO build_errors (error_hash, category, severity, message, file_path, line_number, column_number, code, resolved, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, $9)
    "#)
    .bind(&error_hash)
    .bind(&req.category)
    .bind(severity)
    .bind(&req.message)
    .bind(&req.file_path)
    .bind(req.line_number)
    .bind(None::<i32>) // column_number
    .bind(&req.code)
    .bind(now)
    .execute(db)
    .await?;

    Ok(serde_json::json!({
        "status": "recorded",
        "error_id": result.last_insert_rowid(),
        "error_hash": error_hash,
        "severity": severity,
    }))
}

/// Mark an error as resolved
pub async fn resolve_error(db: &SqlitePool, error_id: i64) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();

    let result = sqlx::query(r#"
        UPDATE build_errors
        SET resolved = 1, resolved_at = $1
        WHERE id = $2
    "#)
    .bind(now)
    .bind(error_id)
    .execute(db)
    .await?;

    if result.rows_affected() > 0 {
        Ok(serde_json::json!({
            "status": "resolved",
            "error_id": error_id,
        }))
    } else {
        Ok(serde_json::json!({
            "status": "not_found",
            "error_id": error_id,
        }))
    }
}
