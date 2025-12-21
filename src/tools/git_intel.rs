// src/tools/git_intel.rs
// Git intelligence tools - thin wrapper over core::ops::git and core::ops::build

use std::sync::Arc;

use sqlx::sqlite::SqlitePool;

use crate::core::ops::build as core_build;
use crate::core::ops::git as core_git;
use crate::core::primitives::semantic::SemanticSearch;
use crate::core::OpContext;
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
    semantic: &Arc<SemanticSearch>,
    req: FindSimilarFixesRequest,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(5) as usize;

    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
        .with_db(db.clone())
        .with_semantic(semantic.clone());

    let input = core_build::FindSimilarFixesInput {
        error: req.error,
        category: req.category,
        language: req.language,
        limit,
    };

    let fixes = core_build::find_similar_fixes(&ctx, input).await?;

    Ok(fixes.into_iter().map(|f| {
        serde_json::json!({
            "id": f.id,
            "error_pattern": f.error_pattern,
            "category": f.category,
            "language": f.language,
            "file_pattern": f.file_pattern,
            "fix_description": f.fix_description,
            "fix_diff": f.fix_diff,
            "commit": f.fix_commit,
            "times_seen": f.times_seen,
            "times_fixed": f.times_fixed,
            "last_seen": f.last_seen,
            "score": f.score,
            "search_type": f.search_type,
        })
    }).collect())
}

/// Record an error fix for future learning
pub async fn record_error_fix(
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    req: RecordErrorFixRequest,
) -> anyhow::Result<serde_json::Value> {
    let ctx = OpContext::new(std::env::current_dir().unwrap_or_default())
        .with_db(db.clone())
        .with_semantic(semantic.clone());

    let input = core_build::RecordErrorFixInput {
        error_pattern: req.error_pattern.clone(),
        fix_description: req.fix_description.clone(),
        category: req.category.clone(),
        language: req.language.clone(),
        file_pattern: req.file_pattern.clone(),
        fix_diff: req.fix_diff.clone(),
        fix_commit: req.fix_commit.clone(),
    };

    let output = core_build::record_error_fix(&ctx, input).await?;

    Ok(serde_json::json!({
        "status": output.status,
        "id": output.id,
        "error_pattern": output.error_pattern,
        "category": req.category,
        "semantic_search": output.semantic_indexed,
    }))
}
