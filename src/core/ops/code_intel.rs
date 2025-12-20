//! Core code intelligence operations - shared by MCP and Chat
//!
//! Code analysis: symbols, call graphs, related files, semantic search.

use std::sync::Arc;

use sqlx::SqlitePool;

use mira_core::semantic::{SemanticSearch, COLLECTION_CODE};

use super::super::{CoreError, CoreResult, OpContext};

// ============================================================================
// Input/Output Types
// ============================================================================

pub struct GetSymbolsInput {
    pub file_path: String,
    pub symbol_type: Option<String>,
}

pub struct SymbolInfo {
    pub id: i64,
    pub name: String,
    pub qualified_name: Option<String>,
    pub symbol_type: String,
    pub language: Option<String>,
    pub start_line: i64,
    pub end_line: i64,
    pub signature: Option<String>,
    pub visibility: Option<String>,
    pub documentation: Option<String>,
    pub is_test: bool,
    pub is_async: bool,
    pub complexity_score: Option<f64>,
}

pub struct GetCallGraphInput {
    pub symbol: String,
    pub depth: i32,
}

pub struct CallGraphOutput {
    pub symbol: String,
    pub called_by: Vec<CallInfo>,
    pub calls: Vec<CallInfo>,
    pub unresolved_calls: Vec<UnresolvedCall>,
    pub deeper_calls: Vec<DeeperCall>,
}

pub struct CallInfo {
    pub name: String,
    pub file: String,
    pub symbol_type: String,
    pub call_type: Option<String>,
    pub line: Option<i32>,
}

pub struct UnresolvedCall {
    pub name: String,
    pub call_type: Option<String>,
    pub line: Option<i32>,
}

pub struct DeeperCall {
    pub name: String,
    pub file: String,
    pub via: String,
    pub depth: i32,
}

pub struct GetRelatedFilesInput {
    pub file_path: String,
    pub relation_type: Option<String>,
    pub limit: i64,
}

pub struct RelatedFilesOutput {
    pub file: String,
    pub imports: Vec<ImportInfo>,
    pub cochange_patterns: Vec<CochangeInfo>,
}

pub struct ImportInfo {
    pub import_path: String,
    pub is_external: bool,
}

pub struct CochangeInfo {
    pub file: String,
    pub cochange_count: i64,
    pub confidence: f64,
}

pub struct SemanticSearchInput {
    pub query: String,
    pub language: Option<String>,
    pub limit: usize,
}

pub struct SemanticSearchResult {
    pub content: String,
    pub score: f32,
    pub search_type: String,
    pub file_path: Option<String>,
    pub symbol_name: Option<String>,
    pub symbol_type: Option<String>,
    pub language: Option<String>,
    pub start_line: Option<i64>,
}

pub struct StyleReport {
    pub total_functions: i64,
    pub avg_function_length: f64,
    pub short_functions: i64,
    pub medium_functions: i64,
    pub long_functions: i64,
    pub short_pct: f64,
    pub medium_pct: f64,
    pub long_pct: f64,
    pub trait_count: i64,
    pub struct_count: i64,
    pub abstraction_level: String,
    pub test_functions: i64,
    pub test_ratio: f64,
    pub suggested_max_length: i64,
}

// ============================================================================
// Operations
// ============================================================================

/// Get symbols from a file
pub async fn get_symbols(ctx: &OpContext, input: GetSymbolsInput) -> CoreResult<Vec<SymbolInfo>> {
    let db = ctx.require_db()?;

    let query = r#"
        SELECT id, name, qualified_name, symbol_type, language, start_line, end_line,
               signature, visibility, documentation, is_test, is_async, complexity_score
        FROM code_symbols
        WHERE file_path LIKE $1
          AND ($2 IS NULL OR symbol_type = $2)
        ORDER BY start_line
    "#;

    let file_pattern = format!("%{}", input.file_path);
    let rows = sqlx::query_as::<_, (i64, String, Option<String>, String, Option<String>, i64, i64, Option<String>, Option<String>, Option<String>, bool, bool, Option<f64>)>(query)
        .bind(&file_pattern)
        .bind(&input.symbol_type)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(id, name, qualified_name, symbol_type, language, start_line, end_line, signature, visibility, documentation, is_test, is_async, complexity_score)| {
        SymbolInfo {
            id,
            name,
            qualified_name,
            symbol_type,
            language,
            start_line,
            end_line,
            signature,
            visibility,
            documentation,
            is_test,
            is_async,
            complexity_score,
        }
    }).collect())
}

/// Get call graph for a symbol
pub async fn get_call_graph(ctx: &OpContext, input: GetCallGraphInput) -> CoreResult<CallGraphOutput> {
    let db = ctx.require_db()?;
    let depth = input.depth.min(5);
    let symbol_pattern = format!("%{}", input.symbol);

    // Get callers
    let callers_query = r#"
        SELECT DISTINCT caller.name, caller.file_path, caller.symbol_type, cg.call_type, cg.call_line
        FROM call_graph cg
        JOIN code_symbols caller ON cg.caller_id = caller.id
        JOIN code_symbols callee ON cg.callee_id = callee.id
        WHERE callee.name = $1 OR callee.qualified_name LIKE $2 OR cg.callee_name = $1 OR cg.callee_name LIKE $2
        LIMIT 50
    "#;

    let callers = sqlx::query_as::<_, (String, String, String, Option<String>, Option<i32>)>(callers_query)
        .bind(&input.symbol)
        .bind(&symbol_pattern)
        .fetch_all(db)
        .await
        .unwrap_or_default();

    // Get callees
    let callees_query = r#"
        SELECT DISTINCT callee.name, callee.file_path, callee.symbol_type, cg.call_type, cg.call_line
        FROM call_graph cg
        JOIN code_symbols caller ON cg.caller_id = caller.id
        JOIN code_symbols callee ON cg.callee_id = callee.id
        WHERE caller.name = $1 OR caller.qualified_name LIKE $2
        LIMIT 50
    "#;

    let callees = sqlx::query_as::<_, (String, String, String, Option<String>, Option<i32>)>(callees_query)
        .bind(&input.symbol)
        .bind(&symbol_pattern)
        .fetch_all(db)
        .await
        .unwrap_or_default();

    // Get unresolved calls
    let unresolved_query = r#"
        SELECT uc.callee_name, uc.call_type, uc.call_line
        FROM unresolved_calls uc
        JOIN code_symbols caller ON uc.caller_id = caller.id
        WHERE caller.name = $1 OR caller.qualified_name LIKE $2
        LIMIT 50
    "#;

    let unresolved = sqlx::query_as::<_, (String, Option<String>, Option<i32>)>(unresolved_query)
        .bind(&input.symbol)
        .bind(&symbol_pattern)
        .fetch_all(db)
        .await
        .unwrap_or_default();

    // Deeper calls if depth > 1
    let mut deeper_calls = Vec::new();
    if depth > 1 {
        for (callee_name, _, _, _, _) in &callees {
            let deeper_query = r#"
                SELECT DISTINCT callee2.name, callee2.file_path, caller.name as via_function
                FROM call_graph cg
                JOIN code_symbols caller ON cg.caller_id = caller.id
                JOIN code_symbols callee2 ON cg.callee_id = callee2.id
                WHERE caller.name = $1
                LIMIT 10
            "#;

            let deeper = sqlx::query_as::<_, (String, String, String)>(deeper_query)
                .bind(callee_name)
                .fetch_all(db)
                .await
                .unwrap_or_default();

            for (name, file, via) in deeper {
                deeper_calls.push(DeeperCall {
                    name,
                    file,
                    via,
                    depth: 2,
                });
            }
        }
    }

    Ok(CallGraphOutput {
        symbol: input.symbol,
        called_by: callers.into_iter().map(|(name, file, typ, call_type, line)| {
            CallInfo { name, file, symbol_type: typ, call_type, line }
        }).collect(),
        calls: callees.into_iter().map(|(name, file, typ, call_type, line)| {
            CallInfo { name, file, symbol_type: typ, call_type, line }
        }).collect(),
        unresolved_calls: unresolved.into_iter().map(|(name, call_type, line)| {
            UnresolvedCall { name, call_type, line }
        }).collect(),
        deeper_calls,
    })
}

/// Get files related to a given file
pub async fn get_related_files(ctx: &OpContext, input: GetRelatedFilesInput) -> CoreResult<RelatedFilesOutput> {
    let db = ctx.require_db()?;
    let relation_type = input.relation_type.as_deref().unwrap_or("all");

    let mut imports = Vec::new();
    let mut cochange_patterns = Vec::new();

    // Get imports
    if relation_type == "all" || relation_type == "imports" {
        let imports_query = r#"
            SELECT DISTINCT import_path, is_external
            FROM imports
            WHERE file_path LIKE $1
            LIMIT $2
        "#;

        let file_pattern = format!("%{}", input.file_path);
        let rows = sqlx::query_as::<_, (String, bool)>(imports_query)
            .bind(&file_pattern)
            .bind(input.limit)
            .fetch_all(db)
            .await
            .unwrap_or_default();

        imports = rows.into_iter().map(|(path, is_external)| {
            ImportInfo { import_path: path, is_external }
        }).collect();
    }

    // Get cochange patterns
    if relation_type == "all" || relation_type == "cochange" {
        let cochange_query = r#"
            SELECT
                CASE WHEN file_a = $1 THEN file_b ELSE file_a END as related_file,
                cochange_count,
                confidence
            FROM cochange_patterns
            WHERE file_a = $1 OR file_b = $1
            ORDER BY confidence DESC
            LIMIT $2
        "#;

        let rows = sqlx::query_as::<_, (String, i64, f64)>(cochange_query)
            .bind(&input.file_path)
            .bind(input.limit)
            .fetch_all(db)
            .await
            .unwrap_or_default();

        cochange_patterns = rows.into_iter().map(|(file, count, confidence)| {
            CochangeInfo { file, cochange_count: count, confidence }
        }).collect();
    }

    Ok(RelatedFilesOutput {
        file: input.file_path,
        imports,
        cochange_patterns,
    })
}

/// Semantic code search
pub async fn semantic_code_search(ctx: &OpContext, input: SemanticSearchInput) -> CoreResult<Vec<SemanticSearchResult>> {
    let db = ctx.require_db()?;

    // Try semantic search if available
    if let Some(semantic) = &ctx.semantic {
        if semantic.is_available() {
            let filter = input.language.as_ref().map(|lang| {
                qdrant_client::qdrant::Filter::must([
                    qdrant_client::qdrant::Condition::matches("language", lang.clone())
                ])
            });

            match semantic.search(COLLECTION_CODE, &input.query, input.limit, filter).await {
                Ok(results) if !results.is_empty() => {
                    return Ok(results.into_iter().map(|r| {
                        SemanticSearchResult {
                            content: r.content,
                            score: r.score,
                            search_type: "semantic".to_string(),
                            file_path: r.metadata.get("file_path").and_then(|v| v.as_str()).map(String::from),
                            symbol_name: r.metadata.get("symbol_name").and_then(|v| v.as_str()).map(String::from),
                            symbol_type: r.metadata.get("symbol_type").and_then(|v| v.as_str()).map(String::from),
                            language: r.metadata.get("language").and_then(|v| v.as_str()).map(String::from),
                            start_line: r.metadata.get("start_line").and_then(|v| v.as_i64()),
                        }
                    }).collect());
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("Semantic code search failed, falling back to text: {}", e);
                }
            }
        }
    }

    // Fallback to SQLite text search
    let search_pattern = format!("%{}%", input.query);

    let query = r#"
        SELECT id, file_path, name, qualified_name, symbol_type, language,
               start_line, end_line, signature, documentation
        FROM code_symbols
        WHERE (name LIKE $1 OR qualified_name LIKE $1 OR documentation LIKE $1)
          AND ($2 IS NULL OR language = $2)
        ORDER BY
            CASE WHEN name LIKE $1 THEN 0 ELSE 1 END,
            start_line
        LIMIT $3
    "#;

    let rows = sqlx::query_as::<_, (i64, String, String, Option<String>, String, Option<String>, i64, i64, Option<String>, Option<String>)>(query)
        .bind(&search_pattern)
        .bind(&input.language)
        .bind(input.limit as i64)
        .fetch_all(db)
        .await?;

    Ok(rows.into_iter().map(|(id, file_path, name, qualified_name, symbol_type, language, start_line, end_line, signature, documentation)| {
        SemanticSearchResult {
            content: signature.or(documentation).unwrap_or_default(),
            score: 1.0,
            search_type: "text".to_string(),
            file_path: Some(file_path),
            symbol_name: Some(name),
            symbol_type: Some(symbol_type),
            language,
            start_line: Some(start_line),
        }
    }).collect())
}

/// Analyze codebase style patterns
pub async fn analyze_codebase_style(ctx: &OpContext, project_path: &str) -> CoreResult<StyleReport> {
    let db = ctx.require_db()?;
    let path_pattern = format!("{}%", project_path);

    // Get function length stats
    let length_stats: Option<(i64, f64)> = sqlx::query_as(
        r#"SELECT COUNT(*), AVG(end_line - start_line + 1)
           FROM code_symbols
           WHERE symbol_type = 'function' AND file_path LIKE $1"#
    )
    .bind(&path_pattern)
    .fetch_optional(db)
    .await?;

    let (total_functions, avg_length) = length_stats.unwrap_or((0, 0.0));

    // Get distribution
    let distribution: Option<(i64, i64, i64)> = sqlx::query_as(
        r#"SELECT
            SUM(CASE WHEN (end_line - start_line + 1) < 10 THEN 1 ELSE 0 END),
            SUM(CASE WHEN (end_line - start_line + 1) BETWEEN 10 AND 30 THEN 1 ELSE 0 END),
            SUM(CASE WHEN (end_line - start_line + 1) > 30 THEN 1 ELSE 0 END)
           FROM code_symbols
           WHERE symbol_type = 'function' AND file_path LIKE $1"#
    )
    .bind(&path_pattern)
    .fetch_optional(db)
    .await?;

    let (short, medium, long) = distribution.unwrap_or((0, 0, 0));

    // Count traits and structs
    let trait_count: (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*) FROM code_symbols
           WHERE symbol_type IN ('trait', 'interface') AND file_path LIKE $1"#
    )
    .bind(&path_pattern)
    .fetch_one(db)
    .await
    .unwrap_or((0,));

    let struct_count: (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*) FROM code_symbols
           WHERE symbol_type IN ('struct', 'class') AND file_path LIKE $1"#
    )
    .bind(&path_pattern)
    .fetch_one(db)
    .await
    .unwrap_or((0,));

    // Count test functions
    let test_count: (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*) FROM code_symbols
           WHERE symbol_type = 'function' AND is_test = 1 AND file_path LIKE $1"#
    )
    .bind(&path_pattern)
    .fetch_one(db)
    .await
    .unwrap_or((0,));

    // Calculate percentages
    let total = total_functions.max(1) as f64;
    let short_pct = (short as f64 / total * 100.0).round();
    let medium_pct = (medium as f64 / total * 100.0).round();
    let long_pct = (long as f64 / total * 100.0).round();
    let test_ratio = test_count.0 as f64 / total;

    // Determine abstraction level
    let abstraction_ratio = trait_count.0 as f64 / (struct_count.0 + trait_count.0).max(1) as f64;
    let abstraction_level = if abstraction_ratio < 0.1 {
        "low"
    } else if abstraction_ratio < 0.3 {
        "moderate"
    } else {
        "heavy"
    }.to_string();

    // Suggested max length
    let suggested_max = if long_pct > 20.0 { 40 } else if medium_pct > 50.0 { 30 } else { 20 };

    Ok(StyleReport {
        total_functions,
        avg_function_length: (avg_length * 10.0).round() / 10.0,
        short_functions: short,
        medium_functions: medium,
        long_functions: long,
        short_pct,
        medium_pct,
        long_pct,
        trait_count: trait_count.0,
        struct_count: struct_count.0,
        abstraction_level,
        test_functions: test_count.0,
        test_ratio: (test_ratio * 100.0).round() / 100.0,
        suggested_max_length: suggested_max,
    })
}
