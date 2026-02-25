// hooks/ast_diff.rs
// AST-level change detection using tree-sitter
//
// Compares two versions of a file by parsing both with tree-sitter,
// extracting top-level symbols, and classifying changes as added,
// removed, signature-changed, or body-changed.
//
// No LLM calls -- pure tree-sitter + Rust.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;

#[cfg(feature = "parsers")]
use crate::indexer::parsers::PARSERS;
#[cfg(feature = "parsers")]
use tree_sitter::Parser;

/// Types of structural changes detected between two versions of a file
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// A new symbol was added
    SymbolAdded,
    /// An existing symbol was removed
    SymbolRemoved,
    /// Function/method signature changed (parameters, return type, visibility)
    SignatureChanged,
    /// Only the body/implementation changed (signature stayed the same)
    BodyChanged,
}

impl std::fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangeKind::SymbolAdded => write!(f, "Added"),
            ChangeKind::SymbolRemoved => write!(f, "Removed"),
            ChangeKind::SignatureChanged => write!(f, "Signature changed"),
            ChangeKind::BodyChanged => write!(f, "Body changed"),
        }
    }
}

/// A single detected structural change
#[derive(Debug, Clone)]
pub struct StructuralChange {
    pub symbol_name: String,
    pub symbol_kind: String, // "function", "struct", "enum", etc.
    pub change_kind: ChangeKind,
    pub line_number: usize,
}

/// Lightweight symbol extracted from tree-sitter parse for diffing
#[derive(Debug, Clone)]
struct DiffSymbol {
    name: String,
    kind: String,
    signature: String, // first line or declaration line
    body_hash: u64,    // hash of full content for body-change detection
    start_line: usize,
}

/// Compare two versions of a file and return structural changes.
///
/// Uses tree-sitter to parse both versions, extracts top-level symbols,
/// and compares them to detect added/removed/signature-changed/body-changed.
///
/// Returns None if the file language is not supported or parsing fails.
#[cfg(feature = "parsers")]
pub fn detect_structural_changes(
    file_path: &Path,
    old_content: &str,
    new_content: &str,
) -> Option<Vec<StructuralChange>> {
    let ext = file_path.extension()?.to_str()?;
    let lang_parser = PARSERS.by_extension(ext)?;

    let mut parser = Parser::new();
    lang_parser.configure_parser(&mut parser).ok()?;

    let old_symbols = extract_diff_symbols(&mut parser, old_content)?;
    // Re-configure parser for the second parse
    lang_parser.configure_parser(&mut parser).ok()?;
    let new_symbols = extract_diff_symbols(&mut parser, new_content)?;

    Some(compare_symbols(&old_symbols, &new_symbols))
}

/// Stub when parsers feature is disabled -- always returns None.
#[cfg(not(feature = "parsers"))]
pub fn detect_structural_changes(
    _file_path: &Path,
    _old_content: &str,
    _new_content: &str,
) -> Option<Vec<StructuralChange>> {
    None
}

/// Parse source and extract lightweight symbols for diffing.
#[cfg(feature = "parsers")]
fn extract_diff_symbols(parser: &mut Parser, content: &str) -> Option<Vec<DiffSymbol>> {
    let tree = parser.parse(content, None)?;
    let root = tree.root_node();
    let mut symbols = Vec::new();

    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        extract_from_node(child, content, &mut symbols, None);
    }

    Some(symbols)
}

/// Recursively extract symbols from a node.
///
/// Handles top-level declarations plus impl/class blocks that contain
/// nested method definitions.
#[cfg(feature = "parsers")]
fn extract_from_node(
    node: tree_sitter::Node,
    content: &str,
    symbols: &mut Vec<DiffSymbol>,
    parent_name: Option<&str>,
) {
    let kind = node.kind();

    // Rust impl blocks: descend into the declaration_list with the type name as parent
    if kind == "impl_item" {
        let type_name = node
            .child_by_field_name("type")
            .map(|n| &content[n.start_byte()..n.end_byte()]);
        // The body of an impl is a declaration_list node
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.named_children(&mut cursor) {
                extract_from_node(child, content, symbols, type_name);
            }
        }
        return;
    }

    // Python/TS class bodies: descend into body with class name as parent
    if kind == "class_definition" || kind == "class_declaration" {
        let name = extract_symbol_name(node, content);
        if let Some(ref name) = name {
            // Record the class itself
            push_diff_symbol(node, content, symbols, name, "struct", None);
            // Descend into body for methods
            if let Some(body) = node.child_by_field_name("body") {
                let mut cursor = body.walk();
                for child in body.named_children(&mut cursor) {
                    extract_from_node(child, content, symbols, Some(name));
                }
            }
        }
        return;
    }

    // TS export_statement: unwrap and descend
    if kind == "export_statement" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            extract_from_node(child, content, symbols, parent_name);
        }
        return;
    }

    if is_symbol_node(kind) {
        let name = extract_symbol_name(node, content);
        if let Some(name) = name {
            let qualified = match parent_name {
                Some(p) => format!("{}::{}", p, name),
                None => name.clone(),
            };
            push_diff_symbol(
                node,
                content,
                symbols,
                &qualified,
                normalize_kind(kind),
                parent_name,
            );
        }
    }
}

/// Push a DiffSymbol for the given node.
#[cfg(feature = "parsers")]
fn push_diff_symbol(
    node: tree_sitter::Node,
    content: &str,
    symbols: &mut Vec<DiffSymbol>,
    qualified_name: &str,
    kind: &str,
    _parent: Option<&str>,
) {
    let start_byte = node.start_byte();
    let end_byte = node.end_byte();
    let full_text = &content[start_byte..end_byte];

    // Extract the signature: everything before the body block.
    // This avoids single-line functions where the first line IS the whole function.
    let signature = extract_signature(node, content, full_text);

    let body_hash = hash_content(full_text);

    symbols.push(DiffSymbol {
        name: qualified_name.to_string(),
        kind: kind.to_string(),
        signature,
        body_hash,
        start_line: node.start_position().row + 1,
    });
}

/// Extract the signature portion of a symbol node.
///
/// For function-like nodes, the signature is everything from the start of the
/// node up to (but not including) the body. For struct/enum/trait nodes it is
/// the text up to the opening brace.  Falls back to the first line.
#[cfg(feature = "parsers")]
fn extract_signature(node: tree_sitter::Node, _content: &str, full_text: &str) -> String {
    // Try to find the body child -- its start byte marks where the signature ends
    if let Some(body) = node.child_by_field_name("body") {
        let sig_end = body.start_byte().saturating_sub(node.start_byte());
        if sig_end > 0 && sig_end <= full_text.len() {
            return full_text[..sig_end].trim_end().to_string();
        }
    }

    // For Rust structs/enums with field_declaration_list or variant_list
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let ck = child.kind();
        if ck == "field_declaration_list"
            || ck == "enum_variant_list"
            || ck == "declaration_list"
            || ck == "block"
            || ck == "statement_block"
        {
            let sig_end = child.start_byte().saturating_sub(node.start_byte());
            if sig_end > 0 && sig_end <= full_text.len() {
                return full_text[..sig_end].trim_end().to_string();
            }
        }
    }

    // Fallback: first line, or up to first '{' if on the same line
    let first_line = full_text.lines().next().unwrap_or("");
    if let Some(brace_pos) = first_line.find('{') {
        first_line[..brace_pos].trim_end().to_string()
    } else {
        first_line.to_string()
    }
}

/// Find the body/block child text for hashing just the body portion.
/// Not currently used -- we hash the full_text instead, which includes the
/// signature. That is fine because signature changes are detected separately
/// before body hash comparison.
#[cfg(feature = "parsers")]
fn _extract_body_text<'a>(node: tree_sitter::Node, _content: &str, full_text: &'a str) -> &'a str {
    if let Some(body) = node.child_by_field_name("body") {
        let offset = body.start_byte().saturating_sub(node.start_byte());
        if offset < full_text.len() {
            return &full_text[offset..];
        }
    }
    full_text
}

/// Check if a tree-sitter node kind represents a named symbol declaration.
#[cfg(feature = "parsers")]
fn is_symbol_node(kind: &str) -> bool {
    matches!(
        kind,
        // Rust
        "function_item"
            | "function_signature_item"
            | "struct_item"
            | "enum_item"
            | "trait_item"
            | "const_item"
            | "static_item"
            | "type_alias"
            // Python
            | "function_definition"
            | "class_definition"
            // TypeScript / JavaScript / Go (shared node kinds)
            | "function_declaration"
            | "method_definition"
            | "method_declaration"
            | "class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            // Go
            | "type_declaration"
    )
}

/// Map tree-sitter node kinds to normalized symbol kind strings.
#[cfg(feature = "parsers")]
fn normalize_kind(kind: &str) -> &str {
    match kind {
        "function_item"
        | "function_signature_item"
        | "function_definition"
        | "function_declaration"
        | "method_definition"
        | "method_declaration" => "function",
        "struct_item" | "class_definition" | "class_declaration" => "struct",
        "enum_item" | "enum_declaration" => "enum",
        "trait_item" | "interface_declaration" => "trait",
        "impl_item" => "impl",
        "type_alias" | "type_alias_declaration" | "type_declaration" => "type",
        "const_item" => "const",
        "static_item" => "static",
        _ => kind,
    }
}

/// Extract a symbol name from a tree-sitter node.
///
/// Checks the "name" field first, then falls back to scanning named children
/// for identifier-like nodes.
#[cfg(feature = "parsers")]
fn extract_symbol_name(node: tree_sitter::Node, source: &str) -> Option<String> {
    // Try the "name" field first (works for most languages)
    if let Some(name_node) = node.child_by_field_name("name") {
        let text = &source[name_node.start_byte()..name_node.end_byte()];
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }

    // Fallback: scan named children for identifier-like nodes
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "identifier" | "type_identifier" | "property_identifier" => {
                let text = &source[child.start_byte()..child.end_byte()];
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
            // Go type_declaration wraps type_spec which has the name
            "type_spec" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let text = &source[name_node.start_byte()..name_node.end_byte()];
                    if !text.is_empty() {
                        return Some(text.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    None
}

/// FNV-style hash for a content string.
fn hash_content(s: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Compare old and new symbol lists, returning structural changes.
fn compare_symbols(old: &[DiffSymbol], new: &[DiffSymbol]) -> Vec<StructuralChange> {
    let mut changes = Vec::new();

    let old_map: HashMap<&str, &DiffSymbol> = old.iter().map(|s| (s.name.as_str(), s)).collect();
    let new_map: HashMap<&str, &DiffSymbol> = new.iter().map(|s| (s.name.as_str(), s)).collect();

    // Check for removed and changed symbols
    for (name, old_sym) in &old_map {
        match new_map.get(name) {
            None => {
                changes.push(StructuralChange {
                    symbol_name: name.to_string(),
                    symbol_kind: old_sym.kind.clone(),
                    change_kind: ChangeKind::SymbolRemoved,
                    line_number: old_sym.start_line,
                });
            }
            Some(new_sym) => {
                if old_sym.signature != new_sym.signature {
                    changes.push(StructuralChange {
                        symbol_name: name.to_string(),
                        symbol_kind: new_sym.kind.clone(),
                        change_kind: ChangeKind::SignatureChanged,
                        line_number: new_sym.start_line,
                    });
                } else if old_sym.body_hash != new_sym.body_hash {
                    changes.push(StructuralChange {
                        symbol_name: name.to_string(),
                        symbol_kind: new_sym.kind.clone(),
                        change_kind: ChangeKind::BodyChanged,
                        line_number: new_sym.start_line,
                    });
                }
            }
        }
    }

    // Check for added symbols
    for (name, new_sym) in &new_map {
        if !old_map.contains_key(name) {
            changes.push(StructuralChange {
                symbol_name: name.to_string(),
                symbol_kind: new_sym.kind.clone(),
                change_kind: ChangeKind::SymbolAdded,
                line_number: new_sym.start_line,
            });
        }
    }

    changes
}

/// Retrieve the previous version of a file from git (HEAD).
///
/// Returns None if the file is new, not in a git repo, or git fails.
pub async fn get_previous_content(file_path: &str, project_path: &str) -> Option<String> {
    let rel_path = file_path
        .strip_prefix(project_path)?
        .trim_start_matches('/');

    let output = tokio::process::Command::new("git")
        .args(["show", &format!("HEAD:{}", rel_path)])
        .current_dir(project_path)
        .output()
        .await
        .ok()?;

    if output.status.success() {
        String::from_utf8(output.stdout).ok()
    } else {
        None // New file or not in git
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_detect_function_added() {
        let old = "fn existing() {}";
        let new = "fn existing() {}\nfn new_func() {}";
        let changes = detect_structural_changes(Path::new("test.rs"), old, new).unwrap();
        assert!(
            changes
                .iter()
                .any(|c| c.symbol_name == "new_func" && c.change_kind == ChangeKind::SymbolAdded)
        );
    }

    #[test]
    fn test_detect_function_removed() {
        let old = "fn foo() {}\nfn bar() {}";
        let new = "fn foo() {}";
        let changes = detect_structural_changes(Path::new("test.rs"), old, new).unwrap();
        assert!(
            changes
                .iter()
                .any(|c| c.symbol_name == "bar" && c.change_kind == ChangeKind::SymbolRemoved)
        );
    }

    #[test]
    fn test_detect_signature_changed() {
        let old = "fn foo(x: i32) {}";
        let new = "fn foo(x: i32, y: i32) {}";
        let changes = detect_structural_changes(Path::new("test.rs"), old, new).unwrap();
        assert!(
            changes
                .iter()
                .any(|c| c.symbol_name == "foo" && c.change_kind == ChangeKind::SignatureChanged)
        );
    }

    #[test]
    fn test_detect_body_changed() {
        let old = "fn foo() { let x = 1; }";
        let new = "fn foo() { let x = 2; }";
        let changes = detect_structural_changes(Path::new("test.rs"), old, new).unwrap();
        assert!(
            changes
                .iter()
                .any(|c| c.symbol_name == "foo" && c.change_kind == ChangeKind::BodyChanged)
        );
    }

    #[test]
    fn test_no_changes() {
        let code = "fn foo() { let x = 1; }";
        let changes = detect_structural_changes(Path::new("test.rs"), code, code).unwrap();
        assert!(changes.is_empty());
    }

    #[test]
    fn test_unsupported_extension() {
        let changes = detect_structural_changes(Path::new("test.xyz"), "hello", "world");
        assert!(changes.is_none());
    }

    #[test]
    fn test_python_function() {
        let old = "def foo():\n    pass";
        let new = "def foo():\n    return 1\ndef bar():\n    pass";
        let changes = detect_structural_changes(Path::new("test.py"), old, new).unwrap();
        assert!(
            changes
                .iter()
                .any(|c| c.symbol_name == "bar" && c.change_kind == ChangeKind::SymbolAdded)
        );
        assert!(
            changes
                .iter()
                .any(|c| c.symbol_name == "foo" && c.change_kind == ChangeKind::BodyChanged)
        );
    }

    #[test]
    fn test_typescript_function() {
        let old = "function greet() { return 'hi'; }";
        let new = "function greet() { return 'hello'; }\nfunction farewell() { return 'bye'; }";
        let changes = detect_structural_changes(Path::new("test.ts"), old, new).unwrap();
        assert!(
            changes
                .iter()
                .any(|c| c.symbol_name == "farewell" && c.change_kind == ChangeKind::SymbolAdded)
        );
        assert!(
            changes
                .iter()
                .any(|c| c.symbol_name == "greet" && c.change_kind == ChangeKind::BodyChanged)
        );
    }

    #[test]
    fn test_rust_struct_added() {
        let old = "fn foo() {}";
        let new = "fn foo() {}\npub struct Bar { x: i32 }";
        let changes = detect_structural_changes(Path::new("test.rs"), old, new).unwrap();
        assert!(
            changes
                .iter()
                .any(|c| c.symbol_name == "Bar" && c.change_kind == ChangeKind::SymbolAdded)
        );
    }

    #[test]
    fn test_rust_impl_method_changed() {
        let old = "struct Foo;\nimpl Foo {\n    fn bar(&self) { let x = 1; }\n}";
        let new = "struct Foo;\nimpl Foo {\n    fn bar(&self) { let x = 2; }\n}";
        let changes = detect_structural_changes(Path::new("test.rs"), old, new).unwrap();
        assert!(
            changes
                .iter()
                .any(|c| c.symbol_name == "Foo::bar" && c.change_kind == ChangeKind::BodyChanged)
        );
    }

    #[test]
    fn test_rust_impl_method_added() {
        let old = "struct Foo;\nimpl Foo {\n    fn bar(&self) {}\n}";
        let new = "struct Foo;\nimpl Foo {\n    fn bar(&self) {}\n    fn baz(&self) {}\n}";
        let changes = detect_structural_changes(Path::new("test.rs"), old, new).unwrap();
        assert!(
            changes
                .iter()
                .any(|c| c.symbol_name == "Foo::baz" && c.change_kind == ChangeKind::SymbolAdded)
        );
    }

    #[test]
    fn test_change_kind_display() {
        assert_eq!(format!("{}", ChangeKind::SymbolAdded), "Added");
        assert_eq!(format!("{}", ChangeKind::SymbolRemoved), "Removed");
        assert_eq!(
            format!("{}", ChangeKind::SignatureChanged),
            "Signature changed"
        );
        assert_eq!(format!("{}", ChangeKind::BodyChanged), "Body changed");
    }

    #[test]
    fn test_multiple_changes_at_once() {
        let old = "fn keep() {}\nfn remove_me() {}\nfn change_sig(x: i32) {}";
        let new = "fn keep() {}\nfn added() {}\nfn change_sig(x: i32, y: i32) {}";
        let changes = detect_structural_changes(Path::new("test.rs"), old, new).unwrap();
        assert!(changes
            .iter()
            .any(|c| c.symbol_name == "remove_me" && c.change_kind == ChangeKind::SymbolRemoved));
        assert!(
            changes
                .iter()
                .any(|c| c.symbol_name == "added" && c.change_kind == ChangeKind::SymbolAdded)
        );
        assert!(changes.iter().any(
            |c| c.symbol_name == "change_sig" && c.change_kind == ChangeKind::SignatureChanged
        ));
        // "keep" should not appear in changes
        assert!(!changes.iter().any(|c| c.symbol_name == "keep"));
    }

    #[test]
    fn test_empty_files() {
        let changes = detect_structural_changes(Path::new("test.rs"), "", "").unwrap();
        assert!(changes.is_empty());
    }

    #[test]
    fn test_no_extension() {
        let changes = detect_structural_changes(Path::new("Makefile"), "old", "new");
        assert!(changes.is_none());
    }
}
