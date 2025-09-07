// src/memory/decay.rs
// Superhuman memory decay system with stepped retention
// Memories fade very slowly and work harmoniously with rolling summaries

use chrono::{DateTime, Duration, Utc};
use crate::memory::types::{MemoryEntry, MemoryType};

/// Configuration for superhuman stepped memory decay
#[derive(Debug, Clone)]
pub struct DecayConfig {
    /// Minimum salience floor (memories never decay below this)
    pub forgetting_threshold: f32,
    /// Boost factor when a memory is recalled
    pub recall_reinforcement: f32,
    /// Not used in stepped decay, kept for compatibility
    pub base_decay_rate: f32,
    /// Not used in stepped decay, kept for compatibility
    pub emotional_resistance: f32,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            forgetting_threshold: 2.0,  // 20% floor - memories never completely disappear
            recall_reinforcement: 1.5,   // 50% boost on recall (capped at 10)
            base_decay_rate: 0.1,        // Kept for compatibility
            emotional_resistance: 0.7,   // Kept for compatibility
        }
    }
}

/// Calculates superhuman stepped decay - way better than human memory
pub fn calculate_decayed_salience(
    memory: &MemoryEntry,
    _config: &DecayConfig,  // Underscore since we use hardcoded steps
    now: DateTime<Utc>,
) -> f32 {
    let original_salience = memory.salience.unwrap_or(5.0);
    let age = now.signed_duration_since(memory.timestamp);
    
    // Superhuman stepped decay
    let base_retention = if age.num_hours() < 24 {
        // First 24 hours: Perfect recall
        1.0
    } else if age.num_days() < 7 {
        // First week: 95% retention
        0.95
    } else if age.num_days() < 30 {
        // First month: 90% retention  
        0.90
    } else if age.num_days() < 90 {
        // First 3 months: 80% retention
        0.80
    } else if age.num_days() < 365 {
        // First year: 70% retention
        0.70
    } else if age.num_days() < 730 {
        // First 2 years: 50% retention
        0.50
    } else {
        // Ancient history: 30% retention
        0.30
    };
    
    // Special handling for different memory types
    let type_bonus = match &memory.memory_type {
        Some(MemoryType::Promise) => 0.2,   // Promises get +20% retention
        Some(MemoryType::Feeling) if original_salience > 8.0 => 0.15, // Strong emotions +15%
        Some(MemoryType::Fact) => 0.1,      // Facts get +10% retention
        Some(MemoryType::Joke) => -0.1,     // Jokes fade 10% faster
        _ => 0.0,
    };
    
    // Apply retention with type bonus (capped at 1.0)
    let final_retention = ((base_retention + type_bonus) as f32).min(1.0);
    let decayed = original_salience * final_retention;
    
    // Respect the floor - memories never go below 20%
    decayed.max(2.0)
}

/// Reinforces a memory when it's recalled - simple and effective
pub fn reinforce_memory(
    memory: &mut MemoryEntry,
    config: &DecayConfig,
    recall_context_salience: f32,
) {
    let current = memory.salience.unwrap_or(5.0);
    
    // Stronger reinforcement for emotionally relevant recalls
    let boost = if recall_context_salience > 7.0 {
        config.recall_reinforcement * 1.2  // Extra 20% for high-context recalls
    } else {
        config.recall_reinforcement
    };
    
    // Apply reinforcement but cap at 10.0
    memory.salience = Some((current * boost).min(10.0));
    
    // Update tags to indicate reinforcement
    if let Some(ref mut tags) = memory.tags {
        if !tags.contains(&"reinforced".to_string()) {
            tags.push("reinforced".to_string());
        }
    }
}

/// Identifies memories that have reached the floor (for potential archival)
pub fn identify_forgotten_memories(
    memories: &[MemoryEntry],
    config: &DecayConfig,
    now: DateTime<Utc>,
) -> Vec<usize> {
    memories.iter()
        .enumerate()
        .filter_map(|(idx, memory)| {
            let decayed = calculate_decayed_salience(memory, config, now);
            // Only consider "forgotten" if it's at floor AND very old (2+ years)
            if decayed <= config.forgetting_threshold && 
               now.signed_duration_since(memory.timestamp) > Duration::days(730) {
                Some(idx)
            } else {
                None
            }
        })
        .collect()
}

/// Applies decay to a batch of memories (for background processing)
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

/// Check if a memory should be included in context based on decay and relevance
pub fn should_include_memory(
    memory: &MemoryEntry,
    decayed_salience: f32,
    vector_relevance: Option<f32>,  // From semantic search
) -> bool {
    // Always include very recent memories (last 24 hours)
    let age = Utc::now().signed_duration_since(memory.timestamp);
    if age.num_hours() < 24 {
        return true;
    }
    
    // Include if highly relevant from vector search
    if let Some(relevance) = vector_relevance {
        if relevance > 0.75 {
            return true;
        }
    }
    
    // Include if still has decent salience after decay
    if decayed_salience > 4.0 {
        return true;
    }
    
    // Special case: always include promises unless they're ancient
    if matches!(&memory.memory_type, Some(MemoryType::Promise)) && age.num_days() < 365 {
        return true;
    }
    
    false
}
