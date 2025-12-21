//! Core build operations - shared by MCP and Chat
//!
//! Build error tracking, management, and error fix learning.

use chrono::Utc;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use super::super::{CoreResult, OpContext};
use crate::core::primitives::semantic::COLLECTION_CONVERSATION;

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

// ============================================================================
// Error Fix Learning Operations
// ============================================================================

/// Input for finding similar error fixes
pub struct FindSimilarFixesInput {
    pub error: String,
    pub category: Option<String>,
    pub language: Option<String>,
    pub limit: usize,
}

/// An error fix suggestion
pub struct ErrorFix {
    pub id: i64,
    pub error_pattern: String,
    pub category: Option<String>,
    pub language: Option<String>,
    pub file_pattern: Option<String>,
    pub fix_description: Option<String>,
    pub fix_diff: Option<String>,
    pub fix_commit: Option<String>,
    pub times_seen: i64,
    pub times_fixed: i64,
    pub last_seen: String,
    pub score: f32,
    pub search_type: String,
}

/// Input for recording an error fix
pub struct RecordErrorFixInput {
    pub error_pattern: String,
    pub fix_description: String,
    pub category: Option<String>,
    pub language: Option<String>,
    pub file_pattern: Option<String>,
    pub fix_diff: Option<String>,
    pub fix_commit: Option<String>,
}

/// Output from recording an error fix
pub struct RecordErrorFixOutput {
    pub id: i64,
    pub status: String,
    pub error_pattern: String,
    pub semantic_indexed: bool,
}

/// Find similar error fixes - uses semantic search if available, falls back to text
pub async fn find_similar_fixes(ctx: &OpContext, input: FindSimilarFixesInput) -> CoreResult<Vec<ErrorFix>> {
    let db = ctx.require_db()?;

    // Try semantic search first for better matching
    if let Some(semantic) = &ctx.semantic {
        if semantic.is_available() {
            let filter = input.category.as_ref().map(|category| {
                qdrant_client::qdrant::Filter::must([
                    qdrant_client::qdrant::Condition::matches("category", category.clone())
                ])
            });

            match semantic.search(COLLECTION_CONVERSATION, &input.error, input.limit, filter).await {
                Ok(results) => {
                    let fixes: Vec<ErrorFix> = results.into_iter()
                        .filter(|r| r.metadata.get("type").and_then(|v| v.as_str()) == Some("error_fix"))
                        .map(|r| ErrorFix {
                            id: r.metadata.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
                            error_pattern: r.metadata.get("error_pattern")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            category: r.metadata.get("category").and_then(|v| v.as_str()).map(String::from),
                            language: r.metadata.get("language").and_then(|v| v.as_str()).map(String::from),
                            file_pattern: None,
                            fix_description: Some(r.content),
                            fix_diff: None,
                            fix_commit: None,
                            times_seen: 0,
                            times_fixed: 0,
                            last_seen: String::new(),
                            score: r.score,
                            search_type: "semantic".to_string(),
                        })
                        .collect();

                    if !fixes.is_empty() {
                        return Ok(fixes);
                    }
                }
                Err(e) => {
                    tracing::warn!("Semantic fix search failed, falling back to text: {}", e);
                }
            }
        }
    }

    // Fallback to SQLite text search
    let error_pattern = format!("%{}%", input.error);

    let query = r#"
        SELECT id, error_pattern, error_category, language, file_pattern,
               fix_description, fix_diff, fix_commit, times_seen, times_fixed,
               datetime(last_seen, 'unixepoch', 'localtime') as last_seen
        FROM error_fixes
        WHERE error_pattern LIKE $1
          AND ($2 IS NULL OR error_category = $2)
          AND ($3 IS NULL OR language = $3)
        ORDER BY times_fixed DESC, last_seen DESC
        LIMIT $4
    "#;

    let rows = sqlx::query_as::<_, (i64, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, i64, i64, String)>(query)
        .bind(&error_pattern)
        .bind(&input.category)
        .bind(&input.language)
        .bind(input.limit as i64)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(id, pattern, category, language, file_pattern, fix_desc, fix_diff, commit, seen, fixed, last_seen)| {
        ErrorFix {
            id,
            error_pattern: pattern,
            category,
            language,
            file_pattern,
            fix_description: fix_desc,
            fix_diff,
            fix_commit: commit,
            times_seen: seen,
            times_fixed: fixed,
            last_seen,
            score: 1.0,
            search_type: "text".to_string(),
        }
    }).collect())
}

/// Record an error fix for future learning
pub async fn record_error_fix(ctx: &OpContext, input: RecordErrorFixInput) -> CoreResult<RecordErrorFixOutput> {
    let db = ctx.require_db()?;
    let now = Utc::now().timestamp();

    // Try to update existing pattern or insert new one
    let existing = sqlx::query_as::<_, (i64,)>(
        "SELECT id FROM error_fixes WHERE error_pattern = $1"
    )
    .bind(&input.error_pattern)
    .fetch_optional(db)
    .await?;

    let (status, id) = if let Some((id,)) = existing {
        // Update existing
        sqlx::query(r#"
            UPDATE error_fixes
            SET times_fixed = times_fixed + 1,
                fix_description = COALESCE($1, fix_description),
                fix_diff = COALESCE($2, fix_diff),
                fix_commit = COALESCE($3, fix_commit),
                last_seen = $4
            WHERE id = $5
        "#)
        .bind(&input.fix_description)
        .bind(&input.fix_diff)
        .bind(&input.fix_commit)
        .bind(now)
        .bind(id)
        .execute(db)
        .await?;

        ("updated".to_string(), id)
    } else {
        // Insert new
        let result = sqlx::query(r#"
            INSERT INTO error_fixes (error_pattern, error_category, language, file_pattern,
                                     fix_description, fix_diff, fix_commit,
                                     times_seen, times_fixed, last_seen, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, 1, 1, $8, $8)
        "#)
        .bind(&input.error_pattern)
        .bind(&input.category)
        .bind(&input.language)
        .bind(&input.file_pattern)
        .bind(&input.fix_description)
        .bind(&input.fix_diff)
        .bind(&input.fix_commit)
        .bind(now)
        .execute(db)
        .await?;

        ("recorded".to_string(), result.last_insert_rowid())
    };

    // Store in Qdrant for semantic search
    let mut semantic_indexed = false;
    if let Some(semantic) = &ctx.semantic {
        if semantic.is_available() {
            let content = format!("{}\n\nFix: {}", input.error_pattern, input.fix_description);
            let mut metadata = HashMap::new();
            metadata.insert("type".to_string(), serde_json::Value::String("error_fix".to_string()));
            metadata.insert("id".to_string(), serde_json::Value::Number(id.into()));
            metadata.insert("error_pattern".to_string(), serde_json::Value::String(input.error_pattern.clone()));
            if let Some(ref cat) = input.category {
                metadata.insert("category".to_string(), serde_json::Value::String(cat.clone()));
            }
            if let Some(ref lang) = input.language {
                metadata.insert("language".to_string(), serde_json::Value::String(lang.clone()));
            }

            if let Err(e) = semantic.ensure_collection(COLLECTION_CONVERSATION).await {
                tracing::warn!("Failed to ensure conversation collection: {}", e);
            }

            match semantic.store(COLLECTION_CONVERSATION, &id.to_string(), &content, metadata).await {
                Ok(_) => semantic_indexed = true,
                Err(e) => tracing::warn!("Failed to store error fix in Qdrant: {}", e),
            }
        }
    }

    Ok(RecordErrorFixOutput {
        id,
        status,
        error_pattern: input.error_pattern,
        semantic_indexed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_error_normalizes_numbers() {
        // Numbers should be replaced with 'N' for better deduplication
        let hash1 = hash_error("error at line 42");
        let hash2 = hash_error("error at line 99");
        assert_eq!(hash1, hash2); // Same pattern, different numbers
    }

    #[test]
    fn test_hash_error_case_insensitive() {
        let hash1 = hash_error("Error: Not Found");
        let hash2 = hash_error("error: not found");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_error_first_line_only() {
        let hash1 = hash_error("error on line 1\nmore details\nstack trace");
        let hash2 = hash_error("error on line 1");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_build_error_fields() {
        let err = BuildError {
            id: 1,
            build_run_id: Some(10),
            error_hash: "abc123".to_string(),
            category: Some("type".to_string()),
            severity: "error".to_string(),
            message: "cannot find type".to_string(),
            file_path: Some("src/main.rs".to_string()),
            line_number: Some(42),
            column_number: Some(10),
            code: Some("E0412".to_string()),
            suggestion: Some("did you mean Vec?".to_string()),
            resolved: false,
            created_at: "2024-01-01 12:00:00".to_string(),
            resolved_at: None,
        };
        assert_eq!(err.id, 1);
        assert_eq!(err.severity, "error");
        assert!(!err.resolved);
    }

    #[test]
    fn test_error_fix_fields() {
        let fix = ErrorFix {
            id: 5,
            error_pattern: "cannot find crate".to_string(),
            category: Some("dependency".to_string()),
            language: Some("rust".to_string()),
            file_pattern: Some("Cargo.toml".to_string()),
            fix_description: Some("Add missing dependency".to_string()),
            fix_diff: Some("+serde = \"1.0\"".to_string()),
            fix_commit: Some("abc123".to_string()),
            times_seen: 10,
            times_fixed: 8,
            last_seen: "2024-01-01".to_string(),
            score: 0.95,
            search_type: "semantic".to_string(),
        };
        assert_eq!(fix.id, 5);
        assert_eq!(fix.times_fixed, 8);
        assert_eq!(fix.score, 0.95);
    }

    #[test]
    fn test_record_error_fix_input_fields() {
        let input = RecordErrorFixInput {
            error_pattern: "undefined variable".to_string(),
            fix_description: "Declare the variable first".to_string(),
            category: Some("syntax".to_string()),
            language: Some("rust".to_string()),
            file_pattern: None,
            fix_diff: None,
            fix_commit: None,
        };
        assert_eq!(input.error_pattern, "undefined variable");
        assert!(input.file_pattern.is_none());
    }

    #[test]
    fn test_record_error_fix_output_fields() {
        let output = RecordErrorFixOutput {
            id: 100,
            status: "recorded".to_string(),
            error_pattern: "missing semicolon".to_string(),
            semantic_indexed: true,
        };
        assert_eq!(output.id, 100);
        assert_eq!(output.status, "recorded");
        assert!(output.semantic_indexed);
    }

    #[test]
    fn test_get_build_errors_input_defaults() {
        let input = GetBuildErrorsInput {
            file_path: None,
            category: None,
            include_resolved: false,
            limit: 10,
        };
        assert!(!input.include_resolved);
        assert_eq!(input.limit, 10);
    }
}
