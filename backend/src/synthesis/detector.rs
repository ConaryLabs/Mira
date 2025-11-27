// src/synthesis/detector.rs
// LLM-based pattern detection for tool synthesis

use anyhow::{Context, Result};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::llm::provider::{Gpt5Provider, LlmProvider, Message};
use crate::prompt::internal::synthesis as prompts;
use crate::memory::features::code_intelligence::CodeIntelligenceService;

use super::storage::SynthesisStorage;
use super::types::*;

/// Configuration for pattern detection
#[derive(Debug, Clone)]
pub struct DetectorConfig {
    pub min_occurrences: i64,
    pub min_confidence: f64,
    pub max_patterns_per_run: usize,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            min_occurrences: 3,
            min_confidence: 0.7,
            max_patterns_per_run: 20,
        }
    }
}

/// LLM-based pattern detector
pub struct PatternDetector {
    llm: Gpt5Provider,
    code_intelligence: Arc<CodeIntelligenceService>,
    storage: Arc<SynthesisStorage>,
    config: DetectorConfig,
}

impl PatternDetector {
    pub fn new(
        llm: Gpt5Provider,
        code_intelligence: Arc<CodeIntelligenceService>,
        storage: Arc<SynthesisStorage>,
    ) -> Self {
        Self {
            llm,
            code_intelligence,
            storage,
            config: DetectorConfig::default(),
        }
    }

    pub fn with_config(mut self, config: DetectorConfig) -> Self {
        self.config = config;
        self
    }

    /// Detect patterns in a project
    pub async fn detect_patterns(&self, project_id: &str) -> Result<Vec<ToolPattern>> {
        info!("Detecting patterns in project: {}", project_id);

        // Gather context from code intelligence
        let context = self.gather_context(project_id).await?;

        if context.is_empty() {
            debug!("No context found for pattern detection");
            return Ok(Vec::new());
        }

        // Ask LLM to identify patterns
        let detected = self.call_llm_for_detection(project_id, &context).await?;

        // Filter by thresholds
        let filtered: Vec<ToolPattern> = detected
            .into_iter()
            .filter(|p| {
                p.detected_occurrences >= self.config.min_occurrences
                    && p.confidence_score >= self.config.min_confidence
            })
            .take(self.config.max_patterns_per_run)
            .collect();

        info!("Detected {} patterns above threshold", filtered.len());

        // Store patterns
        for pattern in &filtered {
            if let Err(e) = self.storage.store_pattern(pattern).await {
                warn!("Failed to store pattern {}: {}", pattern.pattern_name, e);
            }
        }

        Ok(filtered)
    }

    /// Gather context from code intelligence for pattern detection
    async fn gather_context(&self, project_id: &str) -> Result<String> {
        // Get code elements from the project using search with wildcard
        let elements = self
            .code_intelligence
            .search_elements_for_project("*", project_id, Some(100))
            .await
            .unwrap_or_default();

        if elements.is_empty() {
            debug!("No code elements found for project {}", project_id);
            return Ok(String::new());
        }

        // Format elements for LLM analysis
        let mut context = String::new();
        context.push_str("# Code Elements in Project\n\n");

        for element in elements.iter().take(100) {
            context.push_str(&format!(
                "## {} ({})\n- File: {}\n- Lines: {}-{}\n- Visibility: {}\n\n",
                element.name,
                element.element_type,
                element.full_path,
                element.start_line,
                element.end_line,
                element.visibility,
            ));
        }

        Ok(context)
    }

    /// Call LLM to detect patterns
    async fn call_llm_for_detection(
        &self,
        project_id: &str,
        context: &str,
    ) -> Result<Vec<ToolPattern>> {
        let system_prompt = prompts::PATTERN_DETECTOR;

        let user_prompt = format!(
            "Analyze the following code elements and identify automatable patterns:\n\n{}",
            context
        );

        let messages = vec![Message::user(user_prompt)];

        let response = self
            .llm
            .chat(messages, system_prompt.to_string())
            .await
            .context("LLM pattern detection failed")?;

        // Parse LLM response
        self.parse_detection_response(project_id, &response.content)
    }

    /// Parse LLM detection response into patterns
    fn parse_detection_response(
        &self,
        project_id: &str,
        content: &str,
    ) -> Result<Vec<ToolPattern>> {
        // Extract JSON from response (handle markdown code blocks)
        let json_content = if content.contains("```json") {
            content
                .split("```json")
                .nth(1)
                .and_then(|s| s.split("```").next())
                .unwrap_or(content)
        } else if content.contains("```") {
            content
                .split("```")
                .nth(1)
                .and_then(|s| s.split("```").next())
                .unwrap_or(content)
        } else {
            content
        };

        let parsed: DetectionResponse = serde_json::from_str(json_content.trim())
            .context("Failed to parse LLM detection response")?;

        let patterns = parsed
            .patterns
            .into_iter()
            .map(|p| {
                let locations: Vec<PatternLocation> = p
                    .example_files
                    .unwrap_or_default()
                    .into_iter()
                    .map(|f| PatternLocation {
                        file_path: f,
                        start_line: 0,
                        end_line: 0,
                        symbol_name: None,
                    })
                    .collect();

                let mut pattern = ToolPattern::new(
                    project_id.to_string(),
                    p.name,
                    PatternType::from_str(&p.pattern_type),
                    p.description,
                );
                pattern.detected_occurrences = p.frequency;
                pattern.confidence_score = p.confidence;
                pattern.example_locations = locations;
                pattern.should_synthesize = p.confidence >= self.config.min_confidence;
                pattern
            })
            .collect();

        Ok(patterns)
    }
}

/// LLM response structure for pattern detection
#[derive(Debug, Deserialize)]
struct DetectionResponse {
    patterns: Vec<DetectedPattern>,
}

#[derive(Debug, Deserialize)]
struct DetectedPattern {
    name: String,
    #[serde(rename = "type")]
    pattern_type: String,
    description: String,
    frequency: i64,
    confidence: f64,
    example_files: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_type_conversion() {
        assert_eq!(PatternType::from_str("file_operation"), PatternType::FileOperation);
        assert_eq!(PatternType::from_str("api_call"), PatternType::ApiCall);
        assert_eq!(PatternType::from_str("unknown").as_str(), "unknown");
    }

    #[test]
    fn test_parse_detection_response() {
        let json = r#"{
            "patterns": [
                {
                    "name": "http_client",
                    "type": "api_call",
                    "description": "HTTP client wrapper",
                    "frequency": 5,
                    "confidence": 0.9,
                    "example_files": ["api.rs", "client.rs"]
                }
            ]
        }"#;

        let parsed: DetectionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.patterns.len(), 1);
        assert_eq!(parsed.patterns[0].name, "http_client");
    }
}
