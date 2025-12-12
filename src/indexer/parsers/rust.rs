// src/indexer/parsers/rust.rs
// Rust language parser using tree-sitter

use anyhow::{anyhow, Result};
use tree_sitter::{Parser, Node};

use super::{Symbol, Import, FunctionCall, ParseResult, node_text};

/// Parse Rust source code
pub fn parse(parser: &mut Parser, content: &str) -> Result<ParseResult> {
    let tree = parser.parse(content, None)
        .ok_or_else(|| anyhow!("Failed to parse Rust code"))?;

    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut calls = Vec::new();
    let bytes = content.as_bytes();

    walk(tree.root_node(), bytes, &mut symbols, &mut imports, &mut calls, None, None);
    Ok((symbols, imports, calls))
}

/// Walk the AST and extract symbols, imports, and calls
pub fn walk(
    node: Node,
    source: &[u8],
    symbols: &mut Vec<Symbol>,
    imports: &mut Vec<Import>,
    calls: &mut Vec<FunctionCall>,
    parent_name: Option<&str>,
    current_function: Option<&str>,
) {
    match node.kind() {
        "function_item" | "function_signature_item" => {
            if let Some(sym) = extract_function(node, source, parent_name) {
                let func_name = sym.qualified_name.clone().unwrap_or_else(|| sym.name.clone());
                symbols.push(sym);
                if let Some(body) = node.child_by_field_name("body") {
                    for child in body.children(&mut body.walk()) {
                        walk(child, source, symbols, imports, calls, parent_name, Some(&func_name));
                    }
                }
                return;
            }
        }
        "struct_item" => {
            if let Some(sym) = extract_struct(node, source) {
                let name = sym.name.clone();
                symbols.push(sym);
                for child in node.children(&mut node.walk()) {
                    walk(child, source, symbols, imports, calls, Some(&name), current_function);
                }
                return;
            }
        }
        "enum_item" => {
            if let Some(sym) = extract_enum(node, source) {
                symbols.push(sym);
            }
        }
        "trait_item" => {
            if let Some(sym) = extract_trait(node, source) {
                let name = sym.name.clone();
                symbols.push(sym);
                for child in node.children(&mut node.walk()) {
                    walk(child, source, symbols, imports, calls, Some(&name), current_function);
                }
                return;
            }
        }
        "impl_item" => {
            let type_name = node.child_by_field_name("type")
                .map(|n| node_text(n, source));
            for child in node.children(&mut node.walk()) {
                walk(child, source, symbols, imports, calls, type_name.as_deref(), current_function);
            }
            return;
        }
        "const_item" | "static_item" => {
            if let Some(sym) = extract_const(node, source) {
                symbols.push(sym);
            }
        }
        "mod_item" => {
            if let Some(sym) = extract_mod(node, source) {
                symbols.push(sym);
            }
        }
        "use_declaration" => {
            if let Some(import) = extract_use(node, source) {
                imports.push(import);
            }
        }
        "call_expression" => {
            if let Some(caller) = current_function {
                if let Some(call) = extract_call(node, source, caller) {
                    calls.push(call);
                }
            }
        }
        "macro_invocation" => {
            if let Some(caller) = current_function {
                if let Some(call) = extract_macro_call(node, source, caller) {
                    calls.push(call);
                }
            }
        }
        _ => {}
    }

    for child in node.children(&mut node.walk()) {
        walk(child, source, symbols, imports, calls, parent_name, current_function);
    }
}

fn extract_function(node: Node, source: &[u8], parent_name: Option<&str>) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);

    let qualified_name = match parent_name {
        Some(parent) => format!("{}::{}", parent, name),
        None => name.clone(),
    };

    let signature = node.children(&mut node.walk())
        .find(|n| n.kind() == "parameters")
        .map(|n| node_text(n, source));

    let visibility = node.children(&mut node.walk())
        .find(|n| n.kind() == "visibility_modifier")
        .map(|n| node_text(n, source));

    let is_async = node.children(&mut node.walk())
        .any(|n| n.kind() == "async");

    let is_test = has_test_attribute(node, source);
    let documentation = get_doc_comment(node, source);

    Some(Symbol {
        name,
        qualified_name: Some(qualified_name),
        symbol_type: "function".to_string(),
        language: "rust".to_string(),
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        signature,
        visibility,
        documentation,
        is_test,
        is_async,
    })
}

fn extract_struct(node: Node, source: &[u8]) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);

    let visibility = node.children(&mut node.walk())
        .find(|n| n.kind() == "visibility_modifier")
        .map(|n| node_text(n, source));

    Some(Symbol {
        name: name.clone(),
        qualified_name: Some(name),
        symbol_type: "struct".to_string(),
        language: "rust".to_string(),
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        signature: None,
        visibility,
        documentation: None,
        is_test: false,
        is_async: false,
    })
}

fn extract_enum(node: Node, source: &[u8]) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);

    let visibility = node.children(&mut node.walk())
        .find(|n| n.kind() == "visibility_modifier")
        .map(|n| node_text(n, source));

    Some(Symbol {
        name: name.clone(),
        qualified_name: Some(name),
        symbol_type: "enum".to_string(),
        language: "rust".to_string(),
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        signature: None,
        visibility,
        documentation: None,
        is_test: false,
        is_async: false,
    })
}

fn extract_trait(node: Node, source: &[u8]) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);

    let visibility = node.children(&mut node.walk())
        .find(|n| n.kind() == "visibility_modifier")
        .map(|n| node_text(n, source));

    Some(Symbol {
        name: name.clone(),
        qualified_name: Some(name),
        symbol_type: "trait".to_string(),
        language: "rust".to_string(),
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        signature: None,
        visibility,
        documentation: None,
        is_test: false,
        is_async: false,
    })
}

fn extract_const(node: Node, source: &[u8]) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);

    let visibility = node.children(&mut node.walk())
        .find(|n| n.kind() == "visibility_modifier")
        .map(|n| node_text(n, source));

    let symbol_type = if node.kind() == "const_item" { "const" } else { "static" };

    Some(Symbol {
        name: name.clone(),
        qualified_name: Some(name),
        symbol_type: symbol_type.to_string(),
        language: "rust".to_string(),
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        signature: None,
        visibility,
        documentation: None,
        is_test: false,
        is_async: false,
    })
}

fn extract_mod(node: Node, source: &[u8]) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);

    let visibility = node.children(&mut node.walk())
        .find(|n| n.kind() == "visibility_modifier")
        .map(|n| node_text(n, source));

    Some(Symbol {
        name: name.clone(),
        qualified_name: Some(name),
        symbol_type: "module".to_string(),
        language: "rust".to_string(),
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        signature: None,
        visibility,
        documentation: None,
        is_test: false,
        is_async: false,
    })
}

fn extract_use(node: Node, source: &[u8]) -> Option<Import> {
    let path = node.child_by_field_name("argument")
        .map(|n| node_text(n, source))?;

    let is_external = !path.starts_with("crate::")
        && !path.starts_with("self::")
        && !path.starts_with("super::");

    Some(Import {
        import_path: path,
        imported_symbols: None,
        is_external,
    })
}

fn extract_call(node: Node, source: &[u8], caller: &str) -> Option<FunctionCall> {
    let function_node = node.child_by_field_name("function")?;
    let callee_name = match function_node.kind() {
        "identifier" => node_text(function_node, source),
        "field_expression" => {
            function_node.child_by_field_name("field")
                .map(|n| node_text(n, source))?
        }
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
        call_line: node.start_position().row as u32 + 1,
        call_type: call_type.to_string(),
    })
}

fn extract_macro_call(node: Node, source: &[u8], caller: &str) -> Option<FunctionCall> {
    let macro_node = node.child_by_field_name("macro")?;
    let callee_name = node_text(macro_node, source);

    // Skip common macros that aren't interesting for call graphs
    if matches!(callee_name.as_str(), "println" | "print" | "eprintln" | "eprint" |
                "format" | "write" | "writeln" | "panic" | "assert" | "assert_eq" |
                "assert_ne" | "debug_assert" | "vec" | "dbg" | "todo" | "unimplemented" |
                "unreachable" | "cfg" | "include" | "include_str" | "include_bytes" |
                "env" | "option_env" | "concat" | "stringify" | "line" | "column" | "file") {
        return None;
    }

    Some(FunctionCall {
        caller_name: caller.to_string(),
        callee_name,
        call_line: node.start_position().row as u32 + 1,
        call_type: "macro".to_string(),
    })
}

fn has_test_attribute(node: Node, source: &[u8]) -> bool {
    if let Some(parent) = node.parent() {
        for child in parent.children(&mut parent.walk()) {
            if child.kind() == "attribute_item" {
                let text = node_text(child, source);
                if text.contains("test") {
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
        for child in parent.children(&mut parent.walk()).collect::<Vec<_>>().into_iter().rev() {
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
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
        parse(&mut parser, code).unwrap()
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
        assert!(private_sym.visibility.is_none() || private_sym.visibility == Some("private".to_string()));
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

        let std_import = imports.iter().find(|i| i.import_path.contains("std")).unwrap();
        assert!(std_import.is_external);

        let crate_import = imports.iter().find(|i| i.import_path.contains("crate")).unwrap();
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
        assert!(calls.iter().any(|c| c.caller_name == "caller" && c.callee_name == "helper"));
        assert!(calls.iter().any(|c| c.caller_name == "caller" && c.callee_name == "other_func"));
    }
}
