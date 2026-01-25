// crates/mira-server/src/indexer/mod.rs
// Code indexing for symbol extraction and semantic search

pub mod parsers;
mod batch;
mod chunking;
mod parsing;
mod project;
mod types;

// Re-export public types
pub use types::{CodeChunk, FileParseResult, IndexStats, ParsedImport, ParsedSymbol};

// Re-export parser types
pub use parsers::{FunctionCall, Import, Symbol};

// Re-export parsing functions
pub use parsing::{extract_all, extract_symbols, parse_file};

// Re-export project indexing
pub use project::index_project;

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // IndexStats tests
    // ============================================================================

    #[test]
    fn test_index_stats_default() {
        let stats = IndexStats {
            files: 0,
            symbols: 0,
            chunks: 0,
            errors: 0,
        };
        assert_eq!(stats.files, 0);
        assert_eq!(stats.errors, 0);
    }

    // ============================================================================
    // CodeChunk tests
    // ============================================================================

    #[test]
    fn test_code_chunk_creation() {
        let chunk = CodeChunk {
            content: "fn test() {}".to_string(),
            start_line: 42,
        };
        assert_eq!(chunk.start_line, 42);
        assert!(chunk.content.contains("test"));
    }

    // ============================================================================
    // ParsedSymbol tests
    // ============================================================================

    #[test]
    fn test_parsed_symbol_creation() {
        let sym = ParsedSymbol {
            name: "foo".to_string(),
            kind: "function".to_string(),
            start_line: 1,
            end_line: 10,
            signature: Some("fn foo() -> i32".to_string()),
        };
        assert_eq!(sym.name, "foo");
        assert_eq!(sym.kind, "function");
    }

    // ============================================================================
    // ParsedImport tests
    // ============================================================================

    #[test]
    fn test_parsed_import_creation() {
        let import = ParsedImport {
            path: "std::collections::HashMap".to_string(),
            is_external: false,
        };
        assert!(import.path.contains("HashMap"));
        assert!(!import.is_external);
    }
}
