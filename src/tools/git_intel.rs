// src/tools/git_intel.rs
// Git intelligence tools - thin wrapper over core::ops::git

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use std::collections::HashMap;

use crate::core::ops::git as core_git;
use crate::core::OpContext;
use super::semantic::{SemanticSearch, COLLECTION_CONVERSATION};
use super::types::*;

/// Get recent commits
pub async fn get_recent_commits(db: &SqlitePool, req: GetRecentCommitsRequest) -> anyhow::Result<Vec<serde_json::Value>> {
    let ctx = OpContext::just_db(db.clone());
    let limit = req.limit.unwrap_or(20);

    let input = core_git::GetRecentCommitsInput {
        file_path: req.file_path,
        author: req.author,
        limit,
    };

    let commits = core_git::get_recent_commits(&ctx, input).await?;

    Ok(commits.into_iter().map(|c| {
        serde_json::json!({
            "commit_hash": c.commit_hash,
            "author": c.author,
            "email": c.email,
            "message": c.message,
            "files_changed": c.files_changed,
            "insertions": c.insertions,
            "deletions": c.deletions,
            "committed_at": c.committed_at,
        })
    }).collect())
}

/// Search commits by message
pub async fn search_commits(db: &SqlitePool, req: SearchCommitsRequest) -> anyhow::Result<Vec<serde_json::Value>> {
    let ctx = OpContext::just_db(db.clone());
    let limit = req.limit.unwrap_or(20);

    let input = core_git::SearchCommitsInput {
        query: req.query,
        limit,
    };

    let commits = core_git::search_commits(&ctx, input).await?;

    Ok(commits.into_iter().map(|c| {
        serde_json::json!({
            "commit_hash": c.commit_hash,
            "author": c.author,
            "email": c.email,
            "message": c.message,
            "files_changed": c.files_changed,
            "committed_at": c.committed_at,
        })
    }).collect())
}

/// Find co-change patterns for a file
pub async fn find_cochange_patterns(db: &SqlitePool, req: FindCochangeRequest) -> anyhow::Result<Vec<serde_json::Value>> {
    let ctx = OpContext::just_db(db.clone());
    let limit = req.limit.unwrap_or(10);

    let input = core_git::FindCochangeInput {
        file_path: req.file_path,
        limit,
    };

    let patterns = core_git::find_cochange_patterns(&ctx, input).await?;

    Ok(patterns.into_iter().map(|p| {
        serde_json::json!({
            "file": p.file,
            "cochange_count": p.cochange_count,
            "confidence": p.confidence,
            "last_seen": p.last_seen,
        })
    }).collect())
}

/// Find similar error fixes - uses semantic search if available
pub async fn find_similar_fixes(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: FindSimilarFixesRequest,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(5) as usize;

    // Try semantic search first for better matching
    if semantic.is_available() {
        let filter = req.category.as_ref().map(|category| qdrant_client::qdrant::Filter::must([
            qdrant_client::qdrant::Condition::matches("category", category.clone())
        ]));

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

    // Store in Qdrant for semantic search
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
