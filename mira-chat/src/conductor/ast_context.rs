//! AST Context - Extract function/class outlines for context budgeting
//!
//! Instead of sending full files, send structured outlines that fit
//! within the 12k context budget. Expand only what's needed.

use std::path::Path;

/// An outline of a source file
#[derive(Debug, Clone)]
pub struct FileOutline {
    pub path: String,
    pub language: Language,
    pub imports: Vec<String>,
    pub symbols: Vec<Symbol>,
    pub total_lines: usize,
}

/// A symbol (function, class, struct, etc.)
#[derive(Debug, Clone)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub name: String,
    pub signature: String,
    pub start_line: usize,
    pub end_line: usize,
    pub doc_comment: Option<String>,
    pub children: Vec<Symbol>,
}

/// Types of symbols
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Trait,
    Interface,
    Impl,
    Module,
    Constant,
    Variable,
}

/// Programming language
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    Unknown,
}

impl Language {
    /// Detect language from file extension
    pub fn from_path(path: &str) -> Self {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        match ext {
            "rs" => Self::Rust,
            "py" => Self::Python,
            "js" | "jsx" | "mjs" => Self::JavaScript,
            "ts" | "tsx" => Self::TypeScript,
            "go" => Self::Go,
            "java" => Self::Java,
            _ => Self::Unknown,
        }
    }
}

impl FileOutline {
    /// Extract an outline from source code
    pub fn extract(path: &str, content: &str) -> Self {
        let language = Language::from_path(path);
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let (imports, symbols) = match language {
            Language::Rust => extract_rust(&lines),
            Language::Python => extract_python(&lines),
            Language::JavaScript | Language::TypeScript => extract_js_ts(&lines),
            Language::Go => extract_go(&lines),
            _ => (Vec::new(), Vec::new()),
        };

        Self {
            path: path.into(),
            language,
            imports,
            symbols,
            total_lines,
        }
    }

    /// Format outline for context prompt
    pub fn format(&self, max_tokens: usize) -> String {
        let mut output = String::new();
        let mut current_tokens = 0;
        let tokens_per_char = 0.25; // Rough estimate

        output.push_str(&format!("// File: {} ({} lines)\n", self.path, self.total_lines));
        current_tokens += estimate_tokens(&output);

        // Add imports summary
        if !self.imports.is_empty() {
            let import_summary = if self.imports.len() <= 5 {
                self.imports.join(", ")
            } else {
                format!(
                    "{} and {} more",
                    self.imports[..3].join(", "),
                    self.imports.len() - 3
                )
            };
            output.push_str(&format!("// Imports: {}\n\n", import_summary));
            current_tokens += estimate_tokens(&import_summary) + 10;
        }

        // Add symbols
        for symbol in &self.symbols {
            let symbol_text = format_symbol(symbol, 0);
            let symbol_tokens = estimate_tokens(&symbol_text);

            if current_tokens + symbol_tokens > max_tokens {
                output.push_str("// ... (more symbols truncated)\n");
                break;
            }

            output.push_str(&symbol_text);
            output.push('\n');
            current_tokens += symbol_tokens;
        }

        output
    }

    /// Get a specific symbol by name
    pub fn find_symbol(&self, name: &str) -> Option<&Symbol> {
        for symbol in &self.symbols {
            if symbol.name == name {
                return Some(symbol);
            }
            // Check children
            for child in &symbol.children {
                if child.name == name {
                    return Some(child);
                }
            }
        }
        None
    }

    /// Get line range for a symbol
    pub fn symbol_range(&self, name: &str) -> Option<(usize, usize)> {
        self.find_symbol(name).map(|s| (s.start_line, s.end_line))
    }

    /// Estimate token count for the outline
    pub fn estimated_tokens(&self) -> usize {
        let text = self.format(usize::MAX);
        estimate_tokens(&text)
    }
}

/// Format a symbol for display
fn format_symbol(symbol: &Symbol, indent: usize) -> String {
    let prefix = "  ".repeat(indent);
    let mut output = String::new();

    // Doc comment
    if let Some(ref doc) = symbol.doc_comment {
        for line in doc.lines() {
            output.push_str(&format!("{}/// {}\n", prefix, line.trim()));
        }
    }

    // Symbol signature
    output.push_str(&format!(
        "{}{}  // L{}-{}\n",
        prefix, symbol.signature, symbol.start_line, symbol.end_line
    ));

    // Children (methods in impl, etc.)
    for child in &symbol.children {
        output.push_str(&format_symbol(child, indent + 1));
    }

    output
}

/// Estimate token count (rough approximation)
fn estimate_tokens(text: &str) -> usize {
    // ~4 chars per token for code
    (text.len() + 3) / 4
}

// ============================================================================
// Language-specific extractors
// ============================================================================

fn extract_rust(lines: &[&str]) -> (Vec<String>, Vec<Symbol>) {
    let mut imports = Vec::new();
    let mut symbols = Vec::new();
    let mut doc_buffer = Vec::new();
    let mut in_impl = false;
    let mut impl_symbol: Option<Symbol> = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let line_num = i + 1;

        // Track doc comments
        if trimmed.starts_with("///") || trimmed.starts_with("//!") {
            doc_buffer.push(trimmed.trim_start_matches('/').trim().to_string());
            continue;
        }

        // Imports
        if trimmed.starts_with("use ") {
            imports.push(trimmed.to_string());
            doc_buffer.clear();
            continue;
        }

        // Functions
        if let Some(sig) = extract_rust_fn(trimmed) {
            let end = find_block_end(lines, i);
            let symbol = Symbol {
                kind: if in_impl { SymbolKind::Method } else { SymbolKind::Function },
                name: extract_fn_name(&sig).into(),
                signature: sig,
                start_line: line_num,
                end_line: end,
                doc_comment: if doc_buffer.is_empty() {
                    None
                } else {
                    Some(doc_buffer.join("\n"))
                },
                children: Vec::new(),
            };

            if in_impl {
                if let Some(ref mut impl_sym) = impl_symbol {
                    impl_sym.children.push(symbol);
                }
            } else {
                symbols.push(symbol);
            }
            doc_buffer.clear();
        }

        // Structs
        if trimmed.starts_with("pub struct ") || trimmed.starts_with("struct ") {
            let end = find_block_end(lines, i);
            let name = trimmed
                .trim_start_matches("pub ")
                .trim_start_matches("struct ")
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("?");

            symbols.push(Symbol {
                kind: SymbolKind::Struct,
                name: name.into(),
                signature: trimmed.to_string(),
                start_line: line_num,
                end_line: end,
                doc_comment: if doc_buffer.is_empty() {
                    None
                } else {
                    Some(doc_buffer.join("\n"))
                },
                children: Vec::new(),
            });
            doc_buffer.clear();
        }

        // Enums
        if trimmed.starts_with("pub enum ") || trimmed.starts_with("enum ") {
            let end = find_block_end(lines, i);
            let name = trimmed
                .trim_start_matches("pub ")
                .trim_start_matches("enum ")
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("?");

            symbols.push(Symbol {
                kind: SymbolKind::Enum,
                name: name.into(),
                signature: trimmed.to_string(),
                start_line: line_num,
                end_line: end,
                doc_comment: if doc_buffer.is_empty() {
                    None
                } else {
                    Some(doc_buffer.join("\n"))
                },
                children: Vec::new(),
            });
            doc_buffer.clear();
        }

        // Traits
        if trimmed.starts_with("pub trait ") || trimmed.starts_with("trait ") {
            let end = find_block_end(lines, i);
            let name = trimmed
                .trim_start_matches("pub ")
                .trim_start_matches("trait ")
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("?");

            symbols.push(Symbol {
                kind: SymbolKind::Trait,
                name: name.into(),
                signature: trimmed.to_string(),
                start_line: line_num,
                end_line: end,
                doc_comment: if doc_buffer.is_empty() {
                    None
                } else {
                    Some(doc_buffer.join("\n"))
                },
                children: Vec::new(),
            });
            doc_buffer.clear();
        }

        // Impl blocks
        if trimmed.starts_with("impl ") || trimmed.starts_with("impl<") {
            let end = find_block_end(lines, i);
            let impl_name = extract_impl_name(trimmed);

            in_impl = true;
            impl_symbol = Some(Symbol {
                kind: SymbolKind::Impl,
                name: impl_name.into(),
                signature: trimmed.to_string(),
                start_line: line_num,
                end_line: end,
                doc_comment: None,
                children: Vec::new(),
            });
            doc_buffer.clear();
        }

        // End of impl
        if in_impl && trimmed == "}" {
            let should_end = impl_symbol
                .as_ref()
                .map(|s| line_num >= s.end_line)
                .unwrap_or(false);

            if should_end {
                if let Some(sym) = impl_symbol.take() {
                    symbols.push(sym);
                }
                in_impl = false;
            }
        }
    }

    // Add remaining impl if any
    if let Some(sym) = impl_symbol {
        symbols.push(sym);
    }

    (imports, symbols)
}

fn extract_rust_fn(line: &str) -> Option<String> {
    if (line.starts_with("pub fn ")
        || line.starts_with("fn ")
        || line.starts_with("pub async fn ")
        || line.starts_with("async fn ")
        || line.starts_with("pub const fn ")
        || line.starts_with("const fn ")
        || line.starts_with("pub unsafe fn ")
        || line.starts_with("unsafe fn "))
        && line.contains('(')
    {
        // Get up to the opening brace or end
        let sig = line.split('{').next().unwrap_or(line).trim();
        Some(sig.to_string())
    } else {
        None
    }
}

fn extract_fn_name(sig: &str) -> &str {
    // Find "fn name("
    if let Some(fn_pos) = sig.find("fn ") {
        let after_fn = &sig[fn_pos + 3..];
        if let Some(paren_pos) = after_fn.find('(') {
            let name = &after_fn[..paren_pos];
            // Handle generics
            let name = name.split('<').next().unwrap_or(name);
            return name.trim();
        }
    }
    "?"
}

fn extract_impl_name(line: &str) -> &str {
    // impl Foo for Bar -> Foo for Bar
    // impl Foo -> Foo
    let after_impl = line
        .trim_start_matches("impl")
        .trim_start_matches(|c: char| c == '<' || c.is_whitespace());

    // Skip generic params if present
    let after_generics = if let Some(gt_pos) = after_impl.find('>') {
        &after_impl[gt_pos + 1..]
    } else {
        after_impl
    };

    after_generics
        .trim()
        .split(|c: char| c == '{' || c == '<' || c.is_whitespace())
        .next()
        .unwrap_or("?")
}

fn find_block_end(lines: &[&str], start: usize) -> usize {
    let mut depth = 0;
    let mut started = false;

    for (i, line) in lines[start..].iter().enumerate() {
        for c in line.chars() {
            if c == '{' {
                depth += 1;
                started = true;
            } else if c == '}' {
                depth -= 1;
                if started && depth == 0 {
                    return start + i + 1;
                }
            }
        }
    }

    // Didn't find closing brace, return last line
    lines.len()
}

fn extract_python(lines: &[&str]) -> (Vec<String>, Vec<Symbol>) {
    let mut imports = Vec::new();
    let mut symbols = Vec::new();
    let mut doc_buffer = String::new();
    let mut in_class = false;
    let mut class_indent = 0;
    let mut class_symbol: Option<Symbol> = None;

    for (i, line) in lines.iter().enumerate() {
        let line_num = i + 1;
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();

        // Imports
        if trimmed.starts_with("import ") || trimmed.starts_with("from ") {
            imports.push(trimmed.to_string());
            continue;
        }

        // Docstrings
        if trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''") {
            doc_buffer = trimmed.trim_matches('"').trim_matches('\'').to_string();
            continue;
        }

        // Classes
        if trimmed.starts_with("class ") {
            // Save previous class if any
            if let Some(sym) = class_symbol.take() {
                symbols.push(sym);
            }

            let name = trimmed
                .trim_start_matches("class ")
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("?");

            in_class = true;
            class_indent = indent;
            class_symbol = Some(Symbol {
                kind: SymbolKind::Class,
                name: name.into(),
                signature: trimmed.to_string(),
                start_line: line_num,
                end_line: line_num, // Updated as we go
                doc_comment: if doc_buffer.is_empty() {
                    None
                } else {
                    Some(std::mem::take(&mut doc_buffer))
                },
                children: Vec::new(),
            });
            continue;
        }

        // Functions/methods
        if trimmed.starts_with("def ") || trimmed.starts_with("async def ") {
            let is_method = in_class && indent > class_indent;

            let name = trimmed
                .trim_start_matches("async ")
                .trim_start_matches("def ")
                .split('(')
                .next()
                .unwrap_or("?");

            // Get signature up to colon
            let sig = trimmed.split(':').next().unwrap_or(trimmed).to_string();

            let symbol = Symbol {
                kind: if is_method { SymbolKind::Method } else { SymbolKind::Function },
                name: name.into(),
                signature: sig,
                start_line: line_num,
                end_line: find_python_block_end(lines, i, indent),
                doc_comment: if doc_buffer.is_empty() {
                    None
                } else {
                    Some(std::mem::take(&mut doc_buffer))
                },
                children: Vec::new(),
            };

            if is_method {
                if let Some(ref mut class_sym) = class_symbol {
                    class_sym.end_line = symbol.end_line;
                    class_sym.children.push(symbol);
                }
            } else {
                // End class if we're back to top level
                if let Some(sym) = class_symbol.take() {
                    symbols.push(sym);
                    in_class = false;
                }
                symbols.push(symbol);
            }
            continue;
        }
    }

    // Add remaining class if any
    if let Some(sym) = class_symbol {
        symbols.push(sym);
    }

    (imports, symbols)
}

fn find_python_block_end(lines: &[&str], start: usize, start_indent: usize) -> usize {
    for (i, line) in lines[start + 1..].iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent <= start_indent {
            return start + i;
        }
    }
    lines.len()
}

fn extract_js_ts(lines: &[&str]) -> (Vec<String>, Vec<Symbol>) {
    let mut imports = Vec::new();
    let mut symbols = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let line_num = i + 1;
        let trimmed = line.trim();

        // Imports
        if trimmed.starts_with("import ") {
            imports.push(trimmed.to_string());
            continue;
        }

        // Functions
        if (trimmed.starts_with("function ")
            || trimmed.starts_with("export function ")
            || trimmed.starts_with("async function ")
            || trimmed.starts_with("export async function "))
            && trimmed.contains('(')
        {
            let sig = trimmed.split('{').next().unwrap_or(trimmed).trim();
            let name = sig
                .replace("export ", "")
                .replace("async ", "")
                .replace("function ", "")
                .split('(')
                .next()
                .unwrap_or("?")
                .trim()
                .to_string();

            symbols.push(Symbol {
                kind: SymbolKind::Function,
                name,
                signature: sig.to_string(),
                start_line: line_num,
                end_line: find_block_end(lines, i),
                doc_comment: None,
                children: Vec::new(),
            });
            continue;
        }

        // Arrow functions assigned to const/let
        if (trimmed.starts_with("const ") || trimmed.starts_with("let ")
            || trimmed.starts_with("export const ") || trimmed.starts_with("export let "))
            && trimmed.contains(" = ")
            && (trimmed.contains("=>") || trimmed.contains("function"))
        {
            let name = trimmed
                .replace("export ", "")
                .replace("const ", "")
                .replace("let ", "")
                .split('=')
                .next()
                .unwrap_or("?")
                .trim()
                .to_string();

            symbols.push(Symbol {
                kind: SymbolKind::Function,
                name,
                signature: trimmed.to_string(),
                start_line: line_num,
                end_line: find_block_end(lines, i),
                doc_comment: None,
                children: Vec::new(),
            });
            continue;
        }

        // Classes
        if trimmed.starts_with("class ") || trimmed.starts_with("export class ") {
            let name = trimmed
                .replace("export ", "")
                .replace("class ", "")
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("?")
                .to_string();

            symbols.push(Symbol {
                kind: SymbolKind::Class,
                name,
                signature: trimmed.to_string(),
                start_line: line_num,
                end_line: find_block_end(lines, i),
                doc_comment: None,
                children: Vec::new(),
            });
            continue;
        }

        // Interfaces (TypeScript)
        if trimmed.starts_with("interface ") || trimmed.starts_with("export interface ") {
            let name = trimmed
                .replace("export ", "")
                .replace("interface ", "")
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("?")
                .to_string();

            symbols.push(Symbol {
                kind: SymbolKind::Interface,
                name,
                signature: trimmed.to_string(),
                start_line: line_num,
                end_line: find_block_end(lines, i),
                doc_comment: None,
                children: Vec::new(),
            });
        }
    }

    (imports, symbols)
}

fn extract_go(lines: &[&str]) -> (Vec<String>, Vec<Symbol>) {
    let mut imports = Vec::new();
    let mut symbols = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let line_num = i + 1;
        let trimmed = line.trim();

        // Imports
        if trimmed.starts_with("import ") {
            imports.push(trimmed.to_string());
            continue;
        }

        // Functions
        if trimmed.starts_with("func ") && trimmed.contains('(') {
            let sig = trimmed.split('{').next().unwrap_or(trimmed).trim();
            // func name( or func (receiver) name(
            let name = if let Some(paren_pos) = sig.find('(') {
                let before_paren = &sig[5..paren_pos]; // After "func "
                if before_paren.trim().is_empty() {
                    // Method with receiver
                    let after_first_paren = &sig[paren_pos + 1..];
                    if let Some(close_paren) = after_first_paren.find(')') {
                        let after_receiver = &after_first_paren[close_paren + 1..];
                        after_receiver
                            .trim()
                            .split('(')
                            .next()
                            .unwrap_or("?")
                    } else {
                        "?"
                    }
                } else {
                    before_paren.trim()
                }
            } else {
                "?"
            };

            symbols.push(Symbol {
                kind: SymbolKind::Function,
                name: name.into(),
                signature: sig.to_string(),
                start_line: line_num,
                end_line: find_block_end(lines, i),
                doc_comment: None,
                children: Vec::new(),
            });
            continue;
        }

        // Structs
        if trimmed.starts_with("type ") && trimmed.contains(" struct") {
            let name = trimmed
                .trim_start_matches("type ")
                .split_whitespace()
                .next()
                .unwrap_or("?");

            symbols.push(Symbol {
                kind: SymbolKind::Struct,
                name: name.into(),
                signature: trimmed.to_string(),
                start_line: line_num,
                end_line: find_block_end(lines, i),
                doc_comment: None,
                children: Vec::new(),
            });
            continue;
        }

        // Interfaces
        if trimmed.starts_with("type ") && trimmed.contains(" interface") {
            let name = trimmed
                .trim_start_matches("type ")
                .split_whitespace()
                .next()
                .unwrap_or("?");

            symbols.push(Symbol {
                kind: SymbolKind::Interface,
                name: name.into(),
                signature: trimmed.to_string(),
                start_line: line_num,
                end_line: find_block_end(lines, i),
                doc_comment: None,
                children: Vec::new(),
            });
        }
    }

    (imports, symbols)
}

/// Budget-aware context builder
pub struct ContextBuilder {
    max_tokens: usize,
    current_tokens: usize,
    content: String,
}

impl ContextBuilder {
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            current_tokens: 0,
            content: String::new(),
        }
    }

    /// Add a file outline
    pub fn add_outline(&mut self, outline: &FileOutline) -> bool {
        let formatted = outline.format(self.max_tokens - self.current_tokens);
        let tokens = estimate_tokens(&formatted);

        if self.current_tokens + tokens <= self.max_tokens {
            self.content.push_str(&formatted);
            self.content.push_str("\n---\n");
            self.current_tokens += tokens + 10;
            true
        } else {
            false
        }
    }

    /// Add a specific symbol's full content
    pub fn add_symbol_content(&mut self, outline: &FileOutline, symbol_name: &str, full_content: &str) -> bool {
        if let Some((start, end)) = outline.symbol_range(symbol_name) {
            let lines: Vec<&str> = full_content.lines().collect();
            let symbol_lines: Vec<&str> = lines
                .get(start.saturating_sub(1)..end.min(lines.len()))
                .unwrap_or(&[])
                .to_vec();
            let content = symbol_lines.join("\n");
            let tokens = estimate_tokens(&content);

            if self.current_tokens + tokens <= self.max_tokens {
                self.content.push_str(&format!("// {} (full content):\n", symbol_name));
                self.content.push_str(&content);
                self.content.push_str("\n---\n");
                self.current_tokens += tokens + 10;
                return true;
            }
        }
        false
    }

    /// Get remaining budget
    pub fn remaining_tokens(&self) -> usize {
        self.max_tokens.saturating_sub(self.current_tokens)
    }

    /// Build the final context
    pub fn build(self) -> String {
        self.content
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_rust() {
        let code = r#"
use std::io;

/// A test struct
pub struct Foo {
    x: i32,
}

impl Foo {
    /// Creates a new Foo
    pub fn new(x: i32) -> Self {
        Self { x }
    }

    pub fn get(&self) -> i32 {
        self.x
    }
}

fn helper() -> bool {
    true
}
"#;

        let outline = FileOutline::extract("test.rs", code);
        assert_eq!(outline.language, Language::Rust);
        assert!(!outline.imports.is_empty());

        // Should have struct, impl with methods, and helper function
        let names: Vec<_> = outline.symbols.iter().map(|s| &s.name).collect();
        assert!(names.contains(&&"Foo".to_string()));
        assert!(names.contains(&&"helper".to_string()));
    }

    #[test]
    fn test_extract_python() {
        let code = r#"
import os
from pathlib import Path

class MyClass:
    def __init__(self, x):
        self.x = x

    def get_value(self):
        return self.x

def standalone():
    pass
"#;

        let outline = FileOutline::extract("test.py", code);
        assert_eq!(outline.language, Language::Python);
        assert_eq!(outline.imports.len(), 2);

        let names: Vec<_> = outline.symbols.iter().map(|s| &s.name).collect();
        assert!(names.contains(&&"MyClass".to_string()));
        assert!(names.contains(&&"standalone".to_string()));
    }

    #[test]
    fn test_context_builder() {
        let code = "fn foo() { }\nfn bar() { }";
        let outline = FileOutline::extract("test.rs", code);

        let mut builder = ContextBuilder::new(1000);
        assert!(builder.add_outline(&outline));
        assert!(builder.remaining_tokens() < 1000);

        let result = builder.build();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_language_detection() {
        assert_eq!(Language::from_path("foo.rs"), Language::Rust);
        assert_eq!(Language::from_path("bar.py"), Language::Python);
        assert_eq!(Language::from_path("baz.ts"), Language::TypeScript);
        assert_eq!(Language::from_path("qux.go"), Language::Go);
        assert_eq!(Language::from_path("unknown.xyz"), Language::Unknown);
    }
}
