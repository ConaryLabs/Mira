// crates/mira-server/src/jsonl/parser.rs
// Parses Claude Code JSONL session logs into structured summaries.
//
// JSONL format (one JSON object per line):
//   - user:    human prompt (string content) or tool_result (array content)
//   - assistant: thinking, text, or tool_use content blocks; has message.usage
//   - progress: hook status, background work
//   - system:  hook summaries, stop events
//   - queue-operation: queued user messages
//   - file-history-snapshot: file state snapshots
//   - summary: conversation compaction summaries

use std::collections::HashMap;
use std::io::{self, BufRead};
use std::path::Path;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// High-level summary of an entire session JSONL file.
#[derive(Debug, Clone, Default)]
pub struct SessionSummary {
    pub session_id: Option<String>,
    pub version: Option<String>,
    pub git_branch: Option<String>,

    /// All API turns (assistant entries with usage data), in order.
    pub turns: Vec<TurnSummary>,

    /// Aggregated tool call counts across all turns.
    pub tool_calls: HashMap<String, u64>,

    /// Total user prompts (non-tool-result user entries).
    pub user_prompt_count: u64,

    /// Total tool results received.
    pub tool_result_count: u64,

    /// Number of compaction/summary events.
    pub compaction_count: u64,

    /// First and last timestamps seen.
    pub first_timestamp: Option<String>,
    pub last_timestamp: Option<String>,

    /// Lines that failed to parse.
    pub parse_errors: u64,
}

impl SessionSummary {
    /// Sum of all input tokens across turns (excluding cache).
    pub fn total_input_tokens(&self) -> u64 {
        self.turns.iter().map(|t| t.usage.input_tokens).sum()
    }

    /// Sum of all output tokens across turns.
    pub fn total_output_tokens(&self) -> u64 {
        self.turns.iter().map(|t| t.usage.output_tokens).sum()
    }

    /// Sum of all cache-read tokens across turns.
    pub fn total_cache_read_tokens(&self) -> u64 {
        self.turns.iter().map(|t| t.usage.cache_read_input_tokens).sum()
    }

    /// Sum of all cache-creation tokens across turns.
    pub fn total_cache_creation_tokens(&self) -> u64 {
        self.turns.iter().map(|t| t.usage.cache_creation_input_tokens).sum()
    }

    /// Total billable input = input + cache_creation + cache_read.
    pub fn total_billable_input(&self) -> u64 {
        self.total_input_tokens() + self.total_cache_creation_tokens() + self.total_cache_read_tokens()
    }

    /// Total tool calls across all turns.
    pub fn total_tool_calls(&self) -> u64 {
        self.tool_calls.values().sum()
    }

    /// Number of API turns.
    pub fn turn_count(&self) -> usize {
        self.turns.len()
    }
}

/// A single API turn (one assistant response with usage data).
#[derive(Debug, Clone)]
pub struct TurnSummary {
    pub uuid: Option<String>,
    pub timestamp: Option<String>,
    pub usage: TokenUsage,
    /// Tool calls made in this turn's content.
    pub tool_calls: Vec<String>,
    /// What content types were in this turn (thinking, text, tool_use).
    pub content_types: Vec<String>,
    pub is_sidechain: bool,
}

/// Token usage from a single API call.
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

/// Aggregated tool call info.
#[derive(Debug, Clone)]
pub struct ToolCallSummary {
    pub name: String,
    pub count: u64,
}

/// Entry types we recognize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryType {
    User,
    Assistant,
    Progress,
    System,
    QueueOperation,
    FileHistorySnapshot,
    SavedHookContext,
    Summary,
    Unknown(String),
}

impl EntryType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "user" => Self::User,
            "assistant" => Self::Assistant,
            "progress" => Self::Progress,
            "system" => Self::System,
            "queue-operation" => Self::QueueOperation,
            "file-history-snapshot" => Self::FileHistorySnapshot,
            "saved_hook_context" => Self::SavedHookContext,
            "summary" => Self::Summary,
            other => Self::Unknown(other.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Deserialization types (internal)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawEntry {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    uuid: Option<String>,
    timestamp: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    version: Option<String>,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    message: Option<RawMessage>,
    #[allow(dead_code)]
    subtype: Option<String>,
}

#[derive(Deserialize)]
struct RawMessage {
    #[serde(default)]
    content: serde_json::Value,
    usage: Option<RawUsage>,
}

#[derive(Deserialize)]
struct RawUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: Option<String>,
    name: Option<String>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a JSONL session file from disk.
pub fn parse_session_file(path: &Path) -> io::Result<SessionSummary> {
    let file = std::fs::File::open(path)?;
    let reader = io::BufReader::new(file);
    Ok(parse_from_reader(reader))
}

/// Parse JSONL entries from a string (for testing or piped input).
pub fn parse_session_entries(data: &str) -> SessionSummary {
    let reader = io::BufReader::new(data.as_bytes());
    parse_from_reader(reader)
}

fn parse_from_reader<R: BufRead>(reader: R) -> SessionSummary {
    let mut summary = SessionSummary::default();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => {
                summary.parse_errors += 1;
                continue;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let entry: RawEntry = match serde_json::from_str(trimmed) {
            Ok(e) => e,
            Err(_) => {
                summary.parse_errors += 1;
                continue;
            }
        };

        // Capture session metadata from first entry that has it
        if summary.session_id.is_none() {
            if let Some(ref sid) = entry.session_id {
                summary.session_id = Some(sid.clone());
            }
        }
        if summary.version.is_none() {
            if let Some(ref v) = entry.version {
                summary.version = Some(v.clone());
            }
        }
        if summary.git_branch.is_none() {
            if let Some(ref b) = entry.git_branch {
                summary.git_branch = Some(b.clone());
            }
        }

        // Track timestamps
        if let Some(ref ts) = entry.timestamp {
            if summary.first_timestamp.is_none() {
                summary.first_timestamp = Some(ts.clone());
            }
            summary.last_timestamp = Some(ts.clone());
        }

        let entry_type = entry.entry_type.as_deref().unwrap_or("unknown");

        match EntryType::from_str(entry_type) {
            EntryType::Assistant => {
                process_assistant_entry(&entry, &mut summary);
            }
            EntryType::User => {
                process_user_entry(&entry, &mut summary);
            }
            EntryType::Summary => {
                summary.compaction_count += 1;
            }
            _ => {}
        }
    }

    summary
}

fn process_assistant_entry(entry: &RawEntry, summary: &mut SessionSummary) {
    let message = match &entry.message {
        Some(m) => m,
        None => return,
    };

    // Extract content block types and tool call names
    let mut content_types = Vec::new();
    let mut tool_calls = Vec::new();

    if let Some(blocks) = message.content.as_array() {
        for block in blocks {
            if let Ok(cb) = serde_json::from_value::<ContentBlock>(block.clone()) {
                if let Some(ref bt) = cb.block_type {
                    if !content_types.contains(bt) {
                        content_types.push(bt.clone());
                    }
                    if bt == "tool_use" {
                        if let Some(ref name) = cb.name {
                            tool_calls.push(name.clone());
                            *summary.tool_calls.entry(name.clone()).or_insert(0) += 1;
                        }
                    }
                }
            }
        }
    }

    // Only count as a turn if there's usage data (actual API call)
    if let Some(ref usage) = message.usage {
        let token_usage = TokenUsage {
            input_tokens: usage.input_tokens.unwrap_or(0),
            output_tokens: usage.output_tokens.unwrap_or(0),
            cache_creation_input_tokens: usage.cache_creation_input_tokens.unwrap_or(0),
            cache_read_input_tokens: usage.cache_read_input_tokens.unwrap_or(0),
        };

        summary.turns.push(TurnSummary {
            uuid: entry.uuid.clone(),
            timestamp: entry.timestamp.clone(),
            usage: token_usage,
            tool_calls,
            content_types,
            is_sidechain: entry.is_sidechain.unwrap_or(false),
        });
    }
}

fn process_user_entry(entry: &RawEntry, summary: &mut SessionSummary) {
    let message = match &entry.message {
        Some(m) => m,
        None => return,
    };

    // User entries with array content containing tool_result are tool results.
    // A single user message can carry multiple tool_results (parallel tool use).
    // User entries with string content are actual user prompts.
    if let Some(blocks) = message.content.as_array() {
        let tool_result_count = blocks.iter().filter(|b| {
            b.get("type")
                .and_then(|t| t.as_str())
                .is_some_and(|t| t == "tool_result")
        }).count();
        if tool_result_count > 0 {
            summary.tool_result_count += tool_result_count as u64;
        } else {
            summary.user_prompt_count += 1;
        }
    } else {
        // String content = user prompt
        summary.user_prompt_count += 1;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_assistant_entry(input: u64, output: u64, cache_read: u64, cache_create: u64, tool_name: Option<&str>) -> String {
        let content = if let Some(name) = tool_name {
            format!(r#"[{{"type":"tool_use","name":"{}","id":"toolu_test","input":{{}}}}]"#, name)
        } else {
            r#"[{"type":"text","text":"hello"}]"#.to_string()
        };

        format!(
            r#"{{"type":"assistant","uuid":"a1","timestamp":"2026-01-01T00:00:00Z","sessionId":"sess1","version":"2.1.58","isSidechain":false,"message":{{"content":{},"usage":{{"input_tokens":{},"output_tokens":{},"cache_read_input_tokens":{},"cache_creation_input_tokens":{}}}}}}}"#,
            content, input, output, cache_read, cache_create
        )
    }

    fn make_user_prompt(content: &str) -> String {
        format!(
            r#"{{"type":"user","uuid":"u1","timestamp":"2026-01-01T00:00:01Z","sessionId":"sess1","message":{{"role":"user","content":"{}"}}}}"#,
            content
        )
    }

    fn make_tool_result() -> String {
        r#"{"type":"user","uuid":"u2","timestamp":"2026-01-01T00:00:02Z","sessionId":"sess1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_test","content":"ok","is_error":false}]}}"#.to_string()
    }

    #[test]
    fn test_parse_empty() {
        let summary = parse_session_entries("");
        assert_eq!(summary.turn_count(), 0);
        assert_eq!(summary.user_prompt_count, 0);
        assert_eq!(summary.parse_errors, 0);
    }

    #[test]
    fn test_parse_single_assistant_turn() {
        let line = make_assistant_entry(100, 50, 8000, 2000, None);
        let summary = parse_session_entries(&line);

        assert_eq!(summary.turn_count(), 1);
        assert_eq!(summary.total_input_tokens(), 100);
        assert_eq!(summary.total_output_tokens(), 50);
        assert_eq!(summary.total_cache_read_tokens(), 8000);
        assert_eq!(summary.total_cache_creation_tokens(), 2000);
        assert_eq!(summary.total_billable_input(), 10100);
        assert_eq!(summary.session_id, Some("sess1".to_string()));
    }

    #[test]
    fn test_parse_tool_use_tracking() {
        let lines = vec![
            make_assistant_entry(1, 10, 100, 50, Some("Read")),
            make_assistant_entry(1, 20, 200, 60, Some("Bash")),
            make_assistant_entry(1, 15, 150, 55, Some("Read")),
        ];
        let data = lines.join("\n");
        let summary = parse_session_entries(&data);

        assert_eq!(summary.turn_count(), 3);
        assert_eq!(summary.total_tool_calls(), 3);
        assert_eq!(*summary.tool_calls.get("Read").unwrap_or(&0), 2);
        assert_eq!(*summary.tool_calls.get("Bash").unwrap_or(&0), 1);
    }

    #[test]
    fn test_user_prompt_vs_tool_result() {
        let lines = vec![
            make_user_prompt("hello there"),
            make_assistant_entry(1, 10, 100, 50, Some("Read")),
            make_tool_result(),
            make_user_prompt("thanks"),
        ];
        let data = lines.join("\n");
        let summary = parse_session_entries(&data);

        assert_eq!(summary.user_prompt_count, 2);
        assert_eq!(summary.tool_result_count, 1);
    }

    #[test]
    fn test_multi_tool_result_counted_individually() {
        // A single user message carrying two parallel tool results
        let multi_result = r#"{"type":"user","uuid":"u3","timestamp":"2026-01-01T00:00:02Z","sessionId":"sess1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok","is_error":false},{"type":"tool_result","tool_use_id":"t2","content":"ok","is_error":false}]}}"#;
        let summary = parse_session_entries(multi_result);
        assert_eq!(summary.tool_result_count, 2);
        assert_eq!(summary.user_prompt_count, 0);
    }

    #[test]
    fn test_parse_errors_counted() {
        let data = "not json\n{\"type\":\"assistant\"}\n{broken";
        let summary = parse_session_entries(data);
        assert_eq!(summary.parse_errors, 2); // "not json" and "{broken"
    }

    #[test]
    fn test_timestamp_tracking() {
        let lines = vec![
            make_user_prompt("first"),
            make_assistant_entry(1, 10, 100, 50, None),
        ];
        let data = lines.join("\n");
        let summary = parse_session_entries(&data);

        assert_eq!(summary.first_timestamp, Some("2026-01-01T00:00:01Z".to_string()));
        assert_eq!(summary.last_timestamp, Some("2026-01-01T00:00:00Z".to_string()));
    }

    #[test]
    fn test_compaction_count() {
        let data = r#"{"type":"summary","uuid":"s1","timestamp":"2026-01-01T00:00:00Z","message":{"content":"compacted"}}
{"type":"summary","uuid":"s2","timestamp":"2026-01-01T00:01:00Z","message":{"content":"compacted again"}}"#;
        let summary = parse_session_entries(data);
        assert_eq!(summary.compaction_count, 2);
    }

    #[test]
    fn test_sidechain_tracking() {
        let mut line = make_assistant_entry(1, 10, 100, 50, None);
        line = line.replace(r#""isSidechain":false"#, r#""isSidechain":true"#);
        let summary = parse_session_entries(&line);

        assert_eq!(summary.turns.len(), 1);
        assert!(summary.turns[0].is_sidechain);
    }

    #[test]
    fn test_content_types_tracked() {
        let line = make_assistant_entry(1, 10, 100, 50, Some("Edit"));
        let summary = parse_session_entries(&line);

        assert_eq!(summary.turns[0].content_types, vec!["tool_use"]);
    }

    #[test]
    fn test_parse_real_file() {
        // Integration test: parse an actual JSONL file if it exists
        let jsonl_dir = dirs::home_dir()
            .map(|h| h.join(".claude/projects/-home-peter-Mira"));

        if let Some(dir) = jsonl_dir {
            if dir.exists() {
                let mut files: Vec<_> = std::fs::read_dir(&dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|e| e.path().extension().is_some_and(|ext| ext == "jsonl"))
                    .collect();
                files.sort_by_key(|e| std::cmp::Reverse(e.metadata().ok().and_then(|m| m.modified().ok())));

                if let Some(file) = files.first() {
                    let summary = parse_session_file(&file.path()).expect("should parse");
                    // Basic sanity: a real session should have at least one turn
                    assert!(summary.turn_count() > 0, "real session should have turns");
                    assert!(summary.session_id.is_some(), "real session should have session_id");
                    assert!(summary.total_output_tokens() > 0, "should have output tokens");
                }
            }
        }
    }
}
