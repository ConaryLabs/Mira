// src/persona/mod.rs

pub mod default;
pub mod forbidden;
pub mod hallow;
pub mod haven;

pub use default::DEFAULT_PERSONA_PROMPT;
pub use forbidden::FORBIDDEN_PERSONA_PROMPT;
pub use hallow::HALLOW_PERSONA_PROMPT;
pub use haven::HAVEN_PERSONA_PROMPT;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonaOverlay {
    Default,
    Forbidden,
    Hallow,
    Haven,
}

impl PersonaOverlay {
    pub fn prompt(&self) -> &'static str {
        match self {
            PersonaOverlay::Default => DEFAULT_PERSONA_PROMPT,
            PersonaOverlay::Forbidden => FORBIDDEN_PERSONA_PROMPT,
            PersonaOverlay::Hallow => HALLOW_PERSONA_PROMPT,
            PersonaOverlay::Haven => HAVEN_PERSONA_PROMPT,
        }
    }
    
    // Add methods needed by ChatService and ClaudeSystem
    pub fn name(&self) -> &str {
        match self {
            PersonaOverlay::Default => "default",
            PersonaOverlay::Forbidden => "forbidden",
            PersonaOverlay::Hallow => "hallow",
            PersonaOverlay::Haven => "haven",
        }
    }
    
    pub fn current_mood(&self) -> String {
        // Just return a neutral "present" for all - let the persona prompts handle mood
        "present".to_string()
    }
    
    pub fn temperature(&self) -> f32 {
        // Different personas have different temperatures for Claude
        match self {
            PersonaOverlay::Default => 0.7,
            PersonaOverlay::Forbidden => 0.9,
            PersonaOverlay::Hallow => 0.6,
            PersonaOverlay::Haven => 0.8,
        }
    }
    
    pub fn description(&self) -> &str {
        // Use the actual persona prompt for description
        self.prompt()
    }
}

impl std::fmt::Display for PersonaOverlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                PersonaOverlay::Default => "default",
                PersonaOverlay::Forbidden => "forbidden",
                PersonaOverlay::Hallow => "hallow",
                PersonaOverlay::Haven => "haven",
            }
        )
    }
}

impl std::str::FromStr for PersonaOverlay {
    type Err = ();
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "default" => Ok(PersonaOverlay::Default),
            "forbidden" => Ok(PersonaOverlay::Forbidden),
            "hallow" => Ok(PersonaOverlay::Hallow),
            "haven" => Ok(PersonaOverlay::Haven),
            _ => Err(()),
        }
    }
}
