//! crates/mira-server/src/utils/mod.rs
//! Shared utility functions used across the codebase

pub mod json;

use std::fmt::Display;
use std::path::{Path, PathBuf};

/// Extension trait for Result to simplify error conversion to String.
///
/// This eliminates the need for verbose `.map_err(|e| e.to_string())?` patterns
/// throughout the codebase. Instead, use `.str_err()?`.
///
/// # Example
/// ```ignore
/// use crate::utils::ResultExt;
///
/// fn example() -> Result<(), String> {
///     some_fallible_operation().str_err()?;
///     Ok(())
/// }
/// ```
pub trait ResultExt<T, E> {
    /// Convert the error type to String.
    fn str_err(self) -> Result<T, String>;
}

impl<T, E: Display> ResultExt<T, E> for Result<T, E> {
    fn str_err(self) -> Result<T, String> {
        self.map_err(|e| e.to_string())
    }
}

/// Normalize a project path to a canonical, consistent form.
///
/// 1. Expands `~` prefix to the user's home directory
/// 2. Canonicalizes the path (resolving symlinks, `.`, `..`), falling back to
///    the raw path if it doesn't exist yet
/// 3. Strips trailing slashes
/// 4. Converts to string with forward slashes via `path_to_string()`
///
/// This ensures that `~/project`, `/home/user/project`, and symlinked paths
/// all resolve to the same canonical string for database lookups.
pub fn normalize_project_path(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return String::new();
    }

    // Expand ~ to home directory
    let expanded: PathBuf = if let Some(rest) = path.strip_prefix("~/") {
        match dirs::home_dir() {
            Some(home) => home.join(rest),
            None => PathBuf::from(path),
        }
    } else if path == "~" {
        match dirs::home_dir() {
            Some(home) => home,
            None => PathBuf::from(path),
        }
    } else {
        PathBuf::from(path)
    };

    // Canonicalize (resolves symlinks, `.`, `..`), fall back to raw path
    let canonical = std::fs::canonicalize(&expanded).unwrap_or_else(|e| {
        tracing::debug!("canonicalize failed for {}: {e}", expanded.display());
        expanded
    });

    // On Windows, canonicalize() returns extended-length paths (\\?\C:\...)
    // which are not useful for display or DB storage. Strip the prefix.
    #[cfg(windows)]
    let canonical = {
        let s = canonical.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            PathBuf::from(stripped)
        } else {
            canonical
        }
    };

    // Convert to string with forward slashes, then strip trailing slashes
    let s = path_to_string(&canonical);
    let s = s.trim_end_matches('/').trim_end_matches('\\');
    if s.is_empty() {
        // Edge case: root path "/" would become empty after trimming
        "/".to_string()
    } else {
        s.to_string()
    }
}

/// Safely join a relative path to a base directory, preventing path traversal.
///
/// Returns `None` if the resulting path escapes the base directory (e.g. via `../`).
/// Both paths are canonicalized before comparison, so symlinks are resolved.
pub fn safe_join(base: &Path, relative: &str) -> Option<PathBuf> {
    let joined = base.join(relative);
    let canonical = joined.canonicalize().ok()?;
    let base_canonical = base.canonicalize().ok()?;
    if canonical.starts_with(&base_canonical) {
        Some(canonical)
    } else {
        None
    }
}

/// Convert a Path to an owned String with forward slashes.
///
/// Normalizes backslashes to forward slashes for cross-platform consistency.
/// Paths are stored and compared using Unix-style separators internally.
pub fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Get a path relative to a base, falling back to the original path if not a prefix.
pub fn relative_to<'a>(path: &'a Path, base: &Path) -> &'a Path {
    path.strip_prefix(base).unwrap_or(path)
}

/// Return a `&str` prefix of at most `max_bytes` bytes, rounded down to a
/// UTF-8 char boundary. Never allocates.
pub fn truncate_at_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        s
    } else {
        let mut end = max_bytes;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

/// Sanitize a project path for use as a directory name.
///
/// Replaces path separators with `-` to match Claude Code's directory naming convention.
/// Handles both `/` and `\` for cross-platform compatibility.
/// e.g. `/home/peter/Mira` -> `-home-peter-Mira`
pub fn sanitize_project_path(path: &str) -> String {
    path.replace(['/', '\\'], "-")
}

/// Redact sensitive data (API keys, credentials, connection strings) from text.
///
/// Applied to error messages before storage to prevent credential leakage
/// to the database or external embedding APIs.
#[allow(clippy::expect_used)] // Regex literals are compile-time known valid
pub fn redact_sensitive(text: &str) -> String {
    use regex::Regex;
    use std::sync::OnceLock;

    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        vec![
            // API keys (OpenAI, Anthropic, etc.)
            Regex::new(r"(?i)(sk-[a-zA-Z0-9]{20,})").expect("valid regex"),
            Regex::new(r"(?i)(api[_-]?key\s*[=:]\s*)\S+").expect("valid regex"),
            // Bearer tokens
            Regex::new(r"(?i)(bearer\s+)\S+").expect("valid regex"),
            // Connection strings with credentials
            Regex::new(r"(?i)((?:postgres|mysql|mongodb|redis)://)[^\s]+@").expect("valid regex"),
            // Environment variable assignments with values
            Regex::new(r"(?i)([A-Z][A-Z0-9_]*(?:KEY|SECRET|TOKEN|PASSWORD|CREDENTIAL)[A-Z0-9_]*\s*=\s*)\S+").expect("valid regex"),
            // Generic long hex/base64 tokens (40+ chars)
            Regex::new(r"\b[A-Za-z0-9+/]{40,}={0,2}\b").expect("valid regex"),
        ]
    });

    let mut result = text.to_string();
    for (i, pattern) in patterns.iter().enumerate() {
        let replacement = match i {
            0 => "sk-<REDACTED>",
            1 => "${1}<REDACTED>",
            2 => "${1}<REDACTED>",
            3 => "${1}<REDACTED>@",
            4 => "${1}<REDACTED>",
            5 => "<REDACTED_TOKEN>",
            _ => "<REDACTED>",
        };
        result = pattern.replace_all(&result, replacement).into_owned();
    }
    result
}

/// Format a `since_days` filter into a human-readable period string.
///
/// e.g. `Some(30)` -> `"last 30 days"`, `None` -> `"all time"`
pub fn format_period(since_days: Option<u32>) -> String {
    since_days
        .map(|d| format!("last {} days", d))
        .unwrap_or_else(|| "all time".to_string())
}

/// Truncate a string to max length with ellipsis.
///
/// If the string is longer than `max_len`, it will be truncated and
/// "..." will be appended. The total length will be at most `max_len + 3`.
pub fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", truncate_at_boundary(s, max_len))
    }
}

/// Expand short queries with context to improve embedding similarity.
///
/// Very short queries (1-3 words) produce poor embeddings because there's
/// not enough semantic signal. Wrapping them with a template provides the
/// embedding model with richer context, improving recall quality.
pub fn prepare_recall_query(query: &str) -> String {
    let word_count = query.split_whitespace().count();
    if word_count <= 3 {
        format!(
            "Information about: {}. Related concepts, decisions, and preferences.",
            query
        )
    } else {
        query.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_to_string() {
        use std::path::PathBuf;
        let path = PathBuf::from("/home/user/project");
        assert_eq!(path_to_string(&path), "/home/user/project");
    }

    #[test]
    fn test_relative_to_with_prefix() {
        use std::path::PathBuf;
        let path = PathBuf::from("/home/user/project/src/main.rs");
        let base = PathBuf::from("/home/user/project");
        assert_eq!(relative_to(&path, &base), Path::new("src/main.rs"));
    }

    #[test]
    fn test_relative_to_without_prefix() {
        use std::path::PathBuf;
        let path = PathBuf::from("/other/path/file.rs");
        let base = PathBuf::from("/home/user/project");
        assert_eq!(relative_to(&path, &base), Path::new("/other/path/file.rs"));
    }

    #[test]
    fn test_truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_exact_length() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_long_string() {
        assert_eq!(truncate("hello world", 5), "hello...");
    }

    #[test]
    fn test_truncate_empty_string() {
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn test_truncate_at_boundary_ascii() {
        assert_eq!(truncate_at_boundary("hello world", 5), "hello");
        assert_eq!(truncate_at_boundary("hi", 10), "hi");
    }

    #[test]
    fn test_truncate_at_boundary_multibyte() {
        // 'é' is 2 bytes in UTF-8; slicing at byte 1 would panic without boundary check
        let s = "é";
        assert_eq!(truncate_at_boundary(s, 1), "");
        assert_eq!(truncate_at_boundary(s, 2), "é");

        // Chinese character is 3 bytes
        let s = "a\u{4e16}b"; // 'a' + CJK char + 'b'
        assert_eq!(truncate_at_boundary(s, 2), "a");
        assert_eq!(truncate_at_boundary(s, 4), "a\u{4e16}");
    }

    #[test]
    fn test_truncate_multibyte() {
        assert_eq!(truncate("caf\u{00e9}", 3), "caf...");
        assert_eq!(truncate("caf\u{00e9}", 4), "caf...");
        assert_eq!(truncate("caf\u{00e9}", 5), "caf\u{00e9}");
    }

    #[test]
    fn test_sanitize_project_path() {
        assert_eq!(
            sanitize_project_path("/home/peter/Mira"),
            "-home-peter-Mira"
        );
        assert_eq!(sanitize_project_path("/tmp/test"), "-tmp-test");
    }

    #[test]
    fn test_sanitize_project_path_backslashes() {
        // Windows-style paths should produce the same result as forward-slash paths
        assert_eq!(
            sanitize_project_path("C:\\Users\\peter\\Mira"),
            "C:-Users-peter-Mira"
        );
        assert_eq!(
            sanitize_project_path("D:\\projects\\test"),
            "D:-projects-test"
        );
    }

    #[test]
    fn test_sanitize_project_path_mixed_separators() {
        // Mixed separators (can happen with user input or path joining)
        assert_eq!(
            sanitize_project_path("C:\\Users/peter\\project"),
            "C:-Users-peter-project"
        );
    }

    #[test]
    fn test_sanitize_project_path_empty_and_edge_cases() {
        assert_eq!(sanitize_project_path(""), "");
        assert_eq!(sanitize_project_path("no-separators"), "no-separators");
        assert_eq!(sanitize_project_path("/"), "-");
        assert_eq!(sanitize_project_path("\\"), "-");
    }

    /// Helper to extract a file name from a path string using either separator.
    /// This mirrors logic that might be used in cross-platform path handling.
    fn extract_filename(path: &str) -> &str {
        path.rsplit(['/', '\\']).next().unwrap_or(path)
    }

    #[test]
    fn test_extract_filename_unix_paths() {
        assert_eq!(extract_filename("/home/user/project/main.rs"), "main.rs");
        assert_eq!(extract_filename("src/lib.rs"), "lib.rs");
        assert_eq!(extract_filename("file.rs"), "file.rs");
    }

    #[test]
    fn test_extract_filename_windows_paths() {
        assert_eq!(
            extract_filename("C:\\Users\\user\\project\\main.rs"),
            "main.rs"
        );
        assert_eq!(extract_filename("src\\lib.rs"), "lib.rs");
    }

    #[test]
    fn test_extract_filename_mixed_separators() {
        assert_eq!(
            extract_filename("C:\\Users/user\\project/main.rs"),
            "main.rs"
        );
    }

    #[test]
    fn test_format_period_some() {
        assert_eq!(format_period(Some(30)), "last 30 days");
        assert_eq!(format_period(Some(7)), "last 7 days");
    }

    #[test]
    fn test_format_period_none() {
        assert_eq!(format_period(None), "all time");
    }

    #[test]
    fn test_redact_openai_key() {
        let input = "Error: OPENAI_API_KEY=sk-abc123def456ghi789jkl012mno345 not valid";
        let result = redact_sensitive(input);
        assert!(!result.contains("sk-abc123"));
        assert!(result.contains("<REDACTED>"));
    }

    #[test]
    fn test_redact_bearer_token() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.long.token";
        let result = redact_sensitive(input);
        assert!(!result.contains("eyJhbGci"));
    }

    #[test]
    fn test_redact_connection_string() {
        let input = "Failed to connect: postgres://admin:supersecret@localhost:5432/mydb";
        let result = redact_sensitive(input);
        assert!(!result.contains("supersecret"));
    }

    #[test]
    fn test_redact_preserves_normal_text() {
        let input = "Error: file not found at /home/user/project/src/main.rs";
        let result = redact_sensitive(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_redact_env_var_assignment() {
        let input = "ANTHROPIC_API_KEY=sk-ant-api03-longsecretvalue123 is invalid";
        let result = redact_sensitive(input);
        assert!(!result.contains("longsecretvalue"));
    }

    #[test]
    fn test_normalize_project_path_strips_trailing_slash() {
        let result = normalize_project_path("/tmp/test/");
        assert!(!result.ends_with('/'));
    }

    #[test]
    fn test_normalize_project_path_tilde_expansion() {
        let result = normalize_project_path("~/some-project");
        assert!(!result.starts_with('~'), "tilde should be expanded");
        assert!(result.contains("some-project"));
    }

    #[test]
    fn test_normalize_project_path_bare_tilde() {
        let result = normalize_project_path("~");
        let home = dirs::home_dir().unwrap();
        assert_eq!(result, path_to_string(&home));
    }

    #[test]
    fn test_normalize_project_path_empty() {
        assert_eq!(normalize_project_path(""), "");
        assert_eq!(normalize_project_path("  "), "");
    }

    #[test]
    #[cfg(unix)]
    fn test_normalize_project_path_root() {
        assert_eq!(normalize_project_path("/"), "/");
    }

    #[test]
    #[cfg(unix)]
    fn test_normalize_project_path_existing_dir() {
        // /tmp always exists on Linux/macOS
        let result = normalize_project_path("/tmp");
        assert_eq!(result, "/tmp");
    }

    #[test]
    fn test_normalize_project_path_nonexistent_falls_back() {
        let result = normalize_project_path("/nonexistent/path/xyz123");
        assert_eq!(result, "/nonexistent/path/xyz123");
    }

    #[test]
    fn test_normalize_project_path_idempotent() {
        let first = normalize_project_path("/tmp");
        let second = normalize_project_path(&first);
        assert_eq!(first, second);
    }
}
