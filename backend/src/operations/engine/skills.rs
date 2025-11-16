// src/operations/engine/skills.rs
// Skills System - Meta-prompt injection for specialized tasks
// Inspired by Claude Code's skills architecture

use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use tokio::fs;
use tracing::{info, warn};

/// A skill represents a specialized capability with custom prompts and tool restrictions
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub prompt_template: String,
    pub preferred_model: PreferredModel,
    pub allowed_tools: Vec<String>, // Empty = all tools allowed
    pub requires_context: bool,
}

/// Which model should handle this skill
#[derive(Debug, Clone, PartialEq)]
pub enum PreferredModel {
    Gpt5,          // Orchestration-heavy tasks
    DeepSeek,      // Code generation tasks
    Either,        // No preference
}

impl Skill {
    /// Load a skill from a markdown file
    pub async fn load_from_file(skill_path: PathBuf) -> Result<Self> {
        let content = fs::read_to_string(&skill_path)
            .await
            .with_context(|| format!("Failed to read skill file: {}", skill_path.display()))?;

        let name = skill_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid skill file name"))?
            .to_string();

        // Parse frontmatter (basic YAML-like parsing)
        let (metadata, prompt_template) = Self::parse_markdown(&content)?;

        let description = metadata
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("No description")
            .to_string();

        let preferred_model = match metadata.get("model").and_then(|v| v.as_str()) {
            Some("gpt5") => PreferredModel::Gpt5,
            Some("deepseek") => PreferredModel::DeepSeek,
            _ => PreferredModel::Either,
        };

        let allowed_tools = metadata
            .get("allowed_tools")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let requires_context = metadata
            .get("requires_context")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        Ok(Self {
            name,
            description,
            prompt_template,
            preferred_model,
            allowed_tools,
            requires_context,
        })
    }

    /// Parse markdown with frontmatter
    fn parse_markdown(content: &str) -> Result<(HashMap<String, Value>, String)> {
        let mut metadata = HashMap::new();
        let mut prompt_lines = Vec::new();
        let mut in_frontmatter = false;
        let mut frontmatter_lines = Vec::new();

        for line in content.lines() {
            if line.trim() == "---" {
                if !in_frontmatter {
                    in_frontmatter = true;
                    continue;
                } else {
                    // End of frontmatter
                    in_frontmatter = false;
                    continue;
                }
            }

            if in_frontmatter {
                frontmatter_lines.push(line);
            } else {
                prompt_lines.push(line);
            }
        }

        // Parse frontmatter (simple key: value format)
        for line in frontmatter_lines {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim().to_string();
                let value_str = value.trim();

                // Parse value type
                let value = if value_str == "true" {
                    Value::Bool(true)
                } else if value_str == "false" {
                    Value::Bool(false)
                } else if value_str.starts_with('[') && value_str.ends_with(']') {
                    // Array
                    let items: Vec<Value> = value_str
                        .trim_matches(|c| c == '[' || c == ']')
                        .split(',')
                        .map(|s| Value::String(s.trim().to_string()))
                        .collect();
                    Value::Array(items)
                } else {
                    Value::String(value_str.to_string())
                };

                metadata.insert(key, value);
            }
        }

        let prompt = prompt_lines.join("\n").trim().to_string();

        Ok((metadata, prompt))
    }

    /// Build the complete prompt with skill injection
    pub fn build_prompt(&self, user_request: &str, context: Option<&str>) -> String {
        let mut prompt = self.prompt_template.clone();

        // Replace placeholders
        prompt = prompt.replace("{user_request}", user_request);

        if let Some(ctx) = context {
            prompt = prompt.replace("{context}", ctx);
        } else {
            prompt = prompt.replace("{context}", "");
        }

        prompt
    }

    /// Check if a tool is allowed for this skill
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        if self.allowed_tools.is_empty() {
            // Empty list = all tools allowed
            return true;
        }

        self.allowed_tools.iter().any(|t| t == tool_name)
    }
}

/// Manages loading and retrieval of skills
/// Thread-safe via RwLock for concurrent access
pub struct SkillRegistry {
    skills: RwLock<HashMap<String, Skill>>,
    skills_dir: PathBuf,
}

impl SkillRegistry {
    /// Create a new skill registry
    pub fn new(skills_dir: PathBuf) -> Self {
        Self {
            skills: RwLock::new(HashMap::new()),
            skills_dir,
        }
    }

    /// Load all skills from the skills directory
    pub async fn load_all(&self) -> Result<()> {
        info!("Loading skills from: {}", self.skills_dir.display());

        let mut loaded_skills = HashMap::new();

        let mut entries = fs::read_dir(&self.skills_dir)
            .await
            .with_context(|| format!("Failed to read skills directory: {}", self.skills_dir.display()))?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                match Skill::load_from_file(path.clone()).await {
                    Ok(skill) => {
                        info!("Loaded skill: {}", skill.name);
                        loaded_skills.insert(skill.name.clone(), skill);
                    }
                    Err(e) => {
                        warn!("Failed to load skill from {}: {}", path.display(), e);
                    }
                }
            }
        }

        // Replace the skills map with the loaded skills
        let mut skills = self.skills.write().unwrap();
        *skills = loaded_skills;
        info!("Loaded {} skills", skills.len());

        Ok(())
    }

    /// Get a skill by name (returns a clone since we're using RwLock)
    pub fn get(&self, name: &str) -> Option<Skill> {
        let skills = self.skills.read().unwrap();
        skills.get(name).cloned()
    }

    /// List all available skills
    pub fn list(&self) -> Vec<Skill> {
        let skills = self.skills.read().unwrap();
        skills.values().cloned().collect()
    }

    /// Get skill names for tool schema
    pub fn skill_names(&self) -> Vec<String> {
        let skills = self.skills.read().unwrap();
        skills.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontmatter_parsing() {
        let content = r#"---
description: Test skill
model: deepseek
allowed_tools: [generate_code, refactor_code]
requires_context: false
---

This is the skill prompt template.
User request: {user_request}
"#;

        let (metadata, prompt) = Skill::parse_markdown(content).unwrap();

        assert_eq!(metadata.get("description").unwrap().as_str().unwrap(), "Test skill");
        assert_eq!(metadata.get("model").unwrap().as_str().unwrap(), "deepseek");
        assert!(prompt.contains("skill prompt template"));
    }
}
