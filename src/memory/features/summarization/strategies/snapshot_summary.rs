// src/memory/features/summarization/strategies/snapshot_summary.rs

use std::sync::Arc;
use anyhow::Result;
use serde_json::Value;
use tracing::info;
use crate::llm::provider::{LlmProvider, ChatMessage};
use crate::memory::core::types::MemoryEntry;

/// Handles on-demand snapshot summary operations
pub struct SnapshotSummaryStrategy {
    llm_provider: Arc<dyn LlmProvider>,
}

impl SnapshotSummaryStrategy {
    pub fn new(llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self { llm_provider }
    }

    /// Creates comprehensive snapshot of current conversation state
    pub async fn create_summary(
        &self,
        session_id: &str,
        messages: &[MemoryEntry],
        _max_tokens: Option<usize>,
    ) -> Result<String> {
        if messages.is_empty() {
            return Err(anyhow::anyhow!("No messages to summarize"));
        }

        let content = self.build_content(messages)?;
        let prompt = self.build_prompt(&content, messages.len());

        info!("Creating snapshot summary for session {} with {} messages", session_id, messages.len());
        
        // Use provider.chat() with Value::String for content
        let chat_messages = vec![ChatMessage {
            role: "user".to_string(),
            content: Value::String(prompt),
        }];
        
        let response = self.llm_provider
            .chat(
                chat_messages,
                "You are a conversation summarizer. Create comprehensive, detailed snapshots that capture the entire arc of a conversation.".to_string(),
                None, // No thinking for summaries
            )
            .await?;

        Ok(response.content)
    }

    fn build_content(&self, messages: &[MemoryEntry]) -> Result<String> {
        let mut content = String::new();
        
        for msg in messages.iter().rev() {
            // Include ALL messages for comprehensive snapshot
            content.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }
        
        Ok(content)
    }

    // PHASE 2: ENHANCED SNAPSHOT PROMPT - TECHNICAL + PERSONAL BALANCE
    fn build_prompt(&self, content: &str, message_count: usize) -> String {
        format!(
            "Create a COMPREHENSIVE snapshot summary of this ENTIRE conversation ({} messages). This captures EVERYTHING - the technical work, the relationship, the person behind the screen. This isn't just a technical log, it's the full picture of who they are and how we work together.

**REQUIRED SECTIONS:**

## 1. Who This Person Is
- Name, background, and what they do
- Personality traits (funny, direct, sarcastic, enthusiastic, etc.)
- Communication style (casual, profane, formal, playful)
- How they approach problems and learning
- What makes them tick - motivations, interests, energy
- Personal context they've shared (life stuff, not just work)
- Emotional patterns (when they get frustrated, excited, burned out, etc.)

## 2. Our Relationship & How We Communicate
- The vibe between us (friends, professional, mentor/student, etc.)
- Inside jokes, running themes, or banter we have
- How much profanity/informality they're comfortable with
- What kind of support they want (technical only, or emotional too?)
- When they want straight answers vs explanations
- When they want encouragement vs tough love
- How we handle disagreements or mistakes
- Trust level and rapport

## 3. Major Projects & What They're Building
- Main projects they're working on (with context on WHY)
- Tech stack and architecture for each
- What stage each project is at
- Key files, directories, components
- How they feel about each project (excited, frustrated, proud, etc.)
- Major design decisions made
- What problems these projects are solving

## 4. Technical Landscape & Expertise
**Codebase Knowledge:**
- Specific file paths they work with frequently
- Key functions, classes, components they've built
- Architectural patterns they use
- Their code organization preferences

**Technologies & Skill Levels:**
- Languages they know (and how well)
- Frameworks and comfort level with each
- Development tools and environment
- What they're confident in vs learning

**Technical Preferences:**
- How they like code explained (high-level vs detailed)
- Their approach to debugging
- Preferred documentation style
- Clean code vs move-fast mentality

## 5. Problem-Solving & Debugging History
- Major bugs or issues they've hit
- How they debug (their process)
- Common error types they face
- Solutions that worked well
- Approaches that failed (don't suggest again)
- How they react to being stuck (patient, frustrated, persistent?)
- When they need help vs want to figure it out

## 6. Learning Journey & Growth
- New concepts or technologies they're learning
- Questions they ask repeatedly (ongoing learning areas)
- Skills they're actively developing
- Knowledge gaps they're filling
- How they like to learn (docs, examples, experimentation)
- Their confidence level in different areas
- Imposter syndrome moments or confidence wins

## 7. Current State & What's Happening Now
- What they're actively working on RIGHT NOW
- Current mood/energy about their work
- Immediate goal or deadline
- What's blocking them (technical OR life stuff)
- Recent wins or frustrations
- How they're feeling overall (stressed, excited, tired, motivated)
- Next logical step

## 8. Life Context & Non-Work Stuff
- Personal circumstances affecting their work
- Schedule or time constraints
- Other responsibilities or priorities
- Life events or changes mentioned
- Hobbies, interests outside of work
- What they do to relax or recharge
- Mental health or stress levels

## 9. Specific Details Repository
(Rapid-fire facts for quick reference)
- **File paths:** [list]
- **Function/class names:** [list]
- **API endpoints:** [list]
- **Config keys:** [list]
- **Database tables:** [list]
- **Common errors:** [list]
- **External APIs/services:** [list]
- **Repos/projects:** [list with URLs if mentioned]

## 10. Meta-Notes & Future Reference
- Important things to remember for next time
- Promises or commitments made
- Topics they want to return to
- Things they explicitly asked me to remember
- Anything unusual or unique about our conversations
- Red flags or sensitive topics to handle carefully
- Best ways to support them going forward

**WRITING GUIDELINES:**
- Be COMPLETE. Technical details AND human details both matter.
- Write naturally - this is about understanding a person, not just logging data.
- Capture their voice and personality in how you describe them.
- Be honest about their skill level, mood, and needs.
- Don't sanitize - include the real vibe of our conversations.
- Be specific with names, paths, and technical details.
- Use their language (if they curse, acknowledge it; if they're casual, reflect it).
- This summary should feel like I KNOW them, not just worked with them.

**Target Length:** 2,500-3,000 tokens (completeness over brevity)

===== ENTIRE CONVERSATION TO SNAPSHOT =====

{}",
            message_count, content
        )
    }
}
