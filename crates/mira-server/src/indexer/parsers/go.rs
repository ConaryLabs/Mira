// src/indexer/parsers/go.rs
// Go language parser using tree-sitter

use anyhow::{Result, anyhow};
use tree_sitter::{Node, Parser};

use super::{
    FunctionCall, Import, LanguageParser, NodeExt, ParseContext, ParseResult, Symbol,
    default_parse, node_text,
};

/// Go language parser
pub struct GoParser;

impl LanguageParser for GoParser {
    fn language_id(&self) -> &'static str {
        "go"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["go"]
    }

    fn configure_parser(&self, parser: &mut Parser) -> Result<()> {
        parser
            .set_language(&tree_sitter_go::LANGUAGE.into())
            .map_err(|e| anyhow!("Failed to set Go language: {}", e))
    }

    fn parse(&self, parser: &mut Parser, content: &str) -> Result<ParseResult> {
        default_parse(parser, content, "go", walk)
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
        "function_declaration" | "method_declaration" => {
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
        "type_declaration" => {
            let syms = extract_type(node, ctx.source);
            if !syms.is_empty() {
                let first_name = syms[0].name.clone();
                for sym in syms {
                    ctx.symbols.push(sym);
                }
                for child in node.children(&mut node.walk()) {
                    walk(child, ctx, Some(&first_name), current_function);
                }
                return;
            }
        }
        "import_declaration" => {
            for import in extract_imports(node, ctx.source) {
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
        "const_declaration" | "var_declaration" => {
            let is_const = node.kind() == "const_declaration";
            for sym in extract_const_or_var(node, ctx.source, is_const) {
                ctx.symbols.push(sym);
            }
        }
        _ => {}
    }

    for child in node.children(&mut node.walk()) {
        walk(child, ctx, parent_name, current_function);
    }
}

fn extract_function(node: Node, source: &[u8], parent_name: Option<&str>) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = node_text(name_node, source);

    // For method declarations, get the receiver type
    let receiver = if node.kind() == "method_declaration" {
        node.child_by_field_name("receiver").and_then(|r| {
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

    let signature = node
        .child_by_field_name("parameters")
        .map(|n| node_text(n, source));

    let return_type = node
        .child_by_field_name("result")
        .map(|n| node_text(n, source));

    // Check for test functions
    let is_test =
        name.starts_with("Test") || name.starts_with("Benchmark") || name.starts_with("Example");

    // Check visibility (exported = starts with uppercase)
    let visibility = if name
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
    {
        Some("public".to_string())
    } else {
        Some("private".to_string())
    };

    let documentation = get_doc_comment(node, source);

    Some(Symbol {
        name,
        qualified_name: Some(qualified_name),
        symbol_type: "function".to_string(),
        language: "go".to_string(),
        start_line: node.start_line(),
        end_line: node.end_line(),
        signature,
        visibility,
        documentation,
        is_test,
        is_async: false, // Go doesn't have async keyword
        return_type,
        decorators: None,
    })
}

fn extract_type(node: Node, source: &[u8]) -> Vec<Symbol> {
    let mut symbols = Vec::new();

    for child in node.children(&mut node.walk()) {
        if child.kind() != "type_spec" {
            continue;
        }

        let name_node = match child.child_by_field_name("name") {
            Some(n) => n,
            None => continue,
        };
        let name = node_text(name_node, source);

        let symbol_type = child
            .child_by_field_name("type")
            .map(|t| match t.kind() {
                "struct_type" => "struct",
                "interface_type" => "interface",
                _ => "type",
            })
            .unwrap_or("type");

        let visibility = if name
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false)
        {
            Some("public".to_string())
        } else {
            Some("private".to_string())
        };

        let documentation = get_doc_comment(child, source);

        symbols.push(Symbol {
            name: name.clone(),
            qualified_name: Some(name),
            symbol_type: symbol_type.to_string(),
            language: "go".to_string(),
            start_line: child.start_line(),
            end_line: child.end_line(),
            signature: None,
            visibility,
            documentation,
            is_test: false,
            is_async: false,
            return_type: None,
            decorators: None,
        });
    }

    symbols
}

fn extract_import_spec(spec: Node, source: &[u8]) -> Option<Import> {
    let path_node = spec.child_by_field_name("path")?;
    let path = node_text(path_node, source).trim_matches('"').to_string();

    let is_external = !path.starts_with('.');

    // Check for alias: `import db "database/sql"` has a `name` field child (package_identifier)
    let imported_symbols = spec
        .child_by_field_name("name")
        .map(|alias| vec![node_text(alias, source)]);

    Some(Import {
        import_path: path,
        imported_symbols,
        is_external,
    })
}

fn extract_imports(node: Node, source: &[u8]) -> Vec<Import> {
    let mut imports = Vec::new();

    // Go imports can be single or grouped
    for child in node.children(&mut node.walk()) {
        if child.kind() == "import_spec" {
            if let Some(import) = extract_import_spec(child, source) {
                imports.push(import);
            }
        } else if child.kind() == "import_spec_list" {
            for spec in child.children(&mut child.walk()) {
                if spec.kind() == "import_spec"
                    && let Some(import) = extract_import_spec(spec, source)
                {
                    imports.push(import);
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

fn extract_spec(spec: Node, source: &[u8], symbol_type: &str, symbols: &mut Vec<Symbol>) {
    let return_type = spec
        .child_by_field_name("type")
        .map(|t| node_text(t, source));
    let documentation = get_doc_comment(spec, source);

    // Go allows multi-name specs: `const a, b = 1, 2`
    // Iterate all identifier children to handle each name separately.
    let names: Vec<String> = spec
        .children(&mut spec.walk())
        .filter(|n| n.kind() == "identifier")
        .map(|n| node_text(n, source))
        .collect();

    if names.is_empty() {
        return;
    }

    for name in names {
        let visibility = if name
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false)
        {
            Some("public".to_string())
        } else {
            Some("private".to_string())
        };
        symbols.push(Symbol {
            name: name.clone(),
            qualified_name: Some(name),
            symbol_type: symbol_type.to_string(),
            language: "go".to_string(),
            start_line: spec.start_line(),
            end_line: spec.end_line(),
            signature: None,
            visibility,
            documentation: documentation.clone(),
            is_test: false,
            is_async: false,
            return_type: return_type.clone(),
            decorators: None,
        });
    }
}

fn extract_const_or_var(node: Node, source: &[u8], is_const: bool) -> Vec<Symbol> {
    let symbol_type = if is_const { "constant" } else { "variable" };
    let mut symbols = Vec::new();

    let spec_kind = if is_const { "const_spec" } else { "var_spec" };
    let list_kind = if is_const {
        "const_spec_list"
    } else {
        "var_spec_list"
    };
    for child in node.children(&mut node.walk()) {
        if child.kind() == spec_kind {
            extract_spec(child, source, symbol_type, &mut symbols);
        } else if child.kind() == list_kind {
            // grouped: const ( ... ) or var ( ... )
            for spec in child.children(&mut child.walk()) {
                if spec.kind() == spec_kind {
                    extract_spec(spec, source, symbol_type, &mut symbols);
                }
            }
        }
    }
    symbols
}

/// Extract doc comment lines immediately preceding a node (Go `// ...` style).
fn get_doc_comment(node: Node, source: &[u8]) -> Option<String> {
    let mut docs = Vec::new();

    let mut sib = node.prev_sibling();
    while let Some(n) = sib {
        if n.kind() == "comment" {
            let text = node_text(n, source);
            if let Some(stripped) = text.strip_prefix("// ") {
                docs.push(stripped.to_string());
            } else if let Some(stripped) = text.strip_prefix("//") {
                docs.push(stripped.to_string());
            } else {
                break;
            }
        } else {
            break;
        }
        sib = n.prev_sibling();
    }

    if docs.is_empty() {
        None
    } else {
        docs.reverse();
        Some(docs.join("\n"))
    }
}

fn extract_call(node: Node, source: &[u8], caller: &str) -> Option<FunctionCall> {
    let function_node = node.child_by_field_name("function")?;
    let callee_name = match function_node.kind() {
        "identifier" => node_text(function_node, source),
        "selector_expression" => function_node
            .child_by_field_name("field")
            .map(|n| node_text(n, source))?,
        _ => return None,
    };

    // Skip common stdlib functions
    if matches!(
        callee_name.as_str(),
        "make"
            | "new"
            | "append"
            | "len"
            | "cap"
            | "copy"
            | "delete"
            | "panic"
            | "recover"
            | "print"
            | "println"
            | "close"
            | "complex"
            | "real"
            | "imag"
    ) {
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
        call_line: node.start_line(),
        call_type: call_type.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_go(code: &str) -> ParseResult {
        crate::indexer::parsers::parse_with(&GoParser, code)
    }

    #[test]
    fn test_parse_function() {
        let code = r#"
package main

func helloWorld() {
    fmt.Println("Hello")
}
"#;
        let (symbols, _, _) = parse_go(code);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "helloWorld");
        assert_eq!(symbols[0].symbol_type, "function");
        assert_eq!(symbols[0].language, "go");
    }

    #[test]
    fn test_parse_exported_function() {
        let code = r#"
package main

func PublicFunc() string {
    return "public"
}

func privateFunc() string {
    return "private"
}
"#;
        let (symbols, _, _) = parse_go(code);

        let public_sym = symbols.iter().find(|s| s.name == "PublicFunc").unwrap();
        assert_eq!(public_sym.visibility, Some("public".to_string()));

        let private_sym = symbols.iter().find(|s| s.name == "privateFunc").unwrap();
        assert_eq!(private_sym.visibility, Some("private".to_string()));
    }

    #[test]
    fn test_parse_struct_with_methods() {
        let code = r#"
package main

type MyStruct struct {
    Value int
}

func (m *MyStruct) GetValue() int {
    return m.Value
}

func (m MyStruct) String() string {
    return "MyStruct"
}
"#;
        let (symbols, _, _) = parse_go(code);

        let struct_sym = symbols.iter().find(|s| s.name == "MyStruct").unwrap();
        assert_eq!(struct_sym.symbol_type, "struct");

        let method_sym = symbols.iter().find(|s| s.name == "GetValue").unwrap();
        assert_eq!(method_sym.symbol_type, "function");
        // Methods are captured with qualified names
        assert!(method_sym.qualified_name.is_some());
    }

    #[test]
    fn test_parse_interface() {
        let code = r#"
package main

type Reader interface {
    Read(p []byte) (n int, err error)
}
"#;
        let (symbols, _, _) = parse_go(code);

        let interface_sym = symbols.iter().find(|s| s.name == "Reader").unwrap();
        assert_eq!(interface_sym.symbol_type, "interface");
    }

    #[test]
    fn test_parse_imports() {
        let code = r#"
package main

import (
    "fmt"
    "os"
    "github.com/user/repo/pkg"
)
"#;
        let (_, imports, _) = parse_go(code);

        assert!(imports.len() >= 3);

        let fmt_import = imports.iter().find(|i| i.import_path == "fmt").unwrap();
        assert!(fmt_import.is_external);

        let pkg_import = imports
            .iter()
            .find(|i| i.import_path.contains("github.com"))
            .unwrap();
        assert!(pkg_import.is_external);
    }

    #[test]
    fn test_parse_test_function() {
        let code = r#"
package main

func TestSomething(t *testing.T) {
    // test code
}

func BenchmarkOperation(b *testing.B) {
    // benchmark code
}

func ExampleUsage() {
    // example code
}
"#;
        let (symbols, _, _) = parse_go(code);

        let test_sym = symbols.iter().find(|s| s.name == "TestSomething").unwrap();
        assert!(test_sym.is_test);

        let bench_sym = symbols
            .iter()
            .find(|s| s.name == "BenchmarkOperation")
            .unwrap();
        assert!(bench_sym.is_test);

        let example_sym = symbols.iter().find(|s| s.name == "ExampleUsage").unwrap();
        assert!(example_sym.is_test);
    }

    #[test]
    fn test_parse_single_import() {
        let code = r#"
package main

import "fmt"
"#;
        let (_, imports, _) = parse_go(code);

        assert!(!imports.is_empty());
        assert!(imports.iter().any(|i| i.import_path == "fmt"));
    }

    #[test]
    fn test_grouped_type_declarations() {
        let code = r#"
package main

type (
    Foo struct {
        X int
    }
    Bar interface {
        Method() string
    }
    Baz int
)
"#;
        let (symbols, _, _) = parse_go(code);

        let foo = symbols.iter().find(|s| s.name == "Foo").unwrap();
        assert_eq!(foo.symbol_type, "struct");

        let bar = symbols.iter().find(|s| s.name == "Bar").unwrap();
        assert_eq!(bar.symbol_type, "interface");

        let baz = symbols.iter().find(|s| s.name == "Baz").unwrap();
        assert_eq!(baz.symbol_type, "type");
    }

    #[test]
    fn test_parse_constants() {
        let code = r#"
package main

const (
    MaxRetries = 3
    DefaultTimeout = 30
)

const SingleConst = "hello"
"#;
        let (symbols, _, _) = parse_go(code);

        let max_retries = symbols.iter().find(|s| s.name == "MaxRetries").unwrap();
        assert_eq!(max_retries.symbol_type, "constant");
        assert_eq!(max_retries.visibility, Some("public".to_string()));

        let default_timeout = symbols.iter().find(|s| s.name == "DefaultTimeout").unwrap();
        assert_eq!(default_timeout.symbol_type, "constant");

        let single = symbols.iter().find(|s| s.name == "SingleConst").unwrap();
        assert_eq!(single.symbol_type, "constant");
    }

    #[test]
    fn test_parse_variables() {
        let code = r#"
package main

var (
    globalCount int
    GlobalName  string
)
"#;
        let (symbols, _, _) = parse_go(code);

        let count = symbols.iter().find(|s| s.name == "globalCount").unwrap();
        assert_eq!(count.symbol_type, "variable");
        assert_eq!(count.visibility, Some("private".to_string()));

        let name = symbols.iter().find(|s| s.name == "GlobalName").unwrap();
        assert_eq!(name.symbol_type, "variable");
        assert_eq!(name.visibility, Some("public".to_string()));
    }

    #[test]
    fn test_parse_go_doc_comments() {
        let code = r#"
package main

// MyFunc does something useful.
// It takes no arguments.
func MyFunc() {}
"#;
        let (symbols, _, _) = parse_go(code);

        let func_sym = symbols.iter().find(|s| s.name == "MyFunc").unwrap();
        assert!(func_sym.documentation.is_some());
        let doc = func_sym.documentation.as_deref().unwrap();
        assert!(doc.contains("does something useful"), "doc: {}", doc);
    }

    #[test]
    fn test_parse_go_return_types() {
        let code = r#"
package main

func GetName() string {
    return "hello"
}

func GetPair() (int, error) {
    return 0, nil
}

func NoReturn() {
}
"#;
        let (symbols, _, _) = parse_go(code);

        let get_name = symbols.iter().find(|s| s.name == "GetName").unwrap();
        assert!(get_name.return_type.is_some());
        assert!(
            get_name.return_type.as_deref().unwrap().contains("string"),
            "got: {:?}",
            get_name.return_type
        );

        let get_pair = symbols.iter().find(|s| s.name == "GetPair").unwrap();
        assert!(get_pair.return_type.is_some());

        let no_return = symbols.iter().find(|s| s.name == "NoReturn").unwrap();
        assert!(no_return.return_type.is_none());
    }

    #[test]
    fn test_parse_go_import_alias() {
        let code = r#"
package main

import (
    db "database/sql"
    "fmt"
)
"#;
        let (_, imports, _) = parse_go(code);

        let db_import = imports
            .iter()
            .find(|i| i.import_path == "database/sql")
            .unwrap();
        assert_eq!(db_import.imported_symbols, Some(vec!["db".to_string()]));

        let fmt_import = imports.iter().find(|i| i.import_path == "fmt").unwrap();
        assert!(fmt_import.imported_symbols.is_none());
    }
}
