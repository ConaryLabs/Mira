// crates/mira-server/src/search/context.rs
// Context expansion for search results

use crate::db::get_symbol_bounds_sync;
use crate::utils::safe_join;
use rusqlite::Connection;
use std::path::Path;

/// Parse symbol name and kind from chunk header
/// Headers look like: "// function foo", "// function foo: sig", "// function foo (continued)"
fn parse_symbol_header(chunk_content: &str) -> Option<(String, String)> {
    let first_line = chunk_content.lines().next()?;
    if !first_line.starts_with("// ") {
        return None;
    }

    let rest = first_line.strip_prefix("// ")?;

    // Skip "module-level code" - no symbol to look up
    if rest.starts_with("module") {
        return None;
    }

    // Split on first space to get kind
    let (kind, remainder) = rest.split_once(' ')?;

    // Get name: everything before ":" or " (continued)" or end of string
    let name = if let Some(idx) = remainder.find(':') {
        &remainder[..idx]
    } else if let Some(idx) = remainder.find(" (continued)") {
        &remainder[..idx]
    } else {
        remainder
    };

    Some((kind.to_string(), name.trim().to_string()))
}

/// Look up symbol bounds from code_symbols table (sync version for pool.interact)
fn lookup_symbol_bounds_sync(
    conn: &Connection,
    project_id: Option<i64>,
    file_path: &str,
    symbol_name: &str,
) -> Option<(u32, u32)> {
    get_symbol_bounds_sync(conn, file_path, symbol_name, project_id)
}

/// Expand search result to full symbol using code_symbols table
pub fn expand_context(
    file_path: &str,
    chunk_content: &str,
    project_path: Option<&str>,
) -> Option<(Option<String>, String)> {
    expand_context_with_conn(file_path, chunk_content, project_path, None, None)
}

/// Max file size for context expansion reads (same as indexer limit).
/// Files larger than this are typically generated code — skip expansion
/// and return the chunk as-is to avoid oversized MCP responses.
const MAX_EXPAND_FILE_BYTES: u64 = 1_024 * 1_024;

/// Check if a file is small enough for context expansion.
fn file_within_size_limit(path: &std::path::Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.len() <= MAX_EXPAND_FILE_BYTES)
        .unwrap_or(false)
}

/// Expand search result to full symbol using code_symbols table (with Connection for pool.interact)
pub fn expand_context_with_conn(
    file_path: &str,
    chunk_content: &str,
    project_path: Option<&str>,
    conn: Option<&Connection>,
    project_id: Option<i64>,
) -> Option<(Option<String>, String)> {
    // Extract symbol info from header
    let symbol_info = if chunk_content.starts_with("// ") {
        chunk_content.lines().next().map(|s| s.to_string())
    } else {
        None
    };

    // Try to expand using symbol bounds from DB
    if let (Some(conn), Some(proj_path)) = (conn, project_path)
        && let Some((kind, name)) = parse_symbol_header(chunk_content)
        && let Some((start_line, end_line)) =
            lookup_symbol_bounds_sync(conn, project_id, file_path, &name)
        && let Some(full_path) = safe_join(Path::new(proj_path), file_path)
        && file_within_size_limit(&full_path)
        && let Ok(file_content) = std::fs::read_to_string(&full_path)
    {
        let all_lines: Vec<&str> = file_content.lines().collect();

        // Convert 1-indexed lines to 0-indexed
        let start = (start_line.saturating_sub(1)) as usize;
        let end = std::cmp::min(end_line as usize, all_lines.len());

        if start < all_lines.len() {
            let full_symbol = all_lines[start..end].join("\n");
            let header = format!("// {} {} (lines {}-{})", kind, name, start_line, end_line);
            return Some((Some(header), full_symbol));
        }
    }

    // Fallback: use original +-5 line approach
    if let Some(proj_path) = project_path
        && let Some(full_path) = safe_join(Path::new(proj_path), file_path)
        && file_within_size_limit(&full_path)
        && let Ok(file_content) = std::fs::read_to_string(&full_path)
    {
        let search_content = if chunk_content.starts_with("// ") {
            chunk_content.lines().skip(1).collect::<Vec<_>>().join("\n")
        } else {
            chunk_content.to_string()
        };

        if let Some(pos) = file_content.find(&search_content) {
            let lines_before = file_content[..pos].matches('\n').count();
            let all_lines: Vec<&str> = file_content.lines().collect();
            let match_lines = search_content.matches('\n').count() + 1;

            let start_line = lines_before.saturating_sub(5);
            let end_line = std::cmp::min(lines_before + match_lines + 5, all_lines.len());

            let context_code = all_lines[start_line..end_line].join("\n");
            return Some((symbol_info, context_code));
        }
    }

    // Final fallback: return chunk as-is
    Some((symbol_info, chunk_content.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // parse_symbol_header tests
    // ============================================================================

    #[test]
    fn test_parse_symbol_header_function() {
        let result = parse_symbol_header("// function foo\nfn foo() {}");
        assert_eq!(result, Some(("function".to_string(), "foo".to_string())));
    }

    #[test]
    fn test_parse_symbol_header_function_with_signature() {
        let result = parse_symbol_header(
            "// function foo: fn foo(x: i32) -> bool\nfn foo(x: i32) -> bool {}",
        );
        assert_eq!(result, Some(("function".to_string(), "foo".to_string())));
    }

    #[test]
    fn test_parse_symbol_header_continued() {
        let result = parse_symbol_header("// function bar (continued)\n    more code here");
        assert_eq!(result, Some(("function".to_string(), "bar".to_string())));
    }

    #[test]
    fn test_parse_symbol_header_struct() {
        let result = parse_symbol_header("// struct MyStruct\npub struct MyStruct {}");
        assert_eq!(result, Some(("struct".to_string(), "MyStruct".to_string())));
    }

    #[test]
    fn test_parse_symbol_header_impl() {
        let result = parse_symbol_header("// impl Database\nimpl Database {}");
        assert_eq!(result, Some(("impl".to_string(), "Database".to_string())));
    }

    #[test]
    fn test_parse_symbol_header_method() {
        let result =
            parse_symbol_header("// method process: fn process(&self)\nfn process(&self) {}");
        assert_eq!(result, Some(("method".to_string(), "process".to_string())));
    }

    #[test]
    fn test_parse_symbol_header_no_comment_prefix() {
        let result = parse_symbol_header("fn foo() {}");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_symbol_header_module_level() {
        let result = parse_symbol_header("// module-level code\nuse std::io;");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_symbol_header_empty() {
        let result = parse_symbol_header("");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_symbol_header_just_comment() {
        // Just "// " with nothing after
        let result = parse_symbol_header("// ");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_symbol_header_whitespace_in_name() {
        let result = parse_symbol_header("// function my_func \nfn my_func() {}");
        assert_eq!(
            result,
            Some(("function".to_string(), "my_func".to_string()))
        );
    }

    // ============================================================================
    // expand_context tests (basic cases without DB)
    // ============================================================================

    #[test]
    fn test_expand_context_no_project_path() {
        let result = expand_context("src/main.rs", "fn main() {}", None);
        assert!(result.is_some());
        let (symbol_info, content) = result.unwrap();
        assert!(symbol_info.is_none());
        assert_eq!(content, "fn main() {}");
    }

    #[test]
    fn test_expand_context_with_header() {
        let chunk = "// function foo\nfn foo() {}";
        let result = expand_context("src/lib.rs", chunk, None);
        assert!(result.is_some());
        let (symbol_info, content) = result.unwrap();
        assert_eq!(symbol_info, Some("// function foo".to_string()));
        assert_eq!(content, chunk);
    }

    #[test]
    fn test_expand_context_preserves_content() {
        let chunk = "pub struct Test {\n    field: i32,\n}";
        let result = expand_context("src/types.rs", chunk, None);
        assert!(result.is_some());
        let (_, content) = result.unwrap();
        assert_eq!(content, chunk);
    }
}
