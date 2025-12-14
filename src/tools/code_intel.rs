// src/tools/code_intel.rs
// Code intelligence tools with semantic search

use serde::Serialize;
use sqlx::sqlite::SqlitePool;

use super::semantic::{SemanticSearch, COLLECTION_CODE};
use super::types::*;

/// Code improvement suggestion
#[derive(Debug, Clone, Serialize)]
pub struct CodeImprovement {
    pub file_path: String,
    pub symbol_name: String,
    pub improvement_type: String,  // "long_function", "high_complexity", "missing_test"
    pub current_value: i64,
    pub threshold: i64,
    pub severity: String,  // "high", "medium", "low"
    pub suggestion: String,
    pub start_line: i64,
}

/// Codebase style analysis report
#[derive(Debug, Clone, Serialize)]
pub struct StyleReport {
    pub total_functions: i64,
    pub avg_function_length: f64,
    pub short_functions: i64,   // <10 lines
    pub medium_functions: i64,  // 10-30 lines
    pub long_functions: i64,    // >30 lines
    pub short_pct: f64,
    pub medium_pct: f64,
    pub long_pct: f64,
    pub trait_count: i64,
    pub struct_count: i64,
    pub abstraction_level: String,  // "low", "moderate", "heavy"
    pub test_functions: i64,
    pub test_ratio: f64,
    pub suggested_max_length: i64,
}

/// Get symbols from a file
pub async fn get_symbols(db: &SqlitePool, req: GetSymbolsRequest) -> anyhow::Result<Vec<serde_json::Value>> {
    let query = r#"
        SELECT id, name, qualified_name, symbol_type, language, start_line, end_line,
               signature, visibility, documentation, is_test, is_async, complexity_score
        FROM code_symbols
        WHERE file_path LIKE $1
          AND ($2 IS NULL OR symbol_type = $2)
        ORDER BY start_line
    "#;

    let file_pattern = format!("%{}", req.file_path);
    let rows = sqlx::query_as::<_, (i64, String, Option<String>, String, Option<String>, i64, i64, Option<String>, Option<String>, Option<String>, bool, bool, Option<f64>)>(query)
        .bind(&file_pattern)
        .bind(&req.symbol_type)
        .fetch_all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(id, name, qualified_name, symbol_type, language, start_line, end_line, signature, visibility, doc, is_test, is_async, complexity)| {
            serde_json::json!({
                "id": id,
                "name": name,
                "qualified_name": qualified_name,
                "type": symbol_type,
                "language": language,
                "start_line": start_line,
                "end_line": end_line,
                "signature": signature,
                "visibility": visibility,
                "documentation": doc,
                "is_test": is_test,
                "is_async": is_async,
                "complexity_score": complexity,
            })
        })
        .collect())
}

/// Get call graph for a symbol
pub async fn get_call_graph(db: &SqlitePool, req: GetCallGraphRequest) -> anyhow::Result<serde_json::Value> {
    let depth = req.depth.unwrap_or(2).min(5); // Cap at 5 to prevent runaway queries

    // Get callers (who calls this symbol) - also search by callee_name for better matching
    let callers_query = r#"
        SELECT DISTINCT caller.name, caller.file_path, caller.symbol_type, cg.call_type, cg.call_line
        FROM call_graph cg
        JOIN code_symbols caller ON cg.caller_id = caller.id
        JOIN code_symbols callee ON cg.callee_id = callee.id
        WHERE callee.name = $1 OR callee.qualified_name LIKE $2 OR cg.callee_name = $1 OR cg.callee_name LIKE $2
        LIMIT 50
    "#;

    let symbol_pattern = format!("%{}", req.symbol);
    let callers = sqlx::query_as::<_, (String, String, String, Option<String>, Option<i32>)>(callers_query)
        .bind(&req.symbol)
        .bind(&symbol_pattern)
        .fetch_all(db)
        .await
        .unwrap_or_default();

    // Get callees (what this symbol calls) - resolved calls
    let callees_query = r#"
        SELECT DISTINCT callee.name, callee.file_path, callee.symbol_type, cg.call_type, cg.call_line
        FROM call_graph cg
        JOIN code_symbols caller ON cg.caller_id = caller.id
        JOIN code_symbols callee ON cg.callee_id = callee.id
        WHERE caller.name = $1 OR caller.qualified_name LIKE $2
        LIMIT 50
    "#;

    let callees = sqlx::query_as::<_, (String, String, String, Option<String>, Option<i32>)>(callees_query)
        .bind(&req.symbol)
        .bind(&symbol_pattern)
        .fetch_all(db)
        .await
        .unwrap_or_default();

    // Get unresolved calls (what this symbol calls but couldn't resolve)
    let unresolved_query = r#"
        SELECT uc.callee_name, uc.call_type, uc.call_line
        FROM unresolved_calls uc
        JOIN code_symbols caller ON uc.caller_id = caller.id
        WHERE caller.name = $1 OR caller.qualified_name LIKE $2
        LIMIT 50
    "#;

    let unresolved = sqlx::query_as::<_, (String, Option<String>, Option<i32>)>(unresolved_query)
        .bind(&req.symbol)
        .bind(&symbol_pattern)
        .fetch_all(db)
        .await
        .unwrap_or_default();

    // Recursive depth traversal for deeper analysis (if depth > 1)
    let mut deeper_calls = Vec::new();
    if depth > 1 {
        for (callee_name, _, _, _, _) in &callees {
            // Get what each callee calls (one level deeper)
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
                deeper_calls.push(serde_json::json!({
                    "name": name,
                    "file": file,
                    "via": via,
                    "depth": 2,
                }));
            }
        }
    }

    Ok(serde_json::json!({
        "symbol": req.symbol,
        "depth": depth,
        "called_by": callers.iter().map(|(name, file, typ, call_type, line)| serde_json::json!({
            "name": name,
            "file": file,
            "type": typ,
            "call_type": call_type,
            "line": line,
        })).collect::<Vec<_>>(),
        "calls": callees.iter().map(|(name, file, typ, call_type, line)| serde_json::json!({
            "name": name,
            "file": file,
            "type": typ,
            "call_type": call_type,
            "line": line,
        })).collect::<Vec<_>>(),
        "unresolved_calls": unresolved.iter().map(|(name, call_type, line)| serde_json::json!({
            "name": name,
            "call_type": call_type,
            "line": line,
            "status": "unresolved",
        })).collect::<Vec<_>>(),
        "deeper_calls": deeper_calls,
    }))
}

/// Get files related to a given file
pub async fn get_related_files(db: &SqlitePool, req: GetRelatedFilesRequest) -> anyhow::Result<serde_json::Value> {
    let limit = req.limit.unwrap_or(10);
    let relation_type = req.relation_type.as_deref().unwrap_or("all");

    let mut imports_result = Vec::new();
    let mut cochange_result = Vec::new();

    // Get imports if requested
    if relation_type == "all" || relation_type == "imports" {
        let imports_query = r#"
            SELECT DISTINCT import_path, is_external
            FROM imports
            WHERE file_path LIKE $1
            LIMIT $2
        "#;

        let file_pattern = format!("%{}", req.file_path);
        let imports = sqlx::query_as::<_, (String, bool)>(imports_query)
            .bind(&file_pattern)
            .bind(limit)
            .fetch_all(db)
            .await
            .unwrap_or_default();

        imports_result = imports.iter().map(|(path, is_external)| {
            serde_json::json!({
                "import_path": path,
                "is_external": is_external,
            })
        }).collect();
    }

    // Get cochange patterns if requested
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

        let cochange = sqlx::query_as::<_, (String, i64, f64)>(cochange_query)
            .bind(&req.file_path)
            .bind(limit)
            .fetch_all(db)
            .await
            .unwrap_or_default();

        cochange_result = cochange.iter().map(|(file, count, conf)| {
            serde_json::json!({
                "file": file,
                "cochange_count": count,
                "confidence": conf,
            })
        }).collect();
    }

    Ok(serde_json::json!({
        "file": req.file_path,
        "imports": imports_result,
        "cochange_patterns": cochange_result,
    }))
}

/// Semantic code search - find code by natural language description
pub async fn semantic_code_search(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: SemanticCodeSearchRequest,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(10) as usize;

    // Try semantic search if available
    if semantic.is_available() {
        let filter = req.language.as_ref().map(|lang| qdrant_client::qdrant::Filter::must([
            qdrant_client::qdrant::Condition::matches("language", lang.clone())
        ]));

        match semantic.search(COLLECTION_CODE, &req.query, limit, filter).await {
            Ok(results) if !results.is_empty() => {
                return Ok(results.into_iter().map(|r| {
                    serde_json::json!({
                        "content": r.content,
                        "score": r.score,
                        "search_type": "semantic",
                        "file_path": r.metadata.get("file_path"),
                        "symbol_name": r.metadata.get("symbol_name"),
                        "symbol_type": r.metadata.get("symbol_type"),
                        "language": r.metadata.get("language"),
                        "start_line": r.metadata.get("start_line"),
                    })
                }).collect());
            }
            Ok(_) => {
                tracing::debug!("No semantic results for code query: {}", req.query);
            }
            Err(e) => {
                tracing::warn!("Semantic code search failed, falling back to text: {}", e);
            }
        }
    }

    // Fallback to SQLite text search on symbol names and documentation
    let search_pattern = format!("%{}%", req.query);

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
        .bind(&req.language)
        .bind(limit as i64)
        .fetch_all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(id, file_path, name, qualified_name, symbol_type, language, start_line, end_line, signature, documentation)| {
            serde_json::json!({
                "id": id,
                "file_path": file_path,
                "symbol_name": name,
                "qualified_name": qualified_name,
                "symbol_type": symbol_type,
                "language": language,
                "start_line": start_line,
                "end_line": end_line,
                "signature": signature,
                "documentation": documentation,
                "search_type": "text",
            })
        })
        .collect())
}

/// Analyze codebase style patterns for a project
pub async fn analyze_codebase_style(db: &SqlitePool, project_path: &str) -> anyhow::Result<StyleReport> {
    let path_pattern = format!("{}%", project_path);

    // 1. Get function length stats
    let length_stats: Option<(i64, f64)> = sqlx::query_as(
        r#"SELECT COUNT(*), AVG(end_line - start_line + 1)
           FROM code_symbols
           WHERE symbol_type = 'function' AND file_path LIKE $1"#
    )
    .bind(&path_pattern)
    .fetch_optional(db)
    .await?;

    let (total_functions, avg_length) = length_stats.unwrap_or((0, 0.0));

    // 2. Get function length distribution
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

    // 3. Count traits/interfaces vs concrete types
    let trait_count: (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*)
           FROM code_symbols
           WHERE symbol_type IN ('trait', 'interface') AND file_path LIKE $1"#
    )
    .bind(&path_pattern)
    .fetch_one(db)
    .await
    .unwrap_or((0,));

    let struct_count: (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*)
           FROM code_symbols
           WHERE symbol_type IN ('struct', 'class') AND file_path LIKE $1"#
    )
    .bind(&path_pattern)
    .fetch_one(db)
    .await
    .unwrap_or((0,));

    // 4. Count test functions
    let test_count: (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*)
           FROM code_symbols
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

    // Suggested max length based on distribution (p75-ish)
    let suggested_max = if long_pct > 20.0 {
        40  // Codebase already has many long functions
    } else if medium_pct > 50.0 {
        30  // Medium-heavy codebase
    } else {
        20  // Prefer shorter functions
    };

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

/// Find improvement opportunities for files
pub async fn find_improvements(
    db: &SqlitePool,
    file_paths: &[String],
    style: &StyleReport,
) -> anyhow::Result<Vec<CodeImprovement>> {
    if file_paths.is_empty() || style.total_functions == 0 {
        return Ok(Vec::new());
    }

    let mut improvements = Vec::new();
    let threshold = style.suggested_max_length;

    // Build placeholders for IN clause
    let placeholders: Vec<String> = file_paths.iter().enumerate()
        .map(|(i, _)| format!("${}", i + 1))
        .collect();
    let in_clause = placeholders.join(", ");

    // Query for long functions (>1.5x suggested max)
    let query = format!(
        r#"SELECT name, file_path, start_line, end_line, (end_line - start_line + 1) as lines, complexity_score
           FROM code_symbols
           WHERE symbol_type = 'function'
             AND file_path IN ({})
             AND (end_line - start_line + 1) > ${}
           ORDER BY (end_line - start_line + 1) DESC
           LIMIT 10"#,
        in_clause,
        file_paths.len() + 1
    );

    let long_threshold = (threshold as f64 * 1.5) as i64;

    let mut query_builder = sqlx::query_as::<_, (String, String, i64, i64, i64, Option<f64>)>(&query);
    for path in file_paths {
        query_builder = query_builder.bind(path);
    }
    query_builder = query_builder.bind(long_threshold);

    let long_functions = query_builder.fetch_all(db).await.unwrap_or_default();

    for (name, file, start_line, _end_line, lines, complexity) in long_functions {
        let severity = if lines > threshold * 2 { "high" } else { "medium" };
        improvements.push(CodeImprovement {
            file_path: file,
            symbol_name: name.clone(),
            improvement_type: "long_function".to_string(),
            current_value: lines,
            threshold,
            severity: severity.to_string(),
            suggestion: format!("Consider splitting this {}-line function (suggested max: {})", lines, threshold),
            start_line,
        });

        // Also flag high complexity if present
        if let Some(cx) = complexity {
            if cx > 10.0 {
                improvements.push(CodeImprovement {
                    file_path: improvements.last().unwrap().file_path.clone(),
                    symbol_name: name,
                    improvement_type: "high_complexity".to_string(),
                    current_value: cx as i64,
                    threshold: 10,
                    severity: if cx > 15.0 { "high" } else { "medium" }.to_string(),
                    suggestion: format!("Complexity score {} is high - consider simplifying", cx as i64),
                    start_line,
                });
            }
        }
    }

    // Deduplicate by (file_path, symbol_name, improvement_type)
    improvements.sort_by(|a, b| {
        (&a.file_path, &a.symbol_name, &a.improvement_type)
            .cmp(&(&b.file_path, &b.symbol_name, &b.improvement_type))
    });
    improvements.dedup_by(|a, b| {
        a.file_path == b.file_path && a.symbol_name == b.symbol_name && a.improvement_type == b.improvement_type
    });

    // Sort by severity (high first), then by current_value descending
    improvements.sort_by(|a, b| {
        let sev_order = |s: &str| match s { "high" => 0, "medium" => 1, _ => 2 };
        sev_order(&a.severity).cmp(&sev_order(&b.severity))
            .then(b.current_value.cmp(&a.current_value))
    });

    Ok(improvements)
}
