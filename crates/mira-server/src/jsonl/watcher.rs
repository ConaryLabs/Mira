// crates/mira-server/src/jsonl/watcher.rs
// Background watcher that tails active Claude Code session JSONL files,
// incrementally parsing new entries and maintaining a running summary.

use super::parser::{self, SessionSummary, TurnSummary};
use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, watch};

/// How often to poll for new data after an inotify event (ms).
const POLL_INTERVAL_MS: u64 = 200;

/// How long to wait for inotify events before checking shutdown (ms).
const SELECT_TIMEOUT_MS: u64 = 2000;

/// Maximum bytes to read in a single cycle. Prevents unbounded memory use
/// if a huge chunk is appended between polls. The next cycle picks up the rest.
const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;

/// Maximum retained turns in the watcher's running summary. The snapshot only
/// uses the last 10, so keeping more than ~100 is wasteful. The batch parser
/// in parser.rs keeps all turns (correct for CLI analysis); this limit is
/// watcher-only.
const MAX_RETAINED_TURNS: usize = 100;

/// Running state for a watched JSONL file.
struct WatchedFile {
    path: PathBuf,
    /// Byte offset of last read position.
    byte_offset: u64,
    /// Running session summary.
    summary: SessionSummary,
}

/// Result from the blocking read closure.
enum ReadResult {
    /// No new data available.
    NoChange,
    /// Incremental new lines parsed from `offset..new_offset`.
    Incremental(Vec<String>, u64),
    /// File was truncated -- full re-read from start. Summary must be reset
    /// before applying these lines.
    Truncated(Vec<String>, u64),
}

/// Snapshot of live session stats, safe to share across threads.
#[derive(Debug, Clone, Default)]
pub struct SessionSnapshot {
    pub session_id: Option<String>,
    pub turn_count: usize,
    pub user_prompt_count: u64,
    pub tool_result_count: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub tool_calls: HashMap<String, u64>,
    pub compaction_count: u64,
    pub last_timestamp: Option<String>,
    /// Recent turns (last N) for live display.
    pub recent_turns: Vec<TurnSummary>,
}

impl SessionSnapshot {
    pub fn total_billable_input(&self) -> u64 {
        self.total_input_tokens + self.total_cache_read_tokens + self.total_cache_creation_tokens
    }

    pub fn total_tool_calls(&self) -> u64 {
        self.tool_calls.values().sum()
    }
}

impl From<&SessionSummary> for SessionSnapshot {
    fn from(s: &SessionSummary) -> Self {
        let recent_count = 10;
        let recent_turns = if s.turns.len() > recent_count {
            s.turns[s.turns.len() - recent_count..].to_vec()
        } else {
            s.turns.clone()
        };

        Self {
            session_id: s.session_id.clone(),
            turn_count: s.turn_count(),
            user_prompt_count: s.user_prompt_count,
            tool_result_count: s.tool_result_count,
            total_input_tokens: s.total_input_tokens(),
            total_output_tokens: s.total_output_tokens(),
            total_cache_read_tokens: s.total_cache_read_tokens(),
            total_cache_creation_tokens: s.total_cache_creation_tokens(),
            tool_calls: s.tool_calls.clone(),
            compaction_count: s.compaction_count,
            last_timestamp: s.last_timestamp.clone(),
            recent_turns,
        }
    }
}

/// Shared handle for reading live session stats.
#[derive(Clone)]
pub struct SessionWatcherHandle {
    snapshot: Arc<RwLock<SessionSnapshot>>,
}

impl SessionWatcherHandle {
    /// Get a snapshot of the current session stats.
    pub async fn snapshot(&self) -> SessionSnapshot {
        self.snapshot.read().await.clone()
    }
}

/// Find the JSONL file for a given session ID.
///
/// Claude Code stores session logs at:
///   ~/.claude/projects/<project-slug>/<session-id>.jsonl
///
/// Since we don't always know the project slug, we search all project dirs.
pub fn find_session_jsonl(session_id: &str) -> Option<PathBuf> {
    let claude_dir = dirs::home_dir()?.join(".claude/projects");
    if !claude_dir.exists() {
        return None;
    }

    let filename = format!("{}.jsonl", session_id);

    for entry in std::fs::read_dir(&claude_dir).ok()?.flatten() {
        if entry.file_type().ok()?.is_dir() {
            let candidate = entry.path().join(&filename);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Spawn a background JSONL watcher for the given session.
///
/// Returns a handle for reading live stats, or None if the file wasn't found.
pub fn spawn_watcher(
    session_id: &str,
    shutdown: watch::Receiver<bool>,
) -> Option<SessionWatcherHandle> {
    let path = find_session_jsonl(session_id)?;
    let snapshot = Arc::new(RwLock::new(SessionSnapshot::default()));
    let handle = SessionWatcherHandle {
        snapshot: snapshot.clone(),
    };

    tokio::spawn(async move {
        if let Err(e) = run_watcher(path, snapshot, shutdown).await {
            tracing::warn!("JSONL watcher exited with error: {}", e);
        }
    });

    Some(handle)
}

/// Also allow watching a specific file path directly (for tests, CLI).
pub fn spawn_watcher_for_path(
    path: PathBuf,
    shutdown: watch::Receiver<bool>,
) -> SessionWatcherHandle {
    let snapshot = Arc::new(RwLock::new(SessionSnapshot::default()));
    let handle = SessionWatcherHandle {
        snapshot: snapshot.clone(),
    };

    tokio::spawn(async move {
        if let Err(e) = run_watcher(path, snapshot, shutdown).await {
            tracing::warn!("JSONL watcher exited with error: {}", e);
        }
    });

    handle
}

/// Main watcher loop: initial parse then incremental tail.
async fn run_watcher(
    path: PathBuf,
    snapshot: Arc<RwLock<SessionSnapshot>>,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    // Initial full parse
    let mut watched = {
        let summary = parser::parse_session_file(&path)?;
        let byte_offset = std::fs::metadata(&path)?.len();

        tracing::debug!(
            "JSONL watcher: initial parse of {:?}: {} turns, {} bytes",
            path,
            summary.turn_count(),
            byte_offset
        );

        // Publish initial snapshot
        {
            let mut snap = snapshot.write().await;
            *snap = SessionSnapshot::from(&summary);
        }

        WatchedFile {
            path: path.clone(),
            byte_offset,
            summary,
        }
    };

    // Set up file system watcher
    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel::<()>(16);
    let _watcher = {
        use notify::{Config, Event, EventKind, RecommendedWatcher, Watcher};

        let tx = notify_tx.clone();
        let mut watcher: RecommendedWatcher = Watcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res
                    && matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_))
                {
                    let _ = tx.try_send(());
                }
            },
            Config::default(),
        )
        .map_err(io::Error::other)?;

        // Watch the parent directory (the file might not exist at the exact path yet
        // if we're racing with Claude Code's first write)
        let watch_path = path.parent().unwrap_or(&path);
        watcher
            .watch(watch_path, notify::RecursiveMode::NonRecursive)
            .map_err(io::Error::other)?;

        watcher
    };

    loop {
        tokio::select! {
            _ = notify_rx.recv() => {
                // Small delay to batch rapid writes
                tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
                // Drain any extra notifications that arrived during the delay
                while notify_rx.try_recv().is_ok() {}

                if let Err(e) = read_new_entries(&mut watched).await {
                    tracing::debug!("JSONL watcher read error: {}", e);
                    continue;
                }

                // Publish updated snapshot
                let new_snapshot = SessionSnapshot::from(&watched.summary);
                let mut snap = snapshot.write().await;
                *snap = new_snapshot;
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(SELECT_TIMEOUT_MS)) => {
                // Periodic poll as fallback (inotify can miss events in some cases)
                let current_len = std::fs::metadata(&watched.path)
                    .map(|m| m.len())
                    .unwrap_or(watched.byte_offset);

                if current_len != watched.byte_offset {
                    if let Err(e) = read_new_entries(&mut watched).await {
                        tracing::debug!("JSONL watcher periodic read error: {}", e);
                    } else {
                        let new_snapshot = SessionSnapshot::from(&watched.summary);
                        let mut snap = snapshot.write().await;
                        *snap = new_snapshot;
                    }
                }
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    tracing::debug!("JSONL watcher shutting down");
                    break;
                }
            }
        }
    }

    Ok(())
}

/// Read bytes from the file and split into complete lines.
///
/// Uses `read_to_end` / `read_exact` into `Vec<u8>` to handle non-UTF-8 data
/// (binary tool output in JSONL). Lines that are not valid UTF-8 are silently
/// skipped. Byte offsets are tracked from the raw buffer to avoid drift from
/// lossy conversion.
fn read_lines_from_offset(path: &PathBuf, offset: u64) -> io::Result<ReadResult> {
    let mut file = std::fs::File::open(path)?;
    let file_len = file.metadata()?.len();

    if file_len < offset {
        // File was truncated -- re-read from the beginning
        let read_len = std::cmp::min(file_len, MAX_READ_BYTES) as usize;
        let mut buf = vec![0u8; read_len];
        file.read_exact(&mut buf)?;

        let (lines, committed) = extract_complete_lines(&buf);
        return Ok(ReadResult::Truncated(lines, committed));
    }
    if file_len == offset {
        return Ok(ReadResult::NoChange);
    }

    file.seek(SeekFrom::Start(offset))?;

    // Cap read size to prevent unbounded memory use
    let read_len = std::cmp::min(file_len - offset, MAX_READ_BYTES) as usize;
    let mut buf = vec![0u8; read_len];
    file.read_exact(&mut buf)?;

    let (lines, committed) = extract_complete_lines(&buf);
    Ok(ReadResult::Incremental(lines, offset + committed))
}

/// Extract complete newline-terminated lines from a raw byte buffer.
///
/// Returns the parsed lines (skipping non-UTF-8) and the number of committed
/// bytes (only complete lines, excluding any trailing partial line).
fn extract_complete_lines(buf: &[u8]) -> (Vec<String>, u64) {
    let mut lines = Vec::new();
    let mut committed_bytes: u64 = 0;

    for chunk in buf.split(|&b| b == b'\n') {
        let line_with_newline = chunk.len() as u64 + 1; // +1 for '\n'
        if committed_bytes + line_with_newline > buf.len() as u64 {
            // This is the last segment with no trailing '\n' -- partial line
            break;
        }
        committed_bytes += line_with_newline;
        // Only parse UTF-8 valid lines; skip non-UTF-8 (binary content)
        if let Ok(s) = std::str::from_utf8(chunk) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
        }
    }

    (lines, committed_bytes)
}

/// Read new lines from the JSONL file starting at the last known byte offset.
async fn read_new_entries(watched: &mut WatchedFile) -> io::Result<()> {
    let path = watched.path.clone();
    let offset = watched.byte_offset;

    // File I/O is blocking, move to blocking thread
    let result = tokio::task::spawn_blocking(move || -> io::Result<ReadResult> {
        read_lines_from_offset(&path, offset)
    })
    .await
    .map_err(io::Error::other)??;

    match result {
        ReadResult::NoChange => {}
        ReadResult::Incremental(new_lines, new_offset) => {
            for line in &new_lines {
                parse_line_into_summary(line, &mut watched.summary);
            }
            watched.byte_offset = new_offset;

            if !new_lines.is_empty() {
                tracing::debug!(
                    "JSONL watcher: parsed {} new lines, now at {} turns",
                    new_lines.len(),
                    watched.summary.turn_count()
                );
            }
        }
        ReadResult::Truncated(new_lines, new_offset) => {
            // File was truncated -- reset summary and re-parse from scratch
            tracing::debug!(
                "JSONL watcher: file truncated (was at {} bytes), resetting summary",
                watched.byte_offset
            );
            watched.summary = SessionSummary::default();
            for line in &new_lines {
                parse_line_into_summary(line, &mut watched.summary);
            }
            watched.byte_offset = new_offset;
        }
    }

    Ok(())
}

/// Parse a single JSONL line and merge it into the running summary.
/// This mirrors the logic in parser.rs but operates on one line at a time.
fn parse_line_into_summary(line: &str, summary: &mut SessionSummary) {
    // Reuse the same parsing approach as the batch parser
    let one_line_summary = parser::parse_session_entries(line);

    // Merge metadata
    if summary.session_id.is_none() {
        summary.session_id = one_line_summary.session_id;
    }
    if summary.version.is_none() {
        summary.version = one_line_summary.version;
    }
    if summary.git_branch.is_none() {
        summary.git_branch = one_line_summary.git_branch;
    }

    // Update last timestamp
    if let Some(ts) = one_line_summary.last_timestamp {
        summary.last_timestamp = Some(ts);
    }
    if summary.first_timestamp.is_none() {
        summary.first_timestamp = one_line_summary.first_timestamp;
    }

    // Merge turns (watcher-only cap to bound memory; batch parser keeps all)
    summary.turns.extend(one_line_summary.turns);
    if summary.turns.len() > MAX_RETAINED_TURNS {
        let drain_count = summary.turns.len() - MAX_RETAINED_TURNS;
        summary.turns.drain(..drain_count);
    }

    // Merge tool calls
    for (name, count) in one_line_summary.tool_calls {
        *summary.tool_calls.entry(name).or_insert(0) += count;
    }

    // Merge counts
    summary.user_prompt_count += one_line_summary.user_prompt_count;
    summary.tool_result_count += one_line_summary.tool_result_count;
    summary.compaction_count += one_line_summary.compaction_count;
    summary.parse_errors += one_line_summary.parse_errors;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonl::TokenUsage;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_assistant_line(input: u64, output: u64, tool: Option<&str>) -> String {
        let content = if let Some(name) = tool {
            format!(
                r#"[{{"type":"tool_use","name":"{}","id":"t1","input":{{}}}}]"#,
                name
            )
        } else {
            r#"[{"type":"text","text":"hi"}]"#.to_string()
        };
        format!(
            r#"{{"type":"assistant","uuid":"a1","timestamp":"2026-01-01T00:00:00Z","sessionId":"test-sess","message":{{"content":{},"usage":{{"input_tokens":{},"output_tokens":{},"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#,
            content, input, output
        )
    }

    fn make_user_line() -> String {
        r#"{"type":"user","uuid":"u1","timestamp":"2026-01-01T00:00:01Z","sessionId":"test-sess","message":{"role":"user","content":"hello"}}"#.to_string()
    }

    #[test]
    fn test_parse_line_into_summary() {
        let mut summary = SessionSummary::default();

        parse_line_into_summary(&make_user_line(), &mut summary);
        assert_eq!(summary.user_prompt_count, 1);
        assert_eq!(summary.turn_count(), 0);

        parse_line_into_summary(&make_assistant_line(10, 50, Some("Read")), &mut summary);
        assert_eq!(summary.turn_count(), 1);
        assert_eq!(summary.total_output_tokens(), 50);
        assert_eq!(*summary.tool_calls.get("Read").unwrap_or(&0), 1);

        parse_line_into_summary(&make_assistant_line(5, 30, Some("Read")), &mut summary);
        assert_eq!(summary.turn_count(), 2);
        assert_eq!(summary.total_output_tokens(), 80);
        assert_eq!(*summary.tool_calls.get("Read").unwrap_or(&0), 2);
    }

    #[test]
    fn test_session_snapshot_from_summary() {
        let mut summary = SessionSummary::default();
        summary.session_id = Some("test".to_string());
        summary.user_prompt_count = 3;
        summary.tool_calls.insert("Bash".to_string(), 5);

        // Add a turn
        summary.turns.push(TurnSummary {
            uuid: Some("a1".to_string()),
            timestamp: Some("2026-01-01T00:00:00Z".to_string()),
            usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 200,
                cache_read_input_tokens: 5000,
                cache_creation_input_tokens: 1000,
            },
            tool_calls: vec!["Bash".to_string()],
            content_types: vec!["tool_use".to_string()],
            is_sidechain: false,
        });

        let snap = SessionSnapshot::from(&summary);
        assert_eq!(snap.session_id, Some("test".to_string()));
        assert_eq!(snap.turn_count, 1);
        assert_eq!(snap.user_prompt_count, 3);
        assert_eq!(snap.total_input_tokens, 100);
        assert_eq!(snap.total_output_tokens, 200);
        assert_eq!(snap.total_cache_read_tokens, 5000);
        assert_eq!(snap.total_cache_creation_tokens, 1000);
        assert_eq!(snap.total_billable_input(), 6100);
        assert_eq!(snap.total_tool_calls(), 5);
    }

    #[test]
    fn test_find_session_jsonl_missing() {
        // A session ID that definitely doesn't exist
        assert!(find_session_jsonl("nonexistent-session-id-12345").is_none());
    }

    #[tokio::test]
    async fn test_incremental_read() {
        // Create a temp file with initial content
        let mut tmpfile = NamedTempFile::new().expect("create temp file");
        writeln!(tmpfile, "{}", make_user_line()).expect("write");
        writeln!(tmpfile, "{}", make_assistant_line(10, 50, None)).expect("write");
        tmpfile.flush().expect("flush");

        let initial_len = tmpfile.as_file().metadata().expect("meta").len();

        let mut watched = WatchedFile {
            path: tmpfile.path().to_path_buf(),
            byte_offset: initial_len,
            summary: parser::parse_session_file(tmpfile.path()).expect("parse"),
        };

        assert_eq!(watched.summary.turn_count(), 1);
        assert_eq!(watched.summary.user_prompt_count, 1);

        // Append new content
        writeln!(tmpfile, "{}", make_assistant_line(5, 30, Some("Grep"))).expect("write");
        writeln!(tmpfile, "{}", make_user_line()).expect("write");
        tmpfile.flush().expect("flush");

        // Read incrementally
        read_new_entries(&mut watched).await.expect("read");

        assert_eq!(watched.summary.turn_count(), 2);
        assert_eq!(watched.summary.user_prompt_count, 2);
        assert_eq!(watched.summary.total_output_tokens(), 80);
        assert_eq!(*watched.summary.tool_calls.get("Grep").unwrap_or(&0), 1);
    }

    #[tokio::test]
    async fn test_partial_line_not_consumed() {
        // Write two complete lines + one partial (no trailing newline)
        let mut tmpfile = NamedTempFile::new().expect("create temp file");
        writeln!(tmpfile, "{}", make_user_line()).expect("write");
        writeln!(tmpfile, "{}", make_assistant_line(10, 50, None)).expect("write");
        // Write partial line WITHOUT trailing newline
        write!(tmpfile, "{}", make_assistant_line(5, 30, Some("Grep"))).expect("write");
        tmpfile.flush().expect("flush");

        let initial_len = {
            // Only count the two complete lines
            let full = format!(
                "{}\n{}\n",
                make_user_line(),
                make_assistant_line(10, 50, None)
            );
            full.len() as u64
        };

        let mut watched = WatchedFile {
            path: tmpfile.path().to_path_buf(),
            byte_offset: initial_len,
            summary: parser::parse_session_file(tmpfile.path()).expect("parse"),
        };

        // The partial line should NOT be consumed yet
        // (initial full parse may have read it, but incremental should not advance past it)
        let offset_before = watched.byte_offset;
        read_new_entries(&mut watched).await.expect("read");
        // Offset should NOT advance past the partial line
        assert_eq!(
            watched.byte_offset, offset_before,
            "offset should not advance past unterminated partial line"
        );

        // Now complete the line
        writeln!(tmpfile).expect("write newline");
        tmpfile.flush().expect("flush");

        read_new_entries(&mut watched).await.expect("read");
        // NOW the line should be consumed
        assert_eq!(
            watched.summary.turn_count(),
            2 + 1,
            "completed line should now be parsed (initial 2 from full parse + 1 new)"
        );
    }

    #[tokio::test]
    async fn test_truncation_resets_summary() {
        // Create a temp file with initial content: 1 user + 2 assistant turns
        let mut tmpfile = NamedTempFile::new().expect("create temp file");
        writeln!(tmpfile, "{}", make_user_line()).expect("write");
        writeln!(tmpfile, "{}", make_assistant_line(10, 50, None)).expect("write");
        writeln!(tmpfile, "{}", make_assistant_line(5, 30, Some("Grep"))).expect("write");
        tmpfile.flush().expect("flush");

        let initial_len = tmpfile.as_file().metadata().expect("meta").len();

        let mut watched = WatchedFile {
            path: tmpfile.path().to_path_buf(),
            byte_offset: initial_len,
            summary: parser::parse_session_file(tmpfile.path()).expect("parse"),
        };

        assert_eq!(watched.summary.turn_count(), 2);
        assert_eq!(watched.summary.user_prompt_count, 1);
        assert_eq!(watched.summary.total_output_tokens(), 80);

        // Truncate the file and write smaller content (1 user + 1 assistant)
        {
            let file = tmpfile.as_file_mut();
            file.set_len(0).expect("truncate");
            file.seek(SeekFrom::Start(0)).expect("seek");
        }
        writeln!(tmpfile, "{}", make_user_line()).expect("write");
        writeln!(tmpfile, "{}", make_assistant_line(20, 100, Some("Edit"))).expect("write");
        tmpfile.flush().expect("flush");

        // The file is now shorter than watched.byte_offset -- triggers truncation path
        read_new_entries(&mut watched).await.expect("read");

        // Summary should reflect ONLY the new file content, not old + new (no double-count)
        assert_eq!(
            watched.summary.turn_count(),
            1,
            "after truncation, should have 1 turn (not 2+1=3)"
        );
        assert_eq!(
            watched.summary.user_prompt_count, 1,
            "after truncation, should have 1 user prompt (not 1+1=2)"
        );
        assert_eq!(
            watched.summary.total_output_tokens(),
            100,
            "after truncation, should have 100 output tokens (not 80+100=180)"
        );
        assert_eq!(*watched.summary.tool_calls.get("Edit").unwrap_or(&0), 1);
        // Old tool call should be gone after reset
        assert_eq!(
            *watched.summary.tool_calls.get("Grep").unwrap_or(&0),
            0,
            "old tool calls should be cleared after truncation"
        );
    }

    #[tokio::test]
    async fn test_binary_data_in_lines() {
        // Simulate a JSONL file with a line containing non-UTF-8 bytes
        let mut tmpfile = NamedTempFile::new().expect("create temp file");

        // Write a valid user line
        let user_line = make_user_line();
        tmpfile.write_all(user_line.as_bytes()).expect("write");
        tmpfile.write_all(b"\n").expect("write newline");

        // Write a line with invalid UTF-8 bytes (simulating binary tool output)
        tmpfile
            .write_all(b"{\"type\":\"result\",\"data\":\"\xff\xfe\xfd\"}\n")
            .expect("write binary");

        // Write another valid assistant line
        let asst_line = make_assistant_line(10, 50, None);
        tmpfile.write_all(asst_line.as_bytes()).expect("write");
        tmpfile.write_all(b"\n").expect("write newline");

        tmpfile.flush().expect("flush");

        let mut watched = WatchedFile {
            path: tmpfile.path().to_path_buf(),
            byte_offset: 0,
            summary: SessionSummary::default(),
        };

        // Should not error -- non-UTF-8 lines are skipped
        read_new_entries(&mut watched)
            .await
            .expect("read should not fail on binary data");

        // Should have parsed the valid user and assistant lines
        assert_eq!(watched.summary.user_prompt_count, 1);
        assert_eq!(watched.summary.turn_count(), 1);

        // Byte offset should have advanced past all three lines
        let expected_len = tmpfile.as_file().metadata().expect("meta").len();
        assert_eq!(
            watched.byte_offset, expected_len,
            "byte offset should advance past all complete lines including non-UTF-8 ones"
        );
    }

    #[test]
    fn test_extract_complete_lines_partial() {
        // Buffer with two complete lines and one partial
        let buf = b"line1\nline2\npartial";
        let (lines, committed) = extract_complete_lines(buf);
        assert_eq!(lines, vec!["line1", "line2"]);
        assert_eq!(committed, 12); // "line1\n" (6) + "line2\n" (6)
    }

    #[test]
    fn test_extract_complete_lines_all_terminated() {
        let buf = b"line1\nline2\n";
        let (lines, committed) = extract_complete_lines(buf);
        assert_eq!(lines, vec!["line1", "line2"]);
        assert_eq!(committed, 12);
    }

    #[test]
    fn test_extract_complete_lines_empty_lines_skipped() {
        let buf = b"line1\n\n\nline2\n";
        let (lines, committed) = extract_complete_lines(buf);
        assert_eq!(lines, vec!["line1", "line2"]);
        assert_eq!(committed, 14);
    }

    #[test]
    fn test_extract_complete_lines_non_utf8_skipped() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"valid\n");
        buf.extend_from_slice(b"\xff\xfe\xfd\n");
        buf.extend_from_slice(b"also_valid\n");
        let (lines, committed) = extract_complete_lines(&buf);
        assert_eq!(lines, vec!["valid", "also_valid"]);
        assert_eq!(committed, buf.len() as u64);
    }

    #[tokio::test]
    async fn test_watcher_lifecycle() {
        // Create temp file
        let mut tmpfile = NamedTempFile::new().expect("create temp file");
        writeln!(tmpfile, "{}", make_user_line()).expect("write");
        writeln!(tmpfile, "{}", make_assistant_line(10, 50, None)).expect("write");
        tmpfile.flush().expect("flush");

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = spawn_watcher_for_path(tmpfile.path().to_path_buf(), shutdown_rx);

        // Give the watcher time to do initial parse
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let snap = handle.snapshot().await;
        assert_eq!(snap.turn_count, 1);
        assert_eq!(snap.user_prompt_count, 1);

        // Append and wait for watcher to pick it up
        writeln!(tmpfile, "{}", make_assistant_line(5, 30, Some("Edit"))).expect("write");
        tmpfile.flush().expect("flush");

        // Wait for watcher poll cycle + processing
        tokio::time::sleep(std::time::Duration::from_millis(3000)).await;

        let snap = handle.snapshot().await;
        assert_eq!(snap.turn_count, 2);
        assert_eq!(snap.total_output_tokens, 80);

        // Shutdown
        shutdown_tx.send(true).expect("shutdown");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
