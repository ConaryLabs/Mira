//! Core build operations - shared by MCP and Chat
//!
//! Build error tracking and management.

use chrono::Utc;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::super::{CoreResult, OpContext};

// ============================================================================
// Input/Output Types
// ============================================================================

pub struct GetBuildErrorsInput {
    pub file_path: Option<String>,
    pub category: Option<String>,
    pub include_resolved: bool,
    pub limit: i64,
}

pub struct BuildError {
    pub id: i64,
    pub build_run_id: Option<i64>,
    pub error_hash: String,
    pub category: Option<String>,
    pub severity: String,
    pub message: String,
    pub file_path: Option<String>,
    pub line_number: Option<i64>,
    pub column_number: Option<i64>,
    pub code: Option<String>,
    pub suggestion: Option<String>,
    pub resolved: bool,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

pub struct RecordBuildInput {
    pub command: String,
    pub success: bool,
    pub duration_ms: Option<i64>,
}

pub struct RecordBuildOutput {
    pub build_run_id: i64,
    pub command: String,
    pub success: bool,
}

pub struct RecordBuildErrorInput {
    pub message: String,
    pub category: Option<String>,
    pub severity: Option<String>,
    pub file_path: Option<String>,
    pub line_number: Option<i32>,
    pub code: Option<String>,
}

pub struct RecordBuildErrorOutput {
    pub error_id: i64,
    pub error_hash: String,
    pub severity: String,
}

// ============================================================================
// Operations
// ============================================================================

/// Hash an error message for deduplication
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
pub async fn get_build_errors(ctx: &OpContext, input: GetBuildErrorsInput) -> CoreResult<Vec<BuildError>> {
    let db = ctx.require_db()?;

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

    let file_pattern = input.file_path.as_ref().map(|f| format!("%{}%", f));
    let rows = sqlx::query_as::<_, (i64, Option<i64>, String, Option<String>, String, String, Option<String>, Option<i64>, Option<i64>, Option<String>, Option<String>, bool, String, Option<String>)>(query)
        .bind(&file_pattern)
        .bind(&input.category)
        .bind(if input.include_resolved { 1 } else { 0 })
        .bind(input.limit)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(id, build_run_id, error_hash, category, severity, message, file_path, line_number, column_number, code, suggestion, resolved, created_at, resolved_at)| {
        BuildError {
            id,
            build_run_id,
            error_hash,
            category,
            severity,
            message,
            file_path,
            line_number,
            column_number,
            code,
            suggestion,
            resolved,
            created_at,
            resolved_at,
        }
    }).collect())
}

/// Record a build run
pub async fn record_build(ctx: &OpContext, input: RecordBuildInput) -> CoreResult<RecordBuildOutput> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    let result = sqlx::query(r#"
        INSERT INTO build_runs (command, success, duration_ms, error_count, warning_count, started_at, completed_at)
        VALUES ($1, $2, $3, 0, 0, $4, $4)
    "#)
    .bind(&input.command)
    .bind(input.success)
    .bind(input.duration_ms)
    .bind(now)
    .execute(db)
    .await?;

    Ok(RecordBuildOutput {
        build_run_id: result.last_insert_rowid(),
        command: input.command,
        success: input.success,
    })
}

/// Record a build error
pub async fn record_build_error(ctx: &OpContext, input: RecordBuildErrorInput) -> CoreResult<RecordBuildErrorOutput> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();
    let error_hash = hash_error(&input.message);
    let severity = input.severity.as_deref().unwrap_or("error").to_string();

    let result = sqlx::query(r#"
        INSERT INTO build_errors (error_hash, category, severity, message, file_path, line_number, column_number, code, resolved, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, $9)
    "#)
    .bind(&error_hash)
    .bind(&input.category)
    .bind(&severity)
    .bind(&input.message)
    .bind(&input.file_path)
    .bind(input.line_number)
    .bind(None::<i32>)
    .bind(&input.code)
    .bind(now)
    .execute(db)
    .await?;

    Ok(RecordBuildErrorOutput {
        error_id: result.last_insert_rowid(),
        error_hash,
        severity,
    })
}

/// Mark an error as resolved
pub async fn resolve_error(ctx: &OpContext, error_id: i64) -> CoreResult<bool> {
    let db = ctx.require_db()?;
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

    Ok(result.rows_affected() > 0)
}
