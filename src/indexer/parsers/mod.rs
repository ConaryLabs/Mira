// src/indexer/parsers/mod.rs
// Language-specific code parsers using tree-sitter

pub mod rust;
pub mod python;
pub mod typescript;
pub mod go;

use tree_sitter::Node;
use std::path::PathBuf;

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
