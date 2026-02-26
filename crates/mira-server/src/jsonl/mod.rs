// crates/mira-server/src/jsonl/mod.rs
// JSONL session log parser for Claude Code session files.

pub mod calibration;
pub mod correlation;
pub mod parser;
pub mod watcher;

pub use parser::{
    parse_session_file, parse_session_entries, SessionSummary, TurnSummary,
    TokenUsage, ToolCallSummary, EntryType,
};
pub use watcher::{
    SessionWatcherHandle, SessionSnapshot, find_session_jsonl,
    spawn_watcher, spawn_watcher_for_path,
};
pub use correlation::CorrelatedSession;
