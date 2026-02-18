// src/indexer/parsers/csharp.rs
// C# language parser using regex-based extraction (no tree-sitter grammar needed)

use anyhow::Result;
use regex::Regex;
use std::sync::LazyLock;
use tree_sitter::Parser;

use super::{FunctionCall, Import, LanguageParser, ParseResult, Symbol};

/// Compiled regex patterns for C# symbol extraction
struct CsharpPatterns {
    namespace: Regex,
    using_directive: Regex,
    class_like: Regex,
    method: Regex,
    property: Regex,
    field_const: Regex,
    doc_comment: Regex,
    test_attr: Regex,
    async_method: Regex,
}

static PATTERNS: LazyLock<CsharpPatterns> = LazyLock::new(|| CsharpPatterns {
    // namespace Foo.Bar.Baz
    namespace: Regex::new(r"^\s*namespace\s+([\w.]+)").unwrap(),

    // using Foo.Bar; or using static Foo; or using Alias = Foo;
    using_directive: Regex::new(r"^\s*using\s+(?:static\s+|[\w]+\s*=\s*)?([\w.]+(?:<[^>]+>)?);").unwrap(),

    // class / interface / struct / enum / record declarations
    // Captures: visibility modifiers + kind + name
    class_like: Regex::new(
        r"^\s*((?:(?:public|private|protected|internal|static|abstract|sealed|partial|readonly)\s+)*)(?:(class|interface|struct|enum|record))\s+([\w<>, ]+?)(?:\s*:\s*[\w<>, .]+?)?\s*(?:\{|$|where)",
    ).unwrap(),

    // Method declarations: visibility + return type + name + (params)
    // Matches: public async Task<T> MethodName(params) or void Method()
    method: Regex::new(
        r"^\s*((?:(?:public|private|protected|internal|static|virtual|override|abstract|sealed|new|async|extern)\s+)+)([\w<>\[\]?,. ]+?)\s+([\w]+)\s*(\([^)]*\))\s*(?:\{|=>|;|where)",
    ).unwrap(),

    // Property declarations: visibility + type + name + { get; set; } or { get; }
    property: Regex::new(
        r"^\s*((?:(?:public|private|protected|internal|static|virtual|override|abstract|sealed|new)\s+)+)([\w<>\[\]?,. ]+?)\s+([\w]+)\s*\{\s*(?:get|set|init)",
    ).unwrap(),

    // Constants and static readonly fields
    field_const: Regex::new(
        r"^\s*((?:(?:public|private|protected|internal|static|readonly|const)\s+)+)(?:const\s+)?([\w<>\[\]?,. ]+?)\s+([\w]+)\s*[=;]",
    ).unwrap(),

    // /// doc comment lines
    doc_comment: Regex::new(r"^\s*///\s*(.*)").unwrap(),

    // [Test] / [TestMethod] / [Fact] / [Theory] attributes
    test_attr: Regex::new(r"\[(?:Test|TestMethod|Fact|Theory|NUnit\.Framework\.Test)\]").unwrap(),

    // async keyword in method line
    async_method: Regex::new(r"\basync\b").unwrap(),
});

/// C# language parser (regex-based)
pub struct CsharpParser;

impl LanguageParser for CsharpParser {
    fn language_id(&self) -> &'static str {
        "csharp"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["cs"]
    }

    /// C# does not use tree-sitter — this is a no-op that leaves the parser unconfigured.
    /// The `parse()` method uses regex and does not call tree-sitter.
    fn configure_parser(&self, _parser: &mut Parser) -> Result<()> {
        Ok(())
    }

    fn parse(&self, _parser: &mut Parser, content: &str) -> Result<ParseResult> {
        Ok(parse_csharp(content))
    }
}

/// Regex-based C# parser that walks lines and extracts symbols, imports, and call edges.
fn parse_csharp(content: &str) -> ParseResult {
    let p = &*PATTERNS;
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len() as u32;

    let mut symbols: Vec<Symbol> = Vec::new();
    let mut imports: Vec<Import> = Vec::new();
    let mut calls: Vec<FunctionCall> = Vec::new();

    // Track parse state
    let mut current_namespace: Option<String> = None;
    let mut current_class: Option<String> = None;
    let mut current_method: Option<String> = None;
    let mut pending_docs: Vec<String> = Vec::new();
    let mut pending_test_attr = false;
    let mut brace_depth: i32 = 0;
    // Track depth at which current_class was entered so we can pop it
    let mut class_brace_depth: i32 = -1;
    let mut method_brace_depth: i32 = -1;

    // These are cheap-to-skip lines
    let is_blank_or_comment = |line: &str| {
        let t = line.trim();
        t.is_empty() || t.starts_with("//") || t.starts_with("/*") || t.starts_with("*")
    };

    for (idx, &line) in lines.iter().enumerate() {
        let lineno = idx as u32 + 1;

        // --- Track brace depth ---
        for ch in line.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth -= 1;
                    if brace_depth < 0 {
                        brace_depth = 0;
                    }
                    // Pop method context when we close its brace
                    if method_brace_depth >= 0 && brace_depth < method_brace_depth {
                        current_method = None;
                        method_brace_depth = -1;
                    }
                    // Pop class context when we close its brace
                    if class_brace_depth >= 0 && brace_depth < class_brace_depth {
                        current_class = None;
                        class_brace_depth = -1;
                    }
                }
                _ => {}
            }
        }

        // --- Accumulate doc comments ---
        if let Some(caps) = p.doc_comment.captures(line) {
            pending_docs.push(caps[1].trim().to_string());
            continue;
        }

        // --- Check for test attribute ---
        if p.test_attr.is_match(line) {
            pending_test_attr = true;
            continue;
        }

        // --- Clear accumulated docs/attrs on blank/comment lines that aren't doc ---
        if is_blank_or_comment(line) {
            // Don't clear — attribute lines can come between docs and declaration
            continue;
        }

        // --- using directives → imports ---
        if let Some(caps) = p.using_directive.captures(line) {
            let path = caps[1].to_string();
            // Heuristic: internal if starts with project namespace patterns
            // In NinjaTrader: NinjaTrader.*, System.* are external
            let is_external = path.starts_with("System")
                || path.starts_with("Microsoft")
                || path.starts_with("NinjaTrader")
                || !path.contains('.');
            imports.push(Import {
                import_path: path,
                imported_symbols: None,
                is_external,
            });
            reset_pending(&mut pending_docs, &mut pending_test_attr);
            continue;
        }

        // --- namespace ---
        if let Some(caps) = p.namespace.captures(line) {
            current_namespace = Some(caps[1].to_string());
            reset_pending(&mut pending_docs, &mut pending_test_attr);
            continue;
        }

        // --- class / interface / struct / enum / record ---
        if let Some(caps) = p.class_like.captures(line) {
            let vis_mods = caps[1].trim();
            let kind = caps[2].to_string();
            let raw_name = caps[3].trim().to_string();
            // Strip generic parameters for clean name
            let name = raw_name
                .split('<')
                .next()
                .unwrap_or(&raw_name)
                .trim()
                .to_string();

            if !name.is_empty() && name.chars().next().map_or(false, |c| c.is_uppercase() || c == '_') {
                let qualified_name = match &current_namespace {
                    Some(ns) => format!("{}.{}", ns, name),
                    None => name.clone(),
                };
                let visibility = extract_visibility(vis_mods);
                let is_test = pending_test_attr || vis_mods.contains("Test");
                let documentation = if pending_docs.is_empty() {
                    None
                } else {
                    Some(pending_docs.join("\n"))
                };

                // Estimate end line (rough: find closing brace at same depth)
                let end_line = estimate_end_line(&lines, idx, total);

                symbols.push(Symbol {
                    name: name.clone(),
                    qualified_name: Some(qualified_name),
                    symbol_type: kind,
                    language: "csharp".to_string(),
                    start_line: lineno,
                    end_line,
                    signature: None,
                    visibility,
                    documentation,
                    is_test,
                    is_async: false,
                });

                current_class = Some(name);
                class_brace_depth = brace_depth;
            }
            reset_pending(&mut pending_docs, &mut pending_test_attr);
            continue;
        }

        // --- method declarations ---
        if let Some(caps) = p.method.captures(line) {
            let vis_mods = caps[1].trim();
            let return_type = caps[2].trim().to_string();
            let name = caps[3].trim().to_string();
            let params = caps[4].trim().to_string();

            // Filter out keywords that regex might catch as method names
            if is_keyword(&name) || is_keyword(&return_type) {
                reset_pending(&mut pending_docs, &mut pending_test_attr);
                continue;
            }

            let is_async = p.async_method.is_match(vis_mods) || p.async_method.is_match(&return_type);
            let is_test_method = pending_test_attr;
            let qualified_name = build_qualified_name(&current_namespace, &current_class, &name);
            let visibility = extract_visibility(vis_mods);
            let signature = Some(format!("{} {}{}", return_type, name, params));
            let documentation = if pending_docs.is_empty() {
                None
            } else {
                Some(pending_docs.join("\n"))
            };
            let end_line = estimate_end_line(&lines, idx, total);

            symbols.push(Symbol {
                name: name.clone(),
                qualified_name: Some(qualified_name),
                symbol_type: "method".to_string(),
                language: "csharp".to_string(),
                start_line: lineno,
                end_line,
                signature,
                visibility,
                documentation,
                is_test: is_test_method,
                is_async,
            });

            current_method = Some(name.clone());
            method_brace_depth = brace_depth;
            reset_pending(&mut pending_docs, &mut pending_test_attr);
            continue;
        }

        // --- property declarations ---
        if let Some(caps) = p.property.captures(line) {
            let vis_mods = caps[1].trim();
            let prop_type = caps[2].trim().to_string();
            let name = caps[3].trim().to_string();

            if !is_keyword(&name) {
                let qualified_name =
                    build_qualified_name(&current_namespace, &current_class, &name);
                let visibility = extract_visibility(vis_mods);
                let documentation = if pending_docs.is_empty() {
                    None
                } else {
                    Some(pending_docs.join("\n"))
                };

                symbols.push(Symbol {
                    name: name.clone(),
                    qualified_name: Some(qualified_name),
                    symbol_type: "property".to_string(),
                    language: "csharp".to_string(),
                    start_line: lineno,
                    end_line: lineno,
                    signature: Some(format!("{} {}", prop_type, name)),
                    visibility,
                    documentation,
                    is_test: false,
                    is_async: false,
                });
            }
            reset_pending(&mut pending_docs, &mut pending_test_attr);
            continue;
        }

        // --- constants / static readonly fields ---
        if let Some(caps) = p.field_const.captures(line) {
            let vis_mods = caps[1].trim();
            let field_type = caps[2].trim().to_string();
            let name = caps[3].trim().to_string();

            let is_const = vis_mods.contains("const") || line.contains("const ");
            if !is_keyword(&name) && !field_type.is_empty() {
                let symbol_type = if is_const { "const" } else { "field" };
                let qualified_name =
                    build_qualified_name(&current_namespace, &current_class, &name);
                let visibility = extract_visibility(vis_mods);

                symbols.push(Symbol {
                    name: name.clone(),
                    qualified_name: Some(qualified_name),
                    symbol_type: symbol_type.to_string(),
                    language: "csharp".to_string(),
                    start_line: lineno,
                    end_line: lineno,
                    signature: Some(format!("{} {}", field_type, name)),
                    visibility,
                    documentation: None,
                    is_test: false,
                    is_async: false,
                });
            }
            reset_pending(&mut pending_docs, &mut pending_test_attr);
            continue;
        }

        // --- call extraction (simple: look for Method( calls within methods) ---
        if let Some(caller) = &current_method {
            extract_method_calls(line, lineno, caller, &mut calls);
        }

        // Clear pending docs/attrs for non-declaration lines
        reset_pending(&mut pending_docs, &mut pending_test_attr);
    }

    (symbols, imports, calls)
}

/// Estimate the end line for a block by scanning forward for the matching closing brace.
/// Returns `total` if not found (conservative — preserves semantic chunk boundaries).
fn estimate_end_line(lines: &[&str], start_idx: usize, total: u32) -> u32 {
    let mut depth = 0i32;
    let mut found_open = false;
    for (i, line) in lines[start_idx..].iter().enumerate() {
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    found_open = true;
                }
                '}' => {
                    depth -= 1;
                    if found_open && depth == 0 {
                        return (start_idx + i + 1) as u32;
                    }
                }
                _ => {}
            }
        }
        // Expression body (=>): ends at semicolon on same or next line
        if !found_open && line.trim_end().ends_with(';') {
            return (start_idx + i + 1) as u32;
        }
    }
    total
}

/// Extract simple method call expressions from a line of code.
/// Looks for `Identifier(` patterns and records them if they look like calls.
fn extract_method_calls(line: &str, lineno: u32, caller: &str, calls: &mut Vec<FunctionCall>) {
    // Simple pattern: word followed by ( not preceded by keyword patterns
    static CALL_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b([A-Z][a-zA-Z0-9_]*|[a-z][a-zA-Z0-9_]+)\s*\(").unwrap());

    // Skip obvious non-calls: if/for/while/switch/catch
    let trimmed = line.trim();
    if trimmed.starts_with("if ")
        || trimmed.starts_with("for ")
        || trimmed.starts_with("foreach ")
        || trimmed.starts_with("while ")
        || trimmed.starts_with("switch ")
        || trimmed.starts_with("catch ")
        || trimmed.starts_with("//")
    {
        return;
    }

    for caps in CALL_RE.captures_iter(line) {
        let callee = &caps[1];
        if !is_keyword(callee) && callee != caller {
            calls.push(FunctionCall {
                caller_name: caller.to_string(),
                callee_name: callee.to_string(),
                call_line: lineno,
                call_type: "method".to_string(),
            });
        }
    }
}

/// Build a qualified name: Namespace.Class.Member or Class.Member or just Member
fn build_qualified_name(
    namespace: &Option<String>,
    class: &Option<String>,
    name: &str,
) -> String {
    match (namespace, class) {
        (Some(ns), Some(cls)) => format!("{}.{}.{}", ns, cls, name),
        (None, Some(cls)) => format!("{}.{}", cls, name),
        (Some(ns), None) => format!("{}.{}", ns, name),
        (None, None) => name.to_string(),
    }
}

/// Extract the primary visibility modifier from a string of modifiers
fn extract_visibility(mods: &str) -> Option<String> {
    if mods.contains("public") {
        Some("public".to_string())
    } else if mods.contains("protected") {
        Some("protected".to_string())
    } else if mods.contains("private") {
        Some("private".to_string())
    } else if mods.contains("internal") {
        Some("internal".to_string())
    } else {
        None
    }
}

/// Reset accumulated pending state (docs, test attributes) after processing a line
fn reset_pending(docs: &mut Vec<String>, test_attr: &mut bool) {
    docs.clear();
    *test_attr = false;
}

/// C# keywords that should never be treated as identifiers
fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "if" | "else"
            | "for"
            | "foreach"
            | "while"
            | "do"
            | "switch"
            | "case"
            | "break"
            | "continue"
            | "return"
            | "new"
            | "this"
            | "base"
            | "null"
            | "true"
            | "false"
            | "var"
            | "let"
            | "in"
            | "is"
            | "as"
            | "try"
            | "catch"
            | "finally"
            | "throw"
            | "using"
            | "namespace"
            | "class"
            | "struct"
            | "interface"
            | "enum"
            | "record"
            | "void"
            | "int"
            | "long"
            | "double"
            | "float"
            | "decimal"
            | "bool"
            | "string"
            | "object"
            | "byte"
            | "char"
            | "short"
            | "uint"
            | "ulong"
            | "ushort"
            | "sbyte"
            | "Task"
            | "get"
            | "set"
            | "init"
            | "value"
            | "async"
            | "await"
            | "yield"
            | "where"
            | "select"
            | "from"
            | "into"
            | "join"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(code: &str) -> ParseResult {
        parse_csharp(code)
    }

    #[test]
    fn test_parse_class() {
        let code = r#"
namespace MyApp
{
    public class MyService
    {
    }
}
"#;
        let (symbols, _, _) = parse(code);
        let cls = symbols.iter().find(|s| s.name == "MyService").unwrap();
        assert_eq!(cls.symbol_type, "class");
        assert_eq!(cls.visibility, Some("public".to_string()));
    }

    #[test]
    fn test_parse_interface() {
        let code = r#"
public interface IStrategy
{
    void Execute();
}
"#;
        let (symbols, _, _) = parse(code);
        let iface = symbols.iter().find(|s| s.name == "IStrategy").unwrap();
        assert_eq!(iface.symbol_type, "interface");
    }

    #[test]
    fn test_parse_method() {
        let code = r#"
public class Calc
{
    public int Add(int a, int b)
    {
        return a + b;
    }
}
"#;
        let (symbols, _, _) = parse(code);
        let method = symbols.iter().find(|s| s.name == "Add").unwrap();
        assert_eq!(method.symbol_type, "method");
        assert_eq!(method.visibility, Some("public".to_string()));
    }

    #[test]
    fn test_parse_async_method() {
        let code = r#"
public class Service
{
    public async Task<int> FetchAsync()
    {
        return 0;
    }
}
"#;
        let (symbols, _, _) = parse(code);
        let method = symbols.iter().find(|s| s.name == "FetchAsync").unwrap();
        assert!(method.is_async);
    }

    #[test]
    fn test_parse_using() {
        let code = r#"
using System;
using System.Collections.Generic;
using NinjaTrader.Core;
"#;
        let (_, imports, _) = parse(code);
        assert!(imports.len() >= 3);
        let sys = imports.iter().find(|i| i.import_path == "System").unwrap();
        assert!(sys.is_external);
    }

    #[test]
    fn test_parse_enum() {
        let code = r#"
public enum TradeDirection
{
    Long,
    Short,
    Flat,
}
"#;
        let (symbols, _, _) = parse(code);
        let e = symbols.iter().find(|s| s.name == "TradeDirection").unwrap();
        assert_eq!(e.symbol_type, "enum");
    }
}
