// crates/mira-server/src/indexer/parsing.rs
// File parsing and symbol extraction

use crate::indexer::chunking::create_semantic_chunks;
use crate::indexer::parsers::{self, PARSERS};
use crate::indexer::types::{FileParseResult, ParsedCall, ParsedImport, ParsedSymbol};
use anyhow::{Context, Result};
use std::path::Path;
use tree_sitter::Parser;

pub use parsers::{FunctionCall, Import, LanguageParser, Symbol};

/// Extracted data from a single file: (symbols, imports, function_calls, file_content)
pub type ExtractedFileData = (Vec<Symbol>, Vec<Import>, Vec<FunctionCall>, String);

/// Extract symbols from a single file
pub fn extract_symbols(path: &Path) -> Result<Vec<Symbol>> {
    let (symbols, _, _, _) = extract_all(path)?;
    Ok(symbols)
}

/// Extract symbols, imports, calls, and file content from a single file
pub fn extract_all(path: &Path) -> Result<ExtractedFileData> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let lang_parser = PARSERS
        .by_extension(ext)
        .ok_or_else(|| anyhow::anyhow!("Unsupported file type: {}", ext))?;

    let mut parser = Parser::new();
    lang_parser.configure_parser(&mut parser)?;
    let (symbols, imports, calls) = lang_parser.parse(&mut parser, &content)?;

    Ok((symbols, imports, calls, content))
}

/// Parse file content directly (for incremental updates)
/// Returns symbols, imports, and content chunks for embedding
pub fn parse_file(content: &str, language: &str) -> Result<FileParseResult> {
    let lang_parser = PARSERS
        .by_language(language)
        .ok_or_else(|| anyhow::anyhow!("Unsupported language: {}", language))?;

    let mut parser = Parser::new();
    lang_parser.configure_parser(&mut parser)?;
    let (symbols, imports, calls) = lang_parser.parse(&mut parser, content)?;

    // Convert to simplified types
    let parsed_symbols: Vec<ParsedSymbol> = symbols
        .into_iter()
        .map(ParsedSymbol::from)
        .collect();

    let parsed_imports: Vec<ParsedImport> = imports
        .into_iter()
        .map(|i| ParsedImport {
            path: i.import_path,
            is_external: i.is_external,
        })
        .collect();

    let parsed_calls: Vec<ParsedCall> = calls
        .into_iter()
        .map(|c| ParsedCall {
            caller_name: c.caller_name,
            callee_name: c.callee_name,
            call_line: c.call_line,
        })
        .collect();

    // AST-aware chunking: chunk at symbol boundaries
    let chunks = create_semantic_chunks(content, &parsed_symbols);

    Ok(FileParseResult {
        symbols: parsed_symbols,
        imports: parsed_imports,
        calls: parsed_calls,
        chunks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // PARSERS registry tests
    // ============================================================================

    #[test]
    fn test_registry_by_extension_rust() {
        assert!(PARSERS.by_extension("rs").is_some());
    }

    #[test]
    fn test_registry_by_extension_python() {
        assert!(PARSERS.by_extension("py").is_some());
    }

    #[test]
    fn test_registry_by_extension_typescript() {
        assert!(PARSERS.by_extension("ts").is_some());
        assert!(PARSERS.by_extension("tsx").is_some());
        assert!(PARSERS.by_extension("js").is_some());
        assert!(PARSERS.by_extension("jsx").is_some());
    }

    #[test]
    fn test_registry_by_extension_go() {
        assert!(PARSERS.by_extension("go").is_some());
    }

    #[test]
    fn test_registry_by_extension_unsupported() {
        assert!(PARSERS.by_extension("unknown").is_none());
    }

    #[test]
    fn test_registry_by_language() {
        assert!(PARSERS.by_language("rust").is_some());
        assert!(PARSERS.by_language("python").is_some());
        assert!(PARSERS.by_language("typescript").is_some());
        assert!(PARSERS.by_language("go").is_some());
        assert!(PARSERS.by_language("cobol").is_none());
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
        let content = "def hello():\n    print('hello')\n\nclass Greeter:\n    def greet(self):\n        pass";
        let result = parse_file(content, "python");
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert!(
            !parsed.symbols.is_empty(),
            "Python parser should extract symbols from def/class, got empty"
        );
        let names: Vec<&str> = parsed.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"hello"),
            "Python parser should find 'hello' function, got: {:?}",
            names
        );
    }

    #[test]
    fn test_parse_file_typescript() {
        let content = "function greet() { console.log('hi'); }\nconst x = 42;";
        let result = parse_file(content, "typescript");
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert!(
            !parsed.symbols.is_empty(),
            "TypeScript parser should extract symbols from function declarations, got empty"
        );
        let names: Vec<&str> = parsed.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"greet"),
            "TypeScript parser should find 'greet' function, got: {:?}",
            names
        );
    }

    #[test]
    fn test_parse_file_go() {
        let content = "package main\n\nfunc main() {}\n\nfunc helper() {}";
        let result = parse_file(content, "go");
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert!(
            !parsed.symbols.is_empty(),
            "Go parser should extract symbols from func declarations, got empty"
        );
        let names: Vec<&str> = parsed.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"main"),
            "Go parser should find 'main' function, got: {:?}",
            names
        );
    }

    #[test]
    fn test_parse_file_unsupported() {
        let content = "some content";
        let result = parse_file(content, "cobol");
        assert!(result.is_err());
    }
}
