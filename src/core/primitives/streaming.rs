//! SSE (Server-Sent Events) streaming utilities
//!
//! Provides a reusable SSE decoder for processing streaming API responses.
//! Used by both OpenAI and DeepSeek providers.

use anyhow::Result;
use serde::de::DeserializeOwned;

// ============================================================================
// SSE Decoder
// ============================================================================

/// SSE stream decoder with buffering
///
/// Handles partial chunks and extracts complete SSE frames.
/// Buffer is bounded to prevent unbounded memory growth.
///
/// # Example
/// ```ignore
/// let mut decoder = SseDecoder::new();
///
/// while let Some(chunk) = stream.next().await {
///     for frame in decoder.push(&chunk?) {
///         if frame.is_done() { break; }
///         let data: MyType = frame.parse()?;
///         // process data
///     }
/// }
/// ```
#[derive(Debug, Default)]
pub struct SseDecoder {
    buffer: String,
}

impl SseDecoder {
    /// Maximum buffer size (1MB) - prevents unbounded growth from malformed streams
    const MAX_BUFFER_SIZE: usize = 1024 * 1024;

    /// Create a new SSE decoder
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    /// Push a chunk of bytes and extract complete SSE frames
    ///
    /// Returns a vector of complete frames. Incomplete data is buffered
    /// for the next push. Buffer is bounded to MAX_BUFFER_SIZE.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseFrame> {
        // Append chunk to buffer (lossy UTF-8 conversion for robustness)
        self.buffer.push_str(&String::from_utf8_lossy(chunk));

        // Safety: prevent unbounded buffer growth from malformed streams
        if self.buffer.len() > Self::MAX_BUFFER_SIZE {
            tracing::warn!(
                "SSE buffer exceeded {}KB limit, truncating",
                Self::MAX_BUFFER_SIZE / 1024
            );
            // Keep only the last portion that might contain a complete frame
            let keep_from = self.buffer.len() - (Self::MAX_BUFFER_SIZE / 2);
            self.buffer = self.buffer[keep_from..].to_string();
        }

        let mut frames = Vec::new();

        // Process complete lines
        while let Some(pos) = self.buffer.find('\n') {
            let line = self.buffer[..pos].trim().to_string();
            self.buffer = self.buffer[pos + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            // Parse SSE data line
            if let Some(data) = line.strip_prefix("data: ") {
                frames.push(SseFrame {
                    data: data.to_string(),
                });
            }
            // Could also handle event:, id:, retry: if needed
        }

        frames
    }

    /// Push a string directly (for testing or pre-decoded content)
    pub fn push_str(&mut self, s: &str) -> Vec<SseFrame> {
        self.push(s.as_bytes())
    }

    /// Clear the internal buffer
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// Check if there's remaining buffered data
    pub fn has_remaining(&self) -> bool {
        !self.buffer.is_empty()
    }
}

// ============================================================================
// SSE Frame
// ============================================================================

/// A complete SSE frame (data line)
#[derive(Debug, Clone)]
pub struct SseFrame {
    /// The data content (without "data: " prefix)
    pub data: String,
}

impl SseFrame {
    /// Check if this is the [DONE] sentinel
    pub fn is_done(&self) -> bool {
        self.data == "[DONE]"
    }

    /// Parse the frame data as JSON
    pub fn parse<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_str(&self.data)
            .map_err(|e| anyhow::anyhow!("SSE JSON parse error: {}. Data: {}", e, self.preview()))
    }

    /// Try to parse the frame data as JSON, returning None on failure
    pub fn try_parse<T: DeserializeOwned>(&self) -> Option<T> {
        serde_json::from_str(&self.data).ok()
    }

    /// Get a preview of the data (first 200 chars) for error messages
    pub fn preview(&self) -> String {
        if self.data.len() > 200 {
            format!("{}...", &self.data[..200])
        } else {
            self.data.clone()
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[test]
    fn test_basic_decode() {
        let mut decoder = SseDecoder::new();

        let frames = decoder.push_str("data: {\"text\": \"hello\"}\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "{\"text\": \"hello\"}");
    }

    #[test]
    fn test_done_frame() {
        let mut decoder = SseDecoder::new();

        let frames = decoder.push_str("data: [DONE]\n");
        assert_eq!(frames.len(), 1);
        assert!(frames[0].is_done());
    }

    #[test]
    fn test_partial_chunks() {
        let mut decoder = SseDecoder::new();

        // First chunk: incomplete line
        let frames1 = decoder.push_str("data: {\"part\":");
        assert!(frames1.is_empty());
        assert!(decoder.has_remaining());

        // Second chunk: completes the line
        let frames2 = decoder.push_str(" 1}\n");
        assert_eq!(frames2.len(), 1);
        assert_eq!(frames2[0].data, "{\"part\": 1}");
    }

    #[test]
    fn test_multiple_frames() {
        let mut decoder = SseDecoder::new();

        let frames = decoder.push_str("data: first\ndata: second\ndata: third\n");
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].data, "first");
        assert_eq!(frames[1].data, "second");
        assert_eq!(frames[2].data, "third");
    }

    #[test]
    fn test_empty_lines_ignored() {
        let mut decoder = SseDecoder::new();

        let frames = decoder.push_str("\n\ndata: content\n\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "content");
    }

    #[test]
    fn test_parse_json() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct TestData {
            value: i32,
        }

        let mut decoder = SseDecoder::new();
        let frames = decoder.push_str("data: {\"value\": 42}\n");

        let parsed: TestData = frames[0].parse().unwrap();
        assert_eq!(parsed.value, 42);
    }

    #[test]
    fn test_try_parse_invalid() {
        let mut decoder = SseDecoder::new();
        let frames = decoder.push_str("data: not-json\n");

        let result: Option<serde_json::Value> = frames[0].try_parse();
        assert!(result.is_none());
    }
}
