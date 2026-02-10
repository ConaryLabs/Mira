use std::path::PathBuf;

/// Line budget for auto memory export (Claude Code truncates after 200 lines)
const AUTO_MEMORY_LINE_BUDGET: usize = 150;

/// Max bytes per memory entry in auto memory (shorter for line budget)
const AUTO_MEMORY_MAX_BYTES: usize = 400;

/// Category line quotas within AUTO_MEMORY_LINE_BUDGET
const AUTO_MEMORY_PREF_QUOTA: usize = 60; // 40%
const AUTO_MEMORY_DECISION_QUOTA: usize = 45; // 30%
const AUTO_MEMORY_PATTERN_QUOTA: usize = 30; // 20%
const AUTO_MEMORY_GENERAL_QUOTA: usize = 15; // 10%

/// Patterns that indicate ephemeral/task-related content (not worth graduating)
const AUTO_MEMORY_NOISE_PATTERNS: &[&str] = &[
    "task #",
    "Task #",
    "task ID",
    "Task ID",
    "(ID:",
    "created task",
    "Created task",
    "created goal",
    "Created goal",
    "Completed creation of",
    "task breakdown",
    "Task breakdown",
];

/// Get the auto memory directory path for a project.
///
/// Claude Code uses: `~/.claude/projects/-home-peter-Mira/memory/`
/// The path is sanitized by replacing `/` with `-`.
pub fn get_auto_memory_dir(project_path: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let sanitized = crate::utils::sanitize_project_path(project_path);
    home.join(".claude/projects")
        .join(&sanitized)
        .join("memory")
}

/// Check if auto memory feature is available (directory exists).
///
/// Non-invasive detection: only returns true if Claude Code has created the directory.
/// We never create it ourselves.
pub fn auto_memory_dir_exists(project_path: &str) -> bool {
    get_auto_memory_dir(project_path).exists()
}

/// Memory candidate for auto memory export.
/// Only contains fields actually used in build_auto_memory_export.
/// Filtering (confidence, session_count, status) and ordering (hotness)
/// are handled in the SQL query.
struct AutoMemoryCandidate {
    content: String,
    fact_type: String,
    category: Option<String>,
}

/// Fetch memories with stricter thresholds for auto memory export.
///
/// Tiered thresholds based on age:
/// - Fresh (≤14 days): confidence >= 0.8, session_count >= 3
/// - Older (>14 days): confidence >= 0.9, session_count >= 5
///
/// Excludes: 'archived' status, 'health'/'persona' fact types
/// Time decay: 30-day half-life (aggressive recency bias)
fn fetch_auto_memory_candidates_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
    limit: usize,
) -> Result<Vec<AutoMemoryCandidate>, String> {
    // Only SELECT fields we actually use; hotness is computed for ORDER BY only
    let sql = r#"
        SELECT content, fact_type, category
        FROM memory_facts
        WHERE project_id = ?1
          AND scope = 'project'
          AND status = 'confirmed'
          AND fact_type NOT IN ('health', 'persona')
          -- Tiered thresholds: fresh memories (≤14 days) vs older memories
          AND (
              (julianday('now') - julianday(COALESCE(updated_at, created_at)) <= 14
               AND confidence >= 0.8 AND session_count >= 3)
              OR
              (julianday('now') - julianday(COALESCE(updated_at, created_at)) > 14
               AND confidence >= 0.9 AND session_count >= 5)
          )
        ORDER BY (
            session_count
            * confidence
            * CASE
                WHEN category = 'preference' THEN 1.4
                WHEN category = 'decision' THEN 1.3
                WHEN category IN ('pattern', 'convention') THEN 1.1
                WHEN category = 'context' THEN 1.0
                ELSE 0.9
              END
            / (1.0 + (CAST(julianday('now') - julianday(COALESCE(updated_at, created_at)) AS REAL) / 30.0))
        ) DESC
        LIMIT ?2
    "#;

    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("Failed to load memories for export: {}", e))?;

    let rows = stmt
        .query_map(rusqlite::params![project_id, limit as i64], |row| {
            Ok(AutoMemoryCandidate {
                content: row.get(0)?,
                fact_type: row.get(1)?,
                category: row.get(2)?,
            })
        })
        .map_err(|e| format!("Failed to load memories for export: {}", e))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect auto memory results: {}", e))
}

/// Build line-budgeted export for auto memory with category quotas.
fn build_auto_memory_export(candidates: &[AutoMemoryCandidate]) -> String {
    if candidates.is_empty() {
        return String::new();
    }

    let header = "# Mira Memory Cache\n\n<!-- Auto-generated from Mira. High-confidence memories only. -->\n\n";

    // Track line counts per category
    let mut pref_lines = 0usize;
    let mut decision_lines = 0usize;
    let mut pattern_lines = 0usize;
    let mut general_lines = 0usize;

    // Collect entries per section
    let mut preferences: Vec<String> = Vec::new();
    let mut decisions: Vec<String> = Vec::new();
    let mut patterns: Vec<String> = Vec::new();
    let mut general: Vec<String> = Vec::new();

    for mem in candidates {
        // Skip ephemeral/task-related noise
        if is_auto_memory_noise(&mem.content) {
            continue;
        }

        let content = super::truncate_content(&mem.content, AUTO_MEMORY_MAX_BYTES);
        let entry_line = format!("- {}\n", content);
        let line_count = entry_line.lines().count();

        // Classify and check quota
        let section = super::classify_by_type_and_category(&mem.fact_type, mem.category.as_deref());

        match section {
            "Preferences" if pref_lines + line_count <= AUTO_MEMORY_PREF_QUOTA => {
                pref_lines += line_count;
                preferences.push(entry_line);
            }
            "Decisions" if decision_lines + line_count <= AUTO_MEMORY_DECISION_QUOTA => {
                decision_lines += line_count;
                decisions.push(entry_line);
            }
            "Patterns" if pattern_lines + line_count <= AUTO_MEMORY_PATTERN_QUOTA => {
                pattern_lines += line_count;
                patterns.push(entry_line);
            }
            "General" if general_lines + line_count <= AUTO_MEMORY_GENERAL_QUOTA => {
                general_lines += line_count;
                general.push(entry_line);
            }
            _ => {
                // Quota exceeded for this category, skip
            }
        }

        // Check total line budget
        let total = pref_lines + decision_lines + pattern_lines + general_lines;
        if total >= AUTO_MEMORY_LINE_BUDGET {
            break;
        }
    }

    let sections: Vec<(&str, Vec<String>)> = vec![
        ("Preferences", preferences),
        ("Decisions", decisions),
        ("Patterns", patterns),
        ("General", general),
    ];
    super::render_sections(header, &sections)
}

/// Check if memory content matches ephemeral/task-related noise patterns
fn is_auto_memory_noise(content: &str) -> bool {
    AUTO_MEMORY_NOISE_PATTERNS
        .iter()
        .any(|pattern| content.contains(pattern))
}

/// Write high-confidence memories to Claude Code's auto memory directory.
///
/// Only writes if the directory exists (feature detection).
/// Writes to MEMORY.mira.md to avoid conflicts with user's MEMORY.md.
/// Uses atomic writes (temp file + rename).
///
/// Returns the number of memories exported, or 0 if:
/// - Auto memory directory doesn't exist
/// - No memories meet the stricter thresholds
pub fn write_auto_memory_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    let dir = get_auto_memory_dir(project_path);

    // Feature detection: only write if directory exists
    if !dir.exists() {
        return Ok(0);
    }

    // Fetch candidates with stricter thresholds
    let candidates =
        fetch_auto_memory_candidates_sync(conn, project_id, super::RANKED_FETCH_LIMIT)?;

    if candidates.is_empty() {
        return Ok(0);
    }

    let content = build_auto_memory_export(&candidates);
    if content.is_empty() {
        return Ok(0);
    }

    // Write to MEMORY.mira.md (Mira-owned), not MEMORY.md (user-owned)
    let memory_path = dir.join("MEMORY.mira.md");
    let temp_path = dir.join(".MEMORY.mira.md.tmp");

    // Atomic write: temp file + rename
    if let Err(e) = std::fs::write(&temp_path, &content) {
        // Clean up temp file on error
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to write temp file: {}", e));
    }

    if let Err(e) = std::fs::rename(&temp_path, &memory_path) {
        // Clean up temp file on rename failure
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to rename temp file: {}", e));
    }

    // Count entries (lines starting with "- ")
    let count = content.lines().filter(|l| l.starts_with("- ")).count();
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_auto_memory_dir() {
        // Test path calculation: slashes become dashes
        let dir = get_auto_memory_dir("/home/peter/Mira");
        let path_str = dir.to_string_lossy();

        // Should contain the sanitized path
        assert!(path_str.contains("-home-peter-Mira"));
        assert!(path_str.contains(".claude/projects"));
        assert!(path_str.ends_with("memory"));
    }

    #[test]
    fn test_get_auto_memory_dir_various_paths() {
        // Normalize to forward slashes for cross-platform comparison
        let normalize = |p: PathBuf| p.to_string_lossy().replace('\\', "/");

        // Root path
        let dir = normalize(get_auto_memory_dir("/"));
        assert!(dir.contains("/-/memory"));

        // Nested path
        let dir = normalize(get_auto_memory_dir("/usr/local/src/myproject"));
        assert!(dir.contains("-usr-local-src-myproject"));

        // Path with trailing slash (shouldn't happen but handle gracefully)
        let dir = normalize(get_auto_memory_dir("/home/user/project/"));
        assert!(dir.contains("-home-user-project-"));
    }

    #[test]
    fn test_auto_memory_dir_exists_nonexistent() {
        // A path that definitely doesn't have an auto memory dir
        assert!(!auto_memory_dir_exists(
            "/nonexistent/path/that/does/not/exist"
        ));
    }

    fn make_auto_memory_candidate(
        content: &str,
        fact_type: &str,
        category: Option<&str>,
        _hotness: f64, // kept for test API compatibility, not stored
    ) -> AutoMemoryCandidate {
        AutoMemoryCandidate {
            content: content.to_string(),
            fact_type: fact_type.to_string(),
            category: category.map(|s| s.to_string()),
        }
    }

    #[test]
    fn test_build_auto_memory_export_empty() {
        let result = build_auto_memory_export(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_build_auto_memory_export_basic_sections() {
        let candidates = vec![
            make_auto_memory_candidate("User prefers tabs", "preference", Some("preference"), 10.0),
            make_auto_memory_candidate("Using SQLite", "decision", Some("decision"), 8.0),
            make_auto_memory_candidate("Builder pattern", "pattern", Some("pattern"), 6.0),
            make_auto_memory_candidate("General fact", "general", Some("general"), 4.0),
        ];
        let result = build_auto_memory_export(&candidates);

        assert!(result.contains("# Mira Memory Cache"));
        assert!(result.contains("## Preferences"));
        assert!(result.contains("- User prefers tabs"));
        assert!(result.contains("## Decisions"));
        assert!(result.contains("- Using SQLite"));
        assert!(result.contains("## Patterns"));
        assert!(result.contains("- Builder pattern"));
        assert!(result.contains("## General"));
        assert!(result.contains("- General fact"));
    }

    #[test]
    fn test_build_auto_memory_line_budget() {
        // Create many candidates that would exceed the line budget
        let candidates: Vec<AutoMemoryCandidate> = (0..200)
            .map(|i| {
                make_auto_memory_candidate(
                    &format!("Memory item number {} with some content", i),
                    "general",
                    Some("general"),
                    200.0 - i as f64,
                )
            })
            .collect();

        let result = build_auto_memory_export(&candidates);

        // Count total lines (excluding empty lines and headers)
        let entry_count = result.lines().filter(|l| l.starts_with("- ")).count();

        // Should be limited by the general quota (15 lines)
        assert!(entry_count <= AUTO_MEMORY_GENERAL_QUOTA);
    }

    #[test]
    fn test_build_auto_memory_category_quotas() {
        // Create many preferences to test quota enforcement
        let mut candidates: Vec<AutoMemoryCandidate> = (0..100)
            .map(|i| {
                make_auto_memory_candidate(
                    &format!("Preference {}", i),
                    "preference",
                    Some("preference"),
                    100.0 - i as f64,
                )
            })
            .collect();

        // Add some decisions too
        candidates.extend((0..50).map(|i| {
            make_auto_memory_candidate(
                &format!("Decision {}", i),
                "decision",
                Some("decision"),
                50.0 - i as f64,
            )
        }));

        let result = build_auto_memory_export(&candidates);

        // Count entries per section
        let pref_count = result
            .lines()
            .filter(|l| l.starts_with("- Preference"))
            .count();
        let decision_count = result
            .lines()
            .filter(|l| l.starts_with("- Decision"))
            .count();

        // Should be limited by quotas
        assert!(pref_count <= AUTO_MEMORY_PREF_QUOTA);
        assert!(decision_count <= AUTO_MEMORY_DECISION_QUOTA);
    }

    #[test]
    fn test_is_auto_memory_noise() {
        // Should match noise patterns
        assert!(is_auto_memory_noise("Created task #42 for refactoring"));
        assert!(is_auto_memory_noise("Task ID: 123 completed"));
        assert!(is_auto_memory_noise("Created goal for milestone"));
        assert!(is_auto_memory_noise("Completed creation of task breakdown"));
        assert!(is_auto_memory_noise("Task breakdown for database pooling"));

        // Should NOT match - these are real insights
        assert!(!is_auto_memory_noise(
            "DatabasePool must be used for all access"
        ));
        assert!(!is_auto_memory_noise("User prefers concise responses"));
        assert!(!is_auto_memory_noise("Using builder pattern for Config"));
    }

    #[test]
    fn test_build_auto_memory_filters_noise() {
        let candidates = vec![
            make_auto_memory_candidate("Real insight about architecture", "decision", None, 5.0),
            make_auto_memory_candidate("Created task #42 for refactoring", "completion", None, 5.0),
            make_auto_memory_candidate("User prefers tabs", "preference", None, 5.0),
        ];
        let result = build_auto_memory_export(&candidates);

        assert!(result.contains("Real insight"));
        assert!(result.contains("User prefers tabs"));
        assert!(!result.contains("Created task #42")); // Filtered out
    }

    #[test]
    fn test_auto_memory_truncates_long_content() {
        let long_content = "x".repeat(600);
        let candidates = vec![make_auto_memory_candidate(
            &long_content,
            "general",
            None,
            5.0,
        )];
        let result = build_auto_memory_export(&candidates);

        // Find the entry line and check it's truncated
        for line in result.lines() {
            if let Some(entry) = line.strip_prefix("- ") {
                assert!(entry.len() <= AUTO_MEMORY_MAX_BYTES + 3); // +3 for "..."
                assert!(entry.ends_with("..."));
            }
        }
    }
}
