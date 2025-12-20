//! Diff Engine - Parse and apply unified diffs
//!
//! Enables diff-only edits to maximize output efficiency within
//! DeepSeek's 8k output limit.

use std::path::Path;
use thiserror::Error;

/// Errors from diff operations
#[derive(Debug, Error)]
pub enum DiffError {
    #[error("Failed to parse diff: {0}")]
    ParseError(String),

    #[error("Hunk application failed at line {line}: {reason}")]
    HunkFailed { line: usize, reason: String },

    #[error("Context mismatch at line {line}: expected '{expected}', found '{found}'")]
    ContextMismatch {
        line: usize,
        expected: String,
        found: String,
    },

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// A parsed unified diff
#[derive(Debug, Clone)]
pub struct UnifiedDiff {
    /// Original file path (from --- line)
    pub old_path: Option<String>,

    /// New file path (from +++ line)
    pub new_path: Option<String>,

    /// Individual hunks
    pub hunks: Vec<Hunk>,
}

/// A single hunk in a diff
#[derive(Debug, Clone)]
pub struct Hunk {
    /// Starting line in old file
    pub old_start: usize,

    /// Number of lines in old file
    pub old_count: usize,

    /// Starting line in new file
    pub new_start: usize,

    /// Number of lines in new file
    pub new_count: usize,

    /// The actual changes
    pub lines: Vec<DiffLine>,
}

/// A single line in a diff
#[derive(Debug, Clone, PartialEq)]
pub enum DiffLine {
    /// Context line (unchanged)
    Context(String),

    /// Added line
    Add(String),

    /// Removed line
    Remove(String),
}

impl UnifiedDiff {
    /// Parse a unified diff from text
    pub fn parse(text: &str) -> Result<Self, DiffError> {
        let mut lines = text.lines().peekable();
        let mut old_path = None;
        let mut new_path = None;
        let mut hunks = Vec::new();

        // Parse header
        while let Some(line) = lines.peek() {
            if line.starts_with("---") {
                old_path = Some(parse_file_path(line));
                lines.next();
            } else if line.starts_with("+++") {
                new_path = Some(parse_file_path(line));
                lines.next();
            } else if line.starts_with("@@") {
                break;
            } else {
                lines.next(); // Skip other header lines
            }
        }

        // Parse hunks
        while let Some(line) = lines.peek() {
            if line.starts_with("@@") {
                let hunk = parse_hunk(&mut lines)?;
                hunks.push(hunk);
            } else {
                lines.next();
            }
        }

        if hunks.is_empty() {
            return Err(DiffError::ParseError("No hunks found in diff".into()));
        }

        Ok(Self {
            old_path,
            new_path,
            hunks,
        })
    }

    /// Apply this diff to file content
    pub fn apply(&self, content: &str) -> Result<String, DiffError> {
        let mut lines: Vec<&str> = content.lines().collect();
        let mut offset: i64 = 0;

        for hunk in &self.hunks {
            let start = ((hunk.old_start as i64 - 1) + offset) as usize;

            // Verify context lines match
            let mut line_idx = start;
            let mut remove_count = 0;
            let mut add_lines: Vec<&str> = Vec::new();

            for diff_line in &hunk.lines {
                match diff_line {
                    DiffLine::Context(expected) => {
                        if line_idx >= lines.len() {
                            return Err(DiffError::ContextMismatch {
                                line: line_idx + 1,
                                expected: expected.clone(),
                                found: "<EOF>".into(),
                            });
                        }
                        let actual = lines[line_idx].trim_end();
                        let expected_trimmed = expected.trim_end();
                        if actual != expected_trimmed {
                            // Try fuzzy match (whitespace differences)
                            if !fuzzy_match(actual, expected_trimmed) {
                                return Err(DiffError::ContextMismatch {
                                    line: line_idx + 1,
                                    expected: expected.clone(),
                                    found: actual.to_string(),
                                });
                            }
                        }
                        line_idx += 1;
                    }
                    DiffLine::Remove(expected) => {
                        if line_idx >= lines.len() {
                            return Err(DiffError::HunkFailed {
                                line: line_idx + 1,
                                reason: format!("Expected to remove '{}' but at EOF", expected),
                            });
                        }
                        let actual = lines[line_idx].trim_end();
                        let expected_trimmed = expected.trim_end();
                        if actual != expected_trimmed && !fuzzy_match(actual, expected_trimmed) {
                            return Err(DiffError::ContextMismatch {
                                line: line_idx + 1,
                                expected: expected.clone(),
                                found: actual.to_string(),
                            });
                        }
                        remove_count += 1;
                        line_idx += 1;
                    }
                    DiffLine::Add(_) => {
                        // Counted separately
                    }
                }
            }

            // Now apply the hunk
            let mut new_lines: Vec<&str> = Vec::new();

            // Copy lines before hunk
            new_lines.extend_from_slice(&lines[..start]);

            // Apply changes
            let mut old_idx = start;
            for diff_line in &hunk.lines {
                match diff_line {
                    DiffLine::Context(_) => {
                        new_lines.push(lines[old_idx]);
                        old_idx += 1;
                    }
                    DiffLine::Remove(_) => {
                        old_idx += 1;
                    }
                    DiffLine::Add(line) => {
                        add_lines.push(line.as_str());
                    }
                }
            }

            // Insert added lines
            for diff_line in &hunk.lines {
                if let DiffLine::Add(line) = diff_line {
                    new_lines.push(line.as_str());
                }
            }

            // Copy remaining lines
            new_lines.extend_from_slice(&lines[old_idx..]);

            // Update offset for next hunk
            offset += (hunk.new_count as i64) - (hunk.old_count as i64);

            // Replace lines for next iteration
            lines = new_lines.into_iter().collect();
        }

        Ok(lines.join("\n"))
    }

    /// Get the target file path
    pub fn target_path(&self) -> Option<&str> {
        self.new_path.as_deref().or(self.old_path.as_deref())
    }

    /// Estimate the output size savings vs full file
    pub fn size_savings(&self, original_size: usize) -> f64 {
        let diff_size: usize = self.hunks.iter()
            .flat_map(|h| h.lines.iter())
            .map(|l| match l {
                DiffLine::Context(s) | DiffLine::Add(s) | DiffLine::Remove(s) => s.len() + 2,
            })
            .sum();

        if original_size > 0 {
            1.0 - (diff_size as f64 / original_size as f64)
        } else {
            0.0
        }
    }
}

/// Parse file path from --- or +++ line
fn parse_file_path(line: &str) -> String {
    let path = line
        .trim_start_matches("---")
        .trim_start_matches("+++")
        .trim();

    // Remove timestamps if present (e.g., "file.rs 2024-01-01 00:00:00")
    path.split_whitespace()
        .next()
        .unwrap_or(path)
        .trim_start_matches("a/")
        .trim_start_matches("b/")
        .to_string()
}

/// Parse a single hunk
fn parse_hunk<'a, I>(lines: &mut std::iter::Peekable<I>) -> Result<Hunk, DiffError>
where
    I: Iterator<Item = &'a str>,
{
    let header = lines.next().ok_or_else(|| {
        DiffError::ParseError("Expected hunk header".into())
    })?;

    // Parse @@ -old_start,old_count +new_start,new_count @@
    let (old_start, old_count, new_start, new_count) = parse_hunk_header(header)?;

    let mut diff_lines = Vec::new();
    let mut old_lines_seen = 0;
    let mut new_lines_seen = 0;

    while let Some(line) = lines.peek() {
        // Stop at next hunk or end
        if line.starts_with("@@") || line.starts_with("diff ") {
            break;
        }

        let line = lines.next().unwrap();

        if line.is_empty() {
            // Empty line is context
            diff_lines.push(DiffLine::Context(String::new()));
            old_lines_seen += 1;
            new_lines_seen += 1;
        } else if let Some(rest) = line.strip_prefix('+') {
            diff_lines.push(DiffLine::Add(rest.to_string()));
            new_lines_seen += 1;
        } else if let Some(rest) = line.strip_prefix('-') {
            diff_lines.push(DiffLine::Remove(rest.to_string()));
            old_lines_seen += 1;
        } else if let Some(rest) = line.strip_prefix(' ') {
            diff_lines.push(DiffLine::Context(rest.to_string()));
            old_lines_seen += 1;
            new_lines_seen += 1;
        } else if line.starts_with('\\') {
            // "\ No newline at end of file" - ignore
        } else {
            // Treat as context line (some diffs don't have leading space)
            diff_lines.push(DiffLine::Context(line.to_string()));
            old_lines_seen += 1;
            new_lines_seen += 1;
        }

        // Check if we've consumed expected lines
        if old_lines_seen >= old_count && new_lines_seen >= new_count {
            break;
        }
    }

    Ok(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        lines: diff_lines,
    })
}

/// Parse hunk header: @@ -1,5 +1,6 @@
fn parse_hunk_header(header: &str) -> Result<(usize, usize, usize, usize), DiffError> {
    let header = header.trim_start_matches('@').trim_end_matches('@').trim();

    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(DiffError::ParseError(format!(
            "Invalid hunk header: {}",
            header
        )));
    }

    let (old_start, old_count) = parse_range(parts[0].trim_start_matches('-'))?;
    let (new_start, new_count) = parse_range(parts[1].trim_start_matches('+'))?;

    Ok((old_start, old_count, new_start, new_count))
}

/// Parse a range like "1,5" or "1"
fn parse_range(s: &str) -> Result<(usize, usize), DiffError> {
    if let Some((start, count)) = s.split_once(',') {
        Ok((
            start.parse().map_err(|_| DiffError::ParseError(format!("Invalid line number: {}", start)))?,
            count.parse().map_err(|_| DiffError::ParseError(format!("Invalid count: {}", count)))?,
        ))
    } else {
        // Single line
        let start = s.parse().map_err(|_| DiffError::ParseError(format!("Invalid line number: {}", s)))?;
        Ok((start, 1))
    }
}

/// Fuzzy match allowing whitespace differences
fn fuzzy_match(a: &str, b: &str) -> bool {
    // Normalize whitespace
    let normalize = |s: &str| -> String {
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    normalize(a) == normalize(b)
}

/// Generate a unified diff between two strings
pub fn generate_diff(old: &str, new: &str, path: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // Use simple LCS-based diff for now
    let hunks = compute_hunks(&old_lines, &new_lines);

    let mut output = String::new();
    output.push_str(&format!("--- a/{}\n", path));
    output.push_str(&format!("+++ b/{}\n", path));

    for hunk in hunks {
        output.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
        ));
        for line in hunk.lines {
            match line {
                DiffLine::Context(s) => output.push_str(&format!(" {}\n", s)),
                DiffLine::Add(s) => output.push_str(&format!("+{}\n", s)),
                DiffLine::Remove(s) => output.push_str(&format!("-{}\n", s)),
            }
        }
    }

    output
}

/// Compute hunks using a simple diff algorithm
fn compute_hunks(old: &[&str], new: &[&str]) -> Vec<Hunk> {
    // This is a simplified implementation
    // For production, consider using the `similar` or `diff` crate

    let mut hunks = Vec::new();
    let mut old_idx = 0;
    let mut new_idx = 0;

    const CONTEXT_LINES: usize = 3;

    while old_idx < old.len() || new_idx < new.len() {
        // Skip matching lines
        while old_idx < old.len() && new_idx < new.len() && old[old_idx] == new[new_idx] {
            old_idx += 1;
            new_idx += 1;
        }

        if old_idx >= old.len() && new_idx >= new.len() {
            break;
        }

        // Found a difference - build a hunk
        let hunk_old_start = old_idx.saturating_sub(CONTEXT_LINES);
        let hunk_new_start = new_idx.saturating_sub(CONTEXT_LINES);

        let mut lines = Vec::new();

        // Add leading context
        for i in hunk_old_start..old_idx {
            lines.push(DiffLine::Context(old[i].to_string()));
        }

        // Find extent of changes
        let mut old_end = old_idx;
        let mut new_end = new_idx;

        while old_end < old.len() || new_end < new.len() {
            if old_end < old.len() && new_end < new.len() && old[old_end] == new[new_end] {
                // Check if we have enough matching context to end the hunk
                let mut matching = 0;
                while old_end + matching < old.len()
                    && new_end + matching < new.len()
                    && old[old_end + matching] == new[new_end + matching]
                {
                    matching += 1;
                    if matching >= CONTEXT_LINES * 2 {
                        break;
                    }
                }

                if matching >= CONTEXT_LINES * 2 {
                    // End the hunk
                    break;
                }

                // Include this match in the hunk
                lines.push(DiffLine::Context(old[old_end].to_string()));
                old_end += 1;
                new_end += 1;
            } else if old_end < old.len() && (new_end >= new.len() || !new.contains(&old[old_end])) {
                // Line removed
                lines.push(DiffLine::Remove(old[old_end].to_string()));
                old_end += 1;
            } else if new_end < new.len() {
                // Line added
                lines.push(DiffLine::Add(new[new_end].to_string()));
                new_end += 1;
            }
        }

        // Add trailing context
        let trailing_end = (old_end + CONTEXT_LINES).min(old.len());
        for i in old_end..trailing_end {
            if new_end < new.len() && old[i] == new[new_end] {
                lines.push(DiffLine::Context(old[i].to_string()));
                new_end += 1;
            }
        }

        if !lines.is_empty() {
            let old_count = lines.iter().filter(|l| !matches!(l, DiffLine::Add(_))).count();
            let new_count = lines.iter().filter(|l| !matches!(l, DiffLine::Remove(_))).count();

            hunks.push(Hunk {
                old_start: hunk_old_start + 1,
                old_count,
                new_start: hunk_new_start + 1,
                new_count,
                lines,
            });
        }

        old_idx = old_end;
        new_idx = new_end;
    }

    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_diff() {
        let diff = r#"--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,4 @@
 fn main() {
-    println!("old");
+    println!("new");
+    println!("added");
 }
"#;

        let parsed = UnifiedDiff::parse(diff).unwrap();
        assert_eq!(parsed.old_path, Some("foo.rs".into()));
        assert_eq!(parsed.new_path, Some("foo.rs".into()));
        assert_eq!(parsed.hunks.len(), 1);

        let hunk = &parsed.hunks[0];
        assert_eq!(hunk.old_start, 1);
        assert_eq!(hunk.old_count, 3);
        assert_eq!(hunk.new_start, 1);
        assert_eq!(hunk.new_count, 4);
    }

    #[test]
    fn test_apply_diff() {
        let original = "fn main() {\n    println!(\"old\");\n}";
        let diff = r#"@@ -1,3 +1,3 @@
 fn main() {
-    println!("old");
+    println!("new");
 }
"#;

        let parsed = UnifiedDiff::parse(diff).unwrap();
        let result = parsed.apply(original).unwrap();

        assert!(result.contains("println!(\"new\")"));
        assert!(!result.contains("println!(\"old\")"));
    }

    #[test]
    fn test_generate_diff() {
        let old = "line1\nline2\nline3";
        let new = "line1\nmodified\nline3";

        let diff = generate_diff(old, new, "test.rs");
        assert!(diff.contains("-line2"));
        assert!(diff.contains("+modified"));
    }

    #[test]
    fn test_context_mismatch() {
        let original = "fn main() {\n    println!(\"different\");\n}";
        let diff = r#"@@ -1,3 +1,3 @@
 fn main() {
-    println!("old");
+    println!("new");
 }
"#;

        let parsed = UnifiedDiff::parse(diff).unwrap();
        let result = parsed.apply(original);

        assert!(matches!(result, Err(DiffError::ContextMismatch { .. })));
    }

    #[test]
    fn test_size_savings() {
        let diff = UnifiedDiff {
            old_path: Some("test.rs".into()),
            new_path: Some("test.rs".into()),
            hunks: vec![Hunk {
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
                lines: vec![
                    DiffLine::Remove("old".into()),
                    DiffLine::Add("new".into()),
                ],
            }],
        };

        // Diff is ~10 bytes, if original was 1000 bytes, savings ~99%
        let savings = diff.size_savings(1000);
        assert!(savings > 0.9);
    }
}
