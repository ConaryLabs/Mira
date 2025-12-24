//! Code intelligence tools for Chat
//!
//! Thin wrapper delegating to core::ops::code_intel for shared implementation with MCP.

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::Path;
use std::sync::Arc;

use crate::core::ops::code_intel as core_code;
use crate::core::{OpContext, SemanticSearch};

/// Code intelligence tool implementations
pub struct CodeIntelTools<'a> {
    pub cwd: &'a Path,
    pub db: &'a Option<SqlitePool>,
    pub semantic: &'a Option<Arc<SemanticSearch>>,
}

impl<'a> CodeIntelTools<'a> {
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

    /// Get symbols from a file
    pub async fn get_symbols(&self, args: &Value) -> Result<String> {
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let file_path = args["file_path"].as_str().unwrap_or("");
        if file_path.is_empty() {
            return Ok("Error: file_path is required".into());
        }

        let input = core_code::GetSymbolsInput {
            file_path: file_path.to_string(),
            symbol_type: args["symbol_type"].as_str().map(String::from),
        };

        let ctx = OpContext::just_db(db.clone());
        match core_code::get_symbols(&ctx, input).await {
            Ok(symbols) => {
                let symbols_json: Vec<Value> = symbols
                    .into_iter()
                    .map(|s| {
                        json!({
                            "name": s.name,
                            "type": s.symbol_type,
                            "language": s.language,
                            "start_line": s.start_line,
                            "end_line": s.end_line,
                            "signature": s.signature,
                            "visibility": s.visibility,
                            "is_async": s.is_async,
                            "complexity_score": s.complexity_score,
                        })
                    })
                    .collect();
                let count = symbols_json.len();
                Ok(json!({
                    "file": file_path,
                    "symbols": symbols_json,
                    "count": count,
                })
                .to_string())
            }
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }

    /// Get call graph for a symbol
    pub async fn get_call_graph(&self, args: &Value) -> Result<String> {
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let symbol = args["symbol"].as_str().unwrap_or("");
        if symbol.is_empty() {
            return Ok("Error: symbol is required".into());
        }

        let depth = args["depth"].as_i64().unwrap_or(2) as i32;

        let input = core_code::GetCallGraphInput {
            symbol: symbol.to_string(),
            depth,
        };

        let ctx = OpContext::just_db(db.clone());
        match core_code::get_call_graph(&ctx, input).await {
            Ok(graph) => {
                let called_by: Vec<Value> = graph
                    .called_by
                    .iter()
                    .map(|c| {
                        json!({
                            "name": c.name,
                            "file": c.file,
                            "type": c.symbol_type,
                            "line": c.line,
                        })
                    })
                    .collect();
                let calls: Vec<Value> = graph
                    .calls
                    .iter()
                    .map(|c| {
                        json!({
                            "name": c.name,
                            "file": c.file,
                            "type": c.symbol_type,
                            "line": c.line,
                        })
                    })
                    .collect();
                Ok(json!({
                    "symbol": graph.symbol,
                    "depth": depth,
                    "called_by": called_by,
                    "calls": calls,
                })
                .to_string())
            }
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }

    /// Semantic code search - find code by natural language description
    pub async fn semantic_code_search(&self, args: &Value) -> Result<String> {
        if self.db.is_none() {
            return Ok("Error: database not configured".into());
        }

        let query = args["query"].as_str().unwrap_or("");
        if query.is_empty() {
            return Ok("Error: query is required".into());
        }

        let limit = args["limit"].as_i64().unwrap_or(10) as usize;

        let input = core_code::SemanticSearchInput {
            query: query.to_string(),
            language: args["language"].as_str().map(String::from),
            limit,
        };

        let ctx = self.make_context();
        match core_code::semantic_code_search(&ctx, input).await {
            Ok(results) => {
                let results_json: Vec<Value> = results
                    .into_iter()
                    .map(|r| {
                        json!({
                            "content": r.content,
                            "score": r.score,
                            "file_path": r.file_path,
                            "symbol_name": r.symbol_name,
                            "symbol_type": r.symbol_type,
                            "language": r.language,
                            "start_line": r.start_line,
                        })
                    })
                    .collect();
                let count = results_json.len();
                Ok(json!({
                    "query": query,
                    "results": results_json,
                    "count": count,
                })
                .to_string())
            }
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }

    /// Get files related to a given file
    pub async fn get_related_files(&self, args: &Value) -> Result<String> {
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let file_path = args["file_path"].as_str().unwrap_or("");
        if file_path.is_empty() {
            return Ok("Error: file_path is required".into());
        }

        let limit = args["limit"].as_i64().unwrap_or(10);

        let input = core_code::GetRelatedFilesInput {
            file_path: file_path.to_string(),
            relation_type: args["relation_type"].as_str().map(String::from),
            limit,
        };

        let ctx = OpContext::just_db(db.clone());
        match core_code::get_related_files(&ctx, input).await {
            Ok(related) => {
                let imports: Vec<Value> = related
                    .imports
                    .iter()
                    .map(|i| {
                        json!({
                            "import_path": i.import_path,
                            "is_external": i.is_external,
                        })
                    })
                    .collect();
                let cochange: Vec<Value> = related
                    .cochange_patterns
                    .iter()
                    .map(|c| {
                        json!({
                            "file": c.file,
                            "cochange_count": c.cochange_count,
                            "confidence": c.confidence,
                        })
                    })
                    .collect();
                Ok(json!({
                    "file": related.file,
                    "imports": imports,
                    "cochange_patterns": cochange,
                })
                .to_string())
            }
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }

    /// Get codebase style metrics
    pub async fn get_codebase_style(&self, args: &Value) -> Result<String> {
        let _db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let project_path = args["project_path"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| self.cwd.to_string_lossy().to_string());

        let ctx = self.make_context();
        match core_code::analyze_codebase_style(&ctx, &project_path).await {
            Ok(style) => Ok(json!({
                "total_functions": style.total_functions,
                "avg_function_length": style.avg_function_length,
                "short_functions": style.short_functions,
                "medium_functions": style.medium_functions,
                "long_functions": style.long_functions,
                "short_pct": style.short_pct,
                "medium_pct": style.medium_pct,
                "long_pct": style.long_pct,
                "trait_count": style.trait_count,
                "struct_count": style.struct_count,
                "abstraction_level": style.abstraction_level,
                "test_functions": style.test_functions,
                "test_ratio": style.test_ratio,
                "suggested_max_length": style.suggested_max_length,
            })
            .to_string()),
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }
}
