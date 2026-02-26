// crates/mira-server/src/jsonl/mod.rs
// JSONL session log parser for Claude Code session files.

pub mod calibration;
pub mod correlation;
pub mod parser;
pub mod watcher;

pub use correlation::CorrelatedSession;
pub use parser::{
    EntryType, SessionSummary, TokenUsage, ToolCallSummary, TurnSummary, parse_session_entries,
    parse_session_file,
};
pub use watcher::{
    SessionSnapshot, SessionWatcherHandle, find_session_jsonl, spawn_watcher,
    spawn_watcher_for_path,
};
