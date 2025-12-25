//! Index tool implementation - code and git history indexing

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;

use crate::indexer::{CodeIndexer, GitIndexer};
use crate::tools::{IndexRequest, SemanticSearch};

/// Result of an indexing operation
#[derive(serde::Serialize)]
pub struct IndexResult {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workers: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_processed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols_found: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imports_found: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embeddings_generated: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commits_indexed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cochange_patterns: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols_indexed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imports_indexed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols_removed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calls_removed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imports_removed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patterns_cleaned: Option<Vec<String>>,
}

/// Index a project directory (with optional git history)
pub async fn index_project(
    db: &SqlitePool,
    semantic: Arc<SemanticSearch>,
    req: &IndexRequest,
) -> Result<IndexResult> {
    let path = req.path.as_ref()
        .ok_or_else(|| anyhow::anyhow!("path required"))?;
    let path = std::path::Path::new(path);

    // Use parallel indexing by default for better performance
    let use_parallel = req.parallel.unwrap_or(true);
    let max_workers = req.max_workers.unwrap_or(4) as usize;

    let mut stats = if use_parallel {
        CodeIndexer::index_directory_parallel(
            db.clone(),
            Some(semantic.clone()),
            path,
            max_workers,
        ).await?
    } else {
        let mut code_indexer = CodeIndexer::with_semantic(
            db.clone(),
            Some(semantic.clone())
        )?;
        code_indexer.index_directory(path).await?
    };

    // Index git if requested (default: true)
    if req.include_git.unwrap_or(true) {
        let git_indexer = GitIndexer::new(db.clone());
        let commit_limit = req.commit_limit.unwrap_or(500) as usize;
        let git_stats = git_indexer.index_repository(path, commit_limit).await?;
        stats.merge(git_stats);
    }

    Ok(IndexResult {
        status: "indexed".to_string(),
        parallel: Some(use_parallel),
        workers: Some(max_workers),
        file: None,
        files_processed: Some(stats.files_processed),
        symbols_found: Some(stats.symbols_found),
        imports_found: Some(stats.imports_found),
        embeddings_generated: Some(stats.embeddings_generated),
        commits_indexed: Some(stats.commits_indexed),
        cochange_patterns: Some(stats.cochange_patterns),
        errors: Some(stats.errors),
        symbols_indexed: None,
        imports_indexed: None,
        symbols_removed: None,
        calls_removed: None,
        imports_removed: None,
        patterns_cleaned: None,
    })
}

/// Index a single file
pub async fn index_file(
    db: &SqlitePool,
    semantic: Arc<SemanticSearch>,
    req: &IndexRequest,
) -> Result<IndexResult> {
    let path = req.path.as_ref()
        .ok_or_else(|| anyhow::anyhow!("path required"))?;
    tracing::info!("[MCP] index action=file, path={}", path);
    let path = std::path::Path::new(path);

    tracing::debug!("[MCP] Creating CodeIndexer...");
    let mut code_indexer = CodeIndexer::with_semantic(
        db.clone(),
        Some(semantic)
    )?;
    tracing::debug!("[MCP] Calling index_file...");
    let stats = code_indexer.index_file(path).await?;
    tracing::info!("[MCP] index_file returned: {:?}", stats);

    Ok(IndexResult {
        status: "indexed".to_string(),
        parallel: None,
        workers: None,
        file: Some(req.path.clone().unwrap_or_default()),
        files_processed: None,
        symbols_found: Some(stats.symbols_found),
        imports_found: Some(stats.imports_found),
        embeddings_generated: Some(stats.embeddings_generated),
        commits_indexed: None,
        cochange_patterns: None,
        errors: None,
        symbols_indexed: None,
        imports_indexed: None,
        symbols_removed: None,
        calls_removed: None,
        imports_removed: None,
        patterns_cleaned: None,
    })
}

/// Get indexing status
pub async fn index_status(db: &SqlitePool) -> Result<IndexResult> {
    let symbols: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM code_symbols")
        .fetch_one(db)
        .await?;
    let imports: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM imports")
        .fetch_one(db)
        .await?;
    let commits: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM git_commits")
        .fetch_one(db)
        .await?;
    let cochange: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM cochange_patterns")
        .fetch_one(db)
        .await?;

    Ok(IndexResult {
        status: "status".to_string(),
        parallel: None,
        workers: None,
        file: None,
        files_processed: None,
        symbols_found: None,
        imports_found: None,
        embeddings_generated: None,
        commits_indexed: Some(commits.0 as usize),
        cochange_patterns: Some(cochange.0 as usize),
        errors: None,
        symbols_indexed: Some(symbols.0),
        imports_indexed: Some(imports.0),
        symbols_removed: None,
        calls_removed: None,
        imports_removed: None,
        patterns_cleaned: None,
    })
}

/// Cleanup stale index data
pub async fn index_cleanup(db: &SqlitePool) -> Result<IndexResult> {
    // Remove stale data from excluded directories and orphaned entries
    let excluded_patterns = vec![
        "%/target/%",
        "%/node_modules/%",
        "%/__pycache__/%",
        "%/.git/%",
    ];

    let mut symbols_removed = 0i64;
    let mut calls_removed = 0i64;
    let mut imports_removed = 0i64;

    for pattern in &excluded_patterns {
        // Remove call_graph entries first (foreign key constraints)
        let result = sqlx::query(
            "DELETE FROM call_graph WHERE caller_id IN (SELECT id FROM code_symbols WHERE file_path LIKE $1)"
        )
        .bind(pattern)
        .execute(db)
        .await?;
        calls_removed += result.rows_affected() as i64;

        let result = sqlx::query(
            "DELETE FROM call_graph WHERE callee_id IN (SELECT id FROM code_symbols WHERE file_path LIKE $1)"
        )
        .bind(pattern)
        .execute(db)
        .await?;
        calls_removed += result.rows_affected() as i64;

        // Remove symbols
        let result = sqlx::query("DELETE FROM code_symbols WHERE file_path LIKE $1")
            .bind(pattern)
            .execute(db)
            .await?;
        symbols_removed += result.rows_affected() as i64;

        // Remove imports
        let result = sqlx::query("DELETE FROM imports WHERE file_path LIKE $1")
            .bind(pattern)
            .execute(db)
            .await?;
        imports_removed += result.rows_affected() as i64;
    }

    // Also clean up orphaned call_graph entries (where caller or callee no longer exists)
    let result = sqlx::query(
        "DELETE FROM call_graph WHERE caller_id NOT IN (SELECT id FROM code_symbols) OR callee_id NOT IN (SELECT id FROM code_symbols)"
    )
    .execute(db)
    .await?;
    let orphans_removed = result.rows_affected() as i64;

    Ok(IndexResult {
        status: "cleaned".to_string(),
        parallel: None,
        workers: None,
        file: None,
        files_processed: None,
        symbols_found: None,
        imports_found: None,
        embeddings_generated: None,
        commits_indexed: None,
        cochange_patterns: None,
        errors: None,
        symbols_indexed: None,
        imports_indexed: None,
        symbols_removed: Some(symbols_removed),
        calls_removed: Some(calls_removed + orphans_removed),
        imports_removed: Some(imports_removed),
        patterns_cleaned: Some(excluded_patterns.iter().map(|s| s.to_string()).collect()),
    })
}
