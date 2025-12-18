//! Smart excerpting and UTF-8 helpers
//!
//! Utilities for creating model-friendly previews of large content.

use crate::limits::{EXCERPT_HEAD_CHARS, EXCERPT_TAIL_CHARS, MAX_DIFF_FILES, MAX_GREP_MATCHES};

/// UTF-8 safe byte slicing - finds valid char boundaries
/// Returns (slice, actual_start, actual_end) where boundaries are adjusted to valid UTF-8
pub fn safe_utf8_slice(text: &str, start: usize, limit: usize) -> (String, usize, usize) {
    let bytes = text.as_bytes();
    let len = bytes.len();

    if start >= len {
        return (String::new(), len, len);
    }

    // Find valid start boundary (move forward to char boundary)
    let mut actual_start = start.min(len);
    while actual_start < len && !text.is_char_boundary(actual_start) {
        actual_start += 1;
    }

    // Find valid end boundary (move backward to char boundary)
    let mut actual_end = (actual_start + limit).min(len);
    while actual_end > actual_start && !text.is_char_boundary(actual_end) {
        actual_end -= 1;
    }

    let content = text[actual_start..actual_end].to_string();
    (content, actual_start, actual_end)
}

/// Create head+tail excerpt with UTF-8 safe slicing
pub fn create_excerpt(content: &str, head_chars: usize, tail_chars: usize) -> String {
    let chars: Vec<char> = content.chars().collect();
    let total = chars.len();

    if total <= head_chars + tail_chars + 50 {
        // Small enough to include entirely
        return content.to_string();
    }

    let head: String = chars[..head_chars].iter().collect();
    let tail: String = chars[total - tail_chars..].iter().collect();

    format!(
        "{}\n\n…[truncated {} chars, use fetch_artifact for full content]…\n\n{}",
        head,
        total - head_chars - tail_chars,
        tail
    )
}

/// Create smart excerpt for grep output - show top N matches with context
pub fn create_grep_excerpt(content: &str, max_matches: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    if total_lines <= max_matches * 2 {
        return content.to_string();
    }

    // Take first N matches (grep output is typically file:line:content or just matches)
    let preview_lines: Vec<&str> = lines.iter().take(max_matches).copied().collect();
    let remaining = total_lines - max_matches;

    format!(
        "{}\n\n…[{} more matches, use search_artifact to find specific content]…",
        preview_lines.join("\n"),
        remaining
    )
}

/// Create smart excerpt for git diff - show file headers + first hunk per file
/// Returns content as-is if it doesn't look like a git diff
pub fn create_diff_excerpt(content: &str, max_files: usize) -> String {
    // Count total files first
    let mut total_files = 0;
    for line in content.lines() {
        if line.starts_with("diff --git") {
            total_files += 1;
        }
    }

    // If no diff headers found, return content as-is (not a git diff)
    if total_files == 0 {
        return content.to_string();
    }

    let mut result = String::new();
    let mut files_shown = 0;
    let mut in_hunk = false;
    let mut hunk_lines = 0;
    let mut current_file_has_hunk = false;

    for line in content.lines() {
        // New file header
        if line.starts_with("diff --git") {
            if files_shown >= max_files {
                break;
            }
            files_shown += 1;
            in_hunk = false;
            hunk_lines = 0;
            current_file_has_hunk = false;
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // File metadata (index, ---, +++)
        if line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ") {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // Hunk header
        if line.starts_with("@@") {
            if current_file_has_hunk {
                // Skip subsequent hunks, just note them
                continue;
            }
            in_hunk = true;
            current_file_has_hunk = true;
            hunk_lines = 0;
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // Hunk content - show first 15 lines of first hunk
        if in_hunk && hunk_lines < 15 {
            result.push_str(line);
            result.push('\n');
            hunk_lines += 1;
            if hunk_lines == 15 {
                result.push_str("  …[hunk truncated]…\n");
            }
        }
    }

    if total_files > max_files {
        result.push_str(&format!(
            "\n…[{} more files changed, use fetch_artifact for full diff]…",
            total_files - max_files
        ));
    }

    result
}

/// Create smart excerpt based on tool type
/// Uses default limits from limits.rs
pub fn create_smart_excerpt(tool_name: &str, content: &str) -> String {
    match tool_name {
        "grep" => create_grep_excerpt(content, MAX_GREP_MATCHES),
        "git_diff" => create_diff_excerpt(content, MAX_DIFF_FILES),
        _ => create_excerpt(content, EXCERPT_HEAD_CHARS, EXCERPT_TAIL_CHARS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_utf8_slice_basic() {
        let text = "hello world";
        let (slice, start, end) = safe_utf8_slice(text, 0, 5);
        assert_eq!(slice, "hello");
        assert_eq!(start, 0);
        assert_eq!(end, 5);
    }

    #[test]
    fn test_safe_utf8_slice_unicode() {
        let text = "héllo wörld";
        // 'é' is 2 bytes, 'ö' is 2 bytes
        let (slice, _, _) = safe_utf8_slice(text, 0, 20);
        assert_eq!(slice, text);
    }

    #[test]
    fn test_safe_utf8_slice_mid_char() {
        let text = "héllo"; // 'é' is bytes 1-2
        // Try to start at byte 2 (middle of 'é') - should adjust to byte 3
        let (slice, start, _) = safe_utf8_slice(text, 2, 10);
        assert!(text.is_char_boundary(start));
        assert!(!slice.contains("é")); // Should skip the partial char
    }

    #[test]
    fn test_safe_utf8_slice_past_end() {
        let text = "short";
        let (slice, start, end) = safe_utf8_slice(text, 100, 50);
        assert_eq!(slice, "");
        assert_eq!(start, 5);
        assert_eq!(end, 5);
    }

    #[test]
    fn test_create_excerpt_short() {
        let short = "short content";
        assert_eq!(create_excerpt(short, 1200, 800), short);
    }

    #[test]
    fn test_create_excerpt_long() {
        let long = "a".repeat(5000);
        let excerpt = create_excerpt(&long, 100, 50);
        assert!(excerpt.contains("truncated"));
        assert!(excerpt.starts_with(&"a".repeat(100)));
        assert!(excerpt.ends_with(&"a".repeat(50)));
    }

    #[test]
    fn test_grep_excerpt_short() {
        let short_grep = "file.rs:1:match\nfile.rs:2:match";
        assert_eq!(create_grep_excerpt(short_grep, 10), short_grep);
    }

    #[test]
    fn test_grep_excerpt_long() {
        let grep_output = (1..=50)
            .map(|i| format!("file.rs:{}:match {}", i, i))
            .collect::<Vec<_>>()
            .join("\n");
        let excerpt = create_grep_excerpt(&grep_output, 10);
        assert!(excerpt.contains("file.rs:1:match 1"));
        assert!(excerpt.contains("file.rs:10:match 10"));
        assert!(!excerpt.contains("file.rs:11:match 11"));
        assert!(excerpt.contains("40 more matches"));
    }

    #[test]
    fn test_diff_excerpt_single_file() {
        let diff = r#"diff --git a/foo.rs b/foo.rs
index abc123..def456 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1,5 +1,6 @@
 fn main() {
+    println!("hello");
 }
"#;
        let excerpt = create_diff_excerpt(diff, 5);
        assert!(excerpt.contains("diff --git a/foo.rs"));
        assert!(excerpt.contains("println!"));
    }

    #[test]
    fn test_diff_excerpt_multiple_files() {
        let diff = r#"diff --git a/foo.rs b/foo.rs
index abc123..def456 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1,5 +1,6 @@
 fn main() {
+    println!("hello");
 }
diff --git a/bar.rs b/bar.rs
index 111..222 100644
--- a/bar.rs
+++ b/bar.rs
@@ -1,2 +1,3 @@
+// new comment
 fn bar() {}
"#;
        let excerpt = create_diff_excerpt(diff, 1);
        assert!(excerpt.contains("diff --git a/foo.rs"));
        assert!(excerpt.contains("println!"));
        assert!(!excerpt.contains("diff --git a/bar.rs"));
        assert!(excerpt.contains("1 more files changed"));
    }

    #[test]
    fn test_smart_excerpt_routing() {
        // Short content returns as-is
        let short = "short";
        assert_eq!(create_smart_excerpt("grep", short), short);
        assert_eq!(create_smart_excerpt("git_diff", short), short);
        assert_eq!(create_smart_excerpt("bash", short), short);

        // Long grep uses grep-specific
        let long_grep = (1..=100)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let excerpt = create_smart_excerpt("grep", &long_grep);
        assert!(excerpt.contains("more matches"));
    }
}
