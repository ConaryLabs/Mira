// src/memory/features/summarization/strategies/rolling_summary.rs

use crate::llm::provider::{LlmProvider, Message};
use crate::memory::core::types::MemoryEntry;
use crate::memory::features::memory_types::SummaryType;
use crate::prompt::internal::summarization as prompts;
use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info};

/// Handles all rolling summary operations (10-message and 100-message windows)
pub struct RollingSummaryStrategy {
    llm_provider: Arc<dyn LlmProvider>,
}

impl RollingSummaryStrategy {
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self { llm_provider }
    }

    /// Creates rolling summary for specified window size
    pub async fn create_summary(
        &self,
        session_id: &str,
        messages: &[MemoryEntry],
        window_size: usize,
    ) -> Result<String> {
        if messages.len() < window_size / 2 {
            return Err(anyhow::anyhow!(
                "Insufficient messages for {}-window summary",
                window_size
            ));
        }

        let content = self.build_content(messages)?;
        let prompt = self.build_prompt(&content, window_size);

        info!(
            "Creating {}-message rolling summary for session {}",
            window_size, session_id
        );

        let messages = vec![Message {
            tool_call_id: None,
            tool_calls: None,
            role: "user".to_string(),
            content: prompt,
        }];

        // FIXED: Remove None argument - .chat() now takes only 2 args
        let response = self.llm_provider
            .chat(
                messages,
                prompts::ROLLING_SUMMARIZER.to_string(),
            )
            .await?;

        Ok(response.content)
    }

    /// Determines if rolling summary should be created based on message count
    pub fn should_create(&self, message_count: usize) -> Option<SummaryType> {
        // Every 10 messages - lightweight rolling summary
        if message_count > 0 && message_count % 10 == 0 {
            return Some(SummaryType::Rolling10);
        }

        // Every 100 messages - comprehensive mega-summary
        if message_count > 0 && message_count % 100 == 0 {
            return Some(SummaryType::Rolling100);
        }

        None
    }

    fn build_content(&self, messages: &[MemoryEntry]) -> Result<String> {
        let mut content = String::new();
        let mut included_count = 0;

        for msg in messages.iter().rev() {
            // Skip existing summaries to avoid recursive summarization
            if let Some(ref tags) = msg.tags {
                if tags.iter().any(|t| t.contains("summary")) {
                    debug!("Skipping existing summary in content building");
                    continue;
                }
            }

            content.push_str(&format!("{}: {}\n", msg.role, msg.content));
            included_count += 1;
        }

        debug!(
            "Built rolling summary content from {} messages",
            included_count
        );
        Ok(content)
    }

    // PHASE 2: ENHANCED PROMPTS WITH TECHNICAL + PERSONAL BALANCE
    fn build_prompt(&self, content: &str, window_size: usize) -> String {
        match window_size {
            100 => format!(
                "Create a comprehensive, detailed summary of the last {} messages. This captures both the TECHNICAL work AND the RELATIONSHIP - how we communicate, what matters to the user, and the vibe of our conversations.

**TECHNICAL CONTENT:**

**Topics & Discussions:**
- What topics were discussed in detail?
- What questions did the user ask?
- What explanations or concepts were covered?
- Any debates, considerations, or alternative approaches discussed?

**Specific Technical Details (Be Precise):**
- File paths mentioned (e.g., src/api/handler.rs, components/Button.tsx)
- Function names, class names, component names
- Variable names or configuration keys
- API endpoints, routes, database tables
- Error messages or debugging steps
- Technology stack, libraries, frameworks
- Code patterns or architectural decisions

**Actions & Decisions:**
- What did the user decide to do?
- What changes were made or planned?
- What approaches were chosen and why?
- What was ruled out or deprioritized?

**PERSONAL & RELATIONSHIP CONTENT:**

**Communication Style & Mood:**
- What was the vibe of the conversation? (casual, focused, frustrated, excited, etc.)
- How were they feeling about their work?
- Any humor, jokes, or banter we shared?
- Level of formality/profanity/casualness
- Were they stressed, confident, confused, energized?

**Personal Context:**
- Any personal life details shared (not just work)
- How they're doing overall - energy level, motivation
- Frustrations vented or celebrations shared
- Support they needed (technical help vs emotional encouragement)
- Their personality coming through (funny, sarcastic, enthusiastic, direct, etc.)

**Relationship Dynamics:**
- How did we interact? (friendly, professional, playful, etc.)
- Did I adapt to their mood appropriately?
- Any moments of connection or understanding?
- Inside jokes or running themes that developed?
- What kind of support did they respond well to?

**User's Bigger Picture:**
- What are they trying to accomplish overall?
- How does this work fit into their life/goals?
- Their skill level and confidence in different areas
- What they care about beyond just getting code to work

**Current State:**
- Where we left off
- What's next for them
- Any blockers (technical OR personal)
- Open questions or things to follow up on

**Format:**
Write in natural, conversational prose. Use bullet points within sections. Be specific with technical details, but also capture the human side - this is a relationship, not just a technical log.

**Target Length:** 2,000-2,500 tokens

===== CONVERSATION TO SUMMARIZE =====

{}",
                window_size, content
            ),
            10 => format!(
                "Create a detailed rolling summary of the last {} messages. Capture both what was discussed technically AND the vibe/mood of the conversation.

**Technical:**
- Main topics discussed
- Specific files, functions, or code elements mentioned
- Decisions made or approaches chosen
- Any issues encountered or errors debugged

**Personal/Relational:**
- How were they feeling? (frustrated, excited, tired, focused, etc.)
- What was the communication style? (casual, serious, joking, etc.)
- Any personal context shared?
- What kind of support did they need?

Be specific with technical details but don't forget the human element.

**Target Length:** 400-600 tokens

===== CONVERSATION TO SUMMARIZE =====

{}",
                window_size, content
            ),
            _ => format!(
                "Summarize the last {} messages, preserving both technical details and the relationship/mood:\n\n{}",
                window_size, content
            ),
        }
    }
}
