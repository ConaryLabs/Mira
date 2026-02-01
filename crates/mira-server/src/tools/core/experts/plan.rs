// crates/mira-server/src/tools/core/experts/plan.rs
// Council plan types and hardened JSON parsing

use crate::llm::{LlmClient, Message};
use crate::utils::json::parse_json_hardened;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::sync::Arc;

// ═══════════════════════════════════════════════════════════════════════════════
// Plan Phase Types
// ═══════════════════════════════════════════════════════════════════════════════

/// A single task assigned to an expert by the coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchTask {
    pub role: String,
    pub task: String,
    #[serde(default)]
    pub focus_areas: Vec<String>,
}

/// A role the coordinator decided was unnecessary for this consultation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcludedRole {
    pub role: String,
    pub reason: String,
}

/// The coordinator's plan for the council consultation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchPlan {
    pub goal: String,
    pub tasks: Vec<ResearchTask>,
    #[serde(default)]
    pub excluded_roles: Vec<ExcludedRole>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Review Phase Types
// ═══════════════════════════════════════════════════════════════════════════════

/// A targeted follow-up question for a specific expert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaQuestion {
    pub role: String,
    pub question: String,
    #[serde(default)]
    pub context: String,
}

/// Result of the coordinator reviewing all findings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewResult {
    #[serde(default)]
    pub needs_followup: bool,
    #[serde(default)]
    pub delta_questions: Vec<DeltaQuestion>,
    #[serde(default)]
    pub consensus: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// JSON Parsing with LLM Retry
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse JSON with LLM retry: try hardened parse first, if that fails,
/// ask the LLM to fix its output (max 2 retries).
pub async fn parse_json_with_retry<T: DeserializeOwned>(
    content: &str,
    client: &Arc<dyn LlmClient>,
    type_description: &str,
) -> Result<T, String> {
    // Try hardened parse first
    if let Ok(v) = parse_json_hardened::<T>(content) {
        return Ok(v);
    }

    // LLM retry loop (max 2 attempts)
    let mut last_content = content.to_string();
    for attempt in 1..=2 {
        tracing::debug!(attempt, "JSON parse failed, asking LLM to fix output");

        let fix_prompt = format!(
            "Your previous response was not valid JSON. Please fix it and return ONLY valid JSON.\n\n\
             Expected format: {}\n\n\
             Your broken output:\n```\n{}\n```\n\n\
             Return ONLY the corrected JSON, no markdown fences or explanations.",
            type_description,
            &last_content[..last_content.len().min(2000)]
        );

        let messages = vec![Message::user(fix_prompt)];
        match client.chat(messages, None).await {
            Ok(result) => {
                if let Some(ref fixed) = result.content {
                    last_content = fixed.clone();
                    if let Ok(v) = parse_json_hardened::<T>(fixed) {
                        tracing::debug!(attempt, "LLM fix succeeded");
                        return Ok(v);
                    }
                }
            }
            Err(e) => {
                tracing::warn!(attempt, error = %e, "LLM fix call failed");
            }
        }
    }

    Err(format!(
        "Failed to parse JSON after 2 LLM retries. Last content start: {}",
        &last_content[..last_content.len().min(200)]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_research_plan() {
        let input = r#"{"goal": "test", "tasks": [], "excluded_roles": []}"#;
        let plan: ResearchPlan = parse_json_hardened(input).unwrap();
        assert_eq!(plan.goal, "test");
        assert!(plan.tasks.is_empty());
    }

    #[test]
    fn test_parse_research_plan_nested_braces() {
        let input = r#"Sure! {"goal": "test", "tasks": [{"role": "architect", "task": "review {patterns}"}]}"#;
        let plan: ResearchPlan = parse_json_hardened(input).unwrap();
        assert_eq!(plan.tasks.len(), 1);
        assert!(plan.tasks[0].task.contains("{patterns}"));
    }

    #[test]
    fn test_parse_review_result() {
        let input = r#"{
            "needs_followup": true,
            "delta_questions": [{"role": "security", "question": "what about XSS?", "context": "gap"}],
            "consensus": ["code is well-structured"],
            "conflicts": ["error handling approach differs"]
        }"#;
        let review: ReviewResult = parse_json_hardened(input).unwrap();
        assert!(review.needs_followup);
        assert_eq!(review.delta_questions.len(), 1);
        assert_eq!(review.consensus.len(), 1);
        assert_eq!(review.conflicts.len(), 1);
    }

    #[test]
    fn test_parse_review_result_defaults() {
        let input = r#"{}"#;
        let review: ReviewResult = parse_json_hardened(input).unwrap();
        assert!(!review.needs_followup);
        assert!(review.delta_questions.is_empty());
        assert!(review.consensus.is_empty());
        assert!(review.conflicts.is_empty());
    }

    #[test]
    fn test_research_task_focus_areas_optional() {
        let input = r#"{"role": "architect", "task": "review design"}"#;
        let task: ResearchTask = parse_json_hardened(input).unwrap();
        assert!(task.focus_areas.is_empty());
    }
}
