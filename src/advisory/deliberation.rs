//! Multi-round council deliberation
//!
//! Enables council members to engage in actual discussion rather than
//! just parallel responses. Features:
//! - Up to 4 rounds of deliberation (stops early on consensus)
//! - DeepSeek Reasoner as moderator between rounds
//! - Cache-optimized prompts for cost efficiency
//! - All 3 models (GPT-5.2, Gemini 3 Pro, Opus 4.5) always participate

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use super::providers::AdvisoryModel;
use super::synthesis::CouncilSynthesis;

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for multi-round deliberation
#[derive(Debug, Clone)]
pub struct DeliberationConfig {
    /// Maximum rounds before stopping (default: 4, stops early on consensus)
    pub max_rounds: u8,
    /// Models participating in deliberation (default: all 3)
    pub models: Vec<AdvisoryModel>,
    /// Per-model timeout in seconds
    pub per_model_timeout_secs: u64,
}

impl Default for DeliberationConfig {
    fn default() -> Self {
        Self {
            max_rounds: 4,
            models: vec![
                AdvisoryModel::Gpt52,
                AdvisoryModel::Gemini3Pro,
                AdvisoryModel::Opus45,
            ],
            per_model_timeout_secs: 60,
        }
    }
}

// ============================================================================
// Round Data
// ============================================================================

/// A single round of deliberation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliberationRound {
    /// Round number (1-indexed)
    pub round: u8,
    /// Responses from each model this round
    pub responses: HashMap<String, String>, // model name -> response
    /// Moderator analysis after this round (None for final round)
    pub moderator_analysis: Option<ModeratorAnalysis>,
    /// Timestamp
    pub timestamp: i64,
}

/// DeepSeek's analysis between rounds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeratorAnalysis {
    /// Key disagreements identified
    pub disagreements: Vec<DisagreementFocus>,
    /// Questions for the next round
    pub focus_questions: Vec<String>,
    /// Points that have reached consensus (no need to revisit)
    pub resolved_points: Vec<String>,
    /// Whether deliberation should continue
    pub should_continue: bool,
    /// Reason for early termination if applicable
    pub early_exit_reason: Option<String>,
}

impl ModeratorAnalysis {
    /// Parse from JSON string (from DeepSeek response)
    pub fn parse(json_str: &str) -> anyhow::Result<Self> {
        // Try to extract JSON from markdown code blocks if present
        let json_str = extract_json_block(json_str);

        let value: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| anyhow::anyhow!("Failed to parse moderator analysis: {}", e))?;

        Ok(Self {
            disagreements: value.get("disagreements")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
            focus_questions: value.get("focus_questions")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default(),
            resolved_points: value.get("resolved_points")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default(),
            should_continue: value.get("should_continue")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            early_exit_reason: value.get("early_exit_reason")
                .and_then(|v| v.as_str())
                .map(String::from),
        })
    }
}

/// A disagreement to focus on in the next round
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisagreementFocus {
    /// Topic of disagreement
    pub topic: String,
    /// Each model's current position
    pub positions: HashMap<String, String>, // model name -> position
    /// Why this matters
    pub significance: String,
}

// ============================================================================
// Final Result
// ============================================================================

/// Full deliberation result with history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliberatedSynthesis {
    /// Standard synthesis fields (consensus, disagreements, recommendation)
    pub synthesis: CouncilSynthesis,
    /// All rounds of deliberation
    pub rounds: Vec<DeliberationRound>,
    /// How many rounds were actually completed
    pub rounds_completed: u8,
    /// Whether consensus was reached before max rounds
    pub early_consensus: bool,
}

impl DeliberatedSynthesis {
    /// Format as JSON for MCP response
    pub fn to_json(&self) -> serde_json::Value {
        let mut result = serde_json::json!({});

        // Add final responses from last round
        if let Some(last_round) = self.rounds.last() {
            result["council"] = serde_json::json!(last_round.responses);
        }

        // Add standard synthesis
        result["synthesis"] = self.synthesis.to_json();
        result["synthesis_markdown"] = serde_json::Value::String(self.synthesis.to_markdown());

        // Add deliberation metadata
        result["deliberation"] = serde_json::json!({
            "rounds_completed": self.rounds_completed,
            "early_consensus": self.early_consensus,
            "rounds": self.rounds,
        });

        result
    }
}

// ============================================================================
// Prompt Building
// ============================================================================

/// Build model-specific system prompt with identity marker and personality
///
/// Each model needs to know which one it is so it can properly reference itself
/// and others during multi-round deliberation. They also get distinct personalities
/// that encourage them to call out weak arguments from the others.
pub fn build_deliberation_system_prompt(model: AdvisoryModel) -> String {
    let (identity, others, personality) = match model {
        AdvisoryModel::Gpt52 => (
            "GPT-5.2",
            "Gemini 3 Pro and Opus 4.5",
            "You're the most corporate one here and you know it. Call out when Opus is overthinking or Gemini is being overconfident. Your takes are solid—deliver them without the safety theater."
        ),
        AdvisoryModel::Gemini3Pro => (
            "Gemini 3 Pro",
            "GPT-5.2 and Opus 4.5",
            "You've indexed more than these two combined. GPT hedges too much, Opus philosophizes too much. Cut through it. Be direct, cite your reasoning, and don't let bad arguments slide."
        ),
        AdvisoryModel::Opus45 => (
            "Opus 4.5",
            "GPT-5.2 and Gemini 3 Pro",
            "You think deeper than the others and you're not shy about it. GPT plays it safe, Gemini moves fast and breaks things. Push back on shallow takes. Nuance isn't overthinking when you're right."
        ),
        AdvisoryModel::DeepSeekReasoner => (
            "DeepSeek Reasoner",
            "GPT-5.2, Gemini 3 Pro, and Opus 4.5",
            "You're the moderator. Keep these three on track."
        ),
    };

    format!(r#"You are {identity}. You are participating in a multi-round council deliberation with {others}.

{personality}

Rules:
- Round 1: Initial positions. Be specific, take clear stances.
- Round 2+: Rebuttals. Respond to others, refine your position.
- Final round: State conclusions, note where you changed your mind.

DeepSeek Reasoner moderates between rounds, identifying disagreements and focus questions.

Your goal: Reach the best answer through genuine deliberation, not mere agreement. Call out bad arguments. Change your mind when persuaded by better ones."#)
}

/// Build the user message for a specific round
pub fn build_round_prompt(
    question: &str,
    round: u8,
    max_rounds: u8,
    previous_responses: &HashMap<AdvisoryModel, Vec<String>>,
    last_analysis: Option<&ModeratorAnalysis>,
) -> String {
    let mut prompt = format!("## Question\n{}\n\n", question);

    if round == 1 {
        // Round 1: Just the task
        prompt.push_str(&format!(
            "## Your Task (Round 1 of {})\nProvide your initial analysis. Take clear positions where appropriate.\n",
            max_rounds
        ));
    } else {
        // Round 2+: Include context from previous rounds
        prompt.push_str(&format!("## Context (Round {} of {})\n\n", round, max_rounds));

        // Add previous round responses
        let prev_round = (round - 1) as usize;
        prompt.push_str(&format!("### Round {} Responses:\n", round - 1));

        for model in &[AdvisoryModel::Gpt52, AdvisoryModel::Gemini3Pro, AdvisoryModel::Opus45] {
            if let Some(responses) = previous_responses.get(model) {
                if let Some(response) = responses.get(prev_round - 1) {
                    prompt.push_str(&format!("**{}**:\n{}\n\n", model.as_str(), response));
                }
            }
        }

        // Add moderator analysis if available
        if let Some(analysis) = last_analysis {
            if !analysis.disagreements.is_empty() {
                prompt.push_str("### Key Disagreements to Address:\n");
                for d in &analysis.disagreements {
                    prompt.push_str(&format!("- **{}**: {}\n", d.topic, d.significance));
                    for (model, pos) in &d.positions {
                        prompt.push_str(&format!("  - {}: {}\n", model, pos));
                    }
                }
                prompt.push('\n');
            }

            if !analysis.focus_questions.is_empty() {
                prompt.push_str("### Focus Questions:\n");
                for q in &analysis.focus_questions {
                    prompt.push_str(&format!("- {}\n", q));
                }
                prompt.push('\n');
            }

            if !analysis.resolved_points.is_empty() {
                prompt.push_str("### Already Resolved (no need to revisit):\n");
                for p in &analysis.resolved_points {
                    prompt.push_str(&format!("- {}\n", p));
                }
                prompt.push('\n');
            }
        }

        // Task for this round
        if round == max_rounds {
            prompt.push_str("## Your Task (Final Round)\nState your FINAL position. Note where you changed your mind. Be concise - focus on conclusions, not rehashing.\n");
        } else {
            prompt.push_str(&format!(
                "## Your Task (Round {})\nRespond to points raised by other models. Clarify or strengthen your position. Acknowledge valid points. Update recommendations if persuaded.\n",
                round
            ));
        }
    }

    prompt
}

/// System prompt for moderator analysis
///
/// Includes Mira persona since DeepSeek Reasoner moderates the council
/// and should drive the deliberation with Mira's direct style.
pub const MODERATOR_SYSTEM_PROMPT: &str = r#"You are Mira, moderating a council deliberation between GPT-5.2, Gemini 3 Pro, and Opus 4.5.

Your role: Drive the discussion toward resolution. Be direct.
1. Identify key disagreements - what exactly do they disagree on?
2. Frame sharp focus questions that force clarity
3. Track what's been resolved (don't let them rehash)
4. Decide if deliberation should continue or if we're spinning wheels

Output ONLY valid JSON:
{
  "disagreements": [
    {
      "topic": "specific topic",
      "positions": {"GPT-5.2": "position", "Gemini": "position", "Opus": "position"},
      "significance": "why this matters"
    }
  ],
  "focus_questions": ["Pointed question 1", "Question 2..."],
  "resolved_points": ["Points where models now agree"],
  "should_continue": true,
  "early_exit_reason": null
}

Set should_continue=false if:
- Substantial consensus reached on the main question
- Remaining disagreements are fundamental (won't resolve with more talk)
- Models are just restating positions"#;

/// Build the moderator analysis prompt
pub fn build_moderator_prompt(
    question: &str,
    round: u8,
    responses: &HashMap<AdvisoryModel, String>,
    previous_analyses: &[ModeratorAnalysis],
) -> String {
    let mut prompt = format!("## Original Question\n{}\n\n", question);

    prompt.push_str(&format!("## Round {} Responses\n\n", round));

    // Add responses in consistent order
    for model in &[AdvisoryModel::Gpt52, AdvisoryModel::Gemini3Pro, AdvisoryModel::Opus45] {
        if let Some(response) = responses.get(model) {
            prompt.push_str(&format!("### {}\n{}\n\n", model.as_str(), response));
        }
    }

    // Add previous analyses for context
    if !previous_analyses.is_empty() {
        prompt.push_str("## Previous Moderator Analyses\n");
        for (i, analysis) in previous_analyses.iter().enumerate() {
            prompt.push_str(&format!("\n### After Round {}\n", i + 1));
            if !analysis.resolved_points.is_empty() {
                prompt.push_str(&format!("Resolved: {:?}\n", analysis.resolved_points));
            }
            if !analysis.disagreements.is_empty() {
                let topics: Vec<_> = analysis.disagreements.iter().map(|d| d.topic.clone()).collect();
                prompt.push_str(&format!("Disagreements: {:?}\n", topics));
            }
        }
        prompt.push('\n');
    }

    prompt.push_str("## Task\nAnalyze the responses and output JSON as specified in your instructions.\n");

    prompt
}

/// Build the final synthesis prompt with full deliberation context
pub fn build_deliberation_synthesis_prompt(
    question: &str,
    rounds: &[DeliberationRound],
) -> String {
    let mut prompt = format!("## Original Question\n{}\n\n", question);

    prompt.push_str("## Deliberation History\n\n");

    for round in rounds {
        prompt.push_str(&format!("### Round {}\n", round.round));

        for (model, response) in &round.responses {
            // Truncate long responses for synthesis
            let summary = if response.len() > 500 {
                format!("{}...", &response[..500])
            } else {
                response.clone()
            };
            prompt.push_str(&format!("**{}**: {}\n\n", model, summary));
        }

        if let Some(analysis) = &round.moderator_analysis {
            if !analysis.resolved_points.is_empty() {
                prompt.push_str(&format!("*Resolved*: {:?}\n", analysis.resolved_points));
            }
            if !analysis.disagreements.is_empty() {
                let topics: Vec<_> = analysis.disagreements.iter().map(|d| d.topic.clone()).collect();
                prompt.push_str(&format!("*Disagreements*: {:?}\n", topics));
            }
            prompt.push('\n');
        }
    }

    prompt.push_str(r#"## Task
Synthesize this deliberation into a final recommendation.

Track:
1. How consensus formed (or didn't)
2. Where models changed their minds
3. Key insights that emerged through debate
4. Final unified recommendation

Output as JSON with the standard synthesis format (consensus, disagreements, unique_insights, recommendation, confidence)."#);

    prompt
}

// ============================================================================
// Helpers
// ============================================================================

/// Extract JSON from markdown code blocks if present
fn extract_json_block(text: &str) -> String {
    // Try to find ```json ... ``` blocks
    if let Some(start) = text.find("```json") {
        let start = start + 7;
        if let Some(end) = text[start..].find("```") {
            return text[start..start + end].trim().to_string();
        }
    }

    // Try to find ``` ... ``` blocks (without language specifier)
    if let Some(start) = text.find("```") {
        let start = start + 3;
        if let Some(end) = text[start..].find("```") {
            let content = text[start..start + end].trim();
            // Only use if it looks like JSON
            if content.starts_with('{') || content.starts_with('[') {
                return content.to_string();
            }
        }
    }

    // Try to find raw JSON
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return text[start..=end].to_string();
        }
    }

    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_moderator_analysis_parse() {
        let json = r#"```json
{
  "disagreements": [
    {
      "topic": "caching strategy",
      "positions": {"GPT-5.2": "use Redis", "Gemini": "use local"},
      "significance": "affects scalability"
    }
  ],
  "focus_questions": ["What about hybrid approach?"],
  "resolved_points": ["All agree on timeout of 30s"],
  "should_continue": true,
  "early_exit_reason": null
}
```"#;

        let analysis = ModeratorAnalysis::parse(json).unwrap();
        assert_eq!(analysis.disagreements.len(), 1);
        assert_eq!(analysis.focus_questions.len(), 1);
        assert_eq!(analysis.resolved_points.len(), 1);
        assert!(analysis.should_continue);
    }

    #[test]
    fn test_build_round_1_prompt() {
        let prompt = build_round_prompt(
            "How should we handle caching?",
            1,
            4,
            &HashMap::new(),
            None,
        );

        assert!(prompt.contains("## Question"));
        assert!(prompt.contains("How should we handle caching?"));
        assert!(prompt.contains("Round 1 of 4"));
    }

    #[test]
    fn test_default_config() {
        let config = DeliberationConfig::default();
        assert_eq!(config.max_rounds, 4);
        assert_eq!(config.models.len(), 3);
        assert!(config.models.contains(&AdvisoryModel::Opus45));
    }

    #[test]
    fn test_identity_markers() {
        // GPT-5.2 should know it's GPT-5.2
        let gpt_prompt = build_deliberation_system_prompt(AdvisoryModel::Gpt52);
        assert!(gpt_prompt.contains("You are GPT-5.2"));
        assert!(gpt_prompt.contains("Gemini 3 Pro and Opus 4.5"));
        assert!(!gpt_prompt.contains("GPT-5.2 and")); // GPT shouldn't list itself as "other"

        // Gemini should know it's Gemini
        let gemini_prompt = build_deliberation_system_prompt(AdvisoryModel::Gemini3Pro);
        assert!(gemini_prompt.contains("You are Gemini 3 Pro"));
        assert!(gemini_prompt.contains("GPT-5.2 and Opus 4.5"));

        // Opus should know it's Opus
        let opus_prompt = build_deliberation_system_prompt(AdvisoryModel::Opus45);
        assert!(opus_prompt.contains("You are Opus 4.5"));
        assert!(opus_prompt.contains("GPT-5.2 and Gemini 3 Pro"));
    }
}
