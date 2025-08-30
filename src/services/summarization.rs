// src/services/summarization.rs
use std::sync::Arc;
use anyhow::{anyhow, Result};
use tracing::{debug, info};

use crate::llm::client::OpenAIClient;
use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::traits::MemoryStore;
use crate::memory::types::MemoryEntry;
use crate::services::chat::{ChatConfig, ChatResponse};
use crate::services::memory::MemoryService;
use crate::config::CONFIG;

pub struct SummarizationService {
    openai_client: Arc<OpenAIClient>,
    config: Arc<ChatConfig>,
    sqlite_store: Option<Arc<SqliteMemoryStore>>,
    memory_service: Option<Arc<MemoryService>>,
}

impl SummarizationService {
    /// Create a summarizer for direct summarization (text-only)
    pub fn new(openai_client: Arc<OpenAIClient>, config: Arc<ChatConfig>) -> Self {
        // Placeholder: we don't have stores here, so summarize_if_needed() is a no-op.
        Self {
            openai_client,
            config,
            sqlite_store: None,
            memory_service: None,
        }
    }

    /// Create a summarizer that can actually read & write memory (used for ChatService).
    pub fn new_with_stores(
        openai_client: Arc<OpenAIClient>,
        config: Arc<ChatConfig>,
        sqlite_store: Arc<SqliteMemoryStore>,
        memory_service: Arc<MemoryService>,
    ) -> Self {
        Self {
            openai_client,
            config,
            sqlite_store: Some(sqlite_store),
            memory_service: Some(memory_service),
        }
    }

    /// Check recent conversation length & trigger legacy summarization if needed.
    /// This is the backward-compatible path when rolling summaries are disabled.
    pub async fn summarize_if_needed(&self, session_id: &str) -> Result<()> {
        // Skip legacy summarization if rolling summaries are enabled
        if CONFIG.is_robust_memory_enabled() && 
           (CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100) {
            debug!("Skipping legacy summarization - rolling summaries are enabled");
            return Ok(());
        }

        // We only summarize if we have real stores; if not, return early.
        if let Some(ref sqlite_store) = self.sqlite_store {
            let cap = self.config.history_message_cap().max(8);
            let recent = sqlite_store.load_recent(session_id, cap).await?;
            if recent.len() < cap / 2 {
                // Too few messages to bother summarizing.
                return Ok(());
            }
            self.summarize_recent(session_id).await
        } else {
            // No stores available, skip summarization
            Ok(())
        }
    }

    /// Actually summarize recent messages and persist the summary (legacy method).
    pub async fn summarize_recent(&self, session_id: &str) -> Result<()> {
        let sqlite = self.sqlite_store
            .as_ref()
            .ok_or_else(|| anyhow!("No SQLite store available for summarization"))?;
        let mem = self.memory_service
            .as_ref()
            .ok_or_else(|| anyhow!("No memory service available for summarization"))?;

        // 1) Load recent messages (more than config cap to ensure we catch the tail).
        let cap = self.config.history_message_cap().max(8);
        let take = cap.saturating_mul(2);
        let recent = sqlite.load_recent(session_id, take).await?;
        if recent.is_empty() {
            return Ok(());
        }

        // 2) Build a clean prompt for summarization (order preserved).
        let mut prompt = String::from(
            "Summarize the following recent conversation for fast recall later.\n\
             Keep it faithful, concise, and useful for context stitching.\n\n",
        );
        for msg in &recent {
            // Expecting each entry to have role + content.
            prompt.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        // 3) Ask the model for a compact summary within our configured token budget.
        let token_limit = self.config.max_output_tokens().min(1024);
        let summary = self
            .openai_client
            .summarize_conversation(&prompt, token_limit)
            .await?
            .trim()
            .to_string();

        if summary.is_empty() {
            // Don't save an empty summary.
            return Ok(());
        }

        // 4) Persist as an assistant response so it's retrievable like everything else.
        let response = ChatResponse {
            output: String::new(),            // not user-facing body; summary lives below
            persona: "mira".to_string(),
            mood: "neutral".to_string(),
            salience: 2,                      // low default; tune if desired
            summary,                          // the compacted conversation summary
            memory_type: "context".to_string(),
            tags: vec!["summary".to_string()],
            intent: Some("summarize".to_string()),
            monologue: None,
            reasoning_summary: None,
        };

        mem.save_assistant_response(session_id, &response).await?;
        Ok(())
    }

    /// ── Phase 4: Summarize the last N messages (excluding summaries) ──
    /// This is the key new method for rolling summaries.
    pub async fn summarize_last_n(&self, session_id: &str, n: usize) -> Result<String> {
        let sqlite = self.sqlite_store
            .as_ref()
            .ok_or_else(|| anyhow!("No SQLite store available for summarization"))?;

        // 1) Load more messages than needed to account for filtering out summaries
        let fetch_count = n.saturating_mul(3); // Fetch 3x to account for summary messages
        let all_recent = sqlite.load_recent(session_id, fetch_count).await?;
        
        // 2) Filter out existing summary messages to avoid summarizing summaries
        let non_summary_messages: Vec<_> = all_recent
            .into_iter()
            .filter(|msg| {
                !msg.tags
                    .as_ref()
                    .map(|tags| tags.iter().any(|tag| tag.contains("summary")))
                    .unwrap_or(false)
            })
            .take(n)
            .collect();

        if non_summary_messages.len() < std::cmp::min(n, 3) {
            return Err(anyhow!("Not enough non-summary messages to summarize (found {}, need at least {})", 
                non_summary_messages.len(), std::cmp::min(n, 3)));
        }

        // 3) Build an appropriate prompt based on the window size
        let mut prompt = if n <= 10 {
            format!(
                "Summarize the following last {} exchanges briefly to maintain context.\n\
                 Focus on key information, decisions, and ongoing topics.\n\
                 Keep it faithful, concise, and useful for context stitching.\n\n",
                non_summary_messages.len()
            )
        } else if n <= 50 {
            format!(
                "Summarize the following {} messages, capturing the main topics and themes.\n\
                 Focus on important information, key decisions, and significant developments.\n\
                 Keep it organized and concise for efficient recall.\n\n",
                non_summary_messages.len()
            )
        } else {
            format!(
                "Provide a high-level summary of the following {} messages.\n\
                 Focus on major themes, important conclusions, and overall progression.\n\
                 Organize by topic areas and maintain essential context for long-term recall.\n\n",
                non_summary_messages.len()
            )
        };

        // 4) Add messages in chronological order (reverse the recent order)
        for msg in non_summary_messages.iter().rev() {
            prompt.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }

        // 5) Generate summary with appropriate token limits based on window size
        let token_limit = if n <= 10 {
            256 // Concise for small windows
        } else if n <= 50 {
            512 // Medium for medium windows
        } else {
            1024 // More detail for large windows
        };

        let summary = self
            .openai_client
            .summarize_conversation(&prompt, token_limit)
            .await?
            .trim()
            .to_string();

        if summary.is_empty() {
            return Err(anyhow!("Generated summary was empty"));
        }

        info!("✅ Generated {}-message summary ({} chars) for session {}", n, summary.len(), session_id);
        Ok(summary)
    }

    /// ── Phase 4: Create and save a rolling summary ──
    /// Combines summarize_last_n with proper storage using rolling tags.
    pub async fn create_rolling_summary(&self, session_id: &str, n: usize) -> Result<()> {
        info!("🔄 Creating {}-message rolling summary for session {}", n, session_id);

        // Generate the summary content
        let summary_content = self.summarize_last_n(session_id, n).await?;
        
        let mem = self.memory_service
            .as_ref()
            .ok_or_else(|| anyhow!("No memory service available for saving summary"))?;

        // Create the rolling summary tags
        let rolling_tag = format!("summary:rolling:{}", n);
        let tags = vec![
            "summary".to_string(),
            rolling_tag,
            "compressed".to_string(),
            "auto-generated".to_string(),
        ];

        // Create and save the rolling summary response
        let response = ChatResponse {
            output: String::new(),  // Empty output for rolling summaries
            persona: "mira".to_string(),
            mood: "neutral".to_string(),
            salience: 1,  // Low salience for rolling summaries
            summary: summary_content,
            memory_type: "summary".to_string(),
            tags,
            intent: Some("rolling_summary".to_string()),
            monologue: None,
            reasoning_summary: None,
        };

        // Save using the memory service (which handles embeddings appropriately)
        mem.save_assistant_response(session_id, &response).await?;
        info!("✅ Saved {}-message rolling summary for session {}", n, session_id);
        
        Ok(())
    }

    /// ── Phase 4: Manual snapshot summarization ──
    /// Allows manual triggering of summaries for any message count.
    pub async fn create_snapshot_summary(&self, session_id: &str, n: usize) -> Result<()> {
        if !CONFIG.is_robust_memory_enabled() {
            return Err(anyhow!("Snapshot summaries require robust memory to be enabled"));
        }

        info!("📸 Creating snapshot summary of {} messages for session {}", n, session_id);

        // Generate the summary content
        let summary_content = self.summarize_last_n(session_id, n).await?;
        
        let mem = self.memory_service
            .as_ref()
            .ok_or_else(|| anyhow!("No memory service available for saving summary"))?;

        // Create snapshot-specific tags
        let snapshot_tag = format!("summary:snapshot:{}", n);
        let tags = vec![
            "summary".to_string(),
            snapshot_tag,
            "compressed".to_string(),
            "manual".to_string(),
        ];

        // Create and save the snapshot summary response
        let response = ChatResponse {
            output: String::new(),  // Empty output for snapshot summaries
            persona: "mira".to_string(),
            mood: "analytical".to_string(),
            salience: 2,  // Slightly higher salience for manual summaries
            summary: summary_content,
            memory_type: "summary".to_string(),
            tags,
            intent: Some("snapshot_summary".to_string()),
            monologue: None,
            reasoning_summary: None,
        };

        mem.save_assistant_response(session_id, &response).await?;
        info!("✅ Saved snapshot summary of {} messages for session {}", n, session_id);
        
        Ok(())
    }

    /// Get the latest rolling summary of a specific type for a session
    pub async fn get_latest_rolling_summary(&self, session_id: &str, window_size: usize) -> Result<Option<MemoryEntry>> {
        let sqlite = self.sqlite_store
            .as_ref()
            .ok_or_else(|| anyhow!("No SQLite store available"))?;

        // Load recent messages and look for the target rolling summary
        let recent = sqlite.load_recent(session_id, 200).await?; // Look through more messages
        let rolling_tag = format!("summary:rolling:{}", window_size);

        for entry in recent {
            if let Some(tags) = &entry.tags {
                if tags.contains(&rolling_tag) {
                    debug!("Found latest {}-message rolling summary for session {}", window_size, session_id);
                    return Ok(Some(entry));
                }
            }
        }

        debug!("No {}-message rolling summary found for session {}", window_size, session_id);
        Ok(None)
    }

    /// Get all rolling summaries for a session (for context building)
    pub async fn get_all_rolling_summaries(&self, session_id: &str) -> Result<Vec<MemoryEntry>> {
        let sqlite = self.sqlite_store
            .as_ref()
            .ok_or_else(|| anyhow!("No SQLite store available"))?;

        // Load recent messages and filter for rolling summaries
        let recent = sqlite.load_recent(session_id, 500).await?; // Look through many messages
        
        let rolling_summaries: Vec<MemoryEntry> = recent
            .into_iter()
            .filter(|entry| {
                entry.tags
                    .as_ref()
                    .map(|tags| tags.iter().any(|tag| tag.starts_with("summary:rolling:")))
                    .unwrap_or(false)
            })
            .collect();

        debug!("Found {} rolling summaries for session {}", rolling_summaries.len(), session_id);
        Ok(rolling_summaries)
    }

    /// Check if rolling summaries are available and should be preferred over legacy summarization
    pub fn should_use_rolling_summaries(&self) -> bool {
        CONFIG.is_robust_memory_enabled() && (CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100)
    }

    /// Get summarization statistics for a session
    pub async fn get_summarization_stats(&self, session_id: &str) -> Result<SummarizationStats> {
        let sqlite = self.sqlite_store
            .as_ref()
            .ok_or_else(|| anyhow!("No SQLite store available"))?;

        let recent = sqlite.load_recent(session_id, 1000).await?;
        
        let mut stats = SummarizationStats::default();
        
        for entry in recent {
            if let Some(tags) = &entry.tags {
                if tags.contains(&"summary".to_string()) {
                    if tags.iter().any(|t| t.starts_with("summary:rolling:10")) {
                        stats.rolling_10_count += 1;
                    } else if tags.iter().any(|t| t.starts_with("summary:rolling:100")) {
                        stats.rolling_100_count += 1;
                    } else if tags.iter().any(|t| t.starts_with("summary:snapshot:")) {
                        stats.snapshot_count += 1;
                    } else {
                        stats.legacy_count += 1;
                    }
                }
            }
        }
        
        Ok(stats)
    }

    /// Direct utility: summarize arbitrary text without touching storage.
    pub async fn summarize_text(&self, text: &str) -> Result<String> {
        if text.trim().is_empty() {
            return Err(anyhow!("summarize_text: empty input"));
        }
        let token_limit = self.config.max_output_tokens().min(1024);
        let out = self
            .openai_client
            .summarize_conversation(text, token_limit)
            .await?;
        Ok(out.trim().to_string())
    }

    /// Direct utility: summarize arbitrary text with custom token limit.
    pub async fn summarize_text_with_limit(&self, text: &str, token_limit: usize) -> Result<String> {
        if text.trim().is_empty() {
            return Err(anyhow!("summarize_text_with_limit: empty input"));
        }
        let out = self
            .openai_client
            .summarize_conversation(text, token_limit)
            .await?;
        Ok(out.trim().to_string())
    }
}

/// Statistics about summarization for a session
#[derive(Debug, Default)]
pub struct SummarizationStats {
    pub rolling_10_count: usize,
    pub rolling_100_count: usize,
    pub snapshot_count: usize,
    pub legacy_count: usize,
}

impl SummarizationStats {
    pub fn total_summaries(&self) -> usize {
        self.rolling_10_count + self.rolling_100_count + self.snapshot_count + self.legacy_count
    }

    pub fn has_rolling_summaries(&self) -> bool {
        self.rolling_10_count > 0 || self.rolling_100_count > 0
    }
}
