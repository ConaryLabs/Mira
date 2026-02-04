// background/code_health/patterns.rs
// Heuristic detection of architectural patterns from symbol/import/naming data

use rusqlite::Connection;

/// A detected architectural pattern
#[derive(Debug, Clone)]
pub struct PatternMatch {
    pub pattern: String,
    pub confidence: f64,
    pub evidence: Vec<String>,
}

impl PatternMatch {
    fn to_json(&self) -> String {
        let evidence_json: Vec<String> =
            self.evidence.iter().map(|e| format!("\"{}\"", e)).collect();
        format!(
            "{{\"pattern\":\"{}\",\"confidence\":{:.2},\"evidence\":[{}]}}",
            self.pattern,
            self.confidence,
            evidence_json.join(",")
        )
    }
}

// ============================================================================
// Pattern Detectors
// ============================================================================

/// Repository pattern: get_*, find_*, create_*, update_*, delete_* methods
fn detect_repository(symbols: &[SymbolInfo]) -> Option<PatternMatch> {
    let crud_prefixes = [
        "get_", "find_", "create_", "update_", "delete_", "list_", "store_", "fetch_",
    ];
    let mut evidence = Vec::new();

    for sym in symbols {
        if sym.symbol_type == "function" || sym.symbol_type == "method" {
            for prefix in &crud_prefixes {
                if sym.name.starts_with(prefix) {
                    evidence.push(sym.name.clone());
                    break;
                }
            }
        }
    }

    // Need at least 3 CRUD-like methods from at least 2 different prefixes
    if evidence.len() >= 3 {
        let prefixes_used: std::collections::HashSet<&str> = evidence
            .iter()
            .filter_map(|name| crud_prefixes.iter().find(|p| name.starts_with(*p)).copied())
            .collect();

        if prefixes_used.len() >= 2 {
            let confidence = (evidence.len() as f64 / 8.0).clamp(0.5, 1.0);
            evidence.truncate(6);
            return Some(PatternMatch {
                pattern: "repository".to_string(),
                confidence,
                evidence,
            });
        }
    }

    None
}

/// Builder pattern: with_*, set_* methods + build()
fn detect_builder(symbols: &[SymbolInfo]) -> Option<PatternMatch> {
    let mut builder_methods = Vec::new();
    let mut has_build = false;
    let mut has_new = false;

    for sym in symbols {
        if sym.symbol_type == "function" || sym.symbol_type == "method" {
            if sym.name.starts_with("with_") || sym.name.starts_with("set_") {
                builder_methods.push(sym.name.clone());
            }
            if sym.name == "build" {
                has_build = true;
            }
            if sym.name == "new" {
                has_new = true;
            }
        }
    }

    if builder_methods.len() >= 2 && (has_build || (has_new && builder_methods.len() >= 3)) {
        let mut evidence = builder_methods;
        if has_build {
            evidence.push("build()".to_string());
        }
        evidence.truncate(6);
        let confidence = if has_build { 0.9 } else { 0.6 };
        return Some(PatternMatch {
            pattern: "builder".to_string(),
            confidence,
            evidence,
        });
    }

    None
}

/// Factory pattern: create_*/new_* functions
fn detect_factory(symbols: &[SymbolInfo]) -> Option<PatternMatch> {
    let mut factory_fns = Vec::new();

    for sym in symbols {
        if sym.symbol_type == "function" {
            if sym.name.starts_with("create_")
                || (sym.name.starts_with("new_") && sym.name != "new")
            {
                factory_fns.push(sym.name.clone());
            }
        }
    }

    if factory_fns.len() >= 2 {
        factory_fns.truncate(6);
        let confidence = (factory_fns.len() as f64 / 5.0).clamp(0.5, 0.9);
        return Some(PatternMatch {
            pattern: "factory".to_string(),
            confidence,
            evidence: factory_fns,
        });
    }

    None
}

/// Singleton pattern: lazy_static/OnceLock + instance()
fn detect_singleton(symbols: &[SymbolInfo], imports: &[String]) -> Option<PatternMatch> {
    let has_lazy = imports
        .iter()
        .any(|i| i.contains("lazy_static") || i.contains("OnceLock") || i.contains("once_cell"));
    let has_instance = symbols
        .iter()
        .any(|s| s.name == "instance" || s.name == "global" || s.name == "get_instance");

    if has_lazy && has_instance {
        let mut evidence = Vec::new();
        if has_lazy {
            evidence.push("lazy_static/OnceLock import".to_string());
        }
        if has_instance {
            evidence.push("instance() method".to_string());
        }
        return Some(PatternMatch {
            pattern: "singleton".to_string(),
            confidence: 0.8,
            evidence,
        });
    }

    None
}

/// Observer pattern: subscribe/on_*/register_* + notify/emit/broadcast
fn detect_observer(symbols: &[SymbolInfo]) -> Option<PatternMatch> {
    let subscribe_names = [
        "subscribe",
        "on_",
        "register_",
        "add_listener",
        "add_handler",
    ];
    let notify_names = ["notify", "emit", "broadcast", "publish", "dispatch"];

    let mut has_subscribe = Vec::new();
    let mut has_notify = Vec::new();

    for sym in symbols {
        for prefix in &subscribe_names {
            if sym.name.starts_with(prefix) || sym.name == *prefix {
                has_subscribe.push(sym.name.clone());
                break;
            }
        }
        for prefix in &notify_names {
            if sym.name.starts_with(prefix) || sym.name == *prefix {
                has_notify.push(sym.name.clone());
                break;
            }
        }
    }

    if !has_subscribe.is_empty() && !has_notify.is_empty() {
        let mut evidence = has_subscribe;
        evidence.extend(has_notify);
        evidence.truncate(6);
        return Some(PatternMatch {
            pattern: "observer".to_string(),
            confidence: 0.8,
            evidence,
        });
    }

    None
}

/// Middleware pattern: module named "middleware" or (Request, Next) signatures
fn detect_middleware(symbols: &[SymbolInfo], module_name: &str) -> Option<PatternMatch> {
    if module_name.contains("middleware") {
        return Some(PatternMatch {
            pattern: "middleware".to_string(),
            confidence: 0.9,
            evidence: vec![format!("module name: {}", module_name)],
        });
    }

    // Check for middleware-like function signatures
    let middleware_fns: Vec<String> = symbols
        .iter()
        .filter(|s| {
            s.signature.as_deref().is_some_and(|sig| {
                (sig.contains("Request") && sig.contains("Next"))
                    || (sig.contains("request") && sig.contains("next"))
            })
        })
        .map(|s| s.name.clone())
        .collect();

    if !middleware_fns.is_empty() {
        return Some(PatternMatch {
            pattern: "middleware".to_string(),
            confidence: 0.7,
            evidence: middleware_fns,
        });
    }

    None
}

/// Handler/Dispatch pattern: handle_* functions or match on enum
fn detect_handler(symbols: &[SymbolInfo]) -> Option<PatternMatch> {
    let handlers: Vec<String> = symbols
        .iter()
        .filter(|s| {
            (s.symbol_type == "function" || s.symbol_type == "method")
                && (s.name.starts_with("handle_")
                    || s.name.starts_with("dispatch_")
                    || s.name.starts_with("process_"))
        })
        .map(|s| s.name.clone())
        .collect();

    if handlers.len() >= 3 {
        let mut evidence = handlers;
        evidence.truncate(6);
        let confidence = (evidence.len() as f64 / 6.0).clamp(0.6, 0.9);
        return Some(PatternMatch {
            pattern: "handler".to_string(),
            confidence,
            evidence,
        });
    }

    None
}

/// Strategy pattern: trait with single core method + multiple implementations
fn detect_strategy(
    conn: &Connection,
    project_id: i64,
    _module_path: &str,
    symbols: &[SymbolInfo],
) -> Option<PatternMatch> {
    // Find traits in this module
    let traits: Vec<&SymbolInfo> = symbols
        .iter()
        .filter(|s| s.symbol_type == "trait")
        .collect();

    if traits.is_empty() {
        return None;
    }

    // For each trait, check if there are multiple impl blocks across files
    for trait_sym in &traits {
        // Count implementations of this trait across the project
        let impl_count = count_trait_implementations(conn, project_id, &trait_sym.name);
        if impl_count >= 2 {
            return Some(PatternMatch {
                pattern: "strategy".to_string(),
                confidence: (impl_count as f64 / 4.0).clamp(0.6, 0.9),
                evidence: vec![
                    format!("trait: {}", trait_sym.name),
                    format!("{} implementations", impl_count),
                ],
            });
        }
    }

    // Also check for traits defined elsewhere but with impls in this module
    let impls: Vec<&SymbolInfo> = symbols
        .iter()
        .filter(|s| s.symbol_type == "impl" && s.name.contains(" for "))
        .collect();

    if impls.len() >= 2 {
        // Multiple trait impls in the same module — possible strategy
        let trait_names: std::collections::HashSet<String> = impls
            .iter()
            .filter_map(|s| s.name.split(" for ").next().map(|t| t.to_string()))
            .collect();

        for trait_name in &trait_names {
            let matching: Vec<String> = impls
                .iter()
                .filter(|s| s.name.starts_with(trait_name.as_str()))
                .map(|s| s.name.clone())
                .collect();

            if matching.len() >= 2 {
                return Some(PatternMatch {
                    pattern: "strategy".to_string(),
                    confidence: 0.7,
                    evidence: matching,
                });
            }
        }
    }

    None
}

// ============================================================================
// Database Helpers
// ============================================================================

struct ModuleBasic {
    module_id: String,
    name: String,
    path: String,
}

struct SymbolInfo {
    name: String,
    symbol_type: String,
    signature: Option<String>,
}

/// Get all modules for a project
fn get_modules(conn: &Connection, project_id: i64) -> Result<Vec<ModuleBasic>, String> {
    let mut stmt = conn
        .prepare("SELECT module_id, name, path FROM codebase_modules WHERE project_id = ?")
        .map_err(|e| e.to_string())?;

    let modules = stmt
        .query_map([project_id], |row| {
            Ok(ModuleBasic {
                module_id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(modules)
}

/// Get symbols for files in a module path
fn get_module_symbols(
    conn: &Connection,
    project_id: i64,
    path: &str,
) -> Result<Vec<SymbolInfo>, String> {
    let pattern = format!("{}%", path);
    let mut stmt = conn
        .prepare(
            "SELECT name, symbol_type, signature FROM code_symbols
             WHERE project_id = ? AND file_path LIKE ?",
        )
        .map_err(|e| e.to_string())?;

    let symbols = stmt
        .query_map(rusqlite::params![project_id, pattern], |row| {
            Ok(SymbolInfo {
                name: row.get(0)?,
                symbol_type: row.get(1)?,
                signature: row.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(symbols)
}

/// Get imports for files in a module path
fn get_module_imports(
    conn: &Connection,
    project_id: i64,
    path: &str,
) -> Result<Vec<String>, String> {
    let pattern = format!("{}%", path);
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT import_path FROM imports
             WHERE project_id = ? AND file_path LIKE ?",
        )
        .map_err(|e| e.to_string())?;

    let imports = stmt
        .query_map(rusqlite::params![project_id, pattern], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(imports)
}

/// Count implementations of a trait across the project
fn count_trait_implementations(conn: &Connection, project_id: i64, trait_name: &str) -> usize {
    let pattern = format!("{}%", trait_name);
    conn.query_row(
        "SELECT COUNT(DISTINCT file_path) FROM code_symbols
         WHERE project_id = ? AND symbol_type = 'impl' AND name LIKE ?",
        rusqlite::params![project_id, pattern],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0) as usize
}

/// Update detected_patterns column on codebase_modules
fn update_module_patterns(
    conn: &Connection,
    project_id: i64,
    module_id: &str,
    patterns_json: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE codebase_modules SET detected_patterns = ? WHERE project_id = ? AND module_id = ?",
        rusqlite::params![patterns_json, project_id, module_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Pattern finding data for cross-pool transfer
pub struct PatternFinding {
    pub key: String,
    pub content: String,
    pub confidence: f64,
}

/// Collect pattern data from code DB (runs pattern detection + stores in codebase_modules).
/// Returns findings to be stored in main DB memory_facts.
pub fn collect_pattern_data(
    conn: &Connection,
    project_id: i64,
) -> Result<Vec<PatternFinding>, String> {
    let modules = get_modules(conn, project_id)?;
    if modules.is_empty() {
        return Ok(Vec::new());
    }

    let mut findings = Vec::new();

    for module in &modules {
        let symbols = get_module_symbols(conn, project_id, &module.path)?;
        let imports = get_module_imports(conn, project_id, &module.path)?;

        if symbols.is_empty() {
            continue;
        }

        let mut patterns = Vec::new();

        if let Some(p) = detect_repository(&symbols) {
            patterns.push(p);
        }
        if let Some(p) = detect_builder(&symbols) {
            patterns.push(p);
        }
        if let Some(p) = detect_factory(&symbols) {
            patterns.push(p);
        }
        if let Some(p) = detect_singleton(&symbols, &imports) {
            patterns.push(p);
        }
        if let Some(p) = detect_observer(&symbols) {
            patterns.push(p);
        }
        if let Some(p) = detect_middleware(&symbols, &module.name) {
            patterns.push(p);
        }
        if let Some(p) = detect_handler(&symbols) {
            patterns.push(p);
        }
        if let Some(p) = detect_strategy(conn, project_id, &module.path, &symbols) {
            patterns.push(p);
        }

        if patterns.is_empty() {
            continue;
        }

        // Store patterns JSON in codebase_modules
        let patterns_json = format!(
            "[{}]",
            patterns
                .iter()
                .map(|p| p.to_json())
                .collect::<Vec<_>>()
                .join(",")
        );
        update_module_patterns(conn, project_id, &module.module_id, &patterns_json)?;

        // Collect findings for main DB
        for pattern in &patterns {
            findings.push(PatternFinding {
                key: format!("health:pattern:{}:{}", module.module_id, pattern.pattern),
                content: format!(
                    "[pattern:{}] Module `{}` matches {} pattern (confidence: {:.0}%). Evidence: {}",
                    pattern.pattern, module.module_id, pattern.pattern,
                    pattern.confidence * 100.0, pattern.evidence.join(", ")
                ),
                confidence: pattern.confidence,
            });
        }
    }

    if !findings.is_empty() {
        tracing::info!(
            "Code health: detected {} architectural patterns across modules",
            findings.len()
        );
    }

    Ok(findings)
}

/// Get modules with detected patterns for the tool handler
pub fn get_all_module_patterns(
    conn: &Connection,
    project_id: i64,
) -> Result<Vec<(String, String, String)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT module_id, name, detected_patterns FROM codebase_modules
             WHERE project_id = ? AND detected_patterns IS NOT NULL AND detected_patterns != ''
             ORDER BY module_id",
        )
        .map_err(|e| e.to_string())?;

    let results = stmt
        .query_map([project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sym(name: &str, sym_type: &str) -> SymbolInfo {
        SymbolInfo {
            name: name.to_string(),
            symbol_type: sym_type.to_string(),
            signature: None,
        }
    }

    fn make_sym_with_sig(name: &str, sym_type: &str, sig: &str) -> SymbolInfo {
        SymbolInfo {
            name: name.to_string(),
            symbol_type: sym_type.to_string(),
            signature: Some(sig.to_string()),
        }
    }

    #[test]
    fn test_detect_repository() {
        let symbols = vec![
            make_sym("get_user", "function"),
            make_sym("create_user", "function"),
            make_sym("delete_user", "function"),
            make_sym("find_by_email", "function"),
        ];

        let result = detect_repository(&symbols);
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.pattern, "repository");
        assert!(p.confidence >= 0.5);
    }

    #[test]
    fn test_detect_repository_insufficient() {
        let symbols = vec![
            make_sym("get_user", "function"),
            make_sym("process", "function"),
        ];

        assert!(detect_repository(&symbols).is_none());
    }

    #[test]
    fn test_detect_builder() {
        let symbols = vec![
            make_sym("new", "function"),
            make_sym("with_name", "method"),
            make_sym("with_age", "method"),
            make_sym("build", "method"),
        ];

        let result = detect_builder(&symbols);
        assert!(result.is_some());
        let p = result.unwrap();
        assert_eq!(p.pattern, "builder");
        assert!(p.confidence >= 0.9);
    }

    #[test]
    fn test_detect_builder_no_build() {
        let symbols = vec![
            make_sym("with_a", "method"),
            make_sym("with_b", "method"),
            make_sym("with_c", "method"),
            make_sym("new", "function"),
        ];

        // Should still detect with 3+ with_ methods + new
        let result = detect_builder(&symbols);
        assert!(result.is_some());
    }

    #[test]
    fn test_detect_factory() {
        let symbols = vec![
            make_sym("create_connection", "function"),
            make_sym("create_pool", "function"),
            make_sym("create_client", "function"),
        ];

        let result = detect_factory(&symbols);
        assert!(result.is_some());
        assert_eq!(result.unwrap().pattern, "factory");
    }

    #[test]
    fn test_detect_singleton() {
        let symbols = vec![make_sym("instance", "function")];
        let imports = vec!["std::sync::OnceLock".to_string()];

        let result = detect_singleton(&symbols, &imports);
        assert!(result.is_some());
        assert_eq!(result.unwrap().pattern, "singleton");
    }

    #[test]
    fn test_detect_observer() {
        let symbols = vec![
            make_sym("subscribe", "method"),
            make_sym("notify", "method"),
        ];

        let result = detect_observer(&symbols);
        assert!(result.is_some());
        assert_eq!(result.unwrap().pattern, "observer");
    }

    #[test]
    fn test_detect_middleware_by_name() {
        let symbols = vec![];
        let result = detect_middleware(&symbols, "auth_middleware");
        assert!(result.is_some());
        assert_eq!(result.unwrap().pattern, "middleware");
    }

    #[test]
    fn test_detect_middleware_by_signature() {
        let symbols = vec![make_sym_with_sig(
            "auth",
            "function",
            "fn auth(req: Request, next: Next) -> Response",
        )];

        let result = detect_middleware(&symbols, "auth");
        assert!(result.is_some());
    }

    #[test]
    fn test_detect_handler() {
        let symbols = vec![
            make_sym("handle_request", "function"),
            make_sym("handle_response", "function"),
            make_sym("handle_error", "function"),
        ];

        let result = detect_handler(&symbols);
        assert!(result.is_some());
        assert_eq!(result.unwrap().pattern, "handler");
    }

    #[test]
    fn test_detect_handler_insufficient() {
        let symbols = vec![
            make_sym("handle_request", "function"),
            make_sym("other", "function"),
        ];

        assert!(detect_handler(&symbols).is_none());
    }

    #[test]
    fn test_pattern_to_json() {
        let p = PatternMatch {
            pattern: "repository".to_string(),
            confidence: 0.85,
            evidence: vec!["get_user".to_string(), "create_user".to_string()],
        };

        let json = p.to_json();
        assert!(json.contains("repository"));
        assert!(json.contains("0.85"));
        assert!(json.contains("get_user"));
    }
}
