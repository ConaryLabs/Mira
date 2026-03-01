// crates/mira-server/src/indexer/types.rs
// Public types for the indexer module

use crate::indexer::parsers::Symbol;
use std::collections::HashMap;

/// Index statistics
pub struct IndexStats {
    pub files: usize,
    pub symbols: usize,
    pub chunks: usize,
    pub errors: usize,
    pub skipped: usize,
    /// Files skipped due to unsupported extension, grouped by extension (e.g. ".java" -> 45)
    pub skipped_by_extension: HashMap<String, usize>,
}

/// A code chunk with content and location info
pub struct CodeChunk {
    pub content: String,
    pub start_line: u32,
}

/// Result of parsing file content for incremental updates
pub struct FileParseResult {
    pub symbols: Vec<ParsedSymbol>,
    pub imports: Vec<ParsedImport>,
    pub calls: Vec<ParsedCall>,
    pub chunks: Vec<CodeChunk>,
}

/// Simplified symbol for incremental indexing
pub struct ParsedSymbol {
    pub name: String,
    pub kind: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: Option<String>,
}

impl From<Symbol> for ParsedSymbol {
    fn from(s: Symbol) -> Self {
        Self {
            name: s.name,
            kind: s.symbol_type,
            start_line: s.start_line,
            end_line: s.end_line,
            signature: s.signature,
        }
    }
}

impl From<&Symbol> for ParsedSymbol {
    fn from(s: &Symbol) -> Self {
        Self {
            name: s.name.clone(),
            kind: s.symbol_type.clone(),
            start_line: s.start_line,
            end_line: s.end_line,
            signature: s.signature.clone(),
        }
    }
}

/// Simplified import for incremental indexing
pub struct ParsedImport {
    pub path: String,
    pub is_external: bool,
}

/// Simplified function call for incremental indexing
pub struct ParsedCall {
    pub caller_name: String,
    pub callee_name: String,
    pub call_line: u32,
}
