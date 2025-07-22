// src/memory/decay.rs

//! Human-like memory decay and weighting system.
//! Memories fade over time but can be reinforced through recall.

use chrono::{DateTime, Duration, Utc};
use crate::memory::types::{MemoryEntry, MemoryType};

/// Configuration for memory decay curves
#[derive(Debug, Clone)]
pub struct DecayConfig {
    /// Base decay rate (0.0 = no decay, 1.0 = instant forgetting)
    pub base_decay_rate: f32,
    /// How much emotional memories resist decay
    pub emotional_resistance: f32,
    /// Boost factor when a memory is recalled
    pub recall_reinforcement: f32,
    /// Minimum salience before a memory is "forgotten"
    pub forgetting_threshold: f32,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            base_decay_rate: 0.1,
            emotional_resistance: 0.7,
            recall_reinforcement: 1.5,
            forgetting_threshold: 1.0,
        }
    }
}

/// Calculates the decayed salience of a memory based on time and type
pub fn calculate_decayed_salience(
    memory: &MemoryEntry,
    config: &DecayConfig,
    now: DateTime<Utc>,
) -> f32 {
    let original_salience = memory.salience.unwrap_or(5.0);
    let age = now.signed_duration_since(memory.timestamp);
    
    // Convert age to decay factor (hours as unit)
    let hours_passed = age.num_hours() as f32;
    
    // Different memory types decay at different rates
    let type_modifier = match &memory.memory_type {
        Some(MemoryType::Feeling) => 1.0 - config.emotional_resistance,
        Some(MemoryType::Promise) => 0.1,  // Promises decay very slowly
        Some(MemoryType::Joke) => 1.5,     // Jokes fade faster
        Some(MemoryType::Fact) => 0.8,     // Facts are relatively stable
        Some(MemoryType::Event) => 1.0,    // Events decay normally
        _ => 1.0,
    };
    
    // Emotional intensity affects decay (high salience = slower decay)
    let salience_modifier = 1.0 - (original_salience / 10.0) * 0.5;
    
    // Calculate decay using Ebbinghaus forgetting curve approximation
    // Using sqrt to make decay less aggressive over time
    let decay_factor = (-config.base_decay_rate * type_modifier * salience_modifier * (hours_passed / 24.0).powf(0.3)).exp();
    
    let decayed = original_salience * decay_factor;
    
    // Don't let it go below threshold unless it started there
    if original_salience > config.forgetting_threshold {
        decayed.max(config.forgetting_threshold)
    } else {
        decayed
    }
}

/// Reinforces a memory when it's recalled, fighting decay
pub fn reinforce_memory(
    memory: &mut MemoryEntry,
    config: &DecayConfig,
    recall_context_salience: f32,
) {
    let current = memory.salience.unwrap_or(5.0);
    
    // Reinforcement is stronger for emotionally relevant recalls
    let reinforcement = config.recall_reinforcement * (1.0 + recall_context_salience / 10.0);
    
    // Apply reinforcement but cap at 10.0
    memory.salience = Some((current * reinforcement).min(10.0));
    
    // Update tags to indicate reinforcement
    if let Some(ref mut tags) = memory.tags {
        if !tags.contains(&"reinforced".to_string()) {
            tags.push("reinforced".to_string());
        }
    }
}

/// Identifies memories that are effectively "forgotten" and can be archived
pub fn identify_forgotten_memories(
    memories: &[MemoryEntry],
    config: &DecayConfig,
    now: DateTime<Utc>,
) -> Vec<usize> {
    memories.iter()
        .enumerate()
        .filter_map(|(idx, memory)| {
            let decayed = calculate_decayed_salience(memory, config, now);
            if decayed < config.forgetting_threshold && 
               now.signed_duration_since(memory.timestamp) > Duration::days(30) {
                Some(idx)
            } else {
                None
            }
        })
        .collect()
}

/// Applies time-based decay to a batch of memories (for background processing)
pub fn apply_batch_decay(
    memories: &mut [MemoryEntry],
    config: &DecayConfig,
    now: DateTime<Utc>,
) {
    for memory in memories {
        let decayed = calculate_decayed_salience(memory, config, now);
        memory.salience = Some(decayed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_emotional_memories_decay_slower() {
        let config = DecayConfig::default();
        let now = Utc::now();
        let one_day_ago = now - Duration::days(1);
        
        let mut feeling_memory = MemoryEntry {
            id: Some(1),
            session_id: "test".to_string(),
            role: "user".to_string(),
            content: "I love you".to_string(),
            timestamp: one_day_ago,
            embedding: None,
            salience: Some(9.0),
            tags: None,
            summary: None,
            memory_type: Some(MemoryType::Feeling),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };
        
        let mut joke_memory = feeling_memory.clone();
        joke_memory.memory_type = Some(MemoryType::Joke);
        
        let feeling_decay = calculate_decayed_salience(&feeling_memory, &config, now);
        let joke_decay = calculate_decayed_salience(&joke_memory, &config, now);
        
        assert!(feeling_decay > joke_decay, "Feelings should decay slower than jokes");
    }

    #[test]
    fn test_reinforcement_increases_salience() {
        let config = DecayConfig::default();
        let mut memory = MemoryEntry {
            id: Some(1),
            session_id: "test".to_string(),
            role: "user".to_string(),
            content: "Remember this".to_string(),
            timestamp: Utc::now(),
            embedding: None,
            salience: Some(5.0),
            tags: Some(vec![]),
            summary: None,
            memory_type: Some(MemoryType::Fact),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };
        
        reinforce_memory(&mut memory, &config, 7.0);
        
        assert!(memory.salience.unwrap() > 5.0, "Reinforcement should increase salience");
        assert!(memory.tags.as_ref().unwrap().contains(&"reinforced".to_string()));
    }
}
