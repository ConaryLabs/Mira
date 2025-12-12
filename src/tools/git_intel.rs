// src/tools/git_intel.rs
// Git intelligence tools - commits, cochange patterns, error fixes

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use std::collections::HashMap;

use super::semantic::{SemanticSearch, COLLECTION_CONVERSATION};
use super::types::*;

/// Normalize an absolute path to a relative path for database lookups.
/// Cochange patterns are stored with relative paths (e.g., "src/main.rs").
fn normalize_to_relative(path: &str) -> String {
    // Common project root patterns to strip
    if path.starts_with('/') {
        // Try to find "src/" or other common directories
        for marker in ["src/", "lib/", "tests/", "examples/", "benches/"] {
            if let Some(idx) = path.find(marker) {
                return path[idx..].to_string();
            }
        }
        // Try stripping everything up to and including the last project-like directory
        // e.g., /home/user/MyProject/foo.rs -> foo.rs
        if let Some(last_slash) = path.rfind('/') {
            // Get filename if no better match
            let filename = &path[last_slash + 1..];
            // But also check if there's a recognizable relative path
            // Look for patterns like "project_name/src/..." or "project_name/lib/..."
            let parts: Vec<&str> = path.split('/').collect();
            for (i, part) in parts.iter().enumerate() {
                // Common project indicators: Cargo.toml sibling dirs, etc.
                if *part == "src" || *part == "lib" || *part == "tests" {
                    return parts[i..].join("/");
                }
            }
            // Last resort: use just the filename
            if !filename.is_empty() {
                return filename.to_string();
            }
        }
    }
    path.to_string()
}

/// Get recent commits
pub async fn get_recent_commits(db: &SqlitePool, req: GetRecentCommitsRequest) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(20);

    let query = r#"
        SELECT commit_hash, author_name, author_email, message, files_changed,
               insertions, deletions,
               datetime(committed_at, 'unixepoch', 'localtime') as committed_at
        FROM git_commits
        WHERE ($1 IS NULL OR files_changed LIKE $1)
          AND ($2 IS NULL OR author_email = $2)
        ORDER BY committed_at DESC
        LIMIT $3
    "#;

    let file_pattern = req.file_path.as_ref().map(|f| format!("%{}%", f));
    let rows = sqlx::query_as::<_, (String, Option<String>, Option<String>, String, Option<String>, i64, i64, String)>(query)
        .bind(&file_pattern)
        .bind(&req.author)
        .bind(limit)
        .fetch_all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(hash, author, email, message, files, insertions, deletions, committed_at)| {
            serde_json::json!({
                "commit_hash": hash,
                "author": author,
                "email": email,
                "message": message,
                "files_changed": files,
                "insertions": insertions,
                "deletions": deletions,
                "committed_at": committed_at,
            })
        })
        .collect())
}

/// Search commits by message
pub async fn search_commits(db: &SqlitePool, req: SearchCommitsRequest) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(20);
    let search_pattern = format!("%{}%", req.query);

    let query = r#"
        SELECT commit_hash, author_name, author_email, message, files_changed,
               datetime(committed_at, 'unixepoch', 'localtime') as committed_at
        FROM git_commits
        WHERE message LIKE $1
        ORDER BY committed_at DESC
        LIMIT $2
    "#;

    let rows = sqlx::query_as::<_, (String, Option<String>, Option<String>, String, Option<String>, String)>(query)
        .bind(&search_pattern)
        .bind(limit)
        .fetch_all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(hash, author, email, message, files, committed_at)| {
            serde_json::json!({
                "commit_hash": hash,
                "author": author,
                "email": email,
                "message": message,
                "files_changed": files,
                "committed_at": committed_at,
            })
        })
        .collect())
}

/// Find co-change patterns for a file
pub async fn find_cochange_patterns(db: &SqlitePool, req: FindCochangeRequest) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(10);

    // Normalize path: cochange_patterns stores relative paths (e.g., "src/main.rs")
    // but requests often come with absolute paths (e.g., "/home/user/project/src/main.rs")
    let file_path = normalize_to_relative(&req.file_path);

    let query = r#"
        SELECT
            CASE WHEN file_a = $1 THEN file_b ELSE file_a END as related_file,
            cochange_count,
            confidence,
            datetime(last_seen, 'unixepoch', 'localtime') as last_seen
        FROM cochange_patterns
        WHERE file_a = $1 OR file_b = $1
        ORDER BY confidence DESC
        LIMIT $2
    "#;

    let rows = sqlx::query_as::<_, (String, i64, f64, String)>(query)
        .bind(&file_path)
        .bind(limit)
        .fetch_all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(file, count, confidence, last_seen)| {
            serde_json::json!({
                "file": file,
                "cochange_count": count,
                "confidence": confidence,
                "last_seen": last_seen,
            })
        })
        .collect())
}

/// Find similar error fixes - uses semantic search if available
pub async fn find_similar_fixes(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: FindSimilarFixesRequest,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(5) as usize;

    // Try semantic search first for better "this error feels like..." matching
    if semantic.is_available() {
        let filter = if let Some(ref category) = req.category {
            Some(qdrant_client::qdrant::Filter::must([
                qdrant_client::qdrant::Condition::matches("category", category.clone())
            ]))
        } else {
            None
        };

        match semantic.search(COLLECTION_CONVERSATION, &req.error, limit, filter).await {
            Ok(results) => {
                let error_fixes: Vec<_> = results.into_iter()
                    .filter(|r| r.metadata.get("type").map(|v| v.as_str()) == Some(Some("error_fix")))
                    .map(|r| {
                        serde_json::json!({
                            "error_pattern": r.metadata.get("error_pattern"),
                            "fix_description": r.content,
                            "score": r.score,
                            "search_type": "semantic",
                            "category": r.metadata.get("category"),
                            "language": r.metadata.get("language"),
                        })
                    })
                    .collect();

                if !error_fixes.is_empty() {
                    return Ok(error_fixes);
                }
            }
            Err(e) => {
                tracing::warn!("Semantic fix search failed, falling back to text: {}", e);
            }
        }
    }

    // Fallback to SQLite text search
    let error_pattern = format!("%{}%", req.error);

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
        .bind(&req.category)
        .bind(&req.language)
        .bind(limit as i64)
        .fetch_all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(id, pattern, category, language, file_pattern, fix_desc, fix_diff, commit, seen, fixed, last_seen)| {
            serde_json::json!({
                "id": id,
                "error_pattern": pattern,
                "category": category,
                "language": language,
                "file_pattern": file_pattern,
                "fix_description": fix_desc,
                "fix_diff": fix_diff,
                "commit": commit,
                "times_seen": seen,
                "times_fixed": fixed,
                "last_seen": last_seen,
                "search_type": "text",
            })
        })
        .collect())
}

/// Record an error fix for future learning
pub async fn record_error_fix(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: RecordErrorFixRequest,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();

    // Try to update existing pattern or insert new one
    let existing = sqlx::query_as::<_, (i64,)>(
        "SELECT id FROM error_fixes WHERE error_pattern = $1"
    )
    .bind(&req.error_pattern)
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
        .bind(&req.fix_description)
        .bind(&req.fix_diff)
        .bind(&req.fix_commit)
        .bind(now)
        .bind(id)
        .execute(db)
        .await?;

        ("updated", id)
    } else {
        // Insert new
        let result = sqlx::query(r#"
            INSERT INTO error_fixes (error_pattern, error_category, language, file_pattern,
                                     fix_description, fix_diff, fix_commit,
                                     times_seen, times_fixed, last_seen, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, 1, 1, $8, $8)
        "#)
        .bind(&req.error_pattern)
        .bind(&req.category)
        .bind(&req.language)
        .bind(&req.file_pattern)
        .bind(&req.fix_description)
        .bind(&req.fix_diff)
        .bind(&req.fix_commit)
        .bind(now)
        .execute(db)
        .await?;

        ("recorded", result.last_insert_rowid())
    };

    // Store in Qdrant for semantic search (use error pattern + fix as content)
    if semantic.is_available() {
        let content = format!("{}\n\nFix: {}", req.error_pattern, req.fix_description);
        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), serde_json::Value::String("error_fix".to_string()));
        metadata.insert("error_pattern".to_string(), serde_json::Value::String(req.error_pattern.clone()));
        if let Some(ref cat) = req.category {
            metadata.insert("category".to_string(), serde_json::Value::String(cat.clone()));
        }
        if let Some(ref lang) = req.language {
            metadata.insert("language".to_string(), serde_json::Value::String(lang.clone()));
        }

        if let Err(e) = semantic.ensure_collection(COLLECTION_CONVERSATION).await {
            tracing::warn!("Failed to ensure conversation collection: {}", e);
        }

        if let Err(e) = semantic.store(COLLECTION_CONVERSATION, &id.to_string(), &content, metadata).await {
            tracing::warn!("Failed to store error fix in Qdrant: {}", e);
        }
    }

    Ok(serde_json::json!({
        "status": status,
        "id": id,
        "error_pattern": req.error_pattern,
        "category": req.category,
        "semantic_search": semantic.is_available(),
    }))
}
