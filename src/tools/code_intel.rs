// src/tools/code_intel.rs
// Code intelligence tools with semantic search

use sqlx::sqlite::SqlitePool;

use super::semantic::{SemanticSearch, COLLECTION_CODE};
use super::types::*;

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
