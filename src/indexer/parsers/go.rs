// src/indexer/parsers/go.rs
// Go language parser using tree-sitter

use anyhow::{anyhow, Result};
use tree_sitter::{Parser, Node};

use super::{Symbol, Import, FunctionCall, ParseResult, node_text};

/// Parse Go source code
pub fn parse(parser: &mut Parser, content: &str) -> Result<ParseResult> {
    let tree = parser.parse(content, None)
        .ok_or_else(|| anyhow!("Failed to parse Go code"))?;

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
        "function_declaration" | "method_declaration" => {
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
        "type_declaration" => {
            if let Some(sym) = extract_type(node, source) {
                let name = sym.name.clone();
                symbols.push(sym);
                for child in node.children(&mut node.walk()) {
                    walk(child, source, symbols, imports, calls, Some(&name), current_function);
                }
                return;
            }
        }
        "import_declaration" => {
            for import in extract_imports(node, source) {
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
        "const_declaration" | "var_declaration" => {
            // Could extract top-level constants/vars as symbols
            // For now, skip
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

    // For method declarations, get the receiver type
    let receiver = if node.kind() == "method_declaration" {
        node.child_by_field_name("receiver")
            .and_then(|r| {
                // Find the type identifier in the receiver
                for child in r.children(&mut r.walk()) {
                    if child.kind() == "type_identifier" || child.kind() == "pointer_type" {
                        return Some(node_text(child, source));
                    }
                }
                None
            })
    } else {
        None
    };

    let qualified_name = match (parent_name, receiver) {
        (Some(parent), _) => format!("{}.{}", parent, name),
        (None, Some(recv)) => format!("{}.{}", recv.trim_start_matches('*'), name),
        (None, None) => name.clone(),
    };

    let signature = node.child_by_field_name("parameters")
        .map(|n| node_text(n, source));

    // Check for test functions
    let is_test = name.starts_with("Test") || name.starts_with("Benchmark") || name.starts_with("Example");

    // Check visibility (exported = starts with uppercase)
    let visibility = if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
        Some("public".to_string())
    } else {
        Some("private".to_string())
    };

    Some(Symbol {
        name,
        qualified_name: Some(qualified_name),
        symbol_type: "function".to_string(),
        language: "go".to_string(),
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        signature,
        visibility,
        documentation: None,
        is_test,
        is_async: false, // Go doesn't have async keyword
    })
}

fn extract_type(node: Node, source: &[u8]) -> Option<Symbol> {
    // Go type declarations can contain multiple type specs
    for child in node.children(&mut node.walk()) {
        if child.kind() == "type_spec" {
            let name_node = child.child_by_field_name("name")?;
            let name = node_text(name_node, source);

            let symbol_type = child.child_by_field_name("type")
                .map(|t| match t.kind() {
                    "struct_type" => "struct",
                    "interface_type" => "interface",
                    _ => "type",
                })
                .unwrap_or("type");

            let visibility = if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                Some("public".to_string())
            } else {
                Some("private".to_string())
            };

            return Some(Symbol {
                name: name.clone(),
                qualified_name: Some(name),
                symbol_type: symbol_type.to_string(),
                language: "go".to_string(),
                start_line: node.start_position().row as u32 + 1,
                end_line: node.end_position().row as u32 + 1,
                signature: None,
                visibility,
                documentation: None,
                is_test: false,
                is_async: false,
            });
        }
    }
    None
}

fn extract_imports(node: Node, source: &[u8]) -> Vec<Import> {
    let mut imports = Vec::new();

    // Go imports can be single or grouped
    for child in node.children(&mut node.walk()) {
        if child.kind() == "import_spec" {
            if let Some(path_node) = child.child_by_field_name("path") {
                let path = node_text(path_node, source);
                let path = path.trim_matches('"').to_string();

                let is_external = !path.starts_with('.');

                imports.push(Import {
                    import_path: path,
                    imported_symbols: None,
                    is_external,
                });
            }
        } else if child.kind() == "import_spec_list" {
            for spec in child.children(&mut child.walk()) {
                if spec.kind() == "import_spec" {
                    if let Some(path_node) = spec.child_by_field_name("path") {
                        let path = node_text(path_node, source);
                        let path = path.trim_matches('"').to_string();

                        let is_external = !path.starts_with('.');

                        imports.push(Import {
                            import_path: path,
                            imported_symbols: None,
                            is_external,
                        });
                    }
                }
            }
        } else if child.kind() == "interpreted_string_literal" {
            // Single import without spec
            let path = node_text(child, source);
            let path = path.trim_matches('"').to_string();

            let is_external = !path.starts_with('.');

            imports.push(Import {
                import_path: path,
                imported_symbols: None,
                is_external,
            });
        }
    }

    imports
}

fn extract_call(node: Node, source: &[u8], caller: &str) -> Option<FunctionCall> {
    let function_node = node.child_by_field_name("function")?;
    let callee_name = match function_node.kind() {
        "identifier" => node_text(function_node, source),
        "selector_expression" => {
            function_node.child_by_field_name("field")
                .map(|n| node_text(n, source))?
        }
        _ => return None,
    };

    // Skip common stdlib functions
    if matches!(callee_name.as_str(), "make" | "new" | "append" | "len" | "cap" |
                "copy" | "delete" | "panic" | "recover" | "print" | "println" |
                "close" | "complex" | "real" | "imag") {
        return None;
    }

    let call_type = if function_node.kind() == "selector_expression" {
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
