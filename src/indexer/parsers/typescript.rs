// src/indexer/parsers/typescript.rs
// TypeScript/JavaScript language parser using tree-sitter

use anyhow::{anyhow, Result};
use tree_sitter::{Parser, Node};

use super::{Symbol, Import, FunctionCall, ParseResult, node_text};

/// Parse TypeScript source code
pub fn parse(parser: &mut Parser, content: &str) -> Result<ParseResult> {
    let tree = parser.parse(content, None)
        .ok_or_else(|| anyhow!("Failed to parse TypeScript code"))?;

    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut calls = Vec::new();
    let bytes = content.as_bytes();

    walk(tree.root_node(), bytes, &mut symbols, &mut imports, &mut calls, None, None, "typescript");
    Ok((symbols, imports, calls))
}

/// Parse JavaScript source code (same logic, different language tag)
pub fn parse_javascript(parser: &mut Parser, content: &str) -> Result<ParseResult> {
    let tree = parser.parse(content, None)
        .ok_or_else(|| anyhow!("Failed to parse JavaScript code"))?;

    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut calls = Vec::new();
    let bytes = content.as_bytes();

    walk(tree.root_node(), bytes, &mut symbols, &mut imports, &mut calls, None, None, "javascript");
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
    language: &str,
) {
    match node.kind() {
        "function_declaration" | "method_definition" | "arrow_function" => {
            if let Some(sym) = extract_function(node, source, parent_name, language) {
                let func_name = sym.qualified_name.clone().unwrap_or_else(|| sym.name.clone());
                symbols.push(sym);
                if let Some(body) = node.child_by_field_name("body") {
                    for child in body.children(&mut body.walk()) {
                        walk(child, source, symbols, imports, calls, parent_name, Some(&func_name), language);
                    }
                }
                return;
            }
        }
        "class_declaration" => {
            if let Some(sym) = extract_class(node, source, language) {
                let name = sym.name.clone();
                symbols.push(sym);
                if let Some(body) = node.child_by_field_name("body") {
                    for child in body.children(&mut body.walk()) {
                        walk(child, source, symbols, imports, calls, Some(&name), current_function, language);
                    }
                }
                return;
            }
        }
        "interface_declaration" => {
            if let Some(sym) = extract_interface(node, source) {
                symbols.push(sym);
            }
        }
        "type_alias_declaration" => {
            if let Some(sym) = extract_type_alias(node, source) {
                symbols.push(sym);
            }
        }
        "import_statement" => {
            if let Some(import) = extract_import(node, source) {
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
        "export_statement" => {
            // Handle exported declarations
            for child in node.children(&mut node.walk()) {
                walk(child, source, symbols, imports, calls, parent_name, current_function, language);
            }
            return;
        }
        "variable_declaration" => {
            // Check for const function assignments: const foo = () => {}
            for declarator in node.children(&mut node.walk()) {
                if declarator.kind() == "variable_declarator" {
                    if let Some(value) = declarator.child_by_field_name("value") {
                        if value.kind() == "arrow_function" || value.kind() == "function" {
                            if let Some(name_node) = declarator.child_by_field_name("name") {
                                let name = node_text(name_node, source);
                                if let Some(mut sym) = extract_function(value, source, parent_name, language) {
                                    sym.name = name.clone();
                                    sym.qualified_name = Some(name);
                                    symbols.push(sym);
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }

    for child in node.children(&mut node.walk()) {
        walk(child, source, symbols, imports, calls, parent_name, current_function, language);
    }
}

fn extract_function(node: Node, source: &[u8], parent_name: Option<&str>, language: &str) -> Option<Symbol> {
    let name = node.child_by_field_name("name")
        .map(|n| node_text(n, source))
        .unwrap_or_else(|| "<anonymous>".to_string());

    let qualified_name = match parent_name {
        Some(parent) => format!("{}.{}", parent, name),
        None => name.clone(),
    };

    let signature = node.child_by_field_name("parameters")
        .map(|n| node_text(n, source));

    let is_async = node.children(&mut node.walk())
        .any(|n| n.kind() == "async");

    let is_test = name.starts_with("test") || name.contains("Test") ||
                  name.starts_with("it(") || name.starts_with("describe(");

    Some(Symbol {
        name,
        qualified_name: Some(qualified_name),
        symbol_type: "function".to_string(),
        language: language.to_string(),
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        signature,
        visibility: None,
        documentation: None,
        is_test,
        is_async,
    })
}

fn extract_class(node: Node, source: &[u8], language: &str) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);

    Some(Symbol {
        name: name.clone(),
        qualified_name: Some(name),
        symbol_type: "class".to_string(),
        language: language.to_string(),
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        signature: None,
        visibility: None,
        documentation: None,
        is_test: false,
        is_async: false,
    })
}

fn extract_interface(node: Node, source: &[u8]) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);

    Some(Symbol {
        name: name.clone(),
        qualified_name: Some(name),
        symbol_type: "interface".to_string(),
        language: "typescript".to_string(),
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        signature: None,
        visibility: None,
        documentation: None,
        is_test: false,
        is_async: false,
    })
}

fn extract_type_alias(node: Node, source: &[u8]) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);

    Some(Symbol {
        name: name.clone(),
        qualified_name: Some(name),
        symbol_type: "type".to_string(),
        language: "typescript".to_string(),
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        signature: None,
        visibility: None,
        documentation: None,
        is_test: false,
        is_async: false,
    })
}

fn extract_import(node: Node, source: &[u8]) -> Option<Import> {
    let source_node = node.child_by_field_name("source")?;
    let path = node_text(source_node, source);
    let path = path.trim_matches(|c| c == '"' || c == '\'').to_string();

    let is_external = !path.starts_with('.') && !path.starts_with('/');

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
        "member_expression" => {
            function_node.child_by_field_name("property")
                .map(|n| node_text(n, source))?
        }
        _ => return None,
    };

    // Skip common builtins
    if matches!(callee_name.as_str(), "console" | "log" | "error" | "warn" | "info" |
                "setTimeout" | "setInterval" | "clearTimeout" | "clearInterval" |
                "parseInt" | "parseFloat" | "JSON" | "Object" | "Array" | "String" |
                "require" | "import") {
        return None;
    }

    let call_type = if function_node.kind() == "member_expression" {
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
