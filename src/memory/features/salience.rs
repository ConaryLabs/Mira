//  src/memory/features/salience.rs

//! Handles LLM-driven salience scoring for memories.
//! Salience is extracted by LLM (0.0-1.0 scale); this module can normalize, update, or reprocess as needed.

use crate::memory::core::types::MemoryEntry;

/// Normalize salience to 0.0–1.0 range (defensive, in case LLM screws up).
/// FIXED: Changed from 1.0-10.0 to 0.0-1.0 scale to match LLM output
pub fn normalize_salience(raw: f32) -> f32 {
    raw.clamp(0.0, 1.0)
}

/// Optionally, re-score an entry (could call LLM again or adjust with decay).
pub fn rescore_salience(entry: &MemoryEntry, decay: Option<f32>) -> f32 {
    let base = entry.salience.unwrap_or(0.5);  // FIXED: 0.5 instead of 5.0
    if let Some(decay_factor) = decay {
        // Simple exponential decay—tweak as needed.
        (base * decay_factor).max(0.0)  // FIXED: max(0.0) instead of max(1.0)
    } else {
        base
    }
}
