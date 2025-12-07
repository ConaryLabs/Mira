// src/testing/scenarios/parser.rs
// YAML scenario parser

use anyhow::{Context, Result};
use std::path::Path;
use tracing::info;

use super::types::TestScenario;

/// Parser for test scenario files
pub struct ScenarioParser;

impl ScenarioParser {
    /// Parse a single scenario file
    pub fn parse_file(path: &Path) -> Result<TestScenario> {
        info!("Parsing scenario file: {}", path.display());

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read scenario file: {}", path.display()))?;

        Self::parse_yaml(&content)
            .with_context(|| format!("Failed to parse scenario file: {}", path.display()))
    }

    /// Parse YAML content into a scenario
    pub fn parse_yaml(content: &str) -> Result<TestScenario> {
        // Handle template variables like {{uuid}}
        let processed = Self::process_templates(content);

        let scenario: TestScenario = serde_yaml::from_str(&processed)
            .context("Failed to parse YAML")?;

        Self::validate_scenario(&scenario)?;

        Ok(scenario)
    }

    /// Process template variables in YAML content
    fn process_templates(content: &str) -> String {
        let mut result = content.to_string();

        // Replace {{uuid}} with a new UUID
        while result.contains("{{uuid}}") {
            let uuid = uuid::Uuid::new_v4().to_string();
            result = result.replacen("{{uuid}}", &uuid, 1);
        }

        // Replace {{timestamp}} with current timestamp
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
        result = result.replace("{{timestamp}}", &timestamp);

        // Replace {{temp_dir}} with system temp dir
        if let Some(temp_dir) = std::env::temp_dir().to_str() {
            result = result.replace("{{temp_dir}}", temp_dir);
        }

        result
    }

    /// Validate a parsed scenario
    fn validate_scenario(scenario: &TestScenario) -> Result<()> {
        if scenario.name.is_empty() {
            anyhow::bail!("Scenario name cannot be empty");
        }

        if scenario.steps.is_empty() {
            anyhow::bail!("Scenario must have at least one step");
        }

        for (i, step) in scenario.steps.iter().enumerate() {
            if step.name.is_empty() {
                anyhow::bail!("Step {} has empty name", i + 1);
            }
            if step.prompt.is_empty() && !step.skip {
                anyhow::bail!("Step '{}' has empty prompt", step.name);
            }
        }

        Ok(())
    }

    /// Parse all scenario files in a directory
    pub fn parse_directory(dir: &Path) -> Result<Vec<TestScenario>> {
        info!("Parsing scenarios from directory: {}", dir.display());

        let mut scenarios = Vec::new();

        for entry in std::fs::read_dir(dir)
            .with_context(|| format!("Failed to read directory: {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();

            // Only process .yaml and .yml files
            if let Some(ext) = path.extension() {
                if ext == "yaml" || ext == "yml" {
                    match Self::parse_file(&path) {
                        Ok(scenario) => {
                            info!("Loaded scenario: {}", scenario.name);
                            scenarios.push(scenario);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse {}: {}", path.display(), e);
                        }
                    }
                }
            }
        }

        scenarios.sort_by(|a, b| a.name.cmp(&b.name));

        info!("Loaded {} scenarios", scenarios.len());
        Ok(scenarios)
    }

    /// Filter scenarios by tags
    pub fn filter_by_tags(scenarios: Vec<TestScenario>, tags: &[String]) -> Vec<TestScenario> {
        if tags.is_empty() {
            return scenarios;
        }

        scenarios
            .into_iter()
            .filter(|s| tags.iter().any(|tag| s.tags.contains(tag)))
            .collect()
    }

    /// Filter scenarios by name pattern
    pub fn filter_by_name(scenarios: Vec<TestScenario>, pattern: &str) -> Vec<TestScenario> {
        if pattern.is_empty() || pattern == "*" {
            return scenarios;
        }

        let pattern_lower = pattern.to_lowercase();
        scenarios
            .into_iter()
            .filter(|s| s.name.to_lowercase().contains(&pattern_lower))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_scenario() {
        let yaml = r#"
name: "Simple Test"
description: "A simple test scenario"
tags: ["smoke", "basic"]
steps:
  - name: "Say hello"
    prompt: "Hello, how are you?"
    assertions:
      - type: completed_successfully
"#;

        let scenario = ScenarioParser::parse_yaml(yaml).unwrap();
        assert_eq!(scenario.name, "Simple Test");
        assert_eq!(scenario.tags, vec!["smoke", "basic"]);
        assert_eq!(scenario.steps.len(), 1);
        assert_eq!(scenario.steps[0].name, "Say hello");
    }

    #[test]
    fn test_parse_with_setup() {
        let yaml = r#"
name: "File Test"
setup:
  create_files:
    - path: "test.txt"
      content: "Hello World"
steps:
  - name: "Read file"
    prompt: "Read the file test.txt"
    assertions:
      - type: response_contains
        text: "Hello World"
cleanup:
  remove_project: true
"#;

        let scenario = ScenarioParser::parse_yaml(yaml).unwrap();
        assert_eq!(scenario.setup.create_files.len(), 1);
        assert!(scenario.cleanup.remove_project);
    }

    #[test]
    fn test_parse_with_expected_events() {
        let yaml = r#"
name: "Event Test"
steps:
  - name: "Write file"
    prompt: "Create a file called output.txt"
    expect_events:
      - type: operation.started
      - type: operation.tool_executed
        tool_name: write_project_file
        success: true
      - type: operation.completed
"#;

        let scenario = ScenarioParser::parse_yaml(yaml).unwrap();
        assert_eq!(scenario.steps[0].expect_events.len(), 3);
        assert_eq!(scenario.steps[0].expect_events[1].event_type, "operation.tool_executed");
    }

    #[test]
    fn test_template_processing() {
        let content = "path: /tmp/test-{{uuid}}/data";
        let processed = ScenarioParser::process_templates(content);

        // Should have replaced {{uuid}} with something
        assert!(!processed.contains("{{uuid}}"));
        assert!(processed.starts_with("path: /tmp/test-"));
    }

    #[test]
    fn test_validation_empty_name() {
        let yaml = r#"
name: ""
steps:
  - name: "Test"
    prompt: "Hello"
"#;

        let result = ScenarioParser::parse_yaml(yaml);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("name cannot be empty"));
    }

    #[test]
    fn test_validation_no_steps() {
        let yaml = r#"
name: "Test"
steps: []
"#;

        let result = ScenarioParser::parse_yaml(yaml);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("at least one step"));
    }
}
