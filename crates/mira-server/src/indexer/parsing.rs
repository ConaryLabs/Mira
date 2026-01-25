// crates/mira-server/src/indexer/parsing.rs
// File parsing and symbol extraction

use crate::indexer::chunking::create_semantic_chunks;
use crate::indexer::parsers;
use crate::indexer::types::{FileParseResult, ParsedImport, ParsedSymbol};
use anyhow::{Context, Result};
use std::path::Path;
use tree_sitter::Parser;

pub use parsers::{FunctionCall, Import, Symbol};

/// Create a parser for a given file extension
pub fn create_parser(ext: &str) -> Option<Parser> {
    let mut parser = Parser::new();
    let language = match ext {
        "rs" => tree_sitter_rust::LANGUAGE,
        "py" => tree_sitter_python::LANGUAGE,
        "ts" | "tsx" | "js" | "jsx" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        "go" => tree_sitter_go::LANGUAGE,
        _ => return None,
    };
    parser.set_language(&language.into()).ok()?;
    Some(parser)
}

/// Extract symbols from a single file
pub fn extract_symbols(path: &Path) -> Result<Vec<Symbol>> {
    let (symbols, _, _, _) = extract_all(path)?;
    Ok(symbols)
}

/// Extract symbols, imports, calls, and file content from a single file
pub fn extract_all(path: &Path) -> Result<(Vec<Symbol>, Vec<Import>, Vec<FunctionCall>, String)> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let mut parser = create_parser(ext).ok_or_else(|| anyhow::anyhow!("Unsupported file type"))?;

    let (symbols, imports, calls) = match ext {
        "rs" => parsers::rust::parse(&mut parser, &content)?,
        "py" => parsers::python::parse(&mut parser, &content)?,
        "ts" | "tsx" | "js" | "jsx" => parsers::typescript::parse(&mut parser, &content)?,
        "go" => parsers::go::parse(&mut parser, &content)?,
        _ => return Ok((vec![], vec![], vec![], content)),
    };

    Ok((symbols, imports, calls, content))
}

/// Parse file content directly (for incremental updates)
/// Returns symbols, imports, and content chunks for embedding
pub fn parse_file(content: &str, language: &str) -> Result<FileParseResult> {
    let ext = match language {
        "rust" => "rs",
        "python" => "py",
        "typescript" | "javascript" => "ts",
        "go" => "go",
        _ => return Err(anyhow::anyhow!("Unsupported language: {}", language)),
    };

    let mut parser = create_parser(ext).ok_or_else(|| anyhow::anyhow!("Failed to create parser"))?;

    let (symbols, imports, _) = match ext {
        "rs" => parsers::rust::parse(&mut parser, content)?,
        "py" => parsers::python::parse(&mut parser, content)?,
        "ts" => parsers::typescript::parse(&mut parser, content)?,
        "go" => parsers::go::parse(&mut parser, content)?,
        _ => (vec![], vec![], vec![]),
    };

    // Convert to simplified types
    let parsed_symbols: Vec<ParsedSymbol> = symbols
        .into_iter()
        .map(|s| ParsedSymbol {
            name: s.name,
            kind: s.symbol_type,
            start_line: s.start_line,
            end_line: s.end_line,
            signature: s.signature,
        })
        .collect();

    let parsed_imports: Vec<ParsedImport> = imports
        .into_iter()
        .map(|i| ParsedImport {
            path: i.import_path,
            is_external: i.is_external,
        })
        .collect();

    // AST-aware chunking: chunk at symbol boundaries
    let chunks = create_semantic_chunks(content, &parsed_symbols);

    Ok(FileParseResult {
        symbols: parsed_symbols,
        imports: parsed_imports,
        chunks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // create_parser tests
    // ============================================================================

    #[test]
    fn test_create_parser_rust() {
        let parser = create_parser("rs");
        assert!(parser.is_some());
    }

    #[test]
    fn test_create_parser_python() {
        let parser = create_parser("py");
        assert!(parser.is_some());
    }

    #[test]
    fn test_create_parser_typescript() {
        let parser_ts = create_parser("ts");
        let parser_tsx = create_parser("tsx");
        let parser_js = create_parser("js");
        let parser_jsx = create_parser("jsx");
        assert!(parser_ts.is_some());
        assert!(parser_tsx.is_some());
        assert!(parser_js.is_some());
        assert!(parser_jsx.is_some());
    }

    #[test]
    fn test_create_parser_go() {
        let parser = create_parser("go");
        assert!(parser.is_some());
    }

    #[test]
    fn test_create_parser_unsupported() {
        let parser = create_parser("unknown");
        assert!(parser.is_none());
    }

    // ============================================================================
    // parse_file tests
    // ============================================================================

    #[test]
    fn test_parse_file_rust() {
        let content = "fn main() { println!(\"Hello\"); }";
        let result = parse_file(content, "rust");
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert!(!parsed.symbols.is_empty());
    }

    #[test]
    fn test_parse_file_python() {
        let content = "def hello():\n    print('hello')";
        let result = parse_file(content, "python");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_file_typescript() {
        let content = "function greet() { console.log('hi'); }";
        let result = parse_file(content, "typescript");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_file_go() {
        let content = "package main\nfunc main() {}";
        let result = parse_file(content, "go");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_file_unsupported() {
        let content = "some content";
        let result = parse_file(content, "cobol");
        assert!(result.is_err());
    }
}
