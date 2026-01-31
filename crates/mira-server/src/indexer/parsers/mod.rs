// src/indexer/parsers/mod.rs
// Language-specific code parsers using tree-sitter

pub mod go;
pub mod python;
pub mod rust;
pub mod typescript;

use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::sync::LazyLock;
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
pub static PARSERS: LazyLock<ParserRegistry> = LazyLock::new(|| {
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

/// Parsing context that bundles source bytes, language, and result collectors.
///
/// Replaces the 5-7 separate parameters previously passed through every
/// `walk()` call in each language parser.
pub struct ParseContext<'a> {
    pub source: &'a [u8],
    pub language: &'static str,
    pub symbols: Vec<Symbol>,
    pub imports: Vec<Import>,
    pub calls: Vec<FunctionCall>,
}

impl<'a> ParseContext<'a> {
    /// Create a new parse context for the given source code
    pub fn new(source: &'a [u8], language: &'static str) -> Self {
        Self {
            source,
            language,
            symbols: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
        }
    }

    /// Consume the context and return the parse result tuple
    pub fn into_result(self) -> ParseResult {
        (self.symbols, self.imports, self.calls)
    }
}

/// Shared parse implementation for all language parsers.
///
/// Handles the common boilerplate: parse content into AST, create context,
/// call the language-specific walk function, return results.
pub fn default_parse<F>(
    parser: &mut Parser,
    content: &str,
    language: &'static str,
    walk_fn: F,
) -> Result<ParseResult>
where
    F: FnOnce(Node, &mut ParseContext, Option<&str>, Option<&str>),
{
    let tree = parser
        .parse(content, None)
        .ok_or_else(|| anyhow!("Failed to parse {} code", language))?;

    let mut ctx = ParseContext::new(content.as_bytes(), language);
    walk_fn(tree.root_node(), &mut ctx, None, None);
    Ok(ctx.into_result())
}

/// Builder for constructing Symbol structs with sensible defaults.
/// Reduces boilerplate in language-specific extraction functions.
pub struct SymbolBuilder<'a> {
    node: Node<'a>,
    source: &'a [u8],
    language: &'static str,
    name: Option<String>,
    qualified_name: Option<String>,
    symbol_type: &'static str,
    signature: Option<String>,
    visibility: Option<String>,
    documentation: Option<String>,
    is_test: bool,
    is_async: bool,
}

impl<'a> SymbolBuilder<'a> {
    /// Create a new SymbolBuilder for the given node
    pub fn new(node: Node<'a>, source: &'a [u8], language: &'static str) -> Self {
        Self {
            node,
            source,
            language,
            name: None,
            qualified_name: None,
            symbol_type: "unknown",
            signature: None,
            visibility: None,
            documentation: None,
            is_test: false,
            is_async: false,
        }
    }

    /// Set the symbol name from a field
    pub fn name_from_field(mut self, field: &str) -> Self {
        self.name = self.node.field_text(field, self.source);
        self
    }

    /// Set the symbol name directly
    pub fn name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    /// Set the qualified name with optional parent prefix
    pub fn qualified_with_parent(mut self, parent: Option<&str>, separator: &str) -> Self {
        if let Some(name) = &self.name {
            self.qualified_name = Some(match parent {
                Some(p) => format!("{}{}{}", p, separator, name),
                None => name.clone(),
            });
        }
        self
    }

    /// Set the symbol type (function, class, struct, etc.)
    pub fn symbol_type(mut self, t: &'static str) -> Self {
        self.symbol_type = t;
        self
    }

    /// Set signature from a field
    pub fn signature_from_field(mut self, field: &str) -> Self {
        self.signature = self.node.field_text(field, self.source);
        self
    }

    /// Set signature directly
    pub fn signature(mut self, sig: Option<String>) -> Self {
        self.signature = sig;
        self
    }

    /// Set visibility from a child node kind
    pub fn visibility_from_child(mut self, kind: &str) -> Self {
        self.visibility = self.node.find_child_text(kind, self.source);
        self
    }

    /// Set visibility directly
    pub fn visibility(mut self, vis: Option<String>) -> Self {
        self.visibility = vis;
        self
    }

    /// Set documentation
    pub fn documentation(mut self, doc: Option<String>) -> Self {
        self.documentation = doc;
        self
    }

    /// Set is_test flag
    pub fn is_test(mut self, test: bool) -> Self {
        self.is_test = test;
        self
    }

    /// Set is_async flag
    pub fn is_async(mut self, async_: bool) -> Self {
        self.is_async = async_;
        self
    }

    /// Build the Symbol, returning None if name is missing
    pub fn build(self) -> Option<Symbol> {
        let name = self.name?;
        Some(Symbol {
            name: name.clone(),
            qualified_name: self.qualified_name.or(Some(name)),
            symbol_type: self.symbol_type.to_string(),
            language: self.language.to_string(),
            start_line: self.node.start_line(),
            end_line: self.node.end_line(),
            signature: self.signature,
            visibility: self.visibility,
            documentation: self.documentation,
            is_test: self.is_test,
            is_async: self.is_async,
        })
    }
}

/// Helper to extract text from a tree-sitter node
pub fn node_text(node: Node, source: &[u8]) -> String {
    std::str::from_utf8(&source[node.byte_range()])
        .unwrap_or("")
        .to_string()
}

/// Extension trait for tree-sitter Node with common helper methods
pub trait NodeExt<'a> {
    /// Get 1-indexed start line number
    fn start_line(&self) -> u32;

    /// Get 1-indexed end line number
    fn end_line(&self) -> u32;

    /// Get text of a named field child
    fn field_text(&self, field: &str, source: &[u8]) -> Option<String>;

    /// Find first child with given kind and return its text
    fn find_child_text(&self, kind: &str, source: &[u8]) -> Option<String>;

    /// Check if any direct child has the given kind
    fn has_child_kind(&self, kind: &str) -> bool;
}

impl<'a> NodeExt<'a> for Node<'a> {
    fn start_line(&self) -> u32 {
        self.start_position().row as u32 + 1
    }

    fn end_line(&self) -> u32 {
        self.end_position().row as u32 + 1
    }

    fn field_text(&self, field: &str, source: &[u8]) -> Option<String> {
        self.child_by_field_name(field)
            .map(|n| node_text(n, source))
    }

    fn find_child_text(&self, kind: &str, source: &[u8]) -> Option<String> {
        self.children(&mut self.walk())
            .find(|n| n.kind() == kind)
            .map(|n| node_text(n, source))
    }

    fn has_child_kind(&self, kind: &str) -> bool {
        self.children(&mut self.walk()).any(|n| n.kind() == kind)
    }
}
