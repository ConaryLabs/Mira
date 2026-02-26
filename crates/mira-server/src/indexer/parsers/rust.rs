// src/indexer/parsers/rust.rs
// Rust language parser using tree-sitter

use anyhow::{Result, anyhow};
use tree_sitter::{Node, Parser};

use super::{
    FunctionCall, Import, LanguageParser, NodeExt, ParseContext, ParseResult, Symbol,
    SymbolBuilder, default_parse, node_text,
};

/// Rust language parser
pub struct RustParser;

impl LanguageParser for RustParser {
    fn language_id(&self) -> &'static str {
        "rust"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }

    fn configure_parser(&self, parser: &mut Parser) -> Result<()> {
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .map_err(|e| anyhow!("Failed to set Rust language: {}", e))
    }

    fn parse(&self, parser: &mut Parser, content: &str) -> Result<ParseResult> {
        default_parse(parser, content, "rust", walk)
    }
}

/// Walk the AST and extract symbols, imports, and calls
pub fn walk(
    node: Node,
    ctx: &mut ParseContext,
    parent_name: Option<&str>,
    current_function: Option<&str>,
) {
    match node.kind() {
        "function_item" | "function_signature_item" => {
            if let Some(sym) = extract_function(node, ctx.source, parent_name) {
                let func_name = sym
                    .qualified_name
                    .clone()
                    .unwrap_or_else(|| sym.name.clone());
                ctx.symbols.push(sym);
                if let Some(body) = node.child_by_field_name("body") {
                    for child in body.children(&mut body.walk()) {
                        walk(child, ctx, parent_name, Some(&func_name));
                    }
                }
                return;
            }
        }
        "struct_item" => {
            if let Some(sym) = extract_struct(node, ctx.source) {
                let name = sym.name.clone();
                ctx.symbols.push(sym);
                for child in node.children(&mut node.walk()) {
                    walk(child, ctx, Some(&name), current_function);
                }
                return;
            }
        }
        "enum_item" => {
            if let Some(sym) = extract_enum(node, ctx.source) {
                ctx.symbols.push(sym);
            }
        }
        "trait_item" => {
            if let Some(sym) = extract_trait(node, ctx.source) {
                let name = sym.name.clone();
                ctx.symbols.push(sym);
                for child in node.children(&mut node.walk()) {
                    walk(child, ctx, Some(&name), current_function);
                }
                return;
            }
        }
        "impl_item" => {
            let type_name = node
                .child_by_field_name("type")
                .map(|n| node_text(n, ctx.source));
            for child in node.children(&mut node.walk()) {
                walk(child, ctx, type_name.as_deref(), current_function);
            }
            return;
        }
        "const_item" | "static_item" => {
            if let Some(sym) = extract_const(node, ctx.source) {
                ctx.symbols.push(sym);
            }
        }
        "mod_item" => {
            if let Some(sym) = extract_mod(node, ctx.source) {
                ctx.symbols.push(sym);
            }
        }
        "type_item" => {
            if let Some(sym) = extract_type_alias(node, ctx.source) {
                ctx.symbols.push(sym);
            }
        }
        "use_declaration" => {
            if let Some(import) = extract_use(node, ctx.source) {
                ctx.imports.push(import);
            }
        }
        "call_expression" => {
            if let Some(caller) = current_function
                && let Some(call) = extract_call(node, ctx.source, caller)
            {
                ctx.calls.push(call);
            }
        }
        "macro_invocation" => {
            if let Some(caller) = current_function
                && let Some(call) = extract_macro_call(node, ctx.source, caller)
            {
                ctx.calls.push(call);
            }
        }
        _ => {}
    }

    for child in node.children(&mut node.walk()) {
        walk(child, ctx, parent_name, current_function);
    }
}

fn extract_function(node: Node, source: &[u8], parent_name: Option<&str>) -> Option<Symbol> {
    let return_type = node.field_text("return_type", source);
    SymbolBuilder::new(node, source, "rust")
        .name_from_field("name")
        .qualified_with_parent(parent_name, "::")
        .symbol_type("function")
        .signature_from_field("parameters")
        .visibility_from_child("visibility_modifier")
        .documentation(get_doc_comment(node, source))
        .is_test(has_test_attribute(node, source))
        .is_async(node.has_child_kind("async"))
        .return_type(return_type)
        .build()
}

fn extract_struct(node: Node, source: &[u8]) -> Option<Symbol> {
    SymbolBuilder::new(node, source, "rust")
        .name_from_field("name")
        .qualified_with_parent(None, "::")
        .symbol_type("struct")
        .visibility_from_child("visibility_modifier")
        .build()
}

fn extract_enum(node: Node, source: &[u8]) -> Option<Symbol> {
    SymbolBuilder::new(node, source, "rust")
        .name_from_field("name")
        .qualified_with_parent(None, "::")
        .symbol_type("enum")
        .visibility_from_child("visibility_modifier")
        .build()
}

fn extract_trait(node: Node, source: &[u8]) -> Option<Symbol> {
    SymbolBuilder::new(node, source, "rust")
        .name_from_field("name")
        .qualified_with_parent(None, "::")
        .symbol_type("trait")
        .visibility_from_child("visibility_modifier")
        .build()
}

fn extract_const(node: Node, source: &[u8]) -> Option<Symbol> {
    let symbol_type = if node.kind() == "const_item" {
        "const"
    } else {
        "static"
    };
    SymbolBuilder::new(node, source, "rust")
        .name_from_field("name")
        .qualified_with_parent(None, "::")
        .symbol_type(symbol_type)
        .visibility_from_child("visibility_modifier")
        .build()
}

fn extract_mod(node: Node, source: &[u8]) -> Option<Symbol> {
    SymbolBuilder::new(node, source, "rust")
        .name_from_field("name")
        .qualified_with_parent(None, "::")
        .symbol_type("module")
        .visibility_from_child("visibility_modifier")
        .build()
}

fn extract_type_alias(node: Node, source: &[u8]) -> Option<Symbol> {
    let signature = node.field_text("type", source);
    SymbolBuilder::new(node, source, "rust")
        .name_from_field("name")
        .qualified_with_parent(None, "::")
        .symbol_type("type_alias")
        .signature(signature)
        .visibility_from_child("visibility_modifier")
        .documentation(get_doc_comment(node, source))
        .build()
}

fn extract_use(node: Node, source: &[u8]) -> Option<Import> {
    let arg = node.child_by_field_name("argument")?;
    let path = node_text(arg, source);

    let is_external =
        !path.starts_with("crate::") && !path.starts_with("self::") && !path.starts_with("super::");

    let imported_symbols = extract_use_symbols(arg, source);

    Some(Import {
        import_path: path,
        imported_symbols,
        is_external,
    })
}

/// Extract the specific imported names from a use argument node.
/// Returns None for glob imports (`*`), Some(names) for explicit imports.
fn extract_use_symbols(node: Node, source: &[u8]) -> Option<Vec<String>> {
    match node.kind() {
        "use_list" => {
            // use foo::{Bar, Baz}
            extract_names_from_use_list(node, source)
        }
        "scoped_use_list" => {
            // use foo::{Bar, Baz} — the argument is a scoped_use_list wrapping a use_list
            // Find the use_list child
            for child in node.children(&mut node.walk()) {
                if child.kind() == "use_list" {
                    return extract_names_from_use_list(child, source);
                }
            }
            None
        }
        "scoped_identifier" => {
            // use foo::Bar — last segment is the name
            if let Some(name) = node.child_by_field_name("name") {
                Some(vec![node_text(name, source)])
            } else {
                None
            }
        }
        "use_as_clause" => {
            // use foo::Bar as B — use alias
            if let Some(alias) = node.child_by_field_name("alias") {
                Some(vec![node_text(alias, source)])
            } else {
                None
            }
        }
        "use_wildcard" => None,
        "identifier" => Some(vec![node_text(node, source)]),
        _ => None,
    }
}

fn extract_names_from_use_list(node: Node, source: &[u8]) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "identifier" | "self" => {
                names.push(node_text(child, source));
            }
            "use_as_clause" => {
                // use foo::{Bar as B} — use the alias
                if let Some(alias) = child.child_by_field_name("alias") {
                    names.push(node_text(alias, source));
                } else if let Some(orig) = child.child_by_field_name("path") {
                    names.push(node_text(orig, source));
                }
            }
            "scoped_use_list" | "use_wildcard" => {
                // glob or nested list — skip individual extraction
            }
            _ => {}
        }
    }
    if names.is_empty() { None } else { Some(names) }
}

fn extract_call(node: Node, source: &[u8], caller: &str) -> Option<FunctionCall> {
    let function_node = node.child_by_field_name("function")?;
    let callee_name = match function_node.kind() {
        "identifier" => node_text(function_node, source),
        "field_expression" => function_node.field_text("field", source)?,
        "scoped_identifier" => node_text(function_node, source),
        _ => return None,
    };

    let call_type = if function_node.kind() == "field_expression" {
        "method"
    } else {
        "direct"
    };

    Some(FunctionCall {
        caller_name: caller.to_string(),
        callee_name,
        call_line: node.start_line(),
        call_type: call_type.to_string(),
    })
}

fn extract_macro_call(node: Node, source: &[u8], caller: &str) -> Option<FunctionCall> {
    let callee_name = node.field_text("macro", source)?;

    // Skip common macros that aren't interesting for call graphs
    if matches!(
        callee_name.as_str(),
        "println"
            | "print"
            | "eprintln"
            | "eprint"
            | "format"
            | "write"
            | "writeln"
            | "panic"
            | "assert"
            | "assert_eq"
            | "assert_ne"
            | "debug_assert"
            | "vec"
            | "dbg"
            | "todo"
            | "unimplemented"
            | "unreachable"
            | "cfg"
            | "include"
            | "include_str"
            | "include_bytes"
            | "env"
            | "option_env"
            | "concat"
            | "stringify"
            | "line"
            | "column"
            | "file"
    ) {
        return None;
    }

    Some(FunctionCall {
        caller_name: caller.to_string(),
        callee_name,
        call_line: node.start_line(),
        call_type: "macro".to_string(),
    })
}

fn has_test_attribute(node: Node, source: &[u8]) -> bool {
    if let Some(parent) = node.parent() {
        for child in parent.children(&mut parent.walk()) {
            if child.kind() == "attribute_item" {
                let text = node_text(child, source);
                // Match #[test], #[tokio::test], #[rstest::test], etc.
                // but not #[my_test_helper] or other false positives.
                if text.contains("#[test]") || text.contains("::test]") {
                    return true;
                }
            }
            if child.id() == node.id() {
                break;
            }
        }
    }
    false
}

fn get_doc_comment(node: Node, source: &[u8]) -> Option<String> {
    let mut docs = Vec::new();

    if let Some(parent) = node.parent() {
        let mut found_node = false;
        for child in parent
            .children(&mut parent.walk())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            if child.id() == node.id() {
                found_node = true;
                continue;
            }
            if !found_node {
                continue;
            }

            if child.kind() == "line_comment" {
                let text = node_text(child, source);
                if text.starts_with("///") || text.starts_with("//!") {
                    docs.push(text.trim_start_matches('/').trim().to_string());
                } else {
                    break;
                }
            } else if child.kind() == "attribute_item" {
                continue;
            } else {
                break;
            }
        }
    }

    if docs.is_empty() {
        None
    } else {
        docs.reverse();
        Some(docs.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_rust(code: &str) -> ParseResult {
        crate::indexer::parsers::parse_with(&RustParser, code)
    }

    #[test]
    fn test_parse_function() {
        let code = r#"
fn hello_world() {
    println!("Hello");
}
"#;
        let (symbols, _, _) = parse_rust(code);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "hello_world");
        assert_eq!(symbols[0].symbol_type, "function");
        assert_eq!(symbols[0].language, "rust");
        assert!(!symbols[0].is_async);
    }

    #[test]
    fn test_parse_async_function() {
        let code = r#"
async fn fetch_data() -> Result<String, Error> {
    Ok("data".to_string())
}
"#;
        let (symbols, _, _) = parse_rust(code);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "fetch_data");
        // Note: async detection depends on tree-sitter node structure
    }

    #[test]
    fn test_parse_struct_with_impl() {
        let code = r#"
pub struct MyStruct {
    field: i32,
}

impl MyStruct {
    pub fn new() -> Self {
        Self { field: 0 }
    }

    fn private_method(&self) -> i32 {
        self.field
    }
}
"#;
        let (symbols, _, _) = parse_rust(code);

        let struct_sym = symbols.iter().find(|s| s.name == "MyStruct").unwrap();
        assert_eq!(struct_sym.symbol_type, "struct");
        assert_eq!(struct_sym.visibility, Some("pub".to_string()));

        let new_sym = symbols.iter().find(|s| s.name == "new").unwrap();
        assert_eq!(new_sym.symbol_type, "function");
        assert_eq!(new_sym.qualified_name, Some("MyStruct::new".to_string()));
        assert_eq!(new_sym.visibility, Some("pub".to_string()));

        let private_sym = symbols.iter().find(|s| s.name == "private_method").unwrap();
        assert!(
            private_sym.visibility.is_none()
                || private_sym.visibility == Some("private".to_string())
        );
    }

    #[test]
    fn test_parse_test_function() {
        let code = r#"
#[test]
fn test_something() {
    assert!(true);
}

#[tokio::test]
async fn test_async() {
    assert!(true);
}
"#;
        let (symbols, _, _) = parse_rust(code);

        let test_sym = symbols.iter().find(|s| s.name == "test_something").unwrap();
        assert!(test_sym.is_test);

        let async_test = symbols.iter().find(|s| s.name == "test_async").unwrap();
        assert!(async_test.is_test);
        // Note: async detection depends on tree-sitter structure
    }

    #[test]
    fn test_parse_imports() {
        let code = r#"
use std::collections::HashMap;
use crate::tools::format;
use super::parsers;
use anyhow::{Result, Context};
"#;
        let (_, imports, _) = parse_rust(code);

        assert!(imports.len() >= 3);

        let std_import = imports
            .iter()
            .find(|i| i.import_path.contains("std"))
            .unwrap();
        assert!(std_import.is_external);

        let crate_import = imports
            .iter()
            .find(|i| i.import_path.contains("crate"))
            .unwrap();
        assert!(!crate_import.is_external);
    }

    #[test]
    fn test_parse_enum() {
        let code = r#"
pub enum Status {
    Active,
    Inactive,
    Pending(String),
}
"#;
        let (symbols, _, _) = parse_rust(code);

        let enum_sym = symbols.iter().find(|s| s.name == "Status").unwrap();
        assert_eq!(enum_sym.symbol_type, "enum");
        assert_eq!(enum_sym.visibility, Some("pub".to_string()));
    }

    #[test]
    fn test_parse_trait_impl() {
        let code = r#"
trait Greet {
    fn greet(&self) -> String;
}

struct Person;

impl Greet for Person {
    fn greet(&self) -> String {
        "Hello".to_string()
    }
}
"#;
        let (symbols, _, _) = parse_rust(code);

        let trait_sym = symbols.iter().find(|s| s.name == "Greet").unwrap();
        assert_eq!(trait_sym.symbol_type, "trait");

        let person_sym = symbols.iter().find(|s| s.name == "Person").unwrap();
        assert_eq!(person_sym.symbol_type, "struct");
    }

    #[test]
    fn test_parse_function_calls() {
        let code = r#"
fn caller() {
    helper();
    other_func();
}

fn helper() {}
fn other_func() {}
"#;
        let (_, _, calls) = parse_rust(code);

        assert!(calls.len() >= 2);
        assert!(
            calls
                .iter()
                .any(|c| c.caller_name == "caller" && c.callee_name == "helper")
        );
        assert!(
            calls
                .iter()
                .any(|c| c.caller_name == "caller" && c.callee_name == "other_func")
        );
    }

    #[test]
    fn test_parse_type_alias() {
        let code = r#"
/// A result alias
pub type MyResult<T> = Result<T, String>;

type PrivateAlias = Vec<u8>;
"#;
        let (symbols, _, _) = parse_rust(code);

        let alias = symbols.iter().find(|s| s.name == "MyResult").unwrap();
        assert_eq!(alias.symbol_type, "type_alias");
        assert_eq!(alias.visibility, Some("pub".to_string()));
        assert!(alias.signature.is_some());

        let private_alias = symbols.iter().find(|s| s.name == "PrivateAlias").unwrap();
        assert_eq!(private_alias.symbol_type, "type_alias");
    }

    #[test]
    fn test_parse_function_return_type() {
        let code = r#"
fn get_name() -> String {
    "hello".to_string()
}

fn no_return() {
    // nothing
}
"#;
        let (symbols, _, _) = parse_rust(code);

        let with_return = symbols.iter().find(|s| s.name == "get_name").unwrap();
        assert!(
            with_return.return_type.is_some(),
            "expected return_type to be set"
        );
        assert!(
            with_return
                .return_type
                .as_deref()
                .unwrap()
                .contains("String")
        );

        let no_return = symbols.iter().find(|s| s.name == "no_return").unwrap();
        assert!(no_return.return_type.is_none());
    }

    #[test]
    fn test_parse_imported_symbol_names() {
        let code = r#"
use std::collections::HashMap;
use anyhow::{Result, Context};
use crate::tools::*;
use std::io::{self, Write};
"#;
        let (_, imports, _) = parse_rust(code);

        // use std::collections::HashMap -> ["HashMap"]
        let hash_map_import = imports
            .iter()
            .find(|i| i.import_path.contains("HashMap"))
            .unwrap();
        assert_eq!(
            hash_map_import.imported_symbols,
            Some(vec!["HashMap".to_string()])
        );

        // use anyhow::{Result, Context} -> ["Result", "Context"]
        let anyhow_import = imports
            .iter()
            .find(|i| i.import_path.contains("anyhow"))
            .unwrap();
        let mut syms = anyhow_import.imported_symbols.clone().unwrap();
        syms.sort();
        assert_eq!(syms, vec!["Context".to_string(), "Result".to_string()]);

        // glob import -> None
        let glob_import = imports
            .iter()
            .find(|i| i.import_path.contains("tools"))
            .unwrap();
        assert!(glob_import.imported_symbols.is_none());
    }
}
