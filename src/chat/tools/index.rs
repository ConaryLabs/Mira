//! Code indexing tools for Chat
//!
//! Provides index status and basic indexing capabilities using the CodeIndexer.

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::Path;
use std::sync::Arc;

use crate::core::SemanticSearch;
use crate::indexer::code::CodeIndexer;

/// Index tool implementations
pub struct IndexTools<'a> {
    pub cwd: &'a Path,
    pub db: &'a Option<SqlitePool>,
    pub semantic: &'a Option<Arc<SemanticSearch>>,
}

impl<'a> IndexTools<'a> {
    /// Consolidated index tool - handles project, file, status, cleanup
    pub async fn index(&self, args: &Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("status");
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        match action {
            "project" => {
                let path = args["path"]
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| self.cwd.to_string_lossy().to_string());

                let use_parallel = args["parallel"].as_bool().unwrap_or(true);
                let max_workers = args["max_workers"].as_i64().map(|v| v as usize).unwrap_or(4);

                let path_ref = std::path::Path::new(&path);
                let stats = if use_parallel {
                    CodeIndexer::index_directory_parallel(
                        db.clone(),
                        self.semantic.clone(),
                        path_ref,
                        max_workers,
                    )
                    .await?
                } else {
                    let mut code_indexer =
                        CodeIndexer::with_semantic(db.clone(), self.semantic.clone())?;
                    code_indexer.index_directory(path_ref).await?
                };

                Ok(json!({
                    "status": "indexed",
                    "path": path,
                    "files_processed": stats.files_processed,
                    "symbols_found": stats.symbols_found,
                    "imports_found": stats.imports_found,
                    "embeddings_generated": stats.embeddings_generated,
                    "errors": stats.errors,
                })
                .to_string())
            }

            "file" => {
                let path = args["path"].as_str().unwrap_or("");
                if path.is_empty() {
                    return Ok("Error: path is required".into());
                }

                let mut code_indexer =
                    CodeIndexer::with_semantic(db.clone(), self.semantic.clone())?;

                let path_ref = std::path::Path::new(path);
                let stats = code_indexer.index_file(path_ref).await?;

                Ok(json!({
                    "status": "indexed",
                    "file": path,
                    "symbols_found": stats.symbols_found,
                    "imports_found": stats.imports_found,
                    "embeddings_generated": stats.embeddings_generated,
                })
                .to_string())
            }

            "status" => {
                // Get indexing status from database
                let symbols: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM code_symbols")
                    .fetch_one(db)
                    .await
                    .unwrap_or((0,));

                let calls: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM call_graph")
                    .fetch_one(db)
                    .await
                    .unwrap_or((0,));

                let imports: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM file_imports")
                    .fetch_one(db)
                    .await
                    .unwrap_or((0,));

                let commits: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM git_commits")
                    .fetch_one(db)
                    .await
                    .unwrap_or((0,));

                let cochange: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM cochange_patterns")
                    .fetch_one(db)
                    .await
                    .unwrap_or((0,));

                Ok(json!({
                    "symbols": symbols.0,
                    "call_relationships": calls.0,
                    "imports": imports.0,
                    "commits": commits.0,
                    "cochange_patterns": cochange.0,
                })
                .to_string())
            }

            "cleanup" => {
                // Remove stale data from excluded directories
                let excluded_patterns = vec![
                    "%/target/%",
                    "%/node_modules/%",
                    "%/.git/%",
                    "%/__pycache__/%",
                    "%/dist/%",
                    "%/build/%",
                    "%/.venv/%",
                    "%/venv/%",
                ];

                let mut symbols_removed = 0i64;
                let mut calls_removed = 0i64;
                let mut imports_removed = 0i64;

                for pattern in &excluded_patterns {
                    // Remove symbols
                    let result = sqlx::query("DELETE FROM code_symbols WHERE file_path LIKE $1")
                        .bind(pattern)
                        .execute(db)
                        .await?;
                    symbols_removed += result.rows_affected() as i64;

                    // Remove calls
                    let result = sqlx::query(
                        "DELETE FROM call_graph WHERE caller_file LIKE $1 OR callee_file LIKE $1",
                    )
                    .bind(pattern)
                    .execute(db)
                    .await?;
                    calls_removed += result.rows_affected() as i64;

                    // Remove imports
                    let result = sqlx::query("DELETE FROM file_imports WHERE source_file LIKE $1")
                        .bind(pattern)
                        .execute(db)
                        .await?;
                    imports_removed += result.rows_affected() as i64;
                }

                // Clean orphaned call graph entries
                let result = sqlx::query(
                    "DELETE FROM call_graph WHERE caller_file NOT IN (SELECT DISTINCT file_path FROM code_symbols)",
                )
                .execute(db)
                .await?;
                let orphans_removed = result.rows_affected() as i64;

                Ok(json!({
                    "status": "cleaned",
                    "symbols_removed": symbols_removed,
                    "calls_removed": calls_removed + orphans_removed,
                    "imports_removed": imports_removed,
                })
                .to_string())
            }

            _ => Ok(format!(
                "Unknown action: {}. Use project/file/status/cleanup",
                action
            )),
        }
    }
}
