// crates/mira-server/src/jsonl/watcher.rs
// Background watcher that tails active Claude Code session JSONL files,
// incrementally parsing new entries and maintaining a running summary.

use super::parser::{self, SessionSummary, TurnSummary};
use std::collections::HashMap;
use std::io::{self, BufRead, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, watch};

/// How often to poll for new data after an inotify event (ms).
const POLL_INTERVAL_MS: u64 = 200;

/// How long to wait for inotify events before checking shutdown (ms).
const SELECT_TIMEOUT_MS: u64 = 2000;

/// Running state for a watched JSONL file.
struct WatchedFile {
    path: PathBuf,
    /// Byte offset of last read position.
    byte_offset: u64,
    /// Running session summary.
    summary: SessionSummary,
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

    let path_clone = path.clone();
    tokio::spawn(async move {
        if let Err(e) = run_watcher(path_clone, snapshot, shutdown).await {
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

    let path_clone = path.clone();
    tokio::spawn(async move {
        if let Err(e) = run_watcher(path_clone, snapshot, shutdown).await {
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
                if let Ok(event) = res {
                    if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                        let _ = tx.try_send(());
                    }
                }
            },
            Config::default(),
        )
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // Watch the parent directory (the file might not exist at the exact path yet
        // if we're racing with Claude Code's first write)
        let watch_path = path
            .parent()
            .unwrap_or(&path);
        watcher
            .watch(watch_path, notify::RecursiveMode::NonRecursive)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

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

                if current_len > watched.byte_offset {
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

/// Read new lines from the JSONL file starting at the last known byte offset.
async fn read_new_entries(watched: &mut WatchedFile) -> io::Result<()> {
    let path = watched.path.clone();
    let offset = watched.byte_offset;

    // File I/O is blocking, move to blocking thread
    let (new_lines, new_offset) = tokio::task::spawn_blocking(move || -> io::Result<(Vec<String>, u64)> {
        let mut file = std::fs::File::open(&path)?;
        let file_len = file.metadata()?.len();

        if file_len < offset {
            // File was truncated (shouldn't happen normally, but handle gracefully)
            return Ok((Vec::new(), 0));
        }
        if file_len == offset {
            return Ok((Vec::new(), offset));
        }

        file.seek(SeekFrom::Start(offset))?;
        let reader = io::BufReader::new(&file);
        let mut lines = Vec::new();
        let mut bytes_read: u64 = 0;

        for line_result in reader.lines() {
            match line_result {
                Ok(line) => {
                    // +1 for the newline character
                    bytes_read += line.len() as u64 + 1;
                    if !line.trim().is_empty() {
                        lines.push(line);
                    }
                }
                Err(_) => break,
            }
        }

        Ok((lines, offset + bytes_read))
    })
    .await
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))??;

    // Parse new lines incrementally
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

    // Merge turns
    summary.turns.extend(one_line_summary.turns);

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
            format!(r#"[{{"type":"tool_use","name":"{}","id":"t1","input":{{}}}}]"#, name)
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
