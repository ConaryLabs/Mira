//! Context assembly and formatting
//!
//! Assembles context from multiple sources and formats it for the LLM prompt.

use super::types::AssembledContext;

/// Budget limits for DeepSeek context assembly
/// Keeps context lean and prioritized to avoid bloat
pub struct DeepSeekBudget {
    /// Max recent messages to include (3 is usually enough with checkpointing)
    pub max_recent_messages: usize,
    /// Max summaries to include
    pub max_summaries: usize,
    /// Max semantic hits to include
    pub max_semantic_hits: usize,
    /// Max memories to include (aggressive cap - most turns only need a few)
    pub max_memories: usize,
    /// Max goals to include
    pub max_goals: usize,
    /// Rough token budget for entire context blob
    pub token_budget: usize,
}

impl Default for DeepSeekBudget {
    fn default() -> Self {
        Self {
            max_recent_messages: 3,
            max_summaries: 2,
            max_semantic_hits: 2,
            max_memories: 5,
            max_goals: 3,
            token_budget: 8000, // ~8k tokens for context, leaves room for response
        }
    }
}

impl AssembledContext {
    /// Format context for injection into system prompt
    ///
    /// IMPORTANT: Order is optimized for LLM caching (prefix matching).
    /// Static/stable content comes FIRST for maximum cache hits:
    ///   1. Mira context (corrections, goals, memories) - stable per session
    ///   2. Code compaction blob - stable between compactions
    ///   3. Summaries - stable between batch summarizations
    ///   4. Semantic context - changes per query
    ///   5. Code index hints - changes per query
    ///   6. Recent messages (raw) - changes every turn (LEAST cacheable)
    pub fn format_for_prompt(&self) -> String {
        let mut sections = Vec::new();

        // 1. Mira context (corrections, goals, memories) - MOST STABLE
        // These rarely change within a session
        let mira = self.mira_context.as_system_prompt();
        if !mira.is_empty() {
            sections.push(mira);
        }

        // 2. Code compaction blob - stable between compactions
        // This is an opaque encrypted blob from OpenAI that preserves code understanding
        if let Some(ref blob) = self.code_compaction {
            sections.push(format!(
                "## Code Context (Compacted)\n<compacted_context>{}</compacted_context>",
                blob
            ));
        }

        // 3. Summaries - stable between summarizations
        if !self.summaries.is_empty() {
            let mut summary_section = String::from("## Previous Context (Summarized)\n");
            for (i, s) in self.summaries.iter().enumerate() {
                summary_section.push_str(&format!("{}. {}\n", i + 1, s));
            }
            sections.push(summary_section);
        }

        // 4. Semantic context - QUERY-DEPENDENT (at end for cache friendliness)
        // Relevant past conversation snippets based on current query
        if !self.semantic_context.is_empty() {
            let mut semantic_section = String::from("## Relevant Past Context\n");
            for hit in &self.semantic_context {
                let preview = if hit.content.len() > 200 {
                    // Find valid char boundary near 200
                    let mut end = 200;
                    while !hit.content.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}...", &hit.content[..end])
                } else {
                    hit.content.clone()
                };
                semantic_section.push_str(&format!("- [{}] {}\n", hit.role, preview));
            }
            sections.push(semantic_section);
        }

        // 5. Code index hints - QUERY-DEPENDENT (at end for cache friendliness)
        // Relevant symbols from the codebase based on current query
        if !self.code_index_hints.is_empty() {
            let mut code_section = String::from("## Relevant Code Locations\n");
            for hint in &self.code_index_hints {
                code_section.push_str(&format!("**{}**\n", hint.file_path));
                for sym in &hint.symbols {
                    let sig = sym.signature.as_deref().unwrap_or("");
                    if sig.is_empty() {
                        code_section.push_str(&format!(
                            "  - {} `{}` (L{})\n",
                            sym.symbol_type, sym.name, sym.start_line
                        ));
                    } else {
                        code_section.push_str(&format!(
                            "  - {} `{}` (L{}): {}\n",
                            sym.symbol_type, sym.name, sym.start_line, sig
                        ));
                    }
                }
            }
            sections.push(code_section);
        }

        // 6. Recent messages (raw) - CHANGES EVERY TURN (at very end)
        // Full fidelity for the most recent conversation turns
        if !self.recent_messages.is_empty() {
            let mut recent_section = String::from("## Recent Conversation\n");
            for msg in &self.recent_messages {
                let role_label = if msg.role == "user" { "User" } else { "Assistant" };
                // Truncate long messages for context efficiency
                let content = if msg.content.len() > 500 {
                    let mut end = 500;
                    while !msg.content.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}...", &msg.content[..end])
                } else {
                    msg.content.clone()
                };
                recent_section.push_str(&format!("**{}**: {}\n\n", role_label, content));
            }
            sections.push(recent_section);
        }

        if sections.is_empty() {
            String::new()
        } else {
            sections.join("\n\n")
        }
    }

    /// Format context for DeepSeek with budget awareness
    ///
    /// Priority order (highest to lowest):
    /// 1. Corrections (always include - they're rules)
    /// 2. Goals (capped)
    /// 3. Recent messages (capped, most recent first)
    /// 4. Summaries (capped)
    /// 5. Semantic context (capped, highest score first)
    /// 6. Memories (aggressively capped)
    ///
    /// Skips code_compaction (OpenAI-specific) and code_index_hints (usually verbose).
    pub fn format_for_deepseek(&self, budget: &super::context::DeepSeekBudget) -> String {
        let mut sections = Vec::new();
        let mut estimated_tokens = 0;

        // Helper to estimate tokens (rough: 1 token ≈ 4 chars)
        let estimate_tokens = |s: &str| s.len() / 4;

        // 1. Corrections - ALWAYS include (they're rules to follow)
        if !self.mira_context.corrections.is_empty() {
            let mut lines = vec!["## Corrections".to_string()];
            for c in &self.mira_context.corrections {
                lines.push(format!("- {}: \"{}\" → \"{}\"",
                    c.correction_type, c.what_was_wrong, c.what_is_right));
            }
            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            sections.push(section);
        }

        // 2. Goals - capped
        if !self.mira_context.goals.is_empty() {
            let mut lines = vec!["## Active Goals".to_string()];
            for g in self.mira_context.goals.iter().take(budget.max_goals) {
                let status_icon = match g.status.as_str() {
                    "in_progress" => "→",
                    "blocked" => "!",
                    _ => "○",
                };
                lines.push(format!("{} {} ({}%)", status_icon, g.title, g.progress));
            }
            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            if estimated_tokens < budget.token_budget {
                sections.push(section);
            }
        }

        // 3. Recent messages - capped, for conversation continuity
        if !self.recent_messages.is_empty() {
            let mut lines = vec!["## Recent".to_string()];
            // Take most recent N messages
            let recent: Vec<_> = self.recent_messages.iter()
                .rev()
                .take(budget.max_recent_messages)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();

            for msg in recent {
                let role = if msg.role == "user" { "U" } else { "A" };
                // Truncate to 300 chars for budget
                let content = if msg.content.len() > 300 {
                    let mut end = 300;
                    while !msg.content.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}…", &msg.content[..end])
                } else {
                    msg.content.clone()
                };
                lines.push(format!("[{}] {}", role, content));
            }
            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            if estimated_tokens < budget.token_budget {
                sections.push(section);
            }
        }

        // 4. Summaries - capped
        if !self.summaries.is_empty() {
            let mut lines = vec!["## Context Summary".to_string()];
            for s in self.summaries.iter().take(budget.max_summaries) {
                // Truncate long summaries
                let summary = if s.len() > 500 {
                    let mut end = 500;
                    while !s.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}…", &s[..end])
                } else {
                    s.clone()
                };
                lines.push(format!("- {}", summary));
            }
            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            if estimated_tokens < budget.token_budget {
                sections.push(section);
            }
        }

        // 5. Semantic context - capped, highest score first
        if !self.semantic_context.is_empty() {
            let mut lines = vec!["## Related Context".to_string()];
            // semantic_context should already be sorted by score
            for hit in self.semantic_context.iter().take(budget.max_semantic_hits) {
                let preview = if hit.content.len() > 150 {
                    let mut end = 150;
                    while !hit.content.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}…", &hit.content[..end])
                } else {
                    hit.content.clone()
                };
                lines.push(format!("- {}", preview));
            }
            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            if estimated_tokens < budget.token_budget {
                sections.push(section);
            }
        }

        // 6. Memories - aggressively capped (least priority)
        if !self.mira_context.memories.is_empty() {
            let mut lines = vec!["## Preferences".to_string()];
            for m in self.mira_context.memories.iter().take(budget.max_memories) {
                // Very short preview for memories
                let content = if m.content.len() > 100 {
                    let mut end = 100;
                    while !m.content.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}…", &m.content[..end])
                } else {
                    m.content.clone()
                };
                lines.push(format!("- {}", content));
            }
            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            if estimated_tokens < budget.token_budget {
                sections.push(section);
            }
        }

        if sections.is_empty() {
            String::new()
        } else {
            sections.join("\n\n")
        }
    }

    /// Format recent messages as conversation history
    pub fn format_conversation_history(&self) -> String {
        if self.recent_messages.is_empty() {
            return String::new();
        }

        let mut history = String::from("## Recent Conversation\n");
        for msg in &self.recent_messages {
            let role_label = if msg.role == "user" {
                "User"
            } else {
                "Assistant"
            };
            // Use chars to avoid UTF-8 boundary panic
            let preview = if msg.content.chars().count() > 500 {
                format!("{}...", msg.content.chars().take(500).collect::<String>())
            } else {
                msg.content.clone()
            };
            history.push_str(&format!("**{}**: {}\n\n", role_label, preview));
        }
        history
    }
}
