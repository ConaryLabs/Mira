// src/indexer/parsers/python.rs
// Python language parser using tree-sitter

use anyhow::{Result, anyhow};
use tree_sitter::{Node, Parser};

use super::{
    FunctionCall, Import, LanguageParser, NodeExt, ParseContext, ParseResult, Symbol,
    SymbolBuilder, default_parse, node_text,
};

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
        default_parse(parser, content, "python", walk)
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
        "function_definition" => {
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
        "class_definition" => {
            if let Some(sym) = extract_class(node, ctx.source) {
                let name = sym.name.clone();
                ctx.symbols.push(sym);
                if let Some(body) = node.child_by_field_name("body") {
                    for child in body.children(&mut body.walk()) {
                        walk(child, ctx, Some(&name), current_function);
                    }
                }
                return;
            }
        }
        "import_statement" | "import_from_statement" => {
            if let Some(import) = extract_import(node, ctx.source) {
                ctx.imports.push(import);
            }
        }
        "call" => {
            if let Some(caller) = current_function
                && let Some(call) = extract_call(node, ctx.source, caller)
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

/// Extract docstring from a function or class body node.
/// Returns the content of the first string expression if it's the first statement.
fn get_docstring(node: Node, source: &[u8]) -> Option<String> {
    let body = node.child_by_field_name("body")?;
    let mut cursor = body.walk();
    let first_stmt = body.named_children(&mut cursor).next()?;
    if first_stmt.kind() != "expression_statement" {
        return None;
    }
    let mut ec = first_stmt.walk();
    let expr = first_stmt.named_children(&mut ec).next()?;
    if expr.kind() != "string" {
        return None;
    }
    let raw = node_text(expr, source);
    let stripped = if raw.starts_with("\"\"\"") || raw.starts_with("'''") {
        let text: &str = raw
            .strip_prefix("\"\"\"")
            .or_else(|| raw.strip_prefix("'''"))
            .unwrap_or(&raw);
        let text = text
            .strip_suffix("\"\"\"")
            .or_else(|| text.strip_suffix("'''"))
            .unwrap_or(text);
        text.trim().to_string()
    } else {
        raw.trim_matches('"').trim_matches('\'').to_string()
    };
    if stripped.is_empty() {
        None
    } else {
        Some(stripped)
    }
}

/// Extract decorators from a decorated_definition parent node.
fn get_decorators(node: Node, source: &[u8]) -> Option<Vec<String>> {
    let parent = node.parent()?;
    if parent.kind() != "decorated_definition" {
        return None;
    }
    let decorators: Vec<String> = parent
        .children(&mut parent.walk())
        .filter(|n| n.kind() == "decorator")
        .map(|n| node_text(n, source))
        .collect();
    if decorators.is_empty() {
        None
    } else {
        Some(decorators)
    }
}

fn extract_function(node: Node, source: &[u8], parent_name: Option<&str>) -> Option<Symbol> {
    let name = node.field_text("name", source)?;
    // Fix: only flag exact "test" or names starting with "test_"
    let is_test = name.starts_with("test_") || name == "test";
    let return_type = node.field_text("return_type", source);
    let documentation = get_docstring(node, source);
    let decorators = get_decorators(node, source);
    SymbolBuilder::new(node, source, "python")
        .name(name)
        .qualified_with_parent(parent_name, ".")
        .symbol_type("function")
        .signature_from_field("parameters")
        .is_test(is_test)
        .is_async(node.has_child_kind("async"))
        .return_type(return_type)
        .documentation(documentation)
        .decorators(decorators)
        .build()
}

fn extract_class(node: Node, source: &[u8]) -> Option<Symbol> {
    let documentation = get_docstring(node, source);
    let decorators = get_decorators(node, source);
    SymbolBuilder::new(node, source, "python")
        .name_from_field("name")
        .qualified_with_parent(None, ".")
        .symbol_type("class")
        .signature_from_field("superclasses")
        .documentation(documentation)
        .decorators(decorators)
        .build()
}

fn extract_import(node: Node, source: &[u8]) -> Option<Import> {
    let path = if node.kind() == "import_from_statement" {
        node.field_text("module_name", source)?
    } else {
        node.find_child_text("dotted_name", source)?
    };

    let imported_symbols = if node.kind() == "import_from_statement" {
        let mut cursor = node.walk();
        let mut children = node.named_children(&mut cursor);
        // Skip module_name (first named child is the module)
        let _ = children.next();
        let names: Vec<String> = children
            .filter(|n| n.kind() == "dotted_name" || n.kind() == "aliased_import")
            .map(|n| {
                if n.kind() == "aliased_import" {
                    let mut ac = n.walk();
                    n.named_children(&mut ac)
                        .last()
                        .map(|id| node_text(id, source))
                        .unwrap_or_else(|| node_text(n, source))
                } else {
                    node_text(n, source)
                }
            })
            .collect();
        if names.is_empty() { None } else { Some(names) }
    } else {
        None
    };

    Some(Import {
        import_path: path.clone(),
        imported_symbols,
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
        crate::indexer::parsers::parse_with(&PythonParser, code)
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

    #[test]
    fn test_is_test_not_too_broad() {
        // Functions like "testimony" should NOT be flagged as tests
        let code = r#"
def testimony():
    pass

def testify():
    pass

def test_real():
    assert True
"#;
        let (symbols, _, _) = parse_python(code);

        let testimony = symbols.iter().find(|s| s.name == "testimony").unwrap();
        assert!(!testimony.is_test, "testimony should not be a test");

        let testify = symbols.iter().find(|s| s.name == "testify").unwrap();
        assert!(!testify.is_test, "testify should not be a test");

        let test_real = symbols.iter().find(|s| s.name == "test_real").unwrap();
        assert!(test_real.is_test, "test_real should be a test");
    }

    #[test]
    fn test_docstring_extraction_function() {
        let code = r#"
def greet(name):
    """Greet the given name."""
    return f"Hello, {name}"
"#;
        let (symbols, _, _) = parse_python(code);
        let sym = symbols.iter().find(|s| s.name == "greet").unwrap();
        assert_eq!(sym.documentation, Some("Greet the given name.".to_string()));
    }

    #[test]
    fn test_docstring_extraction_class() {
        let code = r#"
class Greeter:
    """A class for greeting people."""

    def hello(self):
        pass
"#;
        let (symbols, _, _) = parse_python(code);
        let sym = symbols.iter().find(|s| s.name == "Greeter").unwrap();
        assert_eq!(
            sym.documentation,
            Some("A class for greeting people.".to_string())
        );
    }

    #[test]
    fn test_no_docstring_when_absent() {
        let code = r#"
def no_doc():
    x = 1
    return x
"#;
        let (symbols, _, _) = parse_python(code);
        let sym = symbols.iter().find(|s| s.name == "no_doc").unwrap();
        assert!(sym.documentation.is_none());
    }

    #[test]
    fn test_decorator_extraction() {
        let code = r#"
@property
def my_prop(self):
    return self._val

@staticmethod
def static_func():
    pass
"#;
        let (symbols, _, _) = parse_python(code);

        let prop = symbols.iter().find(|s| s.name == "my_prop").unwrap();
        assert!(prop.decorators.is_some());
        let decs = prop.decorators.as_ref().unwrap();
        assert!(decs.iter().any(|d| d.contains("property")));

        let sf = symbols.iter().find(|s| s.name == "static_func").unwrap();
        assert!(sf.decorators.is_some());
        let decs = sf.decorators.as_ref().unwrap();
        assert!(decs.iter().any(|d| d.contains("staticmethod")));
    }

    #[test]
    fn test_no_decorators_on_plain_function() {
        let code = r#"
def plain():
    pass
"#;
        let (symbols, _, _) = parse_python(code);
        let sym = symbols.iter().find(|s| s.name == "plain").unwrap();
        assert!(sym.decorators.is_none());
    }

    #[test]
    fn test_return_type_annotation() {
        let code = r#"
def add(a: int, b: int) -> int:
    return a + b

def greet() -> str:
    return "hello"
"#;
        let (symbols, _, _) = parse_python(code);

        let add = symbols.iter().find(|s| s.name == "add").unwrap();
        assert!(add.return_type.is_some());
        assert!(add.return_type.as_ref().unwrap().contains("int"));

        let greet = symbols.iter().find(|s| s.name == "greet").unwrap();
        assert!(greet.return_type.is_some());
        assert!(greet.return_type.as_ref().unwrap().contains("str"));
    }

    #[test]
    fn test_no_return_type_when_absent() {
        let code = r#"
def no_annotation():
    pass
"#;
        let (symbols, _, _) = parse_python(code);
        let sym = symbols.iter().find(|s| s.name == "no_annotation").unwrap();
        assert!(sym.return_type.is_none());
    }

    #[test]
    fn test_imported_symbols_from_import() {
        let code = r#"
from typing import List, Dict
from os.path import join, exists
"#;
        let (_, imports, _) = parse_python(code);

        let typing_import = imports.iter().find(|i| i.import_path == "typing").unwrap();
        let syms = typing_import.imported_symbols.as_ref().unwrap();
        assert!(syms.contains(&"List".to_string()));
        assert!(syms.contains(&"Dict".to_string()));

        let path_import = imports.iter().find(|i| i.import_path == "os.path").unwrap();
        let syms = path_import.imported_symbols.as_ref().unwrap();
        assert!(syms.contains(&"join".to_string()));
        assert!(syms.contains(&"exists".to_string()));
    }

    #[test]
    fn test_no_imported_symbols_for_plain_import() {
        let code = r#"
import os
import json
"#;
        let (_, imports, _) = parse_python(code);

        let os_import = imports.iter().find(|i| i.import_path == "os").unwrap();
        assert!(os_import.imported_symbols.is_none());
    }
}
