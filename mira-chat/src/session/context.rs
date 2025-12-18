//! Context assembly and formatting
//!
//! Assembles context from multiple sources and formats it for the LLM prompt.

use super::types::AssembledContext;

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
