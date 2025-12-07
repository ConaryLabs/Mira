// src/testing/mock_llm/matcher.rs
// Request matching strategies for replay

use serde_json::Value;
use tracing::{debug, warn};

use crate::llm::provider::Message;
use super::recording::{compute_request_hash, RecordedExchange, Recording};

/// Strategy for matching requests to recorded exchanges
#[derive(Debug, Clone, Default)]
pub enum MatchStrategy {
    /// Exact hash match (default) - requires identical request
    #[default]
    ExactHash,

    /// Match by last user message content only
    LastUserMessage,

    /// Fuzzy match - find closest based on similarity
    Fuzzy {
        /// Minimum similarity threshold (0.0 - 1.0)
        threshold: f64,
    },

    /// Sequential - return exchanges in order of recording
    Sequential,
}

/// Request matcher that finds appropriate recorded responses
pub struct RequestMatcher {
    recordings: Vec<Recording>,
    strategy: MatchStrategy,

    /// For sequential matching - tracks current position
    sequential_index: usize,

    /// All exchanges flattened for easier searching
    all_exchanges: Vec<RecordedExchange>,
}

impl RequestMatcher {
    /// Create a new matcher from recordings
    pub fn new(recordings: Vec<Recording>, strategy: MatchStrategy) -> Self {
        let all_exchanges: Vec<RecordedExchange> = recordings
            .iter()
            .flat_map(|r| r.exchanges.clone())
            .collect();

        Self {
            recordings,
            strategy,
            sequential_index: 0,
            all_exchanges,
        }
    }

    /// Create a matcher from a single recording
    pub fn from_recording(recording: Recording, strategy: MatchStrategy) -> Self {
        Self::new(vec![recording], strategy)
    }

    /// Find a matching exchange for the given request
    pub fn find_match(
        &mut self,
        messages: &[Message],
        system: &str,
        tools: &[Value],
    ) -> Option<&RecordedExchange> {
        match &self.strategy {
            MatchStrategy::ExactHash => self.match_exact_hash(messages, system, tools),
            MatchStrategy::LastUserMessage => self.match_last_user_message(messages),
            MatchStrategy::Fuzzy { threshold } => self.match_fuzzy(messages, *threshold),
            MatchStrategy::Sequential => self.match_sequential(),
        }
    }

    /// Exact hash matching
    fn match_exact_hash(
        &self,
        messages: &[Message],
        system: &str,
        tools: &[Value],
    ) -> Option<&RecordedExchange> {
        let hash = compute_request_hash(messages, system, tools);
        debug!("[MockMatcher] Looking for exact hash: {}", hash);

        self.all_exchanges
            .iter()
            .find(|e| e.request_hash == hash)
    }

    /// Match based on last user message content
    fn match_last_user_message(&self, messages: &[Message]) -> Option<&RecordedExchange> {
        let last_user = messages
            .iter()
            .rev()
            .find(|m| m.role == "user")?;

        debug!("[MockMatcher] Matching last user message: {}",
               &last_user.content[..last_user.content.len().min(50)]);

        // Find first exchange where last user message matches
        self.all_exchanges.iter().find(|e| {
            e.messages
                .iter()
                .rev()
                .find(|m| m.role == "user")
                .map(|m| m.content == last_user.content)
                .unwrap_or(false)
        })
    }

    /// Fuzzy matching based on content similarity
    fn match_fuzzy(&self, messages: &[Message], threshold: f64) -> Option<&RecordedExchange> {
        let last_user = messages
            .iter()
            .rev()
            .find(|m| m.role == "user")?;

        let query = &last_user.content;
        debug!("[MockMatcher] Fuzzy matching query: {}",
               &query[..query.len().min(50)]);

        // Find best match above threshold
        let mut best_match: Option<(&RecordedExchange, f64)> = None;

        for exchange in &self.all_exchanges {
            if let Some(recorded_user) = exchange.messages.iter().rev().find(|m| m.role == "user") {
                let similarity = string_similarity(query, &recorded_user.content);

                if similarity >= threshold {
                    if best_match.map(|(_, s)| similarity > s).unwrap_or(true) {
                        best_match = Some((exchange, similarity));
                    }
                }
            }
        }

        if let Some((exchange, similarity)) = best_match {
            debug!("[MockMatcher] Found fuzzy match with similarity: {:.2}", similarity);
            Some(exchange)
        } else {
            warn!("[MockMatcher] No fuzzy match found above threshold {}", threshold);
            None
        }
    }

    /// Sequential matching - returns exchanges in order
    fn match_sequential(&mut self) -> Option<&RecordedExchange> {
        if self.sequential_index < self.all_exchanges.len() {
            let exchange = &self.all_exchanges[self.sequential_index];
            self.sequential_index += 1;
            debug!("[MockMatcher] Sequential match #{}", self.sequential_index);
            Some(exchange)
        } else {
            warn!("[MockMatcher] No more sequential exchanges available");
            None
        }
    }

    /// Reset sequential index
    pub fn reset_sequential(&mut self) {
        self.sequential_index = 0;
    }

    /// Get total number of recorded exchanges
    pub fn exchange_count(&self) -> usize {
        self.all_exchanges.len()
    }
}

/// Simple string similarity using Jaccard coefficient on words
fn string_similarity(a: &str, b: &str) -> f64 {
    let words_a: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let words_b: std::collections::HashSet<&str> = b.split_whitespace().collect();

    if words_a.is_empty() && words_b.is_empty() {
        return 1.0;
    }

    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();

    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::mock_llm::recording::{RecordedResponse, Recording, RecordingMetadata};
    use crate::llm::provider::TokenUsage;

    fn make_exchange(user_content: &str, response: &str) -> RecordedExchange {
        let messages = vec![Message::user(user_content.to_string())];
        let hash = compute_request_hash(&messages, "system", &[]);

        RecordedExchange {
            request_hash: hash,
            messages,
            system_prompt: "system".to_string(),
            tools: vec![],
            response: RecordedResponse {
                text: response.to_string(),
                function_calls: vec![],
                tokens: TokenUsage { input: 10, output: 5, reasoning: 0, cached: 0 },
                latency_ms: 100,
            },
            metadata: RecordingMetadata::default(),
        }
    }

    #[test]
    fn test_exact_hash_matching() {
        let mut recording = Recording::new("test");
        recording.add_exchange(make_exchange("Hello world", "Hi there!"));

        let mut matcher = RequestMatcher::from_recording(recording, MatchStrategy::ExactHash);

        // Should match
        let messages = vec![Message::user("Hello world".to_string())];
        let result = matcher.find_match(&messages, "system", &[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().response.text, "Hi there!");

        // Should not match
        let messages = vec![Message::user("Hello world!".to_string())];
        let result = matcher.find_match(&messages, "system", &[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_last_user_message_matching() {
        let mut recording = Recording::new("test");
        recording.add_exchange(make_exchange("What is 2+2?", "4"));

        let mut matcher = RequestMatcher::from_recording(recording, MatchStrategy::LastUserMessage);

        // Should match regardless of other messages
        let messages = vec![
            Message::user("Some earlier message".to_string()),
            Message::assistant("Earlier response".to_string()),
            Message::user("What is 2+2?".to_string()),
        ];
        let result = matcher.find_match(&messages, "different system", &[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().response.text, "4");
    }

    #[test]
    fn test_sequential_matching() {
        let mut recording = Recording::new("test");
        recording.add_exchange(make_exchange("First", "Response 1"));
        recording.add_exchange(make_exchange("Second", "Response 2"));
        recording.add_exchange(make_exchange("Third", "Response 3"));

        let mut matcher = RequestMatcher::from_recording(recording, MatchStrategy::Sequential);

        // Any request should return exchanges in order
        let messages = vec![Message::user("Anything".to_string())];

        assert_eq!(matcher.find_match(&messages, "", &[]).unwrap().response.text, "Response 1");
        assert_eq!(matcher.find_match(&messages, "", &[]).unwrap().response.text, "Response 2");
        assert_eq!(matcher.find_match(&messages, "", &[]).unwrap().response.text, "Response 3");
        assert!(matcher.find_match(&messages, "", &[]).is_none());

        // Reset should start over
        matcher.reset_sequential();
        assert_eq!(matcher.find_match(&messages, "", &[]).unwrap().response.text, "Response 1");
    }

    #[test]
    fn test_fuzzy_matching() {
        let mut recording = Recording::new("test");
        recording.add_exchange(make_exchange("Create a file called hello.txt", "Done!"));

        let mut matcher = RequestMatcher::from_recording(
            recording,
            MatchStrategy::Fuzzy { threshold: 0.5 }
        );

        // Similar request should match
        let messages = vec![Message::user("Create a file named hello.txt please".to_string())];
        let result = matcher.find_match(&messages, "", &[]);
        assert!(result.is_some());
    }

    #[test]
    fn test_string_similarity() {
        assert!(string_similarity("hello world", "hello world") > 0.99);
        assert!(string_similarity("hello world", "hello there world") > 0.5);
        assert!(string_similarity("hello world", "goodbye universe") < 0.1);
    }
}
