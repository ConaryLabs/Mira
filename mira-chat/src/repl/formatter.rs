//! Markdown formatter for terminal output with ANSI colors

/// Simple streaming markdown formatter
/// Tracks code block state and applies ANSI colors
pub struct MarkdownFormatter {
    in_code_block: bool,
    pending: String,
}

impl MarkdownFormatter {
    pub fn new() -> Self {
        Self {
            in_code_block: false,
            pending: String::new(),
        }
    }

    /// Process a chunk of text and return formatted output
    pub fn process(&mut self, chunk: &str) -> String {
        // Accumulate chunk with pending content
        self.pending.push_str(chunk);

        let mut output = String::new();
        let mut processed_up_to = 0;

        // Process complete lines and code block markers
        let bytes = self.pending.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            // Only check for code block marker at valid char boundaries
            if self.pending.is_char_boundary(i) && self.pending.get(i..i + 3) == Some("```") {
                // Output everything before the marker
                if i > processed_up_to && self.pending.is_char_boundary(processed_up_to) {
                    output.push_str(&self.format_text(&self.pending[processed_up_to..i]));
                }

                // Toggle code block state
                if self.in_code_block {
                    // End of code block - reset color
                    output.push_str("\x1b[0m```");
                    self.in_code_block = false;
                } else {
                    // Start of code block - dim color
                    output.push_str("```\x1b[2m");
                    self.in_code_block = true;
                }

                // Skip to end of line for language specifier
                let mut j = i + 3;
                while j < bytes.len() && bytes[j] != b'\n' {
                    j += 1;
                }
                if j < bytes.len()
                    && self.pending.is_char_boundary(i + 3)
                    && self.pending.is_char_boundary(j + 1)
                {
                    output.push_str(&self.pending[i + 3..=j]);
                    processed_up_to = j + 1;
                    i = j + 1;
                } else {
                    // No newline yet or invalid boundary, keep pending
                    processed_up_to = i + 3;
                    i = if j < bytes.len() { j + 1 } else { j };
                }
                continue;
            }

            i += 1;
        }

        // Output remaining processed content
        if processed_up_to < self.pending.len() && self.pending.is_char_boundary(processed_up_to) {
            // Check if we might have an incomplete ``` at the end
            let remaining = &self.pending[processed_up_to..];
            let trailing = remaining.len().min(2);

            // Find a valid char boundary for safe_len
            let mut safe_len = remaining.len().saturating_sub(trailing);
            while safe_len > 0 && !remaining.is_char_boundary(safe_len) {
                safe_len -= 1;
            }

            if safe_len > 0 {
                output.push_str(&self.format_text(&remaining[..safe_len]));
                self.pending = remaining[safe_len..].to_string();
            } else {
                self.pending = remaining.to_string();
            }
        } else if processed_up_to >= self.pending.len() {
            self.pending.clear();
        }

        output
    }

    /// Format text with inline styles (bold, italic)
    fn format_text(&self, text: &str) -> String {
        if self.in_code_block {
            // Inside code block, no inline formatting
            return text.to_string();
        }
        text.to_string()
    }

    /// Flush any remaining pending content
    pub fn flush(&mut self) -> String {
        if self.pending.is_empty() {
            return String::new();
        }

        let output = self.format_text(&self.pending);
        self.pending.clear();

        // Reset colors if we were in a code block
        if self.in_code_block {
            self.in_code_block = false;
            format!("{}\x1b[0m", output)
        } else {
            output
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_formatter_plain_text() {
        let mut fmt = MarkdownFormatter::new();
        let out = fmt.process("Hello world");
        // Most text is pending until we're sure there's no ```
        let flush = fmt.flush();
        assert!(out.contains("Hello") || flush.contains("Hello"));
    }

    #[test]
    fn test_markdown_formatter_code_block() {
        let mut fmt = MarkdownFormatter::new();

        // Start code block
        let out1 = fmt.process("```rust\n");
        assert!(out1.contains("```"));
        assert!(out1.contains("\x1b[2m")); // dim color

        // Code content
        let out2 = fmt.process("fn main() {}\n");

        // End code block
        let out3 = fmt.process("```\n");
        assert!(out3.contains("\x1b[0m")); // reset color

        let flush = fmt.flush();
        // Combined output should have code
        let all = format!("{}{}{}{}", out1, out2, out3, flush);
        assert!(all.contains("fn main"));
    }

    #[test]
    fn test_markdown_formatter_flush() {
        let mut fmt = MarkdownFormatter::new();
        let out = fmt.process("partial text here");
        let flush = fmt.flush();
        // Combined output should have the full text
        let all = format!("{}{}", out, flush);
        assert!(all.contains("partial"));
    }
}
