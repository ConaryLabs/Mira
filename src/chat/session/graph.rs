//! Graph-enhanced context: related files and call graph

use sqlx::sqlite::SqlitePool;
use std::collections::HashSet;

use super::types::{RelatedFile, CallReference, CodeIndexFileHint};

/// Load related files from cochange patterns
/// Files that historically change together with the active files
pub async fn load_related_files(
    db: &SqlitePool,
    active_files: &[String],
    limit: usize,
) -> Vec<RelatedFile> {
    if active_files.is_empty() {
        return Vec::new();
    }

    let mut related: Vec<RelatedFile> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Add active files to seen set to avoid including them
    for f in active_files {
        seen.insert(f.clone());
    }

    for file in active_files.iter().take(3) {
        let rows = sqlx::query_as::<_, (String, i64, f64)>(
            r#"
            SELECT file2 as related_file, cochange_count, confidence
            FROM cochange_patterns
            WHERE file1 LIKE $1
              AND cochange_count >= 2
            ORDER BY confidence DESC, cochange_count DESC
            LIMIT $2
            "#,
        )
        .bind(format!("%{}", file))
        .bind(limit as i64)
        .fetch_all(db)
        .await
        .unwrap_or_default();

        for (file_path, cochange_count, confidence) in rows {
            if seen.contains(&file_path) {
                continue;
            }
            seen.insert(file_path.clone());

            related.push(RelatedFile {
                file_path,
                cochange_count,
                confidence,
            });
        }
    }

    // Sort by confidence and truncate
    related.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    related.truncate(limit);

    related
}

/// Load call graph context for active symbols
/// Shows what functions call or are called by the current focus
pub async fn load_call_context(
    db: &SqlitePool,
    symbols: &[String],
    limit: usize,
) -> Vec<CallReference> {
    if symbols.is_empty() {
        return Vec::new();
    }

    let mut call_refs: Vec<CallReference> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for symbol in symbols.iter().take(5) {
        // Get callers (who calls this symbol)
        let callers = sqlx::query_as::<_, (String, String, Option<String>)>(
            r#"
            SELECT caller.name, caller.file_path, cg.call_type
            FROM call_graph cg
            JOIN code_symbols caller ON cg.caller_id = caller.id
            JOIN code_symbols callee ON cg.callee_id = callee.id
            WHERE callee.name = $1 OR cg.callee_name = $1
            LIMIT 10
            "#,
        )
        .bind(symbol)
        .fetch_all(db)
        .await
        .unwrap_or_default();

        for (name, file_path, call_type) in callers {
            let key = format!("caller:{}:{}", file_path, name);
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);

            call_refs.push(CallReference {
                symbol_name: name,
                file_path,
                direction: "caller".to_string(),
                call_type,
            });
        }

        // Get callees (what this symbol calls)
        let callees = sqlx::query_as::<_, (String, String, Option<String>)>(
            r#"
            SELECT callee.name, callee.file_path, cg.call_type
            FROM call_graph cg
            JOIN code_symbols caller ON cg.caller_id = caller.id
            JOIN code_symbols callee ON cg.callee_id = callee.id
            WHERE caller.name = $1 OR cg.caller_name = $1
            LIMIT 10
            "#,
        )
        .bind(symbol)
        .fetch_all(db)
        .await
        .unwrap_or_default();

        for (name, file_path, call_type) in callees {
            let key = format!("callee:{}:{}", file_path, name);
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);

            call_refs.push(CallReference {
                symbol_name: name,
                file_path,
                direction: "callee".to_string(),
                call_type,
            });
        }
    }

    call_refs.truncate(limit);
    call_refs
}

/// Extract symbol names from code hints for call graph expansion
pub fn extract_symbols_from_hints(hints: &[CodeIndexFileHint]) -> Vec<String> {
    hints
        .iter()
        .flat_map(|h| h.symbols.iter().map(|s| s.name.clone()))
        .take(10)
        .collect()
}
