// background/code_health/conventions.rs
// Convention extraction from code index data
//
// Detects coding conventions per module: error handling strategy, test patterns,
// key imports, and naming conventions. Results stored in module_conventions (main DB)
// for use by the convention injector during context injection.

use crate::utils::ResultExt;
use rusqlite::Connection;
use std::collections::HashMap;

/// Convention data collected for a single module
#[derive(Debug, Clone)]
pub struct ModuleConventionData {
    pub module_id: String,
    pub module_path: String,
    pub error_handling: Option<String>,
    pub test_pattern: Option<String>,
    pub key_imports: Option<String>,
    pub naming: Option<String>,
    pub detected_patterns: Option<String>,
    pub confidence: f64,
}

/// Collect convention data from all modules in a project.
/// Runs on the code DB connection.
pub fn collect_convention_data(
    conn: &Connection,
    project_id: i64,
) -> Result<Vec<ModuleConventionData>, String> {
    // Get modules that need convention extraction
    let modules = get_modules_needing_extraction(conn, project_id)?;
    if modules.is_empty() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();

    for module in &modules {
        let chunks = get_module_chunks(conn, project_id, &module.path)?;
        let imports = get_module_imports(conn, project_id, &module.path)?;
        let symbols = get_module_symbols(conn, project_id, &module.path)?;
        let file_count = get_module_file_count(conn, project_id, &module.path)?;

        let error_handling = detect_error_handling(&chunks, &imports);
        let test_pattern = detect_test_patterns(&chunks);
        let key_imports = detect_key_imports(&imports, file_count);
        let naming = detect_naming_conventions(&symbols);

        // Compute confidence based on data richness
        let mut bonus = 0.0;
        if error_handling.is_some() {
            bonus += 0.1;
        }
        if test_pattern.is_some() {
            bonus += 0.05;
        }
        if key_imports.is_some() {
            bonus += 0.05;
        }
        if naming.is_some() {
            bonus += 0.05;
        }
        let confidence = (0.7_f64 + bonus).min(1.0);

        // Skip modules with no conventions detected
        if error_handling.is_none()
            && test_pattern.is_none()
            && key_imports.is_none()
            && naming.is_none()
        {
            // Still store the row so module mapping works, but with low confidence
            results.push(ModuleConventionData {
                module_id: module.module_id.clone(),
                module_path: module.path.clone(),
                error_handling: None,
                test_pattern: None,
                key_imports: None,
                naming: None,
                detected_patterns: module.detected_patterns.clone(),
                confidence: 0.3,
            });
            continue;
        }

        results.push(ModuleConventionData {
            module_id: module.module_id.clone(),
            module_path: module.path.clone(),
            error_handling,
            test_pattern,
            key_imports,
            naming,
            detected_patterns: module.detected_patterns.clone(),
            confidence,
        });
    }

    Ok(results)
}

/// Mark modules as having conventions extracted (update conventions_extracted_at)
pub fn mark_conventions_extracted(
    conn: &Connection,
    project_id: i64,
    module_paths: &[String],
) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "UPDATE codebase_modules SET conventions_extracted_at = datetime('now')
             WHERE project_id = ? AND path = ?",
        )
        .str_err()?;

    for path in module_paths {
        stmt.execute(rusqlite::params![project_id, path])
            .str_err()?;
    }
    Ok(())
}

/// Upsert convention data into the module_conventions table (main DB)
pub fn upsert_module_conventions(
    conn: &Connection,
    project_id: i64,
    data: &[ModuleConventionData],
) -> Result<usize, String> {
    let mut stmt = conn
        .prepare(
            "INSERT INTO module_conventions
                (project_id, module_id, module_path, error_handling, test_pattern,
                 key_imports, naming, detected_patterns, confidence, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))
             ON CONFLICT(project_id, module_path) DO UPDATE SET
                module_id = excluded.module_id,
                error_handling = excluded.error_handling,
                test_pattern = excluded.test_pattern,
                key_imports = excluded.key_imports,
                naming = excluded.naming,
                detected_patterns = excluded.detected_patterns,
                confidence = excluded.confidence,
                updated_at = datetime('now')",
        )
        .str_err()?;

    let mut count = 0;
    for d in data {
        stmt.execute(rusqlite::params![
            project_id,
            d.module_id,
            d.module_path,
            d.error_handling,
            d.test_pattern,
            d.key_imports,
            d.naming,
            d.detected_patterns,
            d.confidence,
        ])
        .str_err()?;
        count += 1;
    }

    Ok(count)
}

// ============================================================================
// Convention Detectors
// ============================================================================

/// Detect error handling strategy by scanning chunk content for return type patterns
fn detect_error_handling(chunks: &[String], imports: &[ImportInfo]) -> Option<String> {
    let mut anyhow_count = 0u32;
    let mut string_err_count = 0u32;
    let mut custom_errors: HashMap<String, u32> = HashMap::new();
    let mut result_fn_count = 0u32;

    for chunk in chunks {
        // Look for -> Result< patterns in chunk content
        for line in chunk.lines() {
            let trimmed = line.trim();
            if !trimmed.contains("-> Result<") && !trimmed.contains("-> anyhow::Result") {
                continue;
            }
            result_fn_count += 1;

            if trimmed.contains("anyhow::Result") || trimmed.contains("anyhow::Error") {
                anyhow_count += 1;
            } else if trimmed.contains("Result<_, String>")
                || trimmed.contains("Result<(), String>")
            {
                string_err_count += 1;
            } else {
                // Try to extract custom error type: Result<_, SomeError>
                if let Some(start) = trimmed.find("Result<") {
                    let after = &trimmed[start..];
                    if let Some(comma) = after.find(", ") {
                        let after_comma = &after[comma + 2..];
                        if let Some(end) = after_comma.find('>') {
                            let err_type = after_comma[..end].trim();
                            if !err_type.is_empty()
                                && err_type != "String"
                                && err_type != "anyhow::Error"
                            {
                                *custom_errors.entry(err_type.to_string()).or_default() += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    // Need at least 3 result-returning functions to be meaningful
    if result_fn_count < 3 {
        return None;
    }

    // Cross-check with imports
    let has_anyhow_import = imports.iter().any(|i| i.path.contains("anyhow"));
    let has_thiserror_import = imports.iter().any(|i| i.path.contains("thiserror"));

    // Determine dominant strategy
    let total = result_fn_count as f64;
    if anyhow_count > 0 && (anyhow_count as f64 / total) > 0.5 || has_anyhow_import {
        let pct = (anyhow_count as f64 / total * 100.0) as u32;
        let suffix = if has_thiserror_import {
            " + thiserror"
        } else {
            ""
        };
        Some(format!("anyhow::Result ({}% of fns){}", pct, suffix))
    } else if string_err_count > 0 && (string_err_count as f64 / total) > 0.5 {
        let pct = (string_err_count as f64 / total * 100.0) as u32;
        Some(format!("string errors ({}% of fns)", pct))
    } else if let Some((err_type, count)) = custom_errors.iter().max_by_key(|(_, c)| *c) {
        let pct = (*count as f64 / total * 100.0) as u32;
        Some(format!("custom: {} ({}% of fns)", err_type, pct))
    } else if has_anyhow_import {
        Some("anyhow (from imports)".to_string())
    } else {
        None
    }
}

/// Detect test patterns by scanning chunk content for test attributes
fn detect_test_patterns(chunks: &[String]) -> Option<String> {
    let mut tokio_test_count = 0u32;
    let mut plain_test_count = 0u32;
    let mut has_cfg_test = false;
    let mut total_test_chunks = 0u32;

    for chunk in chunks {
        let has_test = chunk.contains("#[test]") || chunk.contains("#[tokio::test]");
        if !has_test && !chunk.contains("#[cfg(test)]") {
            continue;
        }
        total_test_chunks += 1;

        if chunk.contains("#[tokio::test]") {
            tokio_test_count += 1;
        }
        if chunk.contains("#[test]") && !chunk.contains("#[tokio::test]") {
            plain_test_count += 1;
        }
        if chunk.contains("#[cfg(test)]") {
            has_cfg_test = true;
        }
    }

    if total_test_chunks == 0 {
        return None;
    }

    let mut parts = Vec::new();
    if tokio_test_count > plain_test_count {
        parts.push("tokio::test");
    } else if plain_test_count > 0 {
        parts.push("#[test]");
    }
    if has_cfg_test {
        parts.push("inline #[cfg(test)]");
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

/// Detect key imports by querying the imports table
/// Only include imports used by ≥40% of files in the module
fn detect_key_imports(imports: &[ImportInfo], file_count: usize) -> Option<String> {
    if file_count < 1 {
        return None;
    }

    // Categorize imports
    let categories: &[(&str, &[&str])] = &[
        ("tracing", &["tracing"]),
        ("serde", &["serde"]),
        ("anyhow", &["anyhow"]),
        ("thiserror", &["thiserror"]),
        ("tokio", &["tokio"]),
        ("rusqlite", &["rusqlite", "deadpool_sqlite", "deadpool"]),
    ];

    let mut category_files: HashMap<&str, std::collections::HashSet<&str>> = HashMap::new();

    for imp in imports {
        for &(cat_name, cat_patterns) in categories {
            if cat_patterns.iter().any(|p| imp.path.contains(p)) {
                category_files
                    .entry(cat_name)
                    .or_default()
                    .insert(&imp.file_path);
            }
        }
    }

    // Filter to categories used by ≥40% of files
    let threshold = (file_count as f64 * 0.4).ceil() as usize;
    let mut notable: Vec<&str> = category_files
        .iter()
        .filter(|(_, files)| files.len() >= threshold)
        .map(|(&name, _)| name)
        .collect();
    notable.sort();

    if notable.len() < 2 {
        return None;
    }

    Some(notable.join(", "))
}

/// Detect naming conventions from symbol names
fn detect_naming_conventions(symbols: &[SymbolBasic]) -> Option<String> {
    let prefixes = ["handle_", "get_", "create_", "with_", "is_", "set_"];
    let mut prefix_counts: HashMap<&str, Vec<&str>> = HashMap::new();

    for sym in symbols {
        if sym.symbol_type != "function" && sym.symbol_type != "method" {
            continue;
        }
        for &prefix in &prefixes {
            if sym.name.starts_with(prefix) {
                prefix_counts.entry(prefix).or_default().push(&sym.name);
                break;
            }
        }
    }

    // Only report prefixes with ≥3 functions
    let mut notable: Vec<(&str, usize)> = prefix_counts
        .iter()
        .filter(|(_, names)| names.len() >= 3)
        .map(|(&prefix, names)| (prefix, names.len()))
        .collect();
    notable.sort_by(|a, b| b.1.cmp(&a.1));

    if notable.is_empty() {
        return None;
    }

    let parts: Vec<String> = notable
        .iter()
        .take(3)
        .map(|(prefix, count)| format!("{}* ({} fns)", prefix, count))
        .collect();

    Some(parts.join(", "))
}

// ============================================================================
// Database Helpers
// ============================================================================

struct ModuleInfo {
    module_id: String,
    path: String,
    detected_patterns: Option<String>,
}

struct ImportInfo {
    file_path: String,
    path: String,
}

struct SymbolBasic {
    name: String,
    symbol_type: String,
}

/// Get modules needing convention extraction (incremental)
fn get_modules_needing_extraction(
    conn: &Connection,
    project_id: i64,
) -> Result<Vec<ModuleInfo>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT module_id, path, detected_patterns FROM codebase_modules
             WHERE project_id = ?
             AND (conventions_extracted_at IS NULL
                  OR updated_at > conventions_extracted_at)",
        )
        .str_err()?;

    let modules = stmt
        .query_map([project_id], |row| {
            Ok(ModuleInfo {
                module_id: row.get(0)?,
                path: row.get(1)?,
                detected_patterns: row.get(2)?,
            })
        })
        .str_err()?
        .filter_map(crate::db::log_and_discard)
        .collect();

    Ok(modules)
}

/// Get chunk content for files in a module
fn get_module_chunks(
    conn: &Connection,
    project_id: i64,
    module_path: &str,
) -> Result<Vec<String>, String> {
    let pattern = format!("{}%", module_path);
    let mut stmt = conn
        .prepare(
            "SELECT chunk_content FROM code_chunks
             WHERE project_id = ? AND file_path LIKE ?",
        )
        .str_err()?;

    let chunks = stmt
        .query_map(rusqlite::params![project_id, pattern], |row| {
            row.get::<_, String>(0)
        })
        .str_err()?
        .filter_map(crate::db::log_and_discard)
        .collect();

    Ok(chunks)
}

/// Get imports for files in a module
fn get_module_imports(
    conn: &Connection,
    project_id: i64,
    module_path: &str,
) -> Result<Vec<ImportInfo>, String> {
    let pattern = format!("{}%", module_path);
    let mut stmt = conn
        .prepare(
            "SELECT file_path, import_path FROM imports
             WHERE project_id = ? AND file_path LIKE ?",
        )
        .str_err()?;

    let imports = stmt
        .query_map(rusqlite::params![project_id, pattern], |row| {
            Ok(ImportInfo {
                file_path: row.get(0)?,
                path: row.get(1)?,
            })
        })
        .str_err()?
        .filter_map(crate::db::log_and_discard)
        .collect();

    Ok(imports)
}

/// Get symbols for files in a module
fn get_module_symbols(
    conn: &Connection,
    project_id: i64,
    module_path: &str,
) -> Result<Vec<SymbolBasic>, String> {
    let pattern = format!("{}%", module_path);
    let mut stmt = conn
        .prepare(
            "SELECT name, symbol_type FROM code_symbols
             WHERE project_id = ? AND file_path LIKE ?",
        )
        .str_err()?;

    let symbols = stmt
        .query_map(rusqlite::params![project_id, pattern], |row| {
            Ok(SymbolBasic {
                name: row.get(0)?,
                symbol_type: row.get(1)?,
            })
        })
        .str_err()?
        .filter_map(crate::db::log_and_discard)
        .collect();

    Ok(symbols)
}

/// Count distinct files in a module
fn get_module_file_count(
    conn: &Connection,
    project_id: i64,
    module_path: &str,
) -> Result<usize, String> {
    let pattern = format!("{}%", module_path);
    conn.query_row(
        "SELECT COUNT(DISTINCT file_path) FROM imports WHERE project_id = ? AND file_path LIKE ?",
        rusqlite::params![project_id, pattern],
        |row| row.get::<_, i64>(0),
    )
    .map(|c| c as usize)
    .str_err()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chunk(content: &str) -> String {
        content.to_string()
    }

    fn make_import(file: &str, path: &str) -> ImportInfo {
        ImportInfo {
            file_path: file.to_string(),
            path: path.to_string(),
        }
    }

    fn make_symbol(name: &str, sym_type: &str) -> SymbolBasic {
        SymbolBasic {
            name: name.to_string(),
            symbol_type: sym_type.to_string(),
        }
    }

    #[test]
    fn test_detect_error_handling_anyhow() {
        let chunks = vec![
            make_chunk("pub fn foo() -> anyhow::Result<()> { Ok(()) }"),
            make_chunk("pub fn bar() -> anyhow::Result<String> { Ok(\"hi\".into()) }"),
            make_chunk("pub fn baz() -> anyhow::Result<i64> { Ok(42) }"),
            make_chunk("pub fn qux() -> Result<_, String> { Ok(()) }"),
        ];
        let imports = vec![make_import("src/lib.rs", "anyhow")];

        let result = detect_error_handling(&chunks, &imports);
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(r.contains("anyhow"), "Expected 'anyhow' in: {}", r);
    }

    #[test]
    fn test_detect_error_handling_string() {
        let chunks = vec![
            make_chunk("fn a() -> Result<(), String> { Ok(()) }"),
            make_chunk("fn b() -> Result<_, String> { Ok(1) }"),
            make_chunk("fn c() -> Result<(), String> { Ok(()) }"),
        ];
        let imports = vec![];

        let result = detect_error_handling(&chunks, &imports);
        assert!(result.is_some());
        assert!(result.unwrap().contains("string errors"));
    }

    #[test]
    fn test_detect_error_handling_sparse() {
        // Less than 3 result-returning functions → None
        let chunks = vec![
            make_chunk("fn a() -> Result<(), String> { Ok(()) }"),
            make_chunk("fn b() -> i64 { 42 }"),
        ];
        let imports = vec![];

        assert!(detect_error_handling(&chunks, &imports).is_none());
    }

    #[test]
    fn test_detect_test_pattern_tokio() {
        let chunks = vec![
            make_chunk("#[tokio::test]\nasync fn test_foo() {}"),
            make_chunk("#[tokio::test]\nasync fn test_bar() {}"),
            make_chunk("#[cfg(test)]\nmod tests {}"),
        ];

        let result = detect_test_patterns(&chunks);
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(
            r.contains("tokio::test"),
            "Expected 'tokio::test' in: {}",
            r
        );
        assert!(r.contains("inline #[cfg(test)]"));
    }

    #[test]
    fn test_detect_test_pattern_plain() {
        let chunks = vec![
            make_chunk("#[test]\nfn test_foo() {}"),
            make_chunk("#[test]\nfn test_bar() {}"),
        ];

        let result = detect_test_patterns(&chunks);
        assert!(result.is_some());
        assert!(result.unwrap().contains("#[test]"));
    }

    #[test]
    fn test_detect_test_pattern_none() {
        let chunks = vec![make_chunk("fn foo() -> i64 { 42 }")];
        assert!(detect_test_patterns(&chunks).is_none());
    }

    #[test]
    fn test_detect_key_imports() {
        let imports = vec![
            make_import("src/a.rs", "tracing::info"),
            make_import("src/b.rs", "tracing::debug"),
            make_import("src/a.rs", "serde::Serialize"),
            make_import("src/b.rs", "serde::Deserialize"),
            make_import("src/a.rs", "anyhow::Result"),
            make_import("src/b.rs", "anyhow::Context"),
        ];

        let result = detect_key_imports(&imports, 2);
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(r.contains("tracing"), "Expected 'tracing' in: {}", r);
        assert!(r.contains("serde"), "Expected 'serde' in: {}", r);
        assert!(r.contains("anyhow"), "Expected 'anyhow' in: {}", r);
    }

    #[test]
    fn test_detect_key_imports_sparse() {
        // Only one notable import category → None (needs ≥2)
        let imports = vec![make_import("src/a.rs", "tracing::info")];
        assert!(detect_key_imports(&imports, 2).is_none());
    }

    #[test]
    fn test_detect_naming_convention() {
        let symbols = vec![
            make_symbol("handle_request", "function"),
            make_symbol("handle_response", "function"),
            make_symbol("handle_error", "function"),
            make_symbol("handle_timeout", "function"),
            make_symbol("new", "function"),
        ];

        let result = detect_naming_conventions(&symbols);
        assert!(result.is_some());
        let r = result.unwrap();
        assert!(r.contains("handle_*"), "Expected 'handle_*' in: {}", r);
        assert!(r.contains("4 fns"), "Expected '4 fns' in: {}", r);
    }

    #[test]
    fn test_detect_naming_convention_sparse() {
        // Less than 3 functions with same prefix → None
        let symbols = vec![
            make_symbol("handle_a", "function"),
            make_symbol("handle_b", "function"),
            make_symbol("foo", "function"),
        ];

        assert!(detect_naming_conventions(&symbols).is_none());
    }

    #[test]
    fn test_no_conventions_sparse_module() {
        let chunks: Vec<String> = vec![];
        let imports: Vec<ImportInfo> = vec![];
        let symbols: Vec<SymbolBasic> = vec![];

        assert!(detect_error_handling(&chunks, &imports).is_none());
        assert!(detect_test_patterns(&chunks).is_none());
        assert!(detect_key_imports(&imports, 0).is_none());
        assert!(detect_naming_conventions(&symbols).is_none());
    }
}
