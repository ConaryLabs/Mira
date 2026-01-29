// src/indexer/parsers/python.rs
// Python language parser using tree-sitter

use anyhow::{Result, anyhow};
use tree_sitter::{Node, Parser};

use super::{FunctionCall, Import, LanguageParser, NodeExt, ParseResult, Symbol, SymbolBuilder, node_text};

/// Python language parser
pub struct PythonParser;

impl LanguageParser for PythonParser {
    fn language_id(&self) -> &'static str {
        "python"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["py"]
    }

    fn configure_parser(&self, parser: &mut Parser) -> Result<()> {
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .map_err(|e| anyhow!("Failed to set Python language: {}", e))
    }

    fn parse(&self, parser: &mut Parser, content: &str) -> Result<ParseResult> {
        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse Python code"))?;

        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let mut calls = Vec::new();
        let bytes = content.as_bytes();

        walk(
            tree.root_node(),
            bytes,
            &mut symbols,
            &mut imports,
            &mut calls,
            None,
            None,
        );
        Ok((symbols, imports, calls))
    }
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
                let func_name = sym
                    .qualified_name
                    .clone()
                    .unwrap_or_else(|| sym.name.clone());
                symbols.push(sym);
                if let Some(body) = node.child_by_field_name("body") {
                    for child in body.children(&mut body.walk()) {
                        walk(
                            child,
                            source,
                            symbols,
                            imports,
                            calls,
                            parent_name,
                            Some(&func_name),
                        );
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
                        walk(
                            child,
                            source,
                            symbols,
                            imports,
                            calls,
                            Some(&name),
                            current_function,
                        );
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
        walk(
            child,
            source,
            symbols,
            imports,
            calls,
            parent_name,
            current_function,
        );
    }
}

fn extract_function(node: Node, source: &[u8], parent_name: Option<&str>) -> Option<Symbol> {
    let name = node.field_text("name", source)?;
    let is_test = name.starts_with("test_") || name.starts_with("test");
    SymbolBuilder::new(node, source, "python")
        .name(name)
        .qualified_with_parent(parent_name, ".")
        .symbol_type("function")
        .signature_from_field("parameters")
        .is_test(is_test)
        .is_async(node.has_child_kind("async"))
        .build()
}

fn extract_class(node: Node, source: &[u8]) -> Option<Symbol> {
    SymbolBuilder::new(node, source, "python")
        .name_from_field("name")
        .qualified_with_parent(None, ".")
        .symbol_type("class")
        .signature_from_field("superclasses")
        .build()
}

fn extract_import(node: Node, source: &[u8]) -> Option<Import> {
    let path = if node.kind() == "import_from_statement" {
        node.field_text("module_name", source)?
    } else {
        node.find_child_text("dotted_name", source)?
    };

    Some(Import {
        import_path: path.clone(),
        imported_symbols: None,
        is_external: !path.starts_with('.'),
    })
}

fn extract_call(node: Node, source: &[u8], caller: &str) -> Option<FunctionCall> {
    let function_node = node.child_by_field_name("function")?;
    let callee_name = match function_node.kind() {
        "identifier" => node_text(function_node, source),
        "attribute" => function_node.field_text("attribute", source)?,
        _ => return None,
    };

    // Skip common builtins
    if matches!(
        callee_name.as_str(),
        "print"
            | "len"
            | "str"
            | "int"
            | "float"
            | "list"
            | "dict"
            | "set"
            | "tuple"
            | "range"
            | "enumerate"
            | "zip"
            | "open"
            | "type"
            | "isinstance"
            | "hasattr"
            | "getattr"
            | "setattr"
            | "super"
            | "sorted"
            | "reversed"
            | "map"
            | "filter"
            | "any"
            | "all"
    ) {
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
        call_line: node.start_line(),
        call_type: call_type.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_python(code: &str) -> ParseResult {
        let python_parser = PythonParser;
        let mut parser = tree_sitter::Parser::new();
        python_parser.configure_parser(&mut parser).unwrap();
        python_parser.parse(&mut parser, code).unwrap()
    }

    #[test]
    fn test_parse_function() {
        let code = r#"
def hello_world():
    print("Hello")
"#;
        let (symbols, _, _) = parse_python(code);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "hello_world");
        assert_eq!(symbols[0].symbol_type, "function");
        assert_eq!(symbols[0].language, "python");
    }

    #[test]
    fn test_parse_async_function() {
        let code = r#"
async def fetch_data():
    return "data"
"#;
        let (symbols, _, _) = parse_python(code);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "fetch_data");
        assert!(symbols[0].is_async);
    }

    #[test]
    fn test_parse_class_with_methods() {
        let code = r#"
class MyClass:
    def __init__(self):
        self.value = 0

    def get_value(self):
        return self.value

    async def async_method(self):
        pass
"#;
        let (symbols, _, _) = parse_python(code);

        let class_sym = symbols.iter().find(|s| s.name == "MyClass").unwrap();
        assert_eq!(class_sym.symbol_type, "class");

        let init_sym = symbols.iter().find(|s| s.name == "__init__").unwrap();
        assert_eq!(
            init_sym.qualified_name,
            Some("MyClass.__init__".to_string())
        );

        let async_sym = symbols.iter().find(|s| s.name == "async_method").unwrap();
        assert!(async_sym.is_async);
    }

    #[test]
    fn test_parse_imports() {
        let code = r#"
import os
import json
from typing import List, Dict
from .local_module import helper
from ..parent import util
"#;
        let (_, imports, _) = parse_python(code);

        assert!(imports.len() >= 3);

        let os_import = imports.iter().find(|i| i.import_path == "os").unwrap();
        assert!(os_import.is_external);

        let local_import = imports
            .iter()
            .find(|i| i.import_path.contains("local_module"));
        assert!(local_import.is_some());
    }

    #[test]
    fn test_parse_test_function() {
        let code = r#"
def test_something():
    assert True

async def test_async_feature():
    assert True
"#;
        let (symbols, _, _) = parse_python(code);

        let test_sym = symbols.iter().find(|s| s.name == "test_something").unwrap();
        assert!(test_sym.is_test);

        let async_test = symbols
            .iter()
            .find(|s| s.name == "test_async_feature")
            .unwrap();
        assert!(async_test.is_test);
        assert!(async_test.is_async);
    }

    #[test]
    fn test_parse_decorated_function() {
        let code = r#"
@decorator
def decorated_func():
    pass

@property
def my_property(self):
    return self._value
"#;
        let (symbols, _, _) = parse_python(code);
        assert!(symbols.iter().any(|s| s.name == "decorated_func"));
        assert!(symbols.iter().any(|s| s.name == "my_property"));
    }
}
