// context/convention.rs
// Convention-aware context injector
//
// Injects coding conventions for modules Claude is currently working in.
// Uses session context (recent file access) rather than user message text.

use crate::db::pool::DatabasePool;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::working_context::{self, WorkingModule};

/// Maximum characters for convention context output
const MAX_CONVENTION_CHARS: usize = 400;
/// Cache TTL in seconds
const CACHE_TTL_SECS: u64 = 60;

/// Cached convention context
struct CachedConvention {
    /// Cache key: sorted module paths joined by "|"
    key: String,
    /// Formatted convention context
    context: String,
    /// When the cache was populated
    created_at: std::time::Instant,
}

/// Convention-aware context injector
pub struct ConventionInjector {
    pool: Arc<DatabasePool>,
    cache: Mutex<Option<CachedConvention>>,
}

impl ConventionInjector {
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self {
            pool,
            cache: Mutex::new(None),
        }
    }

    /// Inject convention context based on current working modules.
    ///
    /// Returns formatted convention string or empty string if no conventions found.
    pub async fn inject_convention_context(
        &self,
        session_id: &str,
        project_id: Option<i64>,
        project_path: Option<&str>,
    ) -> String {
        let project_id = match project_id {
            Some(id) => id,
            None => return String::new(),
        };

        // Detect working modules from session behavior log
        let session_id = session_id.to_string();
        let project_path_owned = project_path.map(|s| s.to_string());
        let pool = self.pool.clone();

        let modules = match pool
            .interact(move |conn| {
                let modules = working_context::detect_working_modules(
                    conn,
                    project_id,
                    &session_id,
                    project_path_owned.as_deref(),
                );
                Ok::<_, anyhow::Error>(modules)
            })
            .await
        {
            Ok(modules) => modules,
            Err(e) => {
                tracing::debug!("Failed to detect working modules: {}", e);
                return String::new();
            }
        };

        if modules.is_empty() {
            return String::new();
        }

        // Check cache
        let cache_key = make_cache_key(&modules);
        {
            let cache = self.cache.lock().await;
            if let Some(cached) = cache.as_ref() {
                if cached.key == cache_key
                    && cached.created_at.elapsed().as_secs() < CACHE_TTL_SECS
                {
                    return cached.context.clone();
                }
            }
        }

        // Query conventions for working modules
        let module_paths: Vec<String> = modules.iter().map(|m| m.module_path.clone()).collect();
        let pool = self.pool.clone();

        let conventions = match pool
            .interact(move |conn| {
                query_conventions(conn, project_id, &module_paths)
            })
            .await
        {
            Ok(convs) => convs,
            Err(e) => {
                tracing::debug!("Failed to query conventions: {}", e);
                return String::new();
            }
        };

        if conventions.is_empty() {
            return String::new();
        }

        // Format output
        let context = format_conventions(&modules, &conventions);

        // Update cache
        {
            let mut cache = self.cache.lock().await;
            *cache = Some(CachedConvention {
                key: cache_key,
                context: context.clone(),
                created_at: std::time::Instant::now(),
            });
        }

        context
    }
}

/// Convention data row from module_conventions table
#[derive(Debug)]
struct ConventionRow {
    module_path: String,
    module_id: String,
    error_handling: Option<String>,
    test_pattern: Option<String>,
    key_imports: Option<String>,
    naming: Option<String>,
    detected_patterns: Option<String>,
}

/// Query module_conventions for the given module paths
fn query_conventions(
    conn: &rusqlite::Connection,
    project_id: i64,
    module_paths: &[String],
) -> Result<Vec<ConventionRow>, anyhow::Error> {
    if module_paths.is_empty() {
        return Ok(Vec::new());
    }

    // Build parameterized IN clause
    let placeholders: Vec<String> = (0..module_paths.len()).map(|i| format!("?{}", i + 2)).collect();
    let sql = format!(
        "SELECT module_path, module_id, error_handling, test_pattern,
                key_imports, naming, detected_patterns
         FROM module_conventions
         WHERE project_id = ?1 AND module_path IN ({})",
        placeholders.join(", ")
    );

    let mut stmt = conn.prepare(&sql)?;

    // Build params: project_id + module_paths
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(project_id));
    for path in module_paths {
        params.push(Box::new(path.clone()));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(ConventionRow {
                module_path: row.get(0)?,
                module_id: row.get(1)?,
                error_handling: row.get(2)?,
                test_pattern: row.get(3)?,
                key_imports: row.get(4)?,
                naming: row.get(5)?,
                detected_patterns: row.get(6)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(rows)
}

/// Format conventions for injection, respecting MAX_CONVENTION_CHARS limit
fn format_conventions(modules: &[WorkingModule], conventions: &[ConventionRow]) -> String {
    if conventions.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    let primary = &modules[0];

    // Find the primary module's conventions
    let primary_conv = conventions
        .iter()
        .find(|c| c.module_path == primary.module_path);

    if let Some(conv) = primary_conv {
        output.push_str(&format!("Conventions ({}/)", conv.module_id));

        let mut details = Vec::new();
        if let Some(ref eh) = conv.error_handling {
            details.push(format!("- Errors: {}", eh));
        }
        if let Some(ref tp) = conv.test_pattern {
            details.push(format!("- Tests: {}", tp));
        }
        if let Some(ref dp) = conv.detected_patterns {
            // Extract pattern names from JSON array
            if let Some(patterns) = extract_pattern_names(dp) {
                details.push(format!("- Patterns: {}", patterns));
            }
        }
        if let Some(ref ki) = conv.key_imports {
            details.push(format!("- Key imports: {}", ki));
        }
        if let Some(ref nm) = conv.naming {
            details.push(format!("- Naming: {}", nm));
        }

        if details.is_empty() {
            return String::new();
        }

        output.push(':');
        output.push('\n');
        output.push_str(&details.join("\n"));
    }

    // Add secondary modules as one-liners if space permits
    for module in modules.iter().skip(1) {
        if output.len() >= MAX_CONVENTION_CHARS {
            break;
        }
        if let Some(conv) = conventions.iter().find(|c| c.module_path == module.module_path) {
            let summary = make_one_liner(conv);
            if !summary.is_empty() {
                let line = format!("\n{}: {}", conv.module_id, summary);
                if output.len() + line.len() <= MAX_CONVENTION_CHARS {
                    output.push_str(&line);
                }
            }
        }
    }

    // Truncate if still over limit
    if output.len() > MAX_CONVENTION_CHARS {
        output.truncate(MAX_CONVENTION_CHARS - 3);
        output.push_str("...");
    }

    output
}

/// Extract pattern names from detected_patterns JSON array
fn extract_pattern_names(json: &str) -> Option<String> {
    // Simple extraction without pulling in full JSON parsing
    // Format: [{"pattern":"repository","confidence":0.85,...}, ...]
    let mut names = Vec::new();
    for part in json.split("\"pattern\":\"") {
        if names.is_empty() && !json.starts_with("{") {
            // Skip the first split chunk (before first match)
            // But only if it's the initial part
        }
        if let Some(end) = part.find('"') {
            let name = &part[..end];
            if !name.is_empty() && !name.contains('{') && !name.contains('[') {
                names.push(name.to_string());
            }
        }
    }

    if names.is_empty() {
        None
    } else {
        Some(names.join(", "))
    }
}

/// Make a one-line summary for a secondary module
fn make_one_liner(conv: &ConventionRow) -> String {
    let mut parts = Vec::new();
    if let Some(ref eh) = conv.error_handling {
        // Take just the strategy name, not percentages
        if let Some(paren) = eh.find(" (") {
            parts.push(eh[..paren].to_string());
        } else {
            parts.push(eh.clone());
        }
    }
    if let Some(ref dp) = conv.detected_patterns {
        if let Some(patterns) = extract_pattern_names(dp) {
            parts.push(patterns);
        }
    }
    parts.join(", ")
}

/// Create cache key from sorted module paths
fn make_cache_key(modules: &[WorkingModule]) -> String {
    let mut paths: Vec<&str> = modules.iter().map(|m| m.module_path.as_str()).collect();
    paths.sort();
    paths.join("|")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_conventions_empty() {
        let modules = vec![WorkingModule {
            module_path: "src/context".to_string(),
            module_id: "context".to_string(),
        }];
        let conventions: Vec<ConventionRow> = vec![];
        assert!(format_conventions(&modules, &conventions).is_empty());
    }

    #[test]
    fn test_format_conventions_with_data() {
        let modules = vec![WorkingModule {
            module_path: "src/context".to_string(),
            module_id: "context".to_string(),
        }];
        let conventions = vec![ConventionRow {
            module_path: "src/context".to_string(),
            module_id: "context".to_string(),
            error_handling: Some("anyhow::Result (85% of fns)".to_string()),
            test_pattern: Some("tokio::test, inline #[cfg(test)]".to_string()),
            key_imports: Some("tracing, serde, anyhow".to_string()),
            naming: None,
            detected_patterns: Some(
                "[{\"pattern\":\"handler\",\"confidence\":0.80,\"evidence\":[]}]".to_string(),
            ),
        }];

        let result = format_conventions(&modules, &conventions);
        assert!(result.contains("Conventions (context/)"));
        assert!(result.contains("Errors: anyhow"));
        assert!(result.contains("Tests: tokio::test"));
        assert!(result.contains("Key imports: tracing"));
        assert!(result.contains("Patterns: handler"));
    }

    #[test]
    fn test_format_conventions_truncates() {
        let modules = vec![WorkingModule {
            module_path: "src/context".to_string(),
            module_id: "context".to_string(),
        }];
        let conventions = vec![ConventionRow {
            module_path: "src/context".to_string(),
            module_id: "context".to_string(),
            error_handling: Some("a".repeat(200)),
            test_pattern: Some("b".repeat(200)),
            key_imports: Some("c".repeat(200)),
            naming: Some("d".repeat(200)),
            detected_patterns: None,
        }];

        let result = format_conventions(&modules, &conventions);
        assert!(result.len() <= MAX_CONVENTION_CHARS);
    }

    #[test]
    fn test_extract_pattern_names() {
        let json =
            r#"[{"pattern":"repository","confidence":0.85,"evidence":[]},{"pattern":"builder","confidence":0.90,"evidence":[]}]"#;
        let result = extract_pattern_names(json);
        assert!(result.is_some());
        let names = result.unwrap();
        assert!(names.contains("repository"));
        assert!(names.contains("builder"));
    }

    #[test]
    fn test_extract_pattern_names_empty() {
        assert!(extract_pattern_names("[]").is_none());
        assert!(extract_pattern_names("").is_none());
    }

    #[test]
    fn test_make_one_liner() {
        let conv = ConventionRow {
            module_path: "src/db".to_string(),
            module_id: "db".to_string(),
            error_handling: Some("anyhow::Result (90% of fns)".to_string()),
            test_pattern: None,
            key_imports: None,
            naming: None,
            detected_patterns: Some(
                r#"[{"pattern":"repository","confidence":0.7,"evidence":[]}]"#.to_string(),
            ),
        };

        let result = make_one_liner(&conv);
        assert!(result.contains("anyhow::Result"));
        assert!(result.contains("repository"));
        // Should not contain the percentage detail
        assert!(!result.contains("90%"));
    }

    #[test]
    fn test_make_cache_key() {
        let modules = vec![
            WorkingModule {
                module_path: "src/b".to_string(),
                module_id: "b".to_string(),
            },
            WorkingModule {
                module_path: "src/a".to_string(),
                module_id: "a".to_string(),
            },
        ];

        let key = make_cache_key(&modules);
        assert_eq!(key, "src/a|src/b"); // Sorted
    }

    #[test]
    fn test_inject_empty_no_conventions() {
        // No conventions data → empty result
        let modules: Vec<WorkingModule> = vec![];
        let conventions: Vec<ConventionRow> = vec![];
        let result = format_conventions(&modules, &conventions);
        assert!(result.is_empty());
    }
}
