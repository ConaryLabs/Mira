// src/persona/mod.rs
// Persona system for Mira's personality overlays.
// Currently only Default persona is implemented.
// Infrastructure preserved for future persona expansion if needed.

pub mod default;

pub use default::DEFAULT_PERSONA_PROMPT;

/// Persona overlays define different personality modes for Mira.
/// Currently only Default is implemented. Additional personas can be
/// added here if/when persona switching becomes a desired feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonaOverlay {
    Default,    // Primary persona - warm, grounded, sharp-tongued, loyal
    // Future personas would be added here as variants
    // Example: Professional, Creative, Technical, etc.
}

impl PersonaOverlay {
    /// Returns the system prompt for this persona overlay.
    /// Each persona defines Mira's complete personality and boundaries.
    pub fn prompt(&self) -> &'static str {
        match self {
            PersonaOverlay::Default => DEFAULT_PERSONA_PROMPT,
            // Future personas would map to their prompts here
        }
    }
}

impl std::fmt::Display for PersonaOverlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                PersonaOverlay::Default => "default",
                // Future personas would format here
            }
        )
    }
}

impl std::str::FromStr for PersonaOverlay {
    type Err = ();
    
    /// Parse a persona name from string.
    /// Used for potential future command-based switching like "/persona [name]"
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "default" => Ok(PersonaOverlay::Default),
            // Future personas would parse here
            _ => Err(()),
        }
    }
}
