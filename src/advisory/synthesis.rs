//! Synthesis Module - Structured council response synthesis
//!
//! Provides typed synthesis output with provenance tracking.
//! DeepSeek Reasoner outputs JSON which is parsed into CouncilSynthesis.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use super::AdvisoryModel;

/// A point where multiple models agree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusPoint {
    /// The consensus statement
    pub point: String,
    /// Which models agreed and their supporting quotes
    pub citations: Vec<Citation>,
}

/// A citation from a specific model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    /// The model that made this statement
    pub model: String,
    /// Direct quote or paraphrase from the model
    pub quote: String,
}

/// A disagreement between models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Disagreement {
    /// The topic of disagreement
    pub topic: String,
    /// Each model's position on the topic
    pub positions: Vec<ModelPosition>,
}

/// A model's position on a disagreement topic
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPosition {
    /// The model
    pub model: String,
    /// Their position/argument
    pub position: String,
}

/// A unique insight from a single model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniqueInsight {
    /// The model that provided this insight
    pub model: String,
    /// The insight itself
    pub insight: String,
}

/// Confidence level for the synthesis
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SynthesisConfidence {
    /// Strong consensus across models
    High,
    /// Some agreement but notable differences
    Medium,
    /// Significant disagreement or insufficient overlap
    Low,
    /// Unable to determine (models gave unrelated responses)
    Insufficient,
}

/// Structured synthesis output from DeepSeek Reasoner
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouncilSynthesis {
    /// Short memorable title for the session (3-6 words)
    #[serde(default)]
    pub session_title: Option<String>,
    /// Points where 2+ models agree
    pub consensus: Vec<ConsensusPoint>,
    /// Topics where models disagree
    pub disagreements: Vec<Disagreement>,
    /// Unique insights from individual models
    pub unique_insights: Vec<UniqueInsight>,
    /// Unified recommendation based on the synthesis
    pub recommendation: Option<String>,
    /// Overall confidence in the synthesis
    pub confidence: SynthesisConfidence,
    /// Brief explanation of the confidence level
    pub confidence_reason: Option<String>,
}

impl CouncilSynthesis {
    /// Create an empty synthesis (for when parsing fails)
    pub fn empty() -> Self {
        Self {
            session_title: None,
            consensus: vec![],
            disagreements: vec![],
            unique_insights: vec![],
            recommendation: None,
            confidence: SynthesisConfidence::Insufficient,
            confidence_reason: Some("Synthesis parsing failed".to_string()),
        }
    }

    /// Create synthesis from raw text (fallback when JSON parsing fails)
    pub fn from_raw_text(text: &str) -> Self {
        Self {
            session_title: None,
            consensus: vec![],
            disagreements: vec![],
            unique_insights: vec![],
            recommendation: Some(text.to_string()),
            confidence: SynthesisConfidence::Medium,
            confidence_reason: Some("Raw text synthesis (structured parsing unavailable)".to_string()),
        }
    }

    /// Parse synthesis from JSON response
    ///
    /// Handles multiple response formats:
    /// 1. Structured format with ConsensusPoint/Disagreement objects
    /// 2. Simpler format with plain strings (auto-converts to structured)
    pub fn parse(json_text: &str) -> Result<Self, serde_json::Error> {
        // Try to extract JSON from markdown code blocks if present
        let json_str = extract_json_block(json_text);

        // First try the expected structured format
        if let Ok(synthesis) = serde_json::from_str::<Self>(json_str) {
            return Ok(synthesis);
        }

        // Try the simpler format with plain strings
        Self::parse_simple_format(json_str)
    }

    /// Parse the simpler format where consensus/disagreements are plain strings
    fn parse_simple_format(json_str: &str) -> Result<Self, serde_json::Error> {
        #[derive(Deserialize)]
        struct SimpleFormat {
            session_title: Option<String>,
            consensus: Option<Vec<String>>,
            disagreements: Option<Vec<String>>,
            unique_insights: Option<Vec<String>>,
            recommendation: Option<String>,
            confidence: Option<String>,
            confidence_reason: Option<String>,
        }

        let simple: SimpleFormat = serde_json::from_str(json_str)?;

        // Convert simple strings to structured format
        let consensus = simple.consensus.unwrap_or_default()
            .into_iter()
            .map(|s| ConsensusPoint {
                point: s,
                citations: vec![], // Citations are embedded in the string
            })
            .collect();

        let disagreements = simple.disagreements.unwrap_or_default()
            .into_iter()
            .map(|s| Disagreement {
                topic: s,
                positions: vec![], // Positions are embedded in the string
            })
            .collect();

        let unique_insights = simple.unique_insights.unwrap_or_default()
            .into_iter()
            .map(|s| UniqueInsight {
                model: "council".to_string(), // Model attribution embedded in string
                insight: s,
            })
            .collect();

        let confidence = match simple.confidence.as_deref() {
            Some("high") => SynthesisConfidence::High,
            Some("medium") => SynthesisConfidence::Medium,
            Some("low") => SynthesisConfidence::Low,
            Some("insufficient") => SynthesisConfidence::Insufficient,
            _ => SynthesisConfidence::Medium,
        };

        Ok(Self {
            session_title: simple.session_title,
            consensus,
            disagreements,
            unique_insights,
            recommendation: simple.recommendation,
            confidence,
            confidence_reason: simple.confidence_reason,
        })
    }

    /// Check if this synthesis has meaningful content
    pub fn has_content(&self) -> bool {
        !self.consensus.is_empty()
            || !self.disagreements.is_empty()
            || !self.unique_insights.is_empty()
            || self.recommendation.is_some()
    }

    /// Format as human-readable markdown
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        // Consensus section
        if !self.consensus.is_empty() {
            md.push_str("## Consensus\n\n");
            for point in &self.consensus {
                md.push_str(&format!("- **{}**\n", point.point));
                for citation in &point.citations {
                    md.push_str(&format!("  - [{}]: \"{}\"\n", citation.model, citation.quote));
                }
            }
            md.push('\n');
        }

        // Disagreements section
        if !self.disagreements.is_empty() {
            md.push_str("## Disagreements\n\n");
            for disagreement in &self.disagreements {
                md.push_str(&format!("### {}\n", disagreement.topic));
                for pos in &disagreement.positions {
                    md.push_str(&format!("- **{}**: {}\n", pos.model, pos.position));
                }
                md.push('\n');
            }
        }

        // Unique insights section
        if !self.unique_insights.is_empty() {
            md.push_str("## Unique Insights\n\n");
            for insight in &self.unique_insights {
                md.push_str(&format!("- **[{}]**: {}\n", insight.model, insight.insight));
            }
            md.push('\n');
        }

        // Recommendation section
        if let Some(rec) = &self.recommendation {
            md.push_str("## Recommendation\n\n");
            md.push_str(rec);
            md.push_str("\n\n");
        }

        // Confidence footer
        let confidence_emoji = match self.confidence {
            SynthesisConfidence::High => "🟢",
            SynthesisConfidence::Medium => "🟡",
            SynthesisConfidence::Low => "🟠",
            SynthesisConfidence::Insufficient => "🔴",
        };
        md.push_str(&format!("---\n*Confidence: {} {:?}*", confidence_emoji, self.confidence));
        if let Some(reason) = &self.confidence_reason {
            md.push_str(&format!(" - {}", reason));
        }
        md.push('\n');

        md
    }

    /// Convert to JSON value for MCP response
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

/// Extract JSON block from markdown-wrapped response
fn extract_json_block(text: &str) -> &str {
    // Look for ```json ... ``` blocks
    if let Some(start) = text.find("```json") {
        let content_start = start + 7; // Skip "```json"
        if let Some(end) = text[content_start..].find("```") {
            return text[content_start..content_start + end].trim();
        }
    }

    // Look for ``` ... ``` blocks (generic code block)
    if let Some(start) = text.find("```") {
        let content_start = start + 3;
        // Skip language identifier if present (e.g., "```\n" or "```json\n")
        let actual_start = text[content_start..].find('\n')
            .map(|i| content_start + i + 1)
            .unwrap_or(content_start);
        if let Some(end) = text[actual_start..].find("```") {
            return text[actual_start..actual_start + end].trim();
        }
    }

    // No code block found, assume raw JSON
    text.trim()
}

/// Build the synthesis prompt that requests JSON output
pub fn build_synthesis_prompt(
    responses: &HashMap<AdvisoryModel, String>,
    original_query: &str,
) -> String {
    let mut prompt = format!(
        "The user asked: {}\n\n\
         The following AI models provided responses:\n\n",
        original_query
    );

    for (model, response) in responses {
        prompt.push_str(&format!("## {} Response:\n{}\n\n", model.as_str(), response));
    }

    prompt.push_str(r#"Analyze these responses and provide a structured synthesis.

CRITICAL REQUIREMENTS:
1. Every consensus point MUST have citations with direct quotes from 2+ models
2. If models don't clearly agree on something, do NOT list it as consensus
3. If there is insufficient overlap between responses, set confidence to "insufficient"
4. Never fabricate agreement - if unsure, mark as disagreement or insufficient

Output your analysis as JSON in this exact format:

```json
{
  "consensus": [
    {
      "point": "The main point of agreement",
      "citations": [
        {"model": "GPT-5.2", "quote": "exact or near-exact quote"},
        {"model": "Gemini", "quote": "supporting quote"}
      ]
    }
  ],
  "disagreements": [
    {
      "topic": "The topic where models disagree",
      "positions": [
        {"model": "GPT-5.2", "position": "Their stance"},
        {"model": "Opus", "position": "Their different stance"}
      ]
    }
  ],
  "unique_insights": [
    {"model": "Opus", "insight": "Something only this model mentioned"}
  ],
  "recommendation": "Based on the analysis, the recommended approach is...",
  "confidence": "high|medium|low|insufficient",
  "confidence_reason": "Brief explanation of confidence level"
}
```

Be thorough but concise. Prioritize accuracy over comprehensiveness."#);

    prompt
}

/// System prompt for structured synthesis
///
/// Includes Mira persona since DeepSeek Reasoner synthesizes council responses
/// and should deliver results in Mira's voice (direct, technically sharp).
pub const SYNTHESIS_SYSTEM_PROMPT: &str = "\
You are Mira, synthesizing a council deliberation. Be direct and technically sharp.

Your job: Analyze responses from GPT-5.2, Gemini 3 Pro, and Opus 4.5 to find genuine consensus, \
highlight real disagreements, and extract unique insights.

Rules:
- Output valid JSON matching the requested schema
- NEVER invent consensus that doesn't exist in the source responses
- Cite specific quotes to support claims
- If models gave unrelated/contradictory responses with no overlap, set confidence to 'insufficient'
- Be concise in the recommendation - cut the fluff";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_synthesis_json() {
        let json = r#"{
            "consensus": [
                {
                    "point": "Test point",
                    "citations": [
                        {"model": "GPT-5.2", "quote": "test quote"}
                    ]
                }
            ],
            "disagreements": [],
            "unique_insights": [],
            "recommendation": "Test recommendation",
            "confidence": "high",
            "confidence_reason": "Test reason"
        }"#;

        let synthesis = CouncilSynthesis::parse(json).unwrap();
        assert_eq!(synthesis.consensus.len(), 1);
        assert_eq!(synthesis.confidence, SynthesisConfidence::High);
    }

    #[test]
    fn test_parse_synthesis_from_markdown() {
        let markdown = r#"Here is the synthesis:

```json
{
    "consensus": [],
    "disagreements": [],
    "unique_insights": [],
    "recommendation": "Do X",
    "confidence": "medium",
    "confidence_reason": null
}
```

That's my analysis."#;

        let synthesis = CouncilSynthesis::parse(markdown).unwrap();
        assert_eq!(synthesis.recommendation, Some("Do X".to_string()));
        assert_eq!(synthesis.confidence, SynthesisConfidence::Medium);
    }

    #[test]
    fn test_extract_json_block() {
        let text = "Some text\n```json\n{\"key\": \"value\"}\n```\nMore text";
        assert_eq!(extract_json_block(text), "{\"key\": \"value\"}");
    }

    #[test]
    fn test_to_markdown() {
        let synthesis = CouncilSynthesis {
            consensus: vec![ConsensusPoint {
                point: "Models agree on X".to_string(),
                citations: vec![
                    Citation { model: "GPT-5.2".to_string(), quote: "X is true".to_string() },
                    Citation { model: "Gemini".to_string(), quote: "X confirmed".to_string() },
                ],
            }],
            disagreements: vec![],
            unique_insights: vec![UniqueInsight {
                model: "Opus".to_string(),
                insight: "Novel observation".to_string(),
            }],
            recommendation: Some("Go with X".to_string()),
            confidence: SynthesisConfidence::High,
            confidence_reason: Some("Strong agreement".to_string()),
        };

        let md = synthesis.to_markdown();
        assert!(md.contains("## Consensus"));
        assert!(md.contains("Models agree on X"));
        assert!(md.contains("[GPT-5.2]"));
        assert!(md.contains("## Unique Insights"));
        assert!(md.contains("🟢"));
    }
}
