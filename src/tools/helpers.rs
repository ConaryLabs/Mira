// src/tools/helpers.rs
// Common helper functions to reduce code duplication across tools

#![allow(dead_code)] // Utility functions for future use

/// Truncate a string to a maximum length, adding ellipsis if truncated.
/// The total output length will be at most `max_len` characters.
///
/// # Examples
/// ```
/// use mira::tools::helpers::truncate;
/// assert_eq!(truncate("Hello World", 5), "He...");
/// assert_eq!(truncate("Hi", 10), "Hi");
/// ```
pub fn truncate(s: &str, max_len: usize) -> String {
    if max_len < 4 {
        // Too short for ellipsis, just truncate
        s.chars().take(max_len).collect()
    } else if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Generate SQL datetime formatting expression.
/// Returns: `datetime(column, 'unixepoch', 'localtime') as alias`
///
/// # Examples
/// ```
/// use mira::tools::helpers::sql_datetime;
/// assert_eq!(sql_datetime("created_at", "created"), "datetime(created_at, 'unixepoch', 'localtime') as created");
/// ```
pub fn sql_datetime(column: &str, alias: &str) -> String {
    format!("datetime({}, 'unixepoch', 'localtime') as {}", column, alias)
}

/// SQL constant for project scoping WHERE clause fragment.
/// Use with $1 bound to project_id (Option<i64>).
pub const PROJECT_SCOPE_CLAUSE: &str = "(project_id IS NULL OR project_id = $1)";

/// Generate SQL ORDER BY clause for status/priority sorting.
/// Returns SQL fragment that orders: in_progress first, then blocked, pending, completed.
/// Priority within each status: urgent > high > medium > low.
pub fn status_priority_order() -> &'static str {
    r#"CASE status
        WHEN 'in_progress' THEN 0
        WHEN 'blocked' THEN 1
        WHEN 'pending' THEN 2
        WHEN 'completed' THEN 3
        ELSE 4
    END,
    CASE priority
        WHEN 'urgent' THEN 0
        WHEN 'high' THEN 1
        WHEN 'medium' THEN 2
        WHEN 'low' THEN 3
        ELSE 4
    END"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short_string() {
        assert_eq!(truncate("Hi", 10), "Hi");
        assert_eq!(truncate("Hello", 5), "Hello");
    }

    #[test]
    fn test_truncate_long_string() {
        assert_eq!(truncate("Hello World", 8), "Hello...");
        assert_eq!(truncate("This is a very long string", 15), "This is a ve...");
    }

    #[test]
    fn test_truncate_edge_cases() {
        assert_eq!(truncate("Hello", 3), "Hel");  // Too short for ellipsis
        assert_eq!(truncate("Hello", 4), "H...");
        assert_eq!(truncate("", 10), "");
    }

    #[test]
    fn test_sql_datetime() {
        assert_eq!(
            sql_datetime("created_at", "created"),
            "datetime(created_at, 'unixepoch', 'localtime') as created"
        );
    }

    #[test]
    fn test_project_scope_clause() {
        assert!(PROJECT_SCOPE_CLAUSE.contains("project_id IS NULL"));
        assert!(PROJECT_SCOPE_CLAUSE.contains("$1"));
    }
}
