// src/memory/core/config.rs
#[derive(Clone, Debug)]
pub struct MemoryConfig {
    pub always_embed_user: bool,
    pub always_embed_assistant: bool,
    pub embed_min_chars: usize,
    pub dedup_sim_threshold: f32,    // cosine similarity 0..1
    pub salience_min_for_embed: f32, // 0.0-1.0 scale
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            always_embed_user: std::env::var("MEM_ALWAYS_EMBED_USER").unwrap_or_default() == "true",
            always_embed_assistant: std::env::var("MEM_ALWAYS_EMBED_ASSISTANT").unwrap_or_default()
                == "true",
            embed_min_chars: std::env::var("MEM_EMBED_MIN_CHARS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(6),
            dedup_sim_threshold: std::env::var("MEM_DEDUP_SIM_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.97),
            salience_min_for_embed: std::env::var("MEM_SALIENCE_MIN_FOR_EMBED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.6),
        }
    }
}
