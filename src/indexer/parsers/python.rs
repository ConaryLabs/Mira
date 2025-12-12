// src/indexer/parsers/python.rs
// Python language parser using tree-sitter

use anyhow::{anyhow, Result};
use tree_sitter::{Parser, Node};

use super::{Symbol, Import, FunctionCall, ParseResult, node_text};

/// Parse Python source code
pub fn parse(parser: &mut Parser, content: &str) -> Result<ParseResult> {
    let tree = parser.parse(content, None)
        .ok_or_else(|| anyhow!("Failed to parse Python code"))?;

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
        "function_definition" => {
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
        "class_definition" => {
            if let Some(sym) = extract_class(node, source) {
                let name = sym.name.clone();
                symbols.push(sym);
                if let Some(body) = node.child_by_field_name("body") {
                    for child in body.children(&mut body.walk()) {
                        walk(child, source, symbols, imports, calls, Some(&name), current_function);
                    }
                }
                return;
            }
        }
        "import_statement" | "import_from_statement" => {
            if let Some(import) = extract_import(node, source) {
                imports.push(import);
            }
        }
        "call" => {
            if let Some(caller) = current_function {
                if let Some(call) = extract_call(node, source, caller) {
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
        Some(parent) => format!("{}.{}", parent, name),
        None => name.clone(),
    };

    let signature = node.child_by_field_name("parameters")
        .map(|n| node_text(n, source));

    let is_async = node.children(&mut node.walk())
        .any(|n| n.kind() == "async");

    let is_test = name.starts_with("test_") || name.starts_with("test");

    Some(Symbol {
        name,
        qualified_name: Some(qualified_name),
        symbol_type: "function".to_string(),
        language: "python".to_string(),
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        signature,
        visibility: None,
        documentation: None,
        is_test,
        is_async,
    })
}

fn extract_class(node: Node, source: &[u8]) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);

    // Get base classes for signature
    let superclasses = node.child_by_field_name("superclasses")
        .map(|n| node_text(n, source));

    Some(Symbol {
        name: name.clone(),
        qualified_name: Some(name),
        symbol_type: "class".to_string(),
        language: "python".to_string(),
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        signature: superclasses,
        visibility: None,
        documentation: None,
        is_test: false,
        is_async: false,
    })
}

fn extract_import(node: Node, source: &[u8]) -> Option<Import> {
    let path = if node.kind() == "import_from_statement" {
        node.child_by_field_name("module_name")
            .map(|n| node_text(n, source))?
    } else {
        node.children(&mut node.walk())
            .find(|n| n.kind() == "dotted_name")
            .map(|n| node_text(n, source))?
    };

    // Determine if external (doesn't start with .)
    let is_external = !path.starts_with('.');

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
        "attribute" => {
            function_node.child_by_field_name("attribute")
                .map(|n| node_text(n, source))?
        }
        _ => return None,
    };

    // Skip common builtins
    if matches!(callee_name.as_str(), "print" | "len" | "str" | "int" | "float" |
                "list" | "dict" | "set" | "tuple" | "range" | "enumerate" | "zip" |
                "open" | "type" | "isinstance" | "hasattr" | "getattr" | "setattr" |
                "super" | "sorted" | "reversed" | "map" | "filter" | "any" | "all") {
        return None;
    }

    let call_type = if function_node.kind() == "attribute" {
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
