// crates/mira-server/src/search/skeleton.rs
// Skeletonize code content to signatures and docstrings only.
// Pure line-level heuristics -- no LLM calls.

/// Reduce code content to signatures and docstrings only.
/// Preserves function/struct/class/trait declarations and their doc comments
/// while omitting implementation bodies.
pub fn skeletonize_content(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let mut output = Vec::new();
    let mut body_omitted = false;

    for line in &lines {
        let trimmed = line.trim();

        if is_doc_comment(trimmed)
            || is_signature_line(trimmed)
            || is_import_line(trimmed)
            || is_closing_brace(trimmed)
            || trimmed.is_empty() && !body_omitted
        {
            // Emit a body-omitted placeholder once before switching back to kept lines
            if body_omitted {
                output.push("    // ... body omitted".to_string());
                body_omitted = false;
            }
            output.push(line.to_string());
        } else {
            // Body line -- mark for omission but don't duplicate the placeholder
            body_omitted = true;
        }
    }

    // If the content ended inside a body, emit the final placeholder
    if body_omitted {
        output.push("    // ... body omitted".to_string());
    }

    output.join("\n")
}

/// Returns true for doc-comment / docstring lines
fn is_doc_comment(trimmed: &str) -> bool {
    trimmed.starts_with("///")
        || trimmed.starts_with("//!")
        || trimmed.starts_with("/**")
        || trimmed.starts_with("\"\"\"")
        || (trimmed.starts_with("# ") && !trimmed.starts_with("#["))
        || trimmed.starts_with("#!")
}

/// Returns true for lines containing function/struct/class/trait/type signatures
fn is_signature_line(trimmed: &str) -> bool {
    // Rust
    trimmed.starts_with("fn ")
        || trimmed.starts_with("pub fn ")
        || trimmed.starts_with("pub(crate) fn ")
        || trimmed.starts_with("pub(super) fn ")
        || trimmed.starts_with("async fn ")
        || trimmed.starts_with("pub async fn ")
        || trimmed.starts_with("pub(crate) async fn ")
        || trimmed.starts_with("unsafe fn ")
        || trimmed.starts_with("pub unsafe fn ")
        || trimmed.starts_with("const fn ")
        || trimmed.starts_with("pub const fn ")
        || trimmed.starts_with("struct ")
        || trimmed.starts_with("pub struct ")
        || trimmed.starts_with("pub(crate) struct ")
        || trimmed.starts_with("enum ")
        || trimmed.starts_with("pub enum ")
        || trimmed.starts_with("pub(crate) enum ")
        || trimmed.starts_with("trait ")
        || trimmed.starts_with("pub trait ")
        || trimmed.starts_with("pub(crate) trait ")
        || trimmed.starts_with("impl ")
        || trimmed.starts_with("impl<")
        || trimmed.starts_with("type ")
        || trimmed.starts_with("pub type ")
        || trimmed.starts_with("pub(crate) type ")
        || trimmed.starts_with("mod ")
        || trimmed.starts_with("pub mod ")
        || trimmed.starts_with("pub(crate) mod ")
        || trimmed.starts_with("macro_rules!")
        || trimmed.starts_with("#[")
        // Python
        || trimmed.starts_with("def ")
        || trimmed.starts_with("async def ")
        || trimmed.starts_with("class ")
        || trimmed.starts_with("@")
        // JavaScript / TypeScript
        || trimmed.starts_with("function ")
        || trimmed.starts_with("async function ")
        || trimmed.starts_with("export ")
        || trimmed.starts_with("interface ")
        || trimmed.starts_with("abstract ")
        // Go
        || trimmed.starts_with("func ")
        || trimmed.starts_with("func (")
        // Java / C#
        || (trimmed.starts_with("public ") && contains_signature_keyword(trimmed))
        || (trimmed.starts_with("private ") && contains_signature_keyword(trimmed))
        || (trimmed.starts_with("protected ") && contains_signature_keyword(trimmed))
        || (trimmed.starts_with("static ") && contains_signature_keyword(trimmed))
}

/// Check whether a line with an access modifier also has a signature keyword
fn contains_signature_keyword(line: &str) -> bool {
    line.contains(" fn ")
        || line.contains(" class ")
        || line.contains(" struct ")
        || line.contains(" enum ")
        || line.contains(" interface ")
        || line.contains(" trait ")
        || line.contains(" void ")
        || line.contains(" int ")
        || line.contains(" string ")
        || line.contains(" bool ")
        || line.contains(" func ")
        || line.contains("(")
}

/// Returns true for use/import/require lines
fn is_import_line(trimmed: &str) -> bool {
    trimmed.starts_with("use ")
        || trimmed.starts_with("pub use ")
        || trimmed.starts_with("pub(crate) use ")
        || trimmed.starts_with("import ")
        || trimmed.starts_with("from ")
        || trimmed.starts_with("require(")
        || trimmed.starts_with("const ") && trimmed.contains("require(")
        || trimmed.starts_with("include ")
        || trimmed.starts_with("#include ")
}

/// Returns true for lines that are just closing braces/brackets
fn is_closing_brace(trimmed: &str) -> bool {
    trimmed == "}" || trimmed == "};" || trimmed == ")" || trimmed == ");"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skeletonize_rust_function() {
        let input = r#"/// Compute the sum of two numbers.
pub fn add(a: i32, b: i32) -> i32 {
    let result = a + b;
    println!("sum = {}", result);
    result
}"#;
        let output = skeletonize_content(input);
        assert!(output.contains("/// Compute the sum of two numbers."));
        assert!(output.contains("pub fn add(a: i32, b: i32) -> i32 {"));
        assert!(output.contains("// ... body omitted"));
        assert!(output.contains("}"));
        assert!(!output.contains("let result"));
        assert!(!output.contains("println!"));
    }

    #[test]
    fn test_skeletonize_python_function() {
        let input = r#"# Helper to greet a user
def greet(name):
    message = f"Hello, {name}!"
    print(message)
    return message"#;
        let output = skeletonize_content(input);
        assert!(output.contains("# Helper to greet a user"));
        assert!(output.contains("def greet(name):"));
        assert!(output.contains("// ... body omitted"));
        assert!(!output.contains("message = "));
        assert!(!output.contains("print(message)"));
    }

    #[test]
    fn test_skeletonize_preserves_imports() {
        let input = r#"use std::collections::HashMap;
use std::path::Path;

pub fn process() {
    let map = HashMap::new();
    map.insert("key", "value");
}"#;
        let output = skeletonize_content(input);
        assert!(output.contains("use std::collections::HashMap;"));
        assert!(output.contains("use std::path::Path;"));
        assert!(output.contains("pub fn process()"));
        assert!(!output.contains("map.insert"));
    }

    #[test]
    fn test_skeletonize_empty_content() {
        assert_eq!(skeletonize_content(""), "");
    }

    #[test]
    fn test_skeletonize_struct_with_impl() {
        let input = r#"/// A point in 2D space.
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    /// Create a new point.
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Distance to another point.
    pub fn distance(&self, other: &Point) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}"#;
        let output = skeletonize_content(input);
        assert!(output.contains("/// A point in 2D space."));
        assert!(output.contains("pub struct Point {"));
        assert!(output.contains("impl Point {"));
        assert!(output.contains("/// Create a new point."));
        assert!(output.contains("pub fn new(x: f64, y: f64) -> Self {"));
        assert!(output.contains("/// Distance to another point."));
        assert!(output.contains("pub fn distance(&self, other: &Point) -> f64 {"));
        assert!(!output.contains("let dx"));
        assert!(!output.contains("sqrt()"));
    }

    #[test]
    fn test_skeletonize_javascript() {
        let input = r#"export function fetchData(url) {
    const response = await fetch(url);
    const data = await response.json();
    return data;
}"#;
        let output = skeletonize_content(input);
        assert!(output.contains("export function fetchData(url) {"));
        assert!(output.contains("// ... body omitted"));
        assert!(!output.contains("const response"));
    }

    #[test]
    fn test_skeletonize_single_placeholder_per_body() {
        let input = r#"pub fn long_function() {
    let a = 1;
    let b = 2;
    let c = 3;
    let d = 4;
    let e = 5;
}"#;
        let output = skeletonize_content(input);
        let placeholder_count = output.matches("// ... body omitted").count();
        assert_eq!(placeholder_count, 1, "Should have exactly one placeholder per body block");
    }

    #[test]
    fn test_skeletonize_multiple_functions() {
        let input = r#"/// First function
fn first() {
    do_stuff();
}

/// Second function
fn second() {
    do_other_stuff();
}"#;
        let output = skeletonize_content(input);
        let placeholder_count = output.matches("// ... body omitted").count();
        assert_eq!(placeholder_count, 2, "Should have one placeholder per function body");
        assert!(output.contains("/// First function"));
        assert!(output.contains("fn first()"));
        assert!(output.contains("/// Second function"));
        assert!(output.contains("fn second()"));
    }

    #[test]
    fn test_skeletonize_preserves_attributes() {
        let input = r#"#[derive(Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub name: String,
    pub value: i32,
}"#;
        let output = skeletonize_content(input);
        assert!(output.contains("#[derive(Debug, Clone)]"));
        assert!(output.contains("#[serde(rename_all = \"camelCase\")]"));
        assert!(output.contains("pub struct Config {"));
    }
}
