//! crates/mira-server/src/utils/json.rs
//! Hardened JSON parsing utilities for extracting structured data from LLM output.

use serde::de::DeserializeOwned;

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
    if stripped != trimmed
        && let Ok(v) = serde_json::from_str::<T>(stripped) {
            return Ok(v);
        }

    // 3. Try extracting first JSON object/array
    if let Some(extracted) = extract_json_block(trimmed)
        && let Ok(v) = serde_json::from_str::<T>(extracted) {
            return Ok(v);
        }

    Err(format!(
        "Failed to parse JSON from LLM output (tried direct, fence-strip, brace-extract). Content start: {}",
        &trimmed[..trimmed.len().min(200)]
    ))
}

/// Strip markdown code fences from a string.
pub(crate) fn strip_code_fences(s: &str) -> &str {
    let trimmed = s.trim();

    // Try ```json ... ```
    if let Some(rest) = trimmed.strip_prefix("```json")
        && let Some(json) = rest.strip_suffix("```") {
            return json.trim();
        }
    // Try ``` ... ```
    if let Some(rest) = trimmed.strip_prefix("```")
        && let Some(json) = rest.strip_suffix("```") {
            return json.trim();
        }

    trimmed
}

/// Extract the first balanced `{...}` or `[...]` block from a string.
pub(crate) fn extract_json_block(s: &str) -> Option<&str> {
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
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct SimpleObj {
        key: String,
    }

    // ========================================================================
    // parse_json_hardened tests
    // ========================================================================

    #[test]
    fn test_parse_plain_json() {
        let input = r#"{"key": "test"}"#;
        let obj: SimpleObj = parse_json_hardened(input).unwrap();
        assert_eq!(obj.key, "test");
    }

    #[test]
    fn test_parse_json_with_fences() {
        let input = "```json\n{\"key\": \"test\"}\n```";
        let obj: SimpleObj = parse_json_hardened(input).unwrap();
        assert_eq!(obj.key, "test");
    }

    #[test]
    fn test_parse_json_with_plain_fences() {
        let input = "```\n{\"key\": \"test\"}\n```";
        let obj: SimpleObj = parse_json_hardened(input).unwrap();
        assert_eq!(obj.key, "test");
    }

    #[test]
    fn test_parse_json_with_surrounding_text() {
        let input = "Here is my result:\n{\"key\": \"test\"}\n\nHope that helps!";
        let obj: SimpleObj = parse_json_hardened(input).unwrap();
        assert_eq!(obj.key, "test");
    }

    #[test]
    fn test_parse_json_with_whitespace() {
        let input = "  \n  {\"key\": \"test\"}  \n  ";
        let obj: SimpleObj = parse_json_hardened(input).unwrap();
        assert_eq!(obj.key, "test");
    }

    #[test]
    fn test_parse_json_invalid() {
        let result = parse_json_hardened::<SimpleObj>("not json at all");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_json_array() {
        let input = r#"[{"key": "a"}, {"key": "b"}]"#;
        let arr: Vec<SimpleObj> = parse_json_hardened(input).unwrap();
        assert_eq!(arr.len(), 2);
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
