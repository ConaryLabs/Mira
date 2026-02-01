//! crates/mira-server/src/utils/mod.rs
//! Shared utility functions used across the codebase

pub mod json;

use std::fmt::Display;
use std::path::Path;

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

/// Convert a Path to an owned String, replacing invalid UTF-8 with U+FFFD.
pub fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

/// Get a path relative to a base, falling back to the original path if not a prefix.
pub fn relative_to<'a>(path: &'a Path, base: &Path) -> &'a Path {
    path.strip_prefix(base).unwrap_or(path)
}

/// Truncate a string to max length with ellipsis.
///
/// If the string is longer than `max_len`, it will be truncated and
/// "..." will be appended. The total length will be `max_len + 3`.
pub fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
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
}
