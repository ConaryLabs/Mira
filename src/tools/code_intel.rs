// src/tools/code_intel.rs
// Code intelligence tools - thin wrapper over core::ops::code_intel

use serde::Serialize;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;

use crate::core::ops::code_intel as core_code;
use crate::core::OpContext;
use super::semantic::SemanticSearch;
use super::types::*;

/// Code improvement suggestion
#[derive(Debug, Clone, Serialize)]
pub struct CodeImprovement {
    pub file_path: String,
    pub symbol_name: String,
    pub improvement_type: String,
    pub current_value: i64,
    pub threshold: i64,
    pub severity: String,
    pub suggestion: String,
    pub start_line: i64,
}

/// Codebase style analysis report (re-export from core)
pub use core_code::StyleReport;

/// Get symbols from a file
pub async fn get_symbols(db: &SqlitePool, req: GetSymbolsRequest) -> anyhow::Result<Vec<serde_json::Value>> {
    let ctx = OpContext::just_db(db.clone());

    let input = core_code::GetSymbolsInput {
        file_path: req.file_path,
        symbol_type: req.symbol_type,
    };

    let symbols = core_code::get_symbols(&ctx, input).await?;

    Ok(symbols.into_iter().map(|s| {
        serde_json::json!({
            "id": s.id,
            "name": s.name,
            "qualified_name": s.qualified_name,
            "type": s.symbol_type,
            "language": s.language,
            "start_line": s.start_line,
            "end_line": s.end_line,
            "signature": s.signature,
            "visibility": s.visibility,
            "documentation": s.documentation,
            "is_test": s.is_test,
            "is_async": s.is_async,
            "complexity_score": s.complexity_score,
        })
    }).collect())
}

/// Get call graph for a symbol
pub async fn get_call_graph(db: &SqlitePool, req: GetCallGraphRequest) -> anyhow::Result<serde_json::Value> {
    let ctx = OpContext::just_db(db.clone());
    let depth = req.depth.unwrap_or(2);

    let input = core_code::GetCallGraphInput {
        symbol: req.symbol.clone(),
        depth,
    };

    let graph = core_code::get_call_graph(&ctx, input).await?;

    Ok(serde_json::json!({
        "symbol": graph.symbol,
        "depth": depth,
        "called_by": graph.called_by.iter().map(|c| serde_json::json!({
            "name": c.name,
            "file": c.file,
            "type": c.symbol_type,
            "call_type": c.call_type,
            "line": c.line,
        })).collect::<Vec<_>>(),
        "calls": graph.calls.iter().map(|c| serde_json::json!({
            "name": c.name,
            "file": c.file,
            "type": c.symbol_type,
            "call_type": c.call_type,
            "line": c.line,
        })).collect::<Vec<_>>(),
        "unresolved_calls": graph.unresolved_calls.iter().map(|c| serde_json::json!({
            "name": c.name,
            "call_type": c.call_type,
            "line": c.line,
            "status": "unresolved",
        })).collect::<Vec<_>>(),
        "deeper_calls": graph.deeper_calls.iter().map(|c| serde_json::json!({
            "name": c.name,
            "file": c.file,
            "via": c.via,
            "depth": c.depth,
        })).collect::<Vec<_>>(),
    }))
}

/// Get files related to a given file
pub async fn get_related_files(db: &SqlitePool, req: GetRelatedFilesRequest) -> anyhow::Result<serde_json::Value> {
    let ctx = OpContext::just_db(db.clone());
    let limit = req.limit.unwrap_or(10);

    let input = core_code::GetRelatedFilesInput {
        file_path: req.file_path.clone(),
        relation_type: req.relation_type,
        limit,
    };

    let related = core_code::get_related_files(&ctx, input).await?;

    Ok(serde_json::json!({
        "file": related.file,
        "imports": related.imports.iter().map(|i| serde_json::json!({
            "import_path": i.import_path,
            "is_external": i.is_external,
        })).collect::<Vec<_>>(),
        "cochange_patterns": related.cochange_patterns.iter().map(|c| serde_json::json!({
            "file": c.file,
            "cochange_count": c.cochange_count,
            "confidence": c.confidence,
        })).collect::<Vec<_>>(),
    }))
}

/// Semantic code search - find code by natural language description
pub async fn semantic_code_search(
    db: &SqlitePool,
    semantic: Arc<SemanticSearch>,
    req: SemanticCodeSearchRequest,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let ctx = OpContext::with_db_and_semantic(db.clone(), semantic);
    let limit = req.limit.unwrap_or(10) as usize;

    let input = core_code::SemanticSearchInput {
        query: req.query,
        language: req.language,
        limit,
    };

    let results = core_code::semantic_code_search(&ctx, input).await?;

    Ok(results.into_iter().map(|r| {
        serde_json::json!({
            "content": r.content,
            "score": r.score,
            "search_type": r.search_type,
            "file_path": r.file_path,
            "symbol_name": r.symbol_name,
            "symbol_type": r.symbol_type,
            "language": r.language,
            "start_line": r.start_line,
        })
    }).collect())
}

/// Analyze codebase style patterns for a project
pub async fn analyze_codebase_style(db: &SqlitePool, project_path: &str) -> anyhow::Result<StyleReport> {
    let ctx = OpContext::just_db(db.clone());
    let report = core_code::analyze_codebase_style(&ctx, project_path).await?;
    Ok(report)
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
            file_path: file.clone(),
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
                    file_path: file,
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

    // Deduplicate
    improvements.sort_by(|a, b| {
        (&a.file_path, &a.symbol_name, &a.improvement_type)
            .cmp(&(&b.file_path, &b.symbol_name, &b.improvement_type))
    });
    improvements.dedup_by(|a, b| {
        a.file_path == b.file_path && a.symbol_name == b.symbol_name && a.improvement_type == b.improvement_type
    });

    // Sort by severity
    improvements.sort_by(|a, b| {
        let sev_order = |s: &str| match s { "high" => 0, "medium" => 1, _ => 2 };
        sev_order(&a.severity).cmp(&sev_order(&b.severity))
            .then(b.current_value.cmp(&a.current_value))
    });

    Ok(improvements)
}
