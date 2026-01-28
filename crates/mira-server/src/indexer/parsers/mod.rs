// src/indexer/parsers/mod.rs
// Language-specific code parsers using tree-sitter

pub mod go;
pub mod python;
pub mod rust;
pub mod typescript;

use anyhow::Result;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::PathBuf;
use tree_sitter::{Node, Parser};

// Re-export parser implementations
pub use go::GoParser;
pub use python::PythonParser;
pub use rust::RustParser;
pub use typescript::TypeScriptParser;

/// Trait for language-specific parsers
pub trait LanguageParser: Send + Sync {
    /// Language identifier (e.g., "rust", "python")
    fn language_id(&self) -> &'static str;

    /// File extensions this parser handles (e.g., ["rs"] or ["ts", "tsx", "js", "jsx"])
    fn extensions(&self) -> &'static [&'static str];

    /// Configure a tree-sitter parser with the appropriate language grammar
    fn configure_parser(&self, parser: &mut Parser) -> Result<()>;

    /// Parse source code and extract symbols, imports, and calls
    fn parse(&self, parser: &mut Parser, content: &str) -> Result<ParseResult>;
}

/// Registry of all available language parsers
pub struct ParserRegistry {
    by_extension: HashMap<&'static str, &'static dyn LanguageParser>,
    by_language: HashMap<&'static str, &'static dyn LanguageParser>,
}

impl ParserRegistry {
    /// Look up a parser by file extension
    pub fn by_extension(&self, ext: &str) -> Option<&'static dyn LanguageParser> {
        self.by_extension.get(ext).copied()
    }

    /// Look up a parser by language name
    pub fn by_language(&self, lang: &str) -> Option<&'static dyn LanguageParser> {
        self.by_language.get(lang).copied()
    }

    /// Get all registered parsers
    pub fn all(&self) -> impl Iterator<Item = &'static dyn LanguageParser> {
        self.by_language.values().copied()
    }
}

// Static parser instances
static RUST_PARSER: RustParser = RustParser;
static PYTHON_PARSER: PythonParser = PythonParser;
static TYPESCRIPT_PARSER: TypeScriptParser = TypeScriptParser;
static GO_PARSER: GoParser = GoParser;

/// Global parser registry - use this for all parser lookups
pub static PARSERS: Lazy<ParserRegistry> = Lazy::new(|| {
    let parsers: &[&'static dyn LanguageParser] =
        &[&RUST_PARSER, &PYTHON_PARSER, &TYPESCRIPT_PARSER, &GO_PARSER];

    let mut by_extension = HashMap::new();
    let mut by_language = HashMap::new();

    for parser in parsers {
        by_language.insert(parser.language_id(), *parser);
        for ext in parser.extensions() {
            by_extension.insert(*ext, *parser);
        }
    }

    ParserRegistry {
        by_extension,
        by_language,
    }
});

/// Extracted symbol from source code
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub qualified_name: Option<String>,
    pub symbol_type: String,
    pub language: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: Option<String>,
    pub visibility: Option<String>,
    pub documentation: Option<String>,
    pub is_test: bool,
    pub is_async: bool,
}

/// Extracted import statement
#[derive(Debug, Clone)]
pub struct Import {
    pub import_path: String,
    pub imported_symbols: Option<Vec<String>>,
    pub is_external: bool,
}

/// Extracted function call (for call graph)
#[derive(Debug, Clone)]
pub struct FunctionCall {
    pub caller_name: String,
    pub callee_name: String,
    pub call_line: u32,
    pub call_type: String, // "direct", "method", "async"
}

/// Parsed file data (for parallel processing)
#[derive(Debug)]
pub struct ParsedFile {
    pub path: PathBuf,
    pub content_hash: String,
    pub symbols: Vec<Symbol>,
    pub imports: Vec<Import>,
    pub calls: Vec<FunctionCall>,
}

/// Result of parsing source code
pub type ParseResult = (Vec<Symbol>, Vec<Import>, Vec<FunctionCall>);

/// Helper to extract text from a tree-sitter node
pub fn node_text(node: Node, source: &[u8]) -> String {
    std::str::from_utf8(&source[node.byte_range()])
        .unwrap_or("")
        .to_string()
}
