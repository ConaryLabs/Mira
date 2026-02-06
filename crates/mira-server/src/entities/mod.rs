// crates/mira-server/src/entities/mod.rs
// Heuristic entity extraction for memory recall boosting
//
// Extracts code identifiers, file paths, and crate names from memory content
// using precompiled regexes. Used for entity-based recall ranking boost.

use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// A raw entity extracted from text content
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawEntity {
    pub name: String,
    pub canonical_name: String,
    pub entity_type: EntityType,
}

/// Entity type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum EntityType {
    CodeIdent,
    FilePath,
    CrateName,
}

impl EntityType {
    pub fn as_str(&self) -> &'static str {
        self.into()
    }
}

// Precompiled regexes via LazyLock — compiled once, used many times

/// File paths: word chars, dots, slashes, hyphens ending in known extensions
#[allow(clippy::expect_used)]
static FILE_PATH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\w./\\\-]+\.(rs|ts|js|py|go|toml|json|yaml|yml|md|sql|sh|css|html|tsx|jsx)")
        .expect("valid regex")
});

/// Backtick code references: content inside backticks, min 3 chars
#[allow(clippy::expect_used)]
static BACKTICK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`([^`]{3,})`").expect("valid regex"));

/// CamelCase identifiers: 2+ humps, min 5 chars total
#[allow(clippy::expect_used)]
static CAMEL_CASE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([A-Z][a-z]+(?:[A-Z][a-z0-9]+)+)\b").expect("valid regex"));

/// snake_case identifiers: 2+ segments, min 5 chars total
#[allow(clippy::expect_used)]
static SNAKE_CASE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([a-z][a-z0-9]*(?:_[a-z0-9]+)+)\b").expect("valid regex"));

/// Crate/module names: after `crate`/`use`/`mod` keywords
#[allow(clippy::expect_used)]
static CRATE_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:crate|use|mod)\s+([a-z][a-z0-9_]*(?:::[a-z][a-z0-9_]*)*)")
        .expect("valid regex")
});

/// Trailing `()` or `:line_number` on backtick refs
#[allow(clippy::expect_used)]
static TRAILING_CALL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\(\)|\:\d+)$").expect("valid regex"));

/// Normalize an identifier to canonical form.
///
/// CamelCase → lowercase_snake_case, hyphens → underscores, collapse underscores, trim.
/// Examples:
/// - `DatabasePool` → `database_pool`
/// - `deadpool-sqlite` → `deadpool_sqlite`
/// - `my__thing` → `my_thing`
pub fn normalize_entity(name: &str) -> String {
    // Split CamelCase into words
    let mut result = String::with_capacity(name.len() + 4);
    let mut prev_was_lower = false;

    for ch in name.chars() {
        if ch.is_uppercase() {
            if prev_was_lower {
                result.push('_');
            }
            result.push(ch.to_lowercase().next().unwrap_or(ch));
            prev_was_lower = false;
        } else if ch == '-' || ch == '_' {
            result.push('_');
            prev_was_lower = false;
        } else {
            result.push(ch);
            prev_was_lower = ch.is_lowercase();
        }
    }

    // Collapse multiple underscores
    let mut collapsed = String::with_capacity(result.len());
    let mut prev_underscore = false;
    for ch in result.chars() {
        if ch == '_' {
            if !prev_underscore {
                collapsed.push('_');
            }
            prev_underscore = true;
        } else {
            collapsed.push(ch);
            prev_underscore = false;
        }
    }

    collapsed.trim_matches('_').to_string()
}

/// Extract entities from text content using heuristic regex patterns.
///
/// Returns deduplicated entities by (canonical_name, entity_type).
/// Designed to run in <1ms on typical memory content.
pub fn extract_entities_heuristic(content: &str) -> Vec<RawEntity> {
    let mut seen = HashSet::new();
    let mut entities = Vec::new();

    // 1. File paths
    for cap in FILE_PATH_RE.find_iter(content) {
        let name = cap.as_str().to_string();
        let canonical = normalize_entity(&name);
        let key = (canonical.clone(), EntityType::FilePath);
        if !seen.contains(&key) {
            seen.insert(key);
            entities.push(RawEntity {
                name,
                canonical_name: canonical,
                entity_type: EntityType::FilePath,
            });
        }
    }

    // 2. Backtick code refs
    for cap in BACKTICK_RE.captures_iter(content) {
        if let Some(inner) = cap.get(1) {
            let mut name = inner.as_str().to_string();
            // Trim trailing () or :line_number
            name = TRAILING_CALL_RE.replace(&name, "").to_string();
            if name.len() < 3 {
                continue;
            }
            let canonical = normalize_entity(&name);
            let key = (canonical.clone(), EntityType::CodeIdent);
            if !seen.contains(&key) {
                seen.insert(key);
                entities.push(RawEntity {
                    name,
                    canonical_name: canonical,
                    entity_type: EntityType::CodeIdent,
                });
            }
        }
    }

    // 3. CamelCase identifiers (2+ humps, min 5 chars)
    for cap in CAMEL_CASE_RE.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            let name = m.as_str().to_string();
            if name.len() < 5 {
                continue;
            }
            let canonical = normalize_entity(&name);
            let key = (canonical.clone(), EntityType::CodeIdent);
            if !seen.contains(&key) {
                seen.insert(key);
                entities.push(RawEntity {
                    name,
                    canonical_name: canonical,
                    entity_type: EntityType::CodeIdent,
                });
            }
        }
    }

    // 4. snake_case identifiers (2+ segments, min 5 chars)
    for cap in SNAKE_CASE_RE.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            let name = m.as_str().to_string();
            if name.len() < 5 {
                continue;
            }
            let canonical = normalize_entity(&name);
            let key = (canonical.clone(), EntityType::CodeIdent);
            if !seen.contains(&key) {
                seen.insert(key);
                entities.push(RawEntity {
                    name,
                    canonical_name: canonical,
                    entity_type: EntityType::CodeIdent,
                });
            }
        }
    }

    // 5. Crate names (after crate/use/mod keywords)
    for cap in CRATE_NAME_RE.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            let name = m.as_str().to_string();
            // Take just the first segment for crate name
            let crate_name = name.split("::").next().unwrap_or(&name).to_string();
            let canonical = normalize_entity(&crate_name);
            let key = (canonical.clone(), EntityType::CrateName);
            if !seen.contains(&key) {
                seen.insert(key);
                entities.push(RawEntity {
                    name: crate_name,
                    canonical_name: canonical,
                    entity_type: EntityType::CrateName,
                });
            }
        }
    }

    entities
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Normalization tests ──────────────────────────────────────────

    #[test]
    fn normalize_camel_case() {
        assert_eq!(normalize_entity("DatabasePool"), "database_pool");
        assert_eq!(normalize_entity("MyHttpClient"), "my_http_client");
        // Consecutive uppercase: no separator between individual uppercase chars
        assert_eq!(normalize_entity("HTMLParser"), "htmlparser");
    }

    #[test]
    fn normalize_snake_case_passthrough() {
        assert_eq!(normalize_entity("database_pool"), "database_pool");
        assert_eq!(normalize_entity("my_http_client"), "my_http_client");
    }

    #[test]
    fn normalize_kebab_case() {
        assert_eq!(normalize_entity("deadpool-sqlite"), "deadpool_sqlite");
        assert_eq!(normalize_entity("my-http-client"), "my_http_client");
    }

    #[test]
    fn normalize_collapses_underscores() {
        assert_eq!(normalize_entity("my__thing"), "my_thing");
        assert_eq!(normalize_entity("a___b"), "a_b");
    }

    #[test]
    fn normalize_trims_underscores() {
        assert_eq!(normalize_entity("_leading"), "leading");
        assert_eq!(normalize_entity("trailing_"), "trailing");
        assert_eq!(normalize_entity("_both_"), "both");
    }

    // ─── Extraction tests ─────────────────────────────────────────────

    #[test]
    fn extract_file_paths() {
        let content = "Modified src/db/memory.rs and config.toml for the new feature";
        let entities = extract_entities_heuristic(content);
        let file_paths: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == EntityType::FilePath)
            .collect();
        assert_eq!(file_paths.len(), 2);
        assert!(file_paths.iter().any(|e| e.name == "src/db/memory.rs"));
        assert!(file_paths.iter().any(|e| e.name == "config.toml"));
    }

    #[test]
    fn extract_backtick_refs() {
        let content = "Use `DatabasePool` for all access. Call `store_memory_sync()` to store.";
        let entities = extract_entities_heuristic(content);
        let code_idents: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == EntityType::CodeIdent)
            .collect();
        // DatabasePool and store_memory_sync (trailing () trimmed)
        assert!(
            code_idents
                .iter()
                .any(|e| e.canonical_name == "database_pool")
        );
        assert!(
            code_idents
                .iter()
                .any(|e| e.canonical_name == "store_memory_sync")
        );
    }

    #[test]
    fn extract_backtick_trims_line_number() {
        let content = "See `memory.rs:284` for details";
        let entities = extract_entities_heuristic(content);
        let code_idents: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == EntityType::CodeIdent)
            .collect();
        assert!(code_idents.iter().any(|e| e.name == "memory.rs"));
    }

    #[test]
    fn extract_camel_case_identifiers() {
        let content = "The DatabasePool and StoreMemoryParams structs are important. Not Ab.";
        let entities = extract_entities_heuristic(content);
        let names: Vec<_> = entities.iter().map(|e| e.canonical_name.as_str()).collect();
        assert!(names.contains(&"database_pool"));
        assert!(names.contains(&"store_memory_params"));
        // "Ab" should not be extracted (too short)
        assert!(!names.contains(&"ab"));
    }

    #[test]
    fn extract_snake_case_identifiers() {
        let content = "Call store_memory_sync and recall_semantic_sync for database ops. Not ab_c.";
        let entities = extract_entities_heuristic(content);
        let names: Vec<_> = entities.iter().map(|e| e.canonical_name.as_str()).collect();
        assert!(names.contains(&"store_memory_sync"));
        assert!(names.contains(&"recall_semantic_sync"));
    }

    #[test]
    fn extract_crate_names() {
        let content = "use deadpool_sqlite for pooling. The crate mira_server provides tools.";
        let entities = extract_entities_heuristic(content);
        let crates: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == EntityType::CrateName)
            .collect();
        assert!(crates.iter().any(|e| e.canonical_name == "deadpool_sqlite"));
        assert!(crates.iter().any(|e| e.canonical_name == "mira_server"));
    }

    #[test]
    fn dedup_by_canonical_and_type() {
        // DatabasePool in backticks and as CamelCase should dedup to one code_ident
        let content = "`DatabasePool` uses the DatabasePool pattern";
        let entities = extract_entities_heuristic(content);
        let pool_idents: Vec<_> = entities
            .iter()
            .filter(|e| {
                e.canonical_name == "database_pool" && e.entity_type == EntityType::CodeIdent
            })
            .collect();
        assert_eq!(pool_idents.len(), 1);
    }

    #[test]
    fn same_canonical_different_type_allowed() {
        // A file_path and code_ident can share canonical text
        let content = "See memory.rs file and `memory_rs` ident";
        let entities = extract_entities_heuristic(content);
        // memory.rs is a file_path, memory_rs could be a code_ident from backtick
        let file_paths: Vec<_> = entities
            .iter()
            .filter(|e| e.entity_type == EntityType::FilePath)
            .collect();
        assert!(!file_paths.is_empty());
    }

    #[test]
    fn empty_content_yields_nothing() {
        let entities = extract_entities_heuristic("");
        assert!(entities.is_empty());
    }

    #[test]
    fn short_identifiers_filtered() {
        // Identifiers < 5 chars or < 2 humps/segments should be filtered out
        let content = "The Ab and xy_z items";
        let entities = extract_entities_heuristic(content);
        // "Ab" is too short for CamelCase (< 2 humps), "xy_z" is < 5 chars
        assert!(entities.is_empty());
    }

    #[test]
    fn regex_statics_initialize() {
        // Force all LazyLock regexes to compile
        let _ = FILE_PATH_RE.is_match("test.rs");
        let _ = BACKTICK_RE.is_match("`test`");
        let _ = CAMEL_CASE_RE.is_match("FooBar");
        let _ = SNAKE_CASE_RE.is_match("foo_bar");
        let _ = CRATE_NAME_RE.is_match("use foo");
        let _ = TRAILING_CALL_RE.is_match("foo()");
    }
}
