// crates/mira-server/src/tools/core/experts/plan.rs
// Council plan types and hardened JSON parsing

use crate::llm::{LlmClient, Message};
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
// Hardened JSON Parsing
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse JSON from LLM output with multiple fallback strategies.
///
/// Tries in order:
/// 1. Direct parse of trimmed content
/// 2. Strip markdown code fences, then parse
/// 3. Extract first `{...}` or `[...]` block, then parse
pub fn parse_json_hardened<T: DeserializeOwned>(content: &str) -> Result<T, String> {
    let trimmed = content.trim();

    // 1. Try direct parse
    if let Ok(v) = serde_json::from_str::<T>(trimmed) {
        return Ok(v);
    }

    // 2. Try stripping markdown code fences
    let stripped = strip_code_fences(trimmed);
    if stripped != trimmed {
        if let Ok(v) = serde_json::from_str::<T>(stripped) {
            return Ok(v);
        }
    }

    // 3. Try extracting first JSON object/array
    if let Some(extracted) = extract_json_block(trimmed) {
        if let Ok(v) = serde_json::from_str::<T>(extracted) {
            return Ok(v);
        }
    }

    Err(format!(
        "Failed to parse JSON from LLM output (tried direct, fence-strip, brace-extract). Content start: {}",
        &trimmed[..trimmed.len().min(200)]
    ))
}

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

/// Strip markdown code fences from a string.
fn strip_code_fences(s: &str) -> &str {
    let trimmed = s.trim();

    // Try ```json ... ```
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(json) = rest.strip_suffix("```") {
            return json.trim();
        }
    }
    // Try ``` ... ```
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(json) = rest.strip_suffix("```") {
            return json.trim();
        }
    }

    trimmed
}

/// Extract the first balanced `{...}` or `[...]` block from a string.
fn extract_json_block(s: &str) -> Option<&str> {
    // Find the first `{` or `[`
    let (open_char, close_char, start) = {
        let brace_pos = s.find('{');
        let bracket_pos = s.find('[');

        match (brace_pos, bracket_pos) {
            (Some(b), Some(k)) if b < k => ('{', '}', b),
            (Some(_), Some(k)) => ('[', ']', k),
            (Some(b), None) => ('{', '}', b),
            (None, Some(k)) => ('[', ']', k),
            (None, None) => return None,
        }
    };

    // Walk forward counting nesting
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape_next = false;

    for i in start..bytes.len() {
        let ch = bytes[i] as char;

        if escape_next {
            escape_next = false;
            continue;
        }

        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }

        if ch == '"' {
            in_string = !in_string;
            continue;
        }

        if in_string {
            continue;
        }

        if ch == open_char {
            depth += 1;
        } else if ch == close_char {
            depth -= 1;
            if depth == 0 {
                return Some(&s[start..=i]);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // parse_json_hardened tests
    // ========================================================================

    #[test]
    fn test_parse_plain_json() {
        let input = r#"{"goal": "test", "tasks": [], "excluded_roles": []}"#;
        let plan: ResearchPlan = parse_json_hardened(input).unwrap();
        assert_eq!(plan.goal, "test");
        assert!(plan.tasks.is_empty());
    }

    #[test]
    fn test_parse_json_with_fences() {
        let input = "```json\n{\"goal\": \"test\", \"tasks\": []}\n```";
        let plan: ResearchPlan = parse_json_hardened(input).unwrap();
        assert_eq!(plan.goal, "test");
    }

    #[test]
    fn test_parse_json_with_plain_fences() {
        let input = "```\n{\"goal\": \"test\", \"tasks\": []}\n```";
        let plan: ResearchPlan = parse_json_hardened(input).unwrap();
        assert_eq!(plan.goal, "test");
    }

    #[test]
    fn test_parse_json_with_surrounding_text() {
        let input = "Here is my plan:\n{\"goal\": \"test\", \"tasks\": []}\n\nHope that helps!";
        let plan: ResearchPlan = parse_json_hardened(input).unwrap();
        assert_eq!(plan.goal, "test");
    }

    #[test]
    fn test_parse_json_with_whitespace() {
        let input = "  \n  {\"goal\": \"test\", \"tasks\": []}  \n  ";
        let plan: ResearchPlan = parse_json_hardened(input).unwrap();
        assert_eq!(plan.goal, "test");
    }

    #[test]
    fn test_parse_json_invalid() {
        let result = parse_json_hardened::<ResearchPlan>("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_json_nested_braces() {
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
        // Minimal JSON — all optional fields default
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

    // ========================================================================
    // extract_json_block tests
    // ========================================================================

    #[test]
    fn test_extract_json_block_object() {
        let input = "prefix {\"key\": \"value\"} suffix";
        let extracted = extract_json_block(input).unwrap();
        assert_eq!(extracted, "{\"key\": \"value\"}");
    }

    #[test]
    fn test_extract_json_block_array() {
        let input = "here is the list: [1, 2, 3] done";
        let extracted = extract_json_block(input).unwrap();
        assert_eq!(extracted, "[1, 2, 3]");
    }

    #[test]
    fn test_extract_json_block_nested() {
        let input = r#"{"outer": {"inner": true}}"#;
        let extracted = extract_json_block(input).unwrap();
        assert_eq!(extracted, input);
    }

    #[test]
    fn test_extract_json_block_with_string_braces() {
        let input = r#"{"msg": "hello {world}"}"#;
        let extracted = extract_json_block(input).unwrap();
        assert_eq!(extracted, input);
    }

    #[test]
    fn test_extract_json_block_none_for_no_json() {
        assert!(extract_json_block("no json here").is_none());
    }

    #[test]
    fn test_extract_json_block_with_escaped_quotes() {
        let input = r#"{"msg": "say \"hello\""}"#;
        let extracted = extract_json_block(input).unwrap();
        assert_eq!(extracted, input);
    }

    // ========================================================================
    // strip_code_fences tests
    // ========================================================================

    #[test]
    fn test_strip_fences_json() {
        assert_eq!(strip_code_fences("```json\n{}\n```"), "{}");
    }

    #[test]
    fn test_strip_fences_plain() {
        assert_eq!(strip_code_fences("```\n{}\n```"), "{}");
    }

    #[test]
    fn test_strip_fences_none() {
        assert_eq!(strip_code_fences("{}"), "{}");
    }
}
