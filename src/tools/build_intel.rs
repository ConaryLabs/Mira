// src/tools/build_intel.rs
// Build intelligence tools - thin wrapper over core::ops::build

use sqlx::sqlite::SqlitePool;

use crate::core::ops::build as core_build;
use crate::core::OpContext;

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

/// Get recent build errors
pub async fn get_build_errors(db: &SqlitePool, req: GetBuildErrorsParams) -> anyhow::Result<Vec<serde_json::Value>> {
    let ctx = OpContext::just_db(db.clone());

    let input = core_build::GetBuildErrorsInput {
        file_path: req.file_path,
        category: req.category,
        include_resolved: req.include_resolved.unwrap_or(false),
        limit: req.limit.unwrap_or(20),
    };

    let errors = core_build::get_build_errors(&ctx, input).await?;

    Ok(errors.into_iter().map(|e| {
        serde_json::json!({
            "id": e.id,
            "build_run_id": e.build_run_id,
            "error_hash": e.error_hash,
            "category": e.category,
            "severity": e.severity,
            "message": e.message,
            "file_path": e.file_path,
            "line_number": e.line_number,
            "column_number": e.column_number,
            "code": e.code,
            "suggestion": e.suggestion,
            "resolved": e.resolved,
            "created_at": e.created_at,
            "resolved_at": e.resolved_at,
        })
    }).collect())
}

/// Record a build run
pub async fn record_build(db: &SqlitePool, req: RecordBuildParams) -> anyhow::Result<serde_json::Value> {
    let ctx = OpContext::just_db(db.clone());

    let input = core_build::RecordBuildInput {
        command: req.command,
        success: req.success,
        duration_ms: req.duration_ms,
    };

    let output = core_build::record_build(&ctx, input).await?;

    Ok(serde_json::json!({
        "status": "recorded",
        "build_run_id": output.build_run_id,
        "command": output.command,
        "success": output.success,
    }))
}

/// Record a build error
pub async fn record_build_error(db: &SqlitePool, req: RecordBuildErrorParams) -> anyhow::Result<serde_json::Value> {
    let ctx = OpContext::just_db(db.clone());

    let input = core_build::RecordBuildErrorInput {
        message: req.message,
        category: req.category,
        severity: req.severity,
        file_path: req.file_path,
        line_number: req.line_number,
        code: req.code,
    };

    let output = core_build::record_build_error(&ctx, input).await?;

    Ok(serde_json::json!({
        "status": "recorded",
        "error_id": output.error_id,
        "error_hash": output.error_hash,
        "severity": output.severity,
    }))
}

/// Mark an error as resolved
pub async fn resolve_error(db: &SqlitePool, error_id: i64) -> anyhow::Result<serde_json::Value> {
    let ctx = OpContext::just_db(db.clone());
    let resolved = core_build::resolve_error(&ctx, error_id).await?;

    if resolved {
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
