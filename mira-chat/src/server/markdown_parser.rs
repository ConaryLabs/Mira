//! Markdown Stream Parser
//!
//! Parses streaming text deltas to detect code blocks and emit typed events.
//! Handles edge cases like backticks split across chunks.

use super::types::ChatEvent;

/// State machine for parsing markdown code blocks from streaming text
#[derive(Debug)]
pub struct MarkdownStreamParser {
    /// Current parser state
    state: ParserState,
    /// Buffer for incomplete tokens (backticks, etc.)
    buffer: String,
    /// Current code block ID (when in code block)
    code_block_id: Option<String>,
    /// Current code block language
    code_block_language: String,
    /// Counter for generating unique code block IDs
    id_counter: u32,
    /// Track if we're at the start of a line (for fence detection)
    at_line_start: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum ParserState {
    /// Normal text mode
    Normal,
    /// Seen one or more backticks, might be fence start
    MaybeFence { backtick_count: usize },
    /// Inside a code block, collecting content
    InCodeBlock,
    /// In code block, seen backticks that might be closing fence
    MaybeClosingFence { backtick_count: usize },
    /// Just started code block, collecting language identifier
    CollectingLanguage,
}

impl Default for MarkdownStreamParser {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkdownStreamParser {
    pub fn new() -> Self {
        Self {
            state: ParserState::Normal,
            buffer: String::new(),
            code_block_id: None,
            code_block_language: String::new(),
            id_counter: 0,
            at_line_start: true,
        }
    }

    /// Generate a unique code block ID
    fn next_id(&mut self) -> String {
        self.id_counter += 1;
        format!("cb_{}", self.id_counter)
    }

    /// Feed a text delta and get back typed events
    pub fn feed(&mut self, delta: &str) -> Vec<ChatEvent> {
        let mut events = Vec::new();

        for ch in delta.chars() {
            let new_events = self.process_char(ch);
            events.extend(new_events);
        }

        events
    }

    /// Process a single character
    fn process_char(&mut self, ch: char) -> Vec<ChatEvent> {
        match self.state.clone() {
            ParserState::Normal => self.handle_normal(ch),
            ParserState::MaybeFence { backtick_count } => {
                self.handle_maybe_fence(ch, backtick_count)
            }
            ParserState::CollectingLanguage => self.handle_collecting_language(ch),
            ParserState::InCodeBlock => self.handle_in_code_block(ch),
            ParserState::MaybeClosingFence { backtick_count } => {
                self.handle_maybe_closing_fence(ch, backtick_count)
            }
        }
    }

    fn handle_normal(&mut self, ch: char) -> Vec<ChatEvent> {
        if ch == '`' && self.at_line_start {
            // Start of potential fence
            self.buffer.push(ch);
            self.state = ParserState::MaybeFence { backtick_count: 1 };
            vec![]
        } else {
            // Regular text
            self.at_line_start = ch == '\n';
            vec![ChatEvent::TextDelta {
                delta: ch.to_string(),
            }]
        }
    }

    fn handle_maybe_fence(&mut self, ch: char, backtick_count: usize) -> Vec<ChatEvent> {
        if ch == '`' {
            self.buffer.push(ch);
            if backtick_count + 1 >= 3 {
                // We have ``` - this is a code fence!
                // Clear buffer and start collecting language
                self.buffer.clear();
                self.code_block_language.clear();
                self.state = ParserState::CollectingLanguage;
            } else {
                self.state = ParserState::MaybeFence {
                    backtick_count: backtick_count + 1,
                };
            }
            vec![]
        } else {
            // Not a fence, flush buffer as text
            let mut text = std::mem::take(&mut self.buffer);
            text.push(ch);
            self.at_line_start = ch == '\n';
            self.state = ParserState::Normal;
            vec![ChatEvent::TextDelta { delta: text }]
        }
    }

    fn handle_collecting_language(&mut self, ch: char) -> Vec<ChatEvent> {
        if ch == '\n' {
            // Language line complete, start code block
            let id = self.next_id();
            let language = self.code_block_language.trim().to_string();
            // Take first word as language (handles "```rust title=foo")
            let language = language.split_whitespace().next().unwrap_or("").to_string();

            self.code_block_id = Some(id.clone());
            self.state = ParserState::InCodeBlock;
            self.at_line_start = true;

            vec![ChatEvent::CodeBlockStart {
                id,
                language,
                filename: None,
            }]
        } else {
            // Accumulate language identifier
            self.code_block_language.push(ch);
            vec![]
        }
    }

    fn handle_in_code_block(&mut self, ch: char) -> Vec<ChatEvent> {
        if ch == '`' && self.at_line_start {
            // Potential closing fence
            self.buffer.push(ch);
            self.state = ParserState::MaybeClosingFence { backtick_count: 1 };
            vec![]
        } else {
            // Code content
            self.at_line_start = ch == '\n';
            if let Some(id) = &self.code_block_id {
                vec![ChatEvent::CodeBlockDelta {
                    id: id.clone(),
                    delta: ch.to_string(),
                }]
            } else {
                vec![]
            }
        }
    }

    fn handle_maybe_closing_fence(&mut self, ch: char, backtick_count: usize) -> Vec<ChatEvent> {
        if ch == '`' {
            self.buffer.push(ch);
            if backtick_count + 1 >= 3 {
                // Closing fence complete!
                self.buffer.clear();
                let id = self.code_block_id.take().unwrap_or_default();
                self.state = ParserState::Normal;
                self.at_line_start = false;
                vec![ChatEvent::CodeBlockEnd { id }]
            } else {
                self.state = ParserState::MaybeClosingFence {
                    backtick_count: backtick_count + 1,
                };
                vec![]
            }
        } else if ch == '\n' && backtick_count >= 3 {
            // Closing fence with newline after
            self.buffer.clear();
            let id = self.code_block_id.take().unwrap_or_default();
            self.state = ParserState::Normal;
            self.at_line_start = true;
            vec![ChatEvent::CodeBlockEnd { id }]
        } else {
            // Not a closing fence, emit buffered backticks as code
            let buffered = std::mem::take(&mut self.buffer);
            self.at_line_start = ch == '\n';
            self.state = ParserState::InCodeBlock;

            if let Some(id) = &self.code_block_id {
                let mut delta = buffered;
                delta.push(ch);
                vec![ChatEvent::CodeBlockDelta {
                    id: id.clone(),
                    delta,
                }]
            } else {
                vec![]
            }
        }
    }

    /// Flush any remaining state (call on stream end)
    pub fn flush(&mut self) -> Vec<ChatEvent> {
        let mut events = Vec::new();

        // If we have buffered content, emit it
        if !self.buffer.is_empty() {
            let buffered = std::mem::take(&mut self.buffer);
            match &self.state {
                ParserState::InCodeBlock | ParserState::MaybeClosingFence { .. } => {
                    if let Some(id) = &self.code_block_id {
                        events.push(ChatEvent::CodeBlockDelta {
                            id: id.clone(),
                            delta: buffered,
                        });
                    }
                }
                _ => {
                    events.push(ChatEvent::TextDelta { delta: buffered });
                }
            }
        }

        // If in code block, close it
        if let Some(id) = self.code_block_id.take() {
            events.push(ChatEvent::CodeBlockEnd { id });
        }

        self.state = ParserState::Normal;
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text() {
        let mut parser = MarkdownStreamParser::new();
        let events = parser.feed("Hello world");
        assert_eq!(events.len(), 11); // One event per char
        assert!(matches!(&events[0], ChatEvent::TextDelta { delta } if delta == "H"));
    }

    #[test]
    fn test_code_block() {
        let mut parser = MarkdownStreamParser::new();
        let events = parser.feed("```rust\nfn main() {}\n```");

        // Should have: CodeBlockStart, deltas for code, CodeBlockEnd
        let starts: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, ChatEvent::CodeBlockStart { .. }))
            .collect();
        let ends: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, ChatEvent::CodeBlockEnd { .. }))
            .collect();

        assert_eq!(starts.len(), 1);
        assert_eq!(ends.len(), 1);

        if let ChatEvent::CodeBlockStart { language, .. } = &starts[0] {
            assert_eq!(language, "rust");
        }
    }

    #[test]
    fn test_split_backticks() {
        let mut parser = MarkdownStreamParser::new();

        // Feed backticks one at a time
        let mut all_events = Vec::new();
        all_events.extend(parser.feed("`"));
        all_events.extend(parser.feed("`"));
        all_events.extend(parser.feed("`"));
        all_events.extend(parser.feed("js\n"));
        all_events.extend(parser.feed("code\n"));
        all_events.extend(parser.feed("```"));

        let starts: Vec<_> = all_events
            .iter()
            .filter(|e| matches!(e, ChatEvent::CodeBlockStart { .. }))
            .collect();
        let ends: Vec<_> = all_events
            .iter()
            .filter(|e| matches!(e, ChatEvent::CodeBlockEnd { .. }))
            .collect();

        assert_eq!(starts.len(), 1);
        assert_eq!(ends.len(), 1);
    }

    #[test]
    fn test_unclosed_block_flush() {
        let mut parser = MarkdownStreamParser::new();
        let mut events = parser.feed("```python\nprint('hello')");
        events.extend(parser.flush());

        let ends: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, ChatEvent::CodeBlockEnd { .. }))
            .collect();

        assert_eq!(ends.len(), 1, "Unclosed block should be closed on flush");
    }

    #[test]
    fn test_inline_backticks_not_fence() {
        let mut parser = MarkdownStreamParser::new();
        // Backticks not at line start should be text
        let events = parser.feed("use `code` here");

        // Should all be text deltas
        assert!(events.iter().all(|e| matches!(e, ChatEvent::TextDelta { .. })));
    }
}
