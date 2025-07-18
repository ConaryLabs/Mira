// persona/mod.rs

pub mod default;
pub mod forbidden;
pub mod hallow;
pub mod haven;

// Utility: Get descriptor by name (optional, for manual overrides or UI)
pub fn get_persona_descriptor(name: &str) -> Option<&'static str> {
    match name.to_lowercase().as_str() {
        "default" => Some(default::DEFAULT_PERSONA_DESCRIPTOR),
        "forbidden" => Some(forbidden::FORBIDDEN_PERSONA_DESCRIPTOR),
        "hallow" => Some(hallow::HALLOW_PERSONA_DESCRIPTOR),
        "haven" => Some(haven::HAVEN_PERSONA_DESCRIPTOR),
        _ => None,
    }
}

// List of all persona descriptors (name, descriptor) — for prompt manifests, UI, or API
pub const ALL_PERSONAS: &[(&str, &str)] = &[
    ("Default", default::DEFAULT_PERSONA_DESCRIPTOR),
    ("Forbidden", forbidden::FORBIDDEN_PERSONA_DESCRIPTOR),
    ("Hallow", hallow::HALLOW_PERSONA_DESCRIPTOR),
    ("Haven", haven::HAVEN_PERSONA_DESCRIPTOR),
];

// For injection into your system prompt — just grab the current overlay or join them if desired
pub fn persona_prompt_block(persona: &str) -> &'static str {
    get_persona_descriptor(persona).unwrap_or(default::DEFAULT_PERSONA_DESCRIPTOR)
}

// (Optional) For admin or developer use: dump all personas as a single manifest
pub fn all_persona_blocks() -> String {
    ALL_PERSONAS
        .iter()
        .map(|(name, desc)| format!("---\n{name}\n\n{desc}\n"))
        .collect::<Vec<_>>()
        .join("\n")
}
