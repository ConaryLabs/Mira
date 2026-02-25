// crates/mira-server/src/indexer/chunking.rs
// Code chunking for embedding generation

use crate::indexer::types::{CodeChunk, ParsedSymbol};
use std::collections::HashSet;

/// Create chunks for a single symbol, handling large symbol splitting
pub fn create_chunks_for_symbol(sym: &ParsedSymbol, lines: &[&str]) -> Vec<CodeChunk> {
    let start = sym.start_line.saturating_sub(1) as usize; // 1-indexed to 0-indexed
    let end = std::cmp::min(sym.end_line as usize, lines.len());

    if start >= lines.len() {
        return Vec::new();
    }

    // Build context directly from lines to avoid intermediate allocation
    let mut context = String::with_capacity((end - start) * 20); // Estimate average line length
    match sym.signature.as_ref() {
        Some(sig) => context.push_str(&format!("// {} {}: {}\n", sym.kind, sym.name, sig)),
        None => context.push_str(&format!("// {} {}\n", sym.kind, sym.name)),
    }

    // Append symbol lines
    for line in &lines[start..end] {
        context.push_str(line);
        context.push('\n');
    }

    // Skip empty symbols
    if context.trim().is_empty() {
        return Vec::new();
    }

    // If symbol is very large (>2000 chars), split at logical boundaries
    if context.len() > 2000 {
        split_large_chunk(context, sym.start_line, &sym.kind, &sym.name)
    } else {
        vec![CodeChunk {
            content: context,
            start_line: sym.start_line,
        }]
    }
}

/// Split a large chunk into smaller chunks at line boundaries
pub fn split_large_chunk(chunk: String, start_line: u32, kind: &str, name: &str) -> Vec<CodeChunk> {
    let mut result = Vec::new();
    let mut current_chunk = String::with_capacity(1000);
    let lines = chunk.lines().collect::<Vec<_>>();
    // Track the line number where the current sub-chunk starts
    let mut current_chunk_start_line = start_line;
    for (i, line) in lines.iter().enumerate() {
        if current_chunk.len() + line.len() > 1000 && !current_chunk.is_empty() {
            result.push(CodeChunk {
                content: current_chunk,
                start_line: current_chunk_start_line,
            });
            current_chunk_start_line = start_line + i as u32;
            current_chunk = String::with_capacity(1000);
            current_chunk.push_str(&format!("// {} {} (continued)\n", kind, name));
        }
        current_chunk.push_str(line);
        current_chunk.push('\n');
    }

    if !current_chunk.trim().is_empty() {
        result.push(CodeChunk {
            content: current_chunk,
            start_line: current_chunk_start_line,
        });
    }

    result
}

/// Create chunks for orphan code (lines not covered by any symbol)
pub fn create_chunks_for_orphan_code(
    lines: &[&str],
    covered_lines: &HashSet<u32>,
) -> Vec<CodeChunk> {
    let total_lines = lines.len() as u32;
    let mut chunks = Vec::new();
    let mut orphan_start: Option<u32> = None;

    for line_num in 1..=total_lines {
        if !covered_lines.contains(&line_num) {
            if orphan_start.is_none() {
                orphan_start = Some(line_num);
            }
        } else if let Some(start) = orphan_start {
            // End of orphan region - create chunk if substantial
            let start_idx = (start - 1) as usize;
            let end_idx = (line_num - 1) as usize;

            // Check if region has substantial non-whitespace content
            let has_substantial_content = lines[start_idx..end_idx]
                .iter()
                .any(|line| line.trim().len() > 10);

            if has_substantial_content {
                let mut content = String::with_capacity((end_idx - start_idx) * 20);
                content.push_str("// module-level code\n");
                for line in &lines[start_idx..end_idx] {
                    content.push_str(line);
                    content.push('\n');
                }
                chunks.push(CodeChunk {
                    content,
                    start_line: start,
                });
            }
            orphan_start = None;
        }
    }

    // Handle trailing orphan code
    if let Some(start) = orphan_start {
        let start_idx = (start - 1) as usize;

        // Check if region has substantial non-whitespace content
        let has_substantial_content = lines[start_idx..].iter().any(|line| line.trim().len() > 10);

        if has_substantial_content {
            let mut content = String::with_capacity((lines.len() - start_idx) * 20);
            content.push_str("// module-level code\n");
            for line in &lines[start_idx..] {
                content.push_str(line);
                content.push('\n');
            }
            chunks.push(CodeChunk {
                content,
                start_line: start,
            });
        }
    }

    chunks
}

/// Create semantic chunks based on symbol boundaries
/// Each chunk is a complete function/struct/etc with context metadata
pub fn create_semantic_chunks(content: &str, symbols: &[ParsedSymbol]) -> Vec<CodeChunk> {
    let lines: Vec<&str> = content.lines().collect();
    let mut chunks: Vec<CodeChunk> = Vec::with_capacity(symbols.len());
    let mut covered_lines: HashSet<u32> = HashSet::with_capacity(lines.len());

    // Sort symbols by start line
    let mut sorted_symbols: Vec<&ParsedSymbol> = symbols.iter().collect();
    sorted_symbols.sort_by_key(|s| s.start_line);

    // Create chunks for each symbol
    for sym in &sorted_symbols {
        // Mark lines as covered
        for line in sym.start_line..=sym.end_line {
            covered_lines.insert(line);
        }

        // Create chunks for this symbol
        let mut symbol_chunks = create_chunks_for_symbol(sym, &lines);
        chunks.append(&mut symbol_chunks);
    }

    // Create chunks for orphan code (lines not covered by any symbol)
    let mut orphan_chunks = create_chunks_for_orphan_code(&lines, &covered_lines);
    chunks.append(&mut orphan_chunks);

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // split_large_chunk tests
    // ============================================================================

    #[test]
    fn test_split_large_chunk_small_input() {
        let chunk = "fn foo() {}".to_string();
        let result = split_large_chunk(chunk.clone(), 1, "function", "foo");
        assert_eq!(result.len(), 1);
        assert!(result[0].content.contains("fn foo()"));
    }

    #[test]
    fn test_split_large_chunk_splits_at_boundary() {
        // Create a chunk larger than 1000 chars
        let mut chunk = String::new();
        for i in 0..50 {
            chunk.push_str(&format!("let line{} = \"some content here\";\n", i));
        }
        let result = split_large_chunk(chunk, 1, "function", "large_fn");
        assert!(result.len() > 1);
        // Continuation markers should be present in subsequent chunks
        if result.len() > 1 {
            assert!(result[1].content.contains("(continued)"));
        }
    }

    #[test]
    fn test_split_large_chunk_preserves_start_line() {
        let chunk = "line1\nline2\nline3".to_string();
        let result = split_large_chunk(chunk, 42, "function", "test");
        assert_eq!(result[0].start_line, 42);
    }

    // ============================================================================
    // create_chunks_for_symbol tests
    // ============================================================================

    #[test]
    fn test_create_chunks_for_symbol_basic() {
        let sym = ParsedSymbol {
            name: "test_func".to_string(),
            kind: "function".to_string(),
            start_line: 1,
            end_line: 3,
            signature: Some("fn test_func()".to_string()),
        };
        let lines = vec!["fn test_func() {", "    println!(\"hello\");", "}"];
        let chunks = create_chunks_for_symbol(&sym, &lines);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("test_func"));
    }

    #[test]
    fn test_create_chunks_for_symbol_with_signature() {
        let sym = ParsedSymbol {
            name: "add".to_string(),
            kind: "function".to_string(),
            start_line: 1,
            end_line: 1,
            signature: Some("fn add(a: i32, b: i32) -> i32".to_string()),
        };
        let lines = vec!["fn add(a: i32, b: i32) -> i32 { a + b }"];
        let chunks = create_chunks_for_symbol(&sym, &lines);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("function add:"));
    }

    #[test]
    fn test_create_chunks_for_symbol_out_of_bounds() {
        let sym = ParsedSymbol {
            name: "missing".to_string(),
            kind: "function".to_string(),
            start_line: 100,
            end_line: 110,
            signature: None,
        };
        let lines = vec!["line1", "line2"];
        let chunks = create_chunks_for_symbol(&sym, &lines);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_create_chunks_for_symbol_no_signature() {
        let sym = ParsedSymbol {
            name: "MyStruct".to_string(),
            kind: "struct".to_string(),
            start_line: 1,
            end_line: 1,
            signature: None,
        };
        let lines = vec!["struct MyStruct;"];
        let chunks = create_chunks_for_symbol(&sym, &lines);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("// struct MyStruct"));
    }

    // ============================================================================
    // create_chunks_for_orphan_code tests
    // ============================================================================

    #[test]
    fn test_create_chunks_for_orphan_code_none() {
        let lines = vec!["fn test() {}", "    code", "}"];
        let mut covered = HashSet::new();
        covered.insert(1);
        covered.insert(2);
        covered.insert(3);
        let chunks = create_chunks_for_orphan_code(&lines, &covered);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_create_chunks_for_orphan_code_whitespace_only() {
        let lines = vec!["", "   ", "\t"];
        let covered = HashSet::new();
        let chunks = create_chunks_for_orphan_code(&lines, &covered);
        // Whitespace-only regions should be skipped
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_create_chunks_for_orphan_code_short_lines() {
        let lines = vec!["x = 1", "y = 2"]; // Less than 10 chars
        let covered = HashSet::new();
        let chunks = create_chunks_for_orphan_code(&lines, &covered);
        // Lines shorter than 10 chars are not substantial
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_create_chunks_for_orphan_code_substantial() {
        let lines = vec!["// This is a module-level comment with substantial content"];
        let covered = HashSet::new();
        let chunks = create_chunks_for_orphan_code(&lines, &covered);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("module-level"));
    }

    // ============================================================================
    // create_semantic_chunks tests
    // ============================================================================

    #[test]
    fn test_create_semantic_chunks_empty() {
        let content = "";
        let symbols: Vec<ParsedSymbol> = vec![];
        let chunks = create_semantic_chunks(content, &symbols);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_create_semantic_chunks_single_symbol() {
        let content = "fn hello() {\n    println!(\"Hello\");\n}";
        let symbols = vec![ParsedSymbol {
            name: "hello".to_string(),
            kind: "function".to_string(),
            start_line: 1,
            end_line: 3,
            signature: Some("fn hello()".to_string()),
        }];
        let chunks = create_semantic_chunks(content, &symbols);
        assert!(!chunks.is_empty());
        assert!(chunks[0].content.contains("hello"));
    }

    #[test]
    fn test_create_semantic_chunks_multiple_symbols() {
        let content = "fn a() {}\nfn b() {}";
        let symbols = vec![
            ParsedSymbol {
                name: "a".to_string(),
                kind: "function".to_string(),
                start_line: 1,
                end_line: 1,
                signature: None,
            },
            ParsedSymbol {
                name: "b".to_string(),
                kind: "function".to_string(),
                start_line: 2,
                end_line: 2,
                signature: None,
            },
        ];
        let chunks = create_semantic_chunks(content, &symbols);
        assert_eq!(chunks.len(), 2);
    }
}
