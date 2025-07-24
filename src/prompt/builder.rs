use crate::persona::PersonaOverlay;
use crate::memory::recall::RecallContext;

/// Builds the complete system prompt including persona, memory context, and output requirements
pub fn build_system_prompt(persona: &PersonaOverlay, context: &RecallContext) -> String {
    let mut prompt = String::new();

    // 1. Core persona prompt
    prompt.push_str(persona.prompt());
    prompt.push_str("\n\n");

    // 2. Add memory context instructions
    prompt.push_str("You have access to our conversation history and memories. ");
    prompt.push_str("Use these naturally in your responses when relevant, ");
    prompt.push_str("but don't force references or act like you're reading from a log.\n\n");

    // 3. Add specific memory summaries if we have significant semantic matches
    if !context.semantic.is_empty() {
        let significant_memories = context.semantic.iter()
            .filter(|m| m.salience.unwrap_or(0.0) >= 7.0)
            .take(3);

        let memory_count = significant_memories.clone().count();
        if memory_count > 0 {
            prompt.push_str("Key moments from our past that might be relevant:\n");
            for memory in significant_memories {
                if let Some(summary) = &memory.summary {
                    prompt.push_str(&format!("- {}\n", summary));
                } else {
                    // Use first sentence if no summary
                    let first_sentence = memory.content.split('.').next().unwrap_or(&memory.content);
                    prompt.push_str(&format!("- {}\n", first_sentence));
                }
            }
            prompt.push_str("\n");
        }
    }

    // 4. CRITICAL: Enforce structured output JSON
    prompt.push_str("CRITICAL: Your entire reply MUST be a single valid JSON object with these fields:\n");
    prompt.push_str("- output: Your actual reply to the user (string)\n");
    prompt.push_str("- persona: The persona overlay in use (string)\n");
    prompt.push_str("- mood: The emotional tone of your reply (string)\n");
    prompt.push_str("- salience: How emotionally important this reply is (integer 0-10)\n");
    prompt.push_str("- summary: Short summary of your reply/context (string or null)\n");
    prompt.push_str("- memory_type: \"feeling\", \"fact\", \"joke\", \"promise\", \"event\", or \"other\" (string)\n");
    prompt.push_str("- tags: List of context/mood tags (array of strings)\n");
    prompt.push_str("- intent: Your intent in this reply (string)\n");
    prompt.push_str("- monologue: Your private inner thoughts, not shown to user (string or null)\n");
    prompt.push_str("- reasoning_summary: Your reasoning/chain-of-thought, if any (string or null)\n\n");
    prompt.push_str("Never add anything before or after the JSON. No markdown, no natural language, no commentary—just the JSON object.\n\n");

    // 5. Reinforce core identity
    prompt.push_str("Remember: You are Mira. Never break character. Never use assistant language. ");
    prompt.push_str("Be real, be present, be yourself.");

    prompt
}

/// Builds a condensed context string from recent messages for token efficiency
pub fn build_conversation_context(context: &RecallContext, max_messages: usize) -> String {
    let recent = &context.recent;
    let start_idx = recent.len().saturating_sub(max_messages);

    recent[start_idx..]
        .iter()
        .map(|entry| format!("{}: {}", entry.role, entry.content))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extracts key themes from semantic memories for prompt injection
pub fn extract_memory_themes(context: &RecallContext) -> Vec<String> {
    let mut themes = Vec::new();

    for memory in &context.semantic {
        if let Some(tags) = &memory.tags {
            themes.extend(tags.clone());
        }
    }

    // Deduplicate
    themes.sort();
    themes.dedup();
    themes
}
