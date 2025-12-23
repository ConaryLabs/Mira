//! Git intelligence tools for Chat
//!
//! Thin wrapper delegating to core::ops::git and core::ops::build for shared implementation with MCP.

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::Path;
use std::sync::Arc;

use crate::core::ops::build as core_build;
use crate::core::ops::git as core_git;
use crate::core::{OpContext, SemanticSearch};

/// Git intelligence tool implementations
pub struct GitIntelTools<'a> {
    pub cwd: &'a Path,
    pub db: &'a Option<SqlitePool>,
    pub semantic: &'a Option<Arc<SemanticSearch>>,
}

impl<'a> GitIntelTools<'a> {
    /// Create OpContext from our fields
    fn make_context(&self) -> OpContext {
        let mut ctx = OpContext::new(self.cwd.to_path_buf());
        if let Some(db) = self.db.as_ref() {
            ctx = ctx.with_db(db.clone());
        }
        if let Some(semantic) = self.semantic.as_ref() {
            ctx = ctx.with_semantic(semantic.clone());
        }
        ctx
    }

    /// Get recent commits
    pub async fn get_recent_commits(&self, args: &Value) -> Result<String> {
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let input = core_git::GetRecentCommitsInput {
            file_path: args["file_path"].as_str().map(String::from),
            author: args["author"].as_str().map(String::from),
            limit: args["limit"].as_i64().unwrap_or(20),
        };

        let ctx = OpContext::just_db(db.clone());
        match core_git::get_recent_commits(&ctx, input).await {
            Ok(commits) => {
                let commits_json: Vec<Value> = commits
                    .into_iter()
                    .map(|c| {
                        json!({
                            "commit_hash": c.commit_hash,
                            "author": c.author,
                            "message": c.message,
                            "files_changed": c.files_changed,
                            "insertions": c.insertions,
                            "deletions": c.deletions,
                            "committed_at": c.committed_at,
                        })
                    })
                    .collect();
                let count = commits_json.len();
                Ok(json!({
                    "commits": commits_json,
                    "count": count,
                })
                .to_string())
            }
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }

    /// Search commits by message
    pub async fn search_commits(&self, args: &Value) -> Result<String> {
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let query = args["query"].as_str().unwrap_or("");
        if query.is_empty() {
            return Ok("Error: query is required".into());
        }

        let input = core_git::SearchCommitsInput {
            query: query.to_string(),
            limit: args["limit"].as_i64().unwrap_or(20),
        };

        let ctx = OpContext::just_db(db.clone());
        match core_git::search_commits(&ctx, input).await {
            Ok(commits) => {
                let commits_json: Vec<Value> = commits
                    .into_iter()
                    .map(|c| {
                        json!({
                            "commit_hash": c.commit_hash,
                            "author": c.author,
                            "message": c.message,
                            "files_changed": c.files_changed,
                            "committed_at": c.committed_at,
                        })
                    })
                    .collect();
                let count = commits_json.len();
                Ok(json!({
                    "query": query,
                    "commits": commits_json,
                    "count": count,
                })
                .to_string())
            }
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }

    /// Find co-change patterns for a file
    pub async fn find_cochange_patterns(&self, args: &Value) -> Result<String> {
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let file_path = args["file_path"].as_str().unwrap_or("");
        if file_path.is_empty() {
            return Ok("Error: file_path is required".into());
        }

        let input = core_git::FindCochangeInput {
            file_path: file_path.to_string(),
            limit: args["limit"].as_i64().unwrap_or(10),
        };

        let ctx = OpContext::just_db(db.clone());
        match core_git::find_cochange_patterns(&ctx, input).await {
            Ok(patterns) => {
                let patterns_json: Vec<Value> = patterns
                    .into_iter()
                    .map(|p| {
                        json!({
                            "file": p.file,
                            "cochange_count": p.cochange_count,
                            "confidence": p.confidence,
                            "last_seen": p.last_seen,
                        })
                    })
                    .collect();
                let count = patterns_json.len();
                Ok(json!({
                    "file": file_path,
                    "patterns": patterns_json,
                    "count": count,
                })
                .to_string())
            }
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }

    /// Find similar error fixes
    pub async fn find_similar_fixes(&self, args: &Value) -> Result<String> {
        let error = args["error"].as_str().unwrap_or("");
        if error.is_empty() {
            return Ok("Error: error message is required".into());
        }

        let limit = args["limit"].as_i64().unwrap_or(5) as usize;

        let input = core_build::FindSimilarFixesInput {
            error: error.to_string(),
            category: args["category"].as_str().map(String::from),
            language: args["language"].as_str().map(String::from),
            limit,
        };

        let ctx = self.make_context();
        match core_build::find_similar_fixes(&ctx, input).await {
            Ok(fixes) => {
                let fixes_json: Vec<Value> = fixes
                    .into_iter()
                    .map(|f| {
                        json!({
                            "id": f.id,
                            "error_pattern": f.error_pattern,
                            "fix_description": f.fix_description,
                            "category": f.category,
                            "language": f.language,
                            "times_seen": f.times_seen,
                            "times_fixed": f.times_fixed,
                            "score": f.score,
                        })
                    })
                    .collect();
                let count = fixes_json.len();
                Ok(json!({
                    "error": error,
                    "fixes": fixes_json,
                    "count": count,
                })
                .to_string())
            }
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }

    /// Record an error fix for future learning
    pub async fn record_error_fix(&self, args: &Value) -> Result<String> {
        let error_pattern = args["error_pattern"].as_str().unwrap_or("");
        let fix_description = args["fix_description"].as_str().unwrap_or("");

        if error_pattern.is_empty() || fix_description.is_empty() {
            return Ok("Error: error_pattern and fix_description are required".into());
        }

        let input = core_build::RecordErrorFixInput {
            error_pattern: error_pattern.to_string(),
            fix_description: fix_description.to_string(),
            category: args["category"].as_str().map(String::from),
            language: args["language"].as_str().map(String::from),
            file_pattern: args["file_pattern"].as_str().map(String::from),
            fix_diff: args["fix_diff"].as_str().map(String::from),
            fix_commit: args["fix_commit"].as_str().map(String::from),
        };

        let ctx = self.make_context();
        match core_build::record_error_fix(&ctx, input).await {
            Ok(output) => Ok(json!({
                "status": output.status,
                "id": output.id,
                "error_pattern": output.error_pattern,
                "semantic_indexed": output.semantic_indexed,
            })
            .to_string()),
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }
}
