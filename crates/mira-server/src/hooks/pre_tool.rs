// crates/mira-server/src/hooks/pre_tool.rs
// PreToolUse hook handler - file reread advisory, symbol hints, and edit/write pattern warnings

use crate::hooks::{HookTimer, read_hook_input, write_hook_output};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum entries in the per-session file-read cache
const MAX_READ_CACHE_ENTRIES: usize = 50;

/// Minimum seconds since last read before we suppress the reread hint
/// (don't warn about rereads immediately after — Claude may have a good reason)
const REREAD_MIN_AGE_SECS: u64 = 30;

pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── File-read cache ─────────────────────────────────────────────────────────
// Tracks which files were read this session so we can advise Claude when it
// tries to re-read an unchanged file.

#[derive(Serialize, Deserialize, Default)]
struct FileReadCache {
    /// Map of file_path -> FileReadEntry
    entries: std::collections::HashMap<String, FileReadEntry>,
}

#[derive(Serialize, Deserialize, Clone)]
struct FileReadEntry {
    /// Unix timestamp of when we last saw this file read
    last_read_at: u64,
    /// File mtime (seconds since epoch) at time of read
    mtime_secs: u64,
}

fn read_cache_path(session_id: &str) -> std::path::PathBuf {
    let mira_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".mira")
        .join("tmp");
    // Sanitize to ASCII alphanumeric + hyphens, then truncate to 16 chars
    let sanitized: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let sid = if sanitized.len() > 16 {
        &sanitized[..16]
    } else {
        &sanitized
    };
    mira_dir.join(format!("reads_{}.json", sid))
}

fn load_read_cache(session_id: &str) -> FileReadCache {
    if session_id.is_empty() {
        return FileReadCache::default();
    }
    std::fs::read_to_string(read_cache_path(session_id))
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

fn save_read_cache(session_id: &str, cache: &FileReadCache) {
    if session_id.is_empty() {
        return;
    }
    let path = read_cache_path(session_id);
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("Failed to create Mira tmp dir: {e}");
        return;
    }
    if let Ok(json) = serde_json::to_string(cache) {
        // Write to temp file then rename for atomicity (prevents corruption on crash)
        let temp = path.with_extension("tmp");
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        if let Ok(mut f) = opts.open(&temp) {
            if let Err(e) = f.write_all(json.as_bytes()) {
                tracing::debug!("Failed to write read cache temp file: {e}");
                return;
            }
            drop(f);
            if let Err(e) = std::fs::rename(&temp, &path) {
                tracing::debug!("Failed to rename read cache temp file: {e}");
            }
        }
    }
}

/// Get file mtime as unix seconds, or 0 if unavailable
fn file_mtime_secs(path: &str) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Check if a file was already read this session and is unchanged.
/// Returns a hint string if so, None otherwise.
/// Also records the read in the cache for future checks.
fn check_and_record_read(session_id: &str, file_path: &str) -> Option<String> {
    let mut cache = load_read_cache(session_id);
    let now = unix_now();
    let current_mtime = file_mtime_secs(file_path);

    let hint = if let Some(entry) = cache.entries.get(file_path) {
        let age_secs = now.saturating_sub(entry.last_read_at);
        // Only hint if: file unchanged AND enough time has passed (not an immediate re-read)
        if current_mtime == entry.mtime_secs && age_secs >= REREAD_MIN_AGE_SECS {
            let age_str = if age_secs >= 3600 {
                format!("{}h ago", age_secs / 3600)
            } else if age_secs >= 60 {
                format!("{}m ago", age_secs / 60)
            } else {
                format!("{}s ago", age_secs)
            };
            // Extract filename for concise hint
            let filename = Path::new(file_path)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(file_path);
            Some(format!(
                "[Mira/efficiency] You already read {} ({}), file unchanged. Prefer using content already in your context window.",
                filename, age_str
            ))
        } else {
            None
        }
    } else {
        None
    };

    // Record/update the read
    cache.entries.insert(
        file_path.to_string(),
        FileReadEntry {
            last_read_at: now,
            mtime_secs: current_mtime,
        },
    );

    // Evict oldest entries if cache is full
    if cache.entries.len() > MAX_READ_CACHE_ENTRIES {
        let mut entries: Vec<(String, FileReadEntry)> = cache.entries.drain().collect();
        entries.sort_by(|a, b| b.1.last_read_at.cmp(&a.1.last_read_at));
        entries.truncate(MAX_READ_CACHE_ENTRIES);
        cache.entries = entries.into_iter().collect();
    }

    save_read_cache(session_id, &cache);
    hint
}

// ── Symbol hints for Read ───────────────────────────────────────────────
// For large files, inject a compact symbol map (function names + line numbers)
// so Claude can navigate without re-reading or guessing locations.

/// Minimum file line count to trigger symbol hints
const SYMBOL_HINT_MIN_LINES: usize = 200;

/// Maximum symbols to include in the hint
const SYMBOL_HINT_MAX_SYMBOLS: usize = 20;

/// Get a compact symbol map for a large file from the code index.
/// Returns None if the file is small, not indexed, or code DB unavailable.
async fn get_symbol_hints(file_path: &str) -> Option<String> {
    // Quick check: is the file large enough?
    let line_count = count_file_lines(file_path);
    if line_count < SYMBOL_HINT_MIN_LINES {
        return None;
    }

    // Open code database
    let code_db_path = crate::hooks::get_code_db_path();
    if !code_db_path.exists() {
        return None;
    }
    let code_pool = crate::db::pool::DatabasePool::open_code_db(&code_db_path)
        .await
        .ok()?;

    // Query symbols for this file.
    // The index stores relative paths (e.g. "src/hooks/pre_tool.rs") while
    // Read events provide absolute paths. Match in both directions:
    //   exact match, absolute-input ends with stored-relative, or vice versa.
    let fp = file_path.to_string();
    let symbols: Vec<(String, String, i64)> = code_pool
        .interact(move |conn| {
            let sql = r#"
                SELECT name, symbol_type, COALESCE(start_line, 0)
                FROM code_symbols
                WHERE file_path = ?1
                   OR ?1 LIKE '%/' || file_path
                   OR file_path LIKE '%/' || ?1
                ORDER BY start_line ASC
                LIMIT ?2
            "#;
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt
                .query_map(
                    rusqlite::params![fp, SYMBOL_HINT_MAX_SYMBOLS as i64],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )?
                .filter_map(|r| r.ok())
                .collect();
            Ok::<_, anyhow::Error>(rows)
        })
        .await
        .ok()?;

    if symbols.is_empty() {
        return None;
    }

    // Format: "[Mira/symbols] pool.rs (450 lines): DatabasePool:struct(12), open:fn(45), ..."
    let filename = Path::new(file_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(file_path);

    let symbol_list: Vec<String> = symbols
        .iter()
        .map(|(name, stype, line)| {
            let short_type = match stype.as_str() {
                "function" => "fn",
                "structure" | "struct" => "struct",
                "implementation" | "impl" => "impl",
                "enumeration" | "enum" => "enum",
                "trait" => "trait",
                "constant" | "const" => "const",
                "module" | "mod" => "mod",
                other => other,
            };
            format!("{}:{}({})", name, short_type, line)
        })
        .collect();

    Some(format!(
        "[Mira/symbols] {} ({} lines): {}",
        filename,
        line_count,
        symbol_list.join(", ")
    ))
}

/// Count lines in a file quickly (no full read, just count newlines).
/// Files larger than 50MB are skipped and return 0.
fn count_file_lines(path: &str) -> usize {
    const MAX_SIZE: u64 = 50 * 1024 * 1024;
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > MAX_SIZE {
        return 0;
    }
    std::fs::read(path)
        .map(|bytes| bytes.iter().filter(|&&b| b == b'\n').count())
        .unwrap_or(0)
}

/// PreToolUse hook input from Claude Code
#[derive(Debug)]
struct PreToolInput {
    tool_name: String,
    /// File path for Edit/Write/Read tools (extracted from tool_input.file_path)
    file_path: Option<String>,
    session_id: String,
}

impl PreToolInput {
    fn from_json(json: &serde_json::Value) -> Self {
        let tool_input = json.get("tool_input");

        // Extract file_path for Edit/Write/Read tools
        let file_path = tool_input
            .and_then(|ti| ti.get("file_path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Self {
            tool_name: json
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            file_path,
            session_id: json
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }
    }
}

/// Run PreToolUse hook
///
/// This hook fires before Grep/Glob/Read/Edit/Write tools execute. We:
/// 1. For Read: check file-read cache (reread advisory) and symbol hints
/// 2. For Edit/Write: check if the target file is a known change hotspot and warn
pub async fn run() -> Result<()> {
    let input = read_hook_input().context("Failed to parse hook input from stdin")?;
    let pre_input = PreToolInput::from_json(&input);

    // Handle Edit/Write: check for change pattern warnings (fast, no embeddings)
    if pre_input.tool_name == "Edit" || pre_input.tool_name == "Write" {
        return handle_edit_write_patterns(&input, &pre_input).await;
    }

    // Handle Read: check file-read cache and symbol hints (fast, no embeddings)
    if pre_input.tool_name == "Read"
        && let Some(ref fp) = pre_input.file_path
    {
        let mut hints: Vec<String> = Vec::new();

        // Reread advisory
        if let Some(hint) = check_and_record_read(&pre_input.session_id, fp) {
            hints.push(hint);
        }

        // Symbol map for large files (> 200 lines)
        if let Some(symbol_hint) = get_symbol_hints(fp).await {
            hints.push(symbol_hint);
        }

        if !hints.is_empty() {
            let output = serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "additionalContext": hints.join("\n")
                }
            });
            write_hook_output(&output);
            return Ok(());
        }
    }

    // No context to inject for other tools (memory recall removed)
    write_hook_output(&serde_json::json!({}));
    Ok(())
}

/// Handle Edit/Write tools: check if the target file is a known change hotspot.
///
/// Queries `behavior_patterns` for `change_pattern` entries whose `pattern_data`
/// mentions the target file path. Only does a simple SQL query (no embeddings)
/// to stay within the hook timeout.
async fn handle_edit_write_patterns(
    _input: &serde_json::Value,
    pre_input: &PreToolInput,
) -> Result<()> {
    let file_path = match &pre_input.file_path {
        Some(fp) if !fp.is_empty() => fp.clone(),
        _ => {
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };

    let _timer = HookTimer::start("PreToolUse:pattern_check");

    // Open DB directly (lightweight, no embeddings needed)
    let db_path = crate::hooks::get_db_path();
    let pool = match crate::db::pool::DatabasePool::open_hook(&db_path).await {
        Ok(p) => std::sync::Arc::new(p),
        Err(_) => {
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };

    // Resolve project using session_id for per-session isolation
    let sid = Some(pre_input.session_id.as_str()).filter(|s| !s.is_empty());
    let (project_id, _, _) = crate::hooks::resolve_project(&pool, sid).await;
    let Some(project_id) = project_id else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Query for change patterns that mention this file
    let fp = file_path.clone();

    let warnings: Vec<String> = pool
        .interact(move |conn| {
            let sql = r#"
                SELECT pattern_data, occurrence_count
                FROM behavior_patterns
                WHERE project_id = ?1
                  AND pattern_type = 'change_pattern'
                  AND pattern_data LIKE ?2 ESCAPE '\'
                ORDER BY occurrence_count DESC
                LIMIT 3
            "#;
            // Use the filename (not full path) for broader matching
            let filename = Path::new(&fp)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(&fp);
            // Escape SQL LIKE wildcards to prevent injection
            let escaped = filename
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            let like_pattern = format!("%{}%", escaped);

            let mut stmt = match conn.prepare(sql) {
                Ok(s) => s,
                Err(_) => return Ok::<_, anyhow::Error>(Vec::new()),
            };
            let rows = stmt
                .query_map(rusqlite::params![project_id, like_pattern], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map(|rows| {
                    rows.filter_map(|r| r.ok()).collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let mut warnings = Vec::new();
            for (pattern_data_str, occurrence_count) in rows {
                if let Some(crate::db::patterns::PatternData::ChangePattern {
                    pattern_subtype,
                    outcome_stats,
                    ..
                }) = crate::db::patterns::PatternData::from_json(&pattern_data_str)
                {
                    let warning = match pattern_subtype.as_str() {
                        "module_hotspot" => format!(
                            "hotspot: modified {} times, {}/{} changes needed follow-up fixes",
                            occurrence_count,
                            outcome_stats.follow_up_fix,
                            outcome_stats.total,
                        ),
                        "size_risk" => format!(
                            "size risk: {}/{} changes to this area needed follow-up fixes",
                            outcome_stats.follow_up_fix, outcome_stats.total,
                        ),
                        "co_change_gap" => format!(
                            "co-change pattern: this file is usually changed with related files ({}/{} had issues when changed alone)",
                            outcome_stats.follow_up_fix, outcome_stats.total,
                        ),
                        other => format!(
                            "{}: modified {} times",
                            other, occurrence_count,
                        ),
                    };
                    warnings.push(warning);
                }
            }
            Ok(warnings)
        })
        .await
        .unwrap_or_default();

    let output = if warnings.is_empty() {
        serde_json::json!({})
    } else {
        let context = format!("[Mira/patterns] \u{26a0} {}", warnings.join("; "),);
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": context
            }
        })
    };

    write_hook_output(&output);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── PreToolInput::from_json ─────────────────────────────────────────────

    #[test]
    fn pre_input_parses_full_input() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": "Edit",
            "tool_input": {
                "file_path": "/home/user/project/src/main.rs"
            }
        }));
        assert_eq!(input.tool_name, "Edit");
        assert_eq!(
            input.file_path.as_deref(),
            Some("/home/user/project/src/main.rs")
        );
    }

    #[test]
    fn pre_input_defaults_on_empty_json() {
        let input = PreToolInput::from_json(&serde_json::json!({}));
        assert!(input.tool_name.is_empty());
        assert!(input.file_path.is_none());
    }

    #[test]
    fn pre_input_ignores_wrong_types() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": 999,
            "tool_input": {
                "file_path": true
            }
        }));
        assert!(input.tool_name.is_empty());
        assert!(input.file_path.is_none());
    }

    #[test]
    fn pre_input_missing_tool_input() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": "Glob"
        }));
        assert_eq!(input.tool_name, "Glob");
        assert!(input.file_path.is_none());
    }

    // ── Edit/Write file_path extraction ───────────────────────────────────

    #[test]
    fn pre_input_extracts_file_path_for_edit() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": "Edit",
            "tool_input": {
                "file_path": "/home/user/project/src/main.rs",
                "old_string": "foo",
                "new_string": "bar"
            }
        }));
        assert_eq!(input.tool_name, "Edit");
        assert_eq!(
            input.file_path.as_deref(),
            Some("/home/user/project/src/main.rs")
        );
    }

    #[test]
    fn pre_input_extracts_file_path_for_write() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": "Write",
            "tool_input": {
                "file_path": "/home/user/project/new_file.rs",
                "content": "fn main() {}"
            }
        }));
        assert_eq!(input.tool_name, "Write");
        assert_eq!(
            input.file_path.as_deref(),
            Some("/home/user/project/new_file.rs")
        );
    }

    #[test]
    fn pre_input_no_file_path_for_grep() {
        let input = PreToolInput::from_json(&serde_json::json!({
            "tool_name": "Grep",
            "tool_input": {
                "pattern": "fn main",
                "path": "/home/user/project/src"
            }
        }));
        assert!(input.file_path.is_none());
    }

    // ── File-read cache ─────────────────────────────────────────────────────

    #[test]
    fn file_read_cache_records_and_detects_reread() {
        let sid = format!("test_cache_{}", std::process::id());
        // Clean up any stale cache
        let _ = std::fs::remove_file(read_cache_path(&sid));

        // First read: should return no hint
        let hint = check_and_record_read(&sid, "/tmp/test_file_abc.rs");
        assert!(hint.is_none(), "First read should not produce a hint");

        // Immediate re-read: should still return no hint (within REREAD_MIN_AGE_SECS)
        let hint = check_and_record_read(&sid, "/tmp/test_file_abc.rs");
        assert!(
            hint.is_none(),
            "Immediate re-read should not produce a hint (too recent)"
        );

        // Clean up
        let _ = std::fs::remove_file(read_cache_path(&sid));
    }

    #[test]
    fn file_read_cache_empty_session_id() {
        // Empty session ID should return no hint and not crash
        let hint = check_and_record_read("", "/tmp/some_file.rs");
        assert!(hint.is_none());
    }

    #[test]
    fn file_read_cache_different_files_no_hint() {
        let sid = format!("test_diff_{}", std::process::id());
        let _ = std::fs::remove_file(read_cache_path(&sid));

        let _ = check_and_record_read(&sid, "/tmp/file_a.rs");
        let hint = check_and_record_read(&sid, "/tmp/file_b.rs");
        assert!(hint.is_none(), "Different files should not produce a hint");

        let _ = std::fs::remove_file(read_cache_path(&sid));
    }

    #[test]
    fn file_read_cache_eviction() {
        let sid = format!("test_evict_{}", std::process::id());
        let _ = std::fs::remove_file(read_cache_path(&sid));

        // Fill cache beyond MAX_READ_CACHE_ENTRIES
        for i in 0..MAX_READ_CACHE_ENTRIES + 10 {
            let _ = check_and_record_read(&sid, &format!("/tmp/evict_test_{}.rs", i));
        }

        // Cache should have been trimmed
        let cache = load_read_cache(&sid);
        assert!(
            cache.entries.len() <= MAX_READ_CACHE_ENTRIES,
            "Cache should be evicted to {} entries, got {}",
            MAX_READ_CACHE_ENTRIES,
            cache.entries.len()
        );

        let _ = std::fs::remove_file(read_cache_path(&sid));
    }

    #[test]
    fn file_read_cache_hint_when_aged_out() {
        let sid = format!("test_aged_{}", std::process::id());
        let cache_path = read_cache_path(&sid);
        let _ = std::fs::remove_file(&cache_path);

        // Create a real temp file so mtime is consistent
        let tmp_file =
            std::env::temp_dir().join(format!("mira_aged_test_{}.rs", std::process::id()));
        let tmp_file = tmp_file.to_string_lossy().to_string();
        std::fs::write(&tmp_file, b"fn foo() {}").unwrap();
        let current_mtime = file_mtime_secs(&tmp_file);

        // Inject a cache entry with last_read_at far in the past
        let past_ts = unix_now().saturating_sub(REREAD_MIN_AGE_SECS + 120);
        let mut cache = FileReadCache::default();
        cache.entries.insert(
            tmp_file.clone(),
            FileReadEntry {
                last_read_at: past_ts,
                mtime_secs: current_mtime,
            },
        );
        if let Some(parent) = cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::to_string(&cache).unwrap();
        std::fs::write(&cache_path, json).unwrap();

        // check_and_record_read should now produce a hint
        let hint = check_and_record_read(&sid, &tmp_file);
        assert!(hint.is_some(), "Aged-out reread should produce a hint");
        let hint_str = hint.unwrap();
        assert!(
            hint_str.contains("[Mira/efficiency]"),
            "Hint should contain [Mira/efficiency]: {hint_str}"
        );
        assert!(
            hint_str.contains("already read"),
            "Hint should contain 'already read': {hint_str}"
        );
        assert!(
            hint_str.contains("unchanged"),
            "Hint should contain 'unchanged': {hint_str}"
        );

        let _ = std::fs::remove_file(&cache_path);
        let _ = std::fs::remove_file(&tmp_file);
    }

    #[test]
    fn file_mtime_secs_nonexistent_file() {
        assert_eq!(file_mtime_secs("/nonexistent/path/xyz.rs"), 0);
    }

    #[test]
    fn read_cache_path_truncates_long_session_id() {
        let long_sid = "a".repeat(100);
        let path = read_cache_path(&long_sid);
        let filename = path.file_name().unwrap().to_str().unwrap();
        // Should use only first 16 chars of session ID
        assert!(
            filename.contains(&"a".repeat(16)),
            "Should contain truncated session ID"
        );
        assert!(
            !filename.contains(&"a".repeat(17)),
            "Should not contain more than 16 chars of session ID"
        );
    }

    // ── Symbol hints ────────────────────────────────────────────────────────

    #[test]
    fn count_file_lines_nonexistent() {
        assert_eq!(count_file_lines("/nonexistent/file.rs"), 0);
    }

    #[test]
    fn count_file_lines_real_file() {
        // This test file itself should have some lines
        let _count = count_file_lines(file!());
        // Use the workspace-relative path; count_file_lines needs absolute/valid path
        let this_file = concat!(env!("CARGO_MANIFEST_DIR"), "/src/hooks/pre_tool.rs");
        let count = count_file_lines(this_file);
        assert!(
            count > 100,
            "pre_tool.rs should have > 100 lines, got {}",
            count
        );
    }

    #[test]
    fn symbol_hint_threshold() {
        // Files under threshold should not trigger hints
        assert_eq!(SYMBOL_HINT_MIN_LINES, 200);
        assert_eq!(SYMBOL_HINT_MAX_SYMBOLS, 20);
    }

    #[tokio::test]
    async fn get_symbol_hints_small_file() {
        // A small/nonexistent file should return None
        let hint = get_symbol_hints("/nonexistent/small.rs").await;
        assert!(hint.is_none());
    }

    #[tokio::test]
    async fn get_symbol_hints_no_code_db() {
        // Even for a large file, if code DB doesn't exist, returns None
        // (the function checks code_db_path.exists())
        let hint = get_symbol_hints("/tmp/definitely_not_indexed.rs").await;
        assert!(hint.is_none());
    }
}
