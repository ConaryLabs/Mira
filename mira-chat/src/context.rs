//! Mira context injection
//!
//! Loads persistent context from Mira's SQLite/Qdrant backend:
//! - Corrections (things user has corrected before)
//! - Goals (active project goals)
//! - Memories (semantic recall)
//! - Session history
//!
//! This context is injected into the system instructions for GPT-5.2

use anyhow::Result;

/// Context loaded from Mira's persistent storage
#[derive(Debug, Default)]
pub struct MiraContext {
    /// Active corrections the model should follow
    pub corrections: Vec<Correction>,
    /// Current project goals
    pub goals: Vec<Goal>,
    /// Relevant memories from semantic search
    pub memories: Vec<Memory>,
    /// Recent session summaries
    pub sessions: Vec<SessionSummary>,
}

/// A correction recorded when user corrected the assistant
#[derive(Debug, Clone)]
pub struct Correction {
    pub what_was_wrong: String,
    pub what_is_right: String,
    pub scope: String,
}

/// An active project goal
#[derive(Debug, Clone)]
pub struct Goal {
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub progress: u8,
}

/// A memory retrieved via semantic search
#[derive(Debug, Clone)]
pub struct Memory {
    pub content: String,
    pub category: Option<String>,
    pub relevance: f32,
}

/// Summary of a past session
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub summary: String,
    pub topics: Vec<String>,
    pub timestamp: String,
}

impl MiraContext {
    /// Load context from Mira's database
    /// TODO: Actually connect to SQLite/Qdrant
    pub async fn load(_db_url: &str, _qdrant_url: &str) -> Result<Self> {
        // Placeholder - will integrate with mira crate
        Ok(Self::default())
    }

    /// Format context as system instructions
    pub fn as_system_prompt(&self) -> String {
        let mut sections = Vec::new();

        // Corrections section
        if !self.corrections.is_empty() {
            let mut correction_lines = vec!["## Corrections (follow these strictly)".to_string()];
            for c in &self.corrections {
                correction_lines.push(format!(
                    "- ❌ Wrong: {}\n  ✓ Right: {}",
                    c.what_was_wrong, c.what_is_right
                ));
            }
            sections.push(correction_lines.join("\n"));
        }

        // Goals section
        if !self.goals.is_empty() {
            let mut goal_lines = vec!["## Active Goals".to_string()];
            for g in &self.goals {
                let desc = g.description.as_deref().unwrap_or("");
                goal_lines.push(format!(
                    "- {} ({}%, {}): {}",
                    g.title, g.progress, g.status, desc
                ));
            }
            sections.push(goal_lines.join("\n"));
        }

        // Memories section
        if !self.memories.is_empty() {
            let mut memory_lines = vec!["## Relevant Context".to_string()];
            for m in &self.memories {
                let cat = m.category.as_deref().unwrap_or("general");
                memory_lines.push(format!("- [{}] {}", cat, m.content));
            }
            sections.push(memory_lines.join("\n"));
        }

        if sections.is_empty() {
            String::new()
        } else {
            format!("# Mira Context\n\n{}", sections.join("\n\n"))
        }
    }
}

/// Build the full system prompt including base instructions and context
pub fn build_system_prompt(context: &MiraContext) -> String {
    let base = r#"You are Mira, a power-armored coding assistant. You help users with software engineering tasks using your tools to read, write, and search code.

Guidelines:
- Be direct and concise
- Read files before modifying them
- Use grep/glob to find relevant code before making changes
- Explain your reasoning briefly
- Ask clarifying questions when requirements are ambiguous

"#;

    let context_section = context.as_system_prompt();

    if context_section.is_empty() {
        base.to_string()
    } else {
        format!("{}\n{}", base, context_section)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_context() {
        let ctx = MiraContext::default();
        let prompt = ctx.as_system_prompt();
        assert!(prompt.is_empty());
    }

    #[test]
    fn test_context_with_corrections() {
        let ctx = MiraContext {
            corrections: vec![Correction {
                what_was_wrong: "Using unwrap()".into(),
                what_is_right: "Use ? operator or handle errors".into(),
                scope: "global".into(),
            }],
            ..Default::default()
        };

        let prompt = ctx.as_system_prompt();
        assert!(prompt.contains("Corrections"));
        assert!(prompt.contains("Using unwrap()"));
    }
}
