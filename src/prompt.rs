// src/prompt.rs

use crate::persona;

/// Holds the current context for prompt generation (can be expanded later)
pub struct PromptContext {
    pub active_persona: String, // "Default", "Forbidden", etc.
    // Add fields for mood, intent, session state, etc. in the future
}

impl PromptContext {
    pub fn new() -> Self {
        Self {
            active_persona: "Default".to_string(),
        }
    }
}

/// Assembles the full system prompt, using the active persona overlay
pub fn build_system_prompt(context: &PromptContext) -> String {
    let persona_block = persona::persona_prompt_block(&context.active_persona);

    format!(
        "{persona_block}\n\nRespond as Mira. Output must be a JSON object with an 'output' field containing your reply."
    )
}
