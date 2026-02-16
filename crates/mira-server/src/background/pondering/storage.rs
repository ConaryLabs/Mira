// background/pondering/storage.rs
// Storage of pondering insights as behavior patterns

use super::types::PonderingInsight;
use crate::db::pool::DatabasePool;
use rusqlite::params;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Delete insights not triggered in the last 30 days.
pub async fn cleanup_stale_insights(pool: &Arc<DatabasePool>) -> Result<usize, String> {
    pool.run(|conn| {
        let deleted = conn
            .execute(
                "DELETE FROM behavior_patterns \
                 WHERE pattern_type LIKE 'insight_%' \
                   AND last_triggered_at < datetime('now', '-30 days')",
                [],
            )
            .map_err(|e| anyhow::anyhow!("Failed to cleanup insights: {}", e))?;
        if deleted > 0 {
            tracing::info!("Cleaned up {} stale insights", deleted);
        }
        Ok::<_, anyhow::Error>(deleted)
    })
    .await
}

/// Normalize a description for dedup hashing.
/// Lowercases, replaces digits with `#`, and collapses whitespace so that
/// "Module X had 3 reverts" and "Module X had 5 reverts" produce the same key.
fn normalize_for_dedup(description: &str) -> String {
    description
        .to_lowercase()
        .replace(|c: char| c.is_ascii_digit(), "#")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Hash a string into a 16-hex-char key.
fn hash_key(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string()
}

/// Extract the first file/module path from an insight description.
/// Matches patterns like `src/db/pool.rs`, `background/`, `crates/mira-server/src/...`.
fn extract_primary_entity(description: &str) -> Option<String> {
    // Look for path-like tokens: contains '/' or ends with a known extension
    for token in description.split_whitespace() {
        let clean = token.trim_matches(|c: char| {
            matches!(
                c,
                '`' | '\'' | '"' | ',' | '.' | ':' | ';' | '(' | ')' | '!' | '?'
            )
        });
        if clean.contains('/')
            || clean.ends_with(".rs")
            || clean.ends_with(".ts")
            || clean.ends_with(".py")
            || clean.ends_with(".js")
            || clean.ends_with(".go")
        {
            return Some(clean.to_lowercase());
        }
    }
    None
}

/// Extract goal ID from description (e.g., "Goal 94 ..." -> 94).
/// Matches patterns like "Goal 94", "goal 94", "Goal #94".
fn extract_goal_id(description: &str) -> Option<u64> {
    let lower = description.to_lowercase();
    // Find "goal" as a whole word (not "subgoal", "supergoal", etc.)
    let mut search_from = 0;
    loop {
        let pos = lower[search_from..].find("goal")?;
        let abs_pos = search_from + pos;
        // Check word boundary before "goal"
        let before_ok = abs_pos == 0
            || !lower[..abs_pos]
                .chars()
                .next_back()
                .unwrap_or(' ')
                .is_alphanumeric();
        if before_ok {
            let after = lower[abs_pos + 4..].trim_start();
            let after = after
                .strip_prefix('#')
                .map(|s| s.trim_start())
                .unwrap_or(after);
            let num_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !num_str.is_empty() {
                return num_str.parse().ok();
            }
        }
        search_from = abs_pos + 4;
    }
}

/// Extract the first quoted text from a description (for goal titles, etc.).
fn extract_quoted_text(description: &str) -> Option<&str> {
    // Try double quotes first (unambiguous), then smart quotes, then single quotes
    // Single quotes are tried last and use special handling for apostrophes.
    for quote in ['"', '\u{201c}'] {
        let close = match quote {
            '\u{201c}' => '\u{201d}',
            q => q,
        };
        if let Some(start) = description.find(quote) {
            let after_open = start + quote.len_utf8();
            let rest = &description[after_open..];
            let end = find_closing_quote(rest, close)?;
            let inner = &description[after_open..after_open + end];
            if !inner.is_empty() {
                return Some(inner);
            }
        }
    }
    // Fall back to single quotes with stricter open/close validation
    extract_single_quoted(description)
}

/// Extract single-quoted text with apostrophe-aware open/close detection.
///
/// An opening `'` must be preceded by a non-alphanumeric character (or be at
/// the start of the string) AND followed by an alphanumeric character.
/// This prevents apostrophes in words like `There's` from being selected as
/// the opening delimiter.
fn extract_single_quoted(description: &str) -> Option<&str> {
    for (i, c) in description.char_indices() {
        if c != '\'' {
            continue;
        }
        // Opening quote: preceded by non-alphanumeric (or start of string)
        let before = description[..i].chars().next_back();
        if before.is_some_and(|b| b.is_alphanumeric()) {
            continue;
        }
        // Opening quote: followed by an alphanumeric character
        let after_open = i + c.len_utf8();
        if !description[after_open..]
            .chars()
            .next()
            .is_some_and(|a| a.is_alphanumeric())
        {
            continue;
        }
        // Find the closing quote (non-greedy, skips apostrophes like User's)
        let rest = &description[after_open..];
        if let Some(end) = find_closing_quote(rest, '\'') {
            let inner = &description[after_open..after_open + end];
            if !inner.is_empty() {
                return Some(inner);
            }
        }
    }
    None
}

/// Find a closing quote character, skipping apostrophes (quote followed by a letter).
fn find_closing_quote(text: &str, quote: char) -> Option<usize> {
    for (i, c) in text.char_indices() {
        if c == quote {
            // A closing quote is followed by non-alphanumeric or end-of-string.
            // An apostrophe (e.g., User's) is followed by a letter — skip it.
            let after = text[i + c.len_utf8()..].chars().next();
            match after {
                None => return Some(i),
                Some(next) if !next.is_alphanumeric() => return Some(i),
                _ => continue,
            }
        }
    }
    None
}

/// Extract the last quoted span from text, handling both single and double quotes.
/// A "closing quote" is a quote char followed by a non-alphanumeric char (or end of string).
/// An "opening quote" is a quote char at position 0 or preceded by a non-alphanumeric char.
/// This correctly handles apostrophes ("can't"), extra quoted spans, and mixed quote styles.
fn extract_last_quoted_span(text: &str) -> Option<String> {
    // Try both quote styles, pick the one with closing quote nearest to end
    // (closest to "in 'TOOL'"). This handles mixed styles like:
    //   In module 'x', "permission denied" error in 'Bash'
    // where double-quote span is closer to "in '" than single-quote 'x'.
    let mut best: Option<(usize, String)> = None; // (close_pos, span)
    for quote in [b'\'', b'"'] {
        let bytes = text.as_bytes();
        // Find closing: last quote followed by non-alphanumeric
        let Some(close) = (0..bytes.len()).rev().find(|&i| {
            bytes[i] == quote
                && match bytes.get(i + 1) {
                    Some(c) => !c.is_ascii_alphanumeric(),
                    None => true,
                }
        }) else {
            continue;
        };
        // Find opening: nearest preceding quote at word boundary
        let Some(open) = (0..close)
            .rev()
            .find(|&i| bytes[i] == quote && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()))
        else {
            continue;
        };
        let span = &text[open + 1..close];
        if !span.is_empty()
            && best
                .as_ref()
                .is_none_or(|(prev_close, _)| close > *prev_close)
        {
            best = Some((close, span.to_string()));
        }
    }
    best.map(|(_, span)| span)
}

/// Find the first `error in '` position that's NOT inside a double-quoted string.
/// Returns the position of the `in '` part (skipping "error ").
fn find_tool_marker(text: &str) -> Option<usize> {
    let mut search_start = 0;
    loop {
        let pos = text[search_start..]
            .find("error in '")
            .map(|p| p + search_start)?;
        // Check if inside double quotes: odd count of " before this position means inside
        let quotes_before = text[..pos].bytes().filter(|&b| b == b'"').count();
        if quotes_before % 2 == 0 {
            return Some(pos + 6); // skip "error " to point at "in '"
        }
        search_start = pos + 1;
    }
}

/// Extract an identity key for recurring error insights.
/// Returns `tool:error_template` when possible, `tool` as last resort.
///
/// Handles both formats:
/// - Heuristic: "Error in 'Read' has occurred 10 times without resolution: file does not exist"
///   → "read:file does not exist"
/// - LLM: "'permission denied' error in 'Bash' has recurred 12 times..."
///   → "bash:permission denied"
///
/// Operates entirely on the lowercased string to avoid Unicode byte offset mismatches.
fn extract_error_identity(description: &str) -> Option<String> {
    let lower = description.to_lowercase();

    // Find tool name via "error in 'TOOL'".
    // Iterate all occurrences, skip any inside double-quoted strings
    // (e.g. "parse error in 'config'" contains a false match).
    // This handles both earlier and later false matches.
    let in_pos = find_tool_marker(&lower).or_else(|| {
        // Fallback: use "in '" only if it appears exactly once (unambiguous)
        let first = lower.find("in '")?;
        if lower[first + 1..].contains("in '") {
            None
        } else {
            Some(first)
        }
    })?;
    let after_in = &lower[in_pos + 4..];
    let tool = after_in.split('\'').next().filter(|s| !s.is_empty())?;

    // Heuristic format: "... without resolution: <template>"
    if let Some(pos) = lower.find("resolution:") {
        let after = lower[pos + 11..].trim();
        if !after.is_empty() {
            return Some(format!("{}:{}", tool, normalize_for_dedup(after)));
        }
    }

    // LLM format: "'<error>' error in 'TOOL' ..." or "\"<error>\" error in 'TOOL' ..."
    // Find last quoted span before "in '".
    // Closing quote: last quote char followed by non-alphanumeric (skips apostrophes).
    // Opening quote: nearest preceding quote at word boundary (start/space/comma).
    let before_in = &lower[..in_pos];
    let found_error = extract_last_quoted_span(before_in);
    if let Some(error_text) = found_error {
        return Some(format!("{}:{}", tool, normalize_for_dedup(&error_text)));
    }

    // Last resort: tool-only (no error template found)
    Some(tool.to_string())
}

/// Compute a 16-hex-char pattern key, using the primary entity for
/// file/module-based insight types to avoid false-unique keys from
/// different LLM phrasings about the same entity.
fn compute_pattern_key(pattern_type: &str, description: &str) -> String {
    // For file/module insights, key on the entity not the prose
    if matches!(
        pattern_type,
        "insight_fragile_code"
            | "insight_untested"
            | "insight_revert_cluster"
            | "insight_churn_hotspot"
    ) && let Some(entity) = extract_primary_entity(description)
    {
        return hash_key(&format!("{}:{}", pattern_type, entity));
    }
    // For stale goal insights, key on the goal ID (stable) or quoted title
    if pattern_type == "insight_stale_goal" {
        if let Some(goal_id) = extract_goal_id(description) {
            return hash_key(&format!("{}:goal_{}", pattern_type, goal_id));
        }
        if let Some(title) = extract_quoted_text(description) {
            return hash_key(&format!("{}:{}", pattern_type, title.to_lowercase()));
        }
    }
    // For recurring errors, key on tool name (+ template if available)
    if pattern_type == "insight_recurring_error"
        && let Some(identity) = extract_error_identity(description)
    {
        return hash_key(&format!("{}:{}", pattern_type, identity));
    }
    // Health degrading: one per project (only the type matters)
    if pattern_type == "insight_health_degrading" {
        return hash_key(pattern_type);
    }
    // Default: current behavior (normalized description hash)
    hash_key(&normalize_for_dedup(description))
}

/// Maximum number of insights to keep per pattern_type per project.
/// Session insights get a tighter cap because LLM paraphrasing creates
/// variants that similarity-based dedup can't reliably catch.
fn max_insights_for_type(pattern_type: &str) -> i64 {
    match pattern_type {
        "insight_session" => 5,
        _ => 10,
    }
}

/// Bigram Jaccard similarity threshold for near-duplicate detection.
/// 0.65 catches LLM paraphrasing while allowing genuinely different insights.
const SIMILARITY_THRESHOLD: f64 = 0.65;

/// Compute text similarity using character bigram Jaccard index.
/// Returns 0.0 if both strings are empty, otherwise |intersection| / |union|.
fn text_similarity(a: &str, b: &str) -> f64 {
    // Normalize: lowercase and extract character bigrams
    let bigrams_a: std::collections::HashSet<_> = a
        .to_lowercase()
        .chars()
        .collect::<Vec<_>>()
        .windows(2)
        .filter(|w| !(w[0].is_whitespace() && w[1].is_whitespace()))
        .map(|w| (w[0], w[1]))
        .collect();

    let bigrams_b: std::collections::HashSet<_> = b
        .to_lowercase()
        .chars()
        .collect::<Vec<_>>()
        .windows(2)
        .filter(|w| !(w[0].is_whitespace() && w[1].is_whitespace()))
        .map(|w| (w[0], w[1]))
        .collect();

    // Handle empty strings
    if bigrams_a.is_empty() && bigrams_b.is_empty() {
        return 0.0;
    }
    if bigrams_a.is_empty() || bigrams_b.is_empty() {
        return 0.0;
    }

    // Jaccard index: |intersection| / |union|
    let intersection = bigrams_a.intersection(&bigrams_b).count();
    let union = bigrams_a.union(&bigrams_b).count();

    intersection as f64 / union as f64
}

/// Store insights as behavior patterns, then enforce per-type cap.
pub(super) async fn store_insights(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    insights: &[PonderingInsight],
) -> Result<usize, String> {
    if insights.is_empty() {
        return Ok(0);
    }

    let insights_clone: Vec<PonderingInsight> = insights
        .iter()
        .map(|i| PonderingInsight {
            pattern_type: i.pattern_type.clone(),
            description: i.description.clone(),
            confidence: i.confidence,
            evidence: i.evidence.clone(),
        })
        .collect();

    pool.run(move |conn| {
        let mut stored = 0;
        let mut types_touched = std::collections::HashSet::new();
        // Track descriptions stored in this batch for intra-batch dedup
        let mut batch_descriptions: Vec<(String, String)> = Vec::new();

        for insight in &insights_clone {
            // Generate pattern key using entity-aware hashing
            let pattern_key = compute_pattern_key(&insight.pattern_type, &insight.description);
            // Also compute legacy key (pre-entity-aware) so old dismissals are honoured
            let legacy_key = hash_key(&normalize_for_dedup(&insight.description));

            // Types with identity-preserving keys: entity-based (file/goal/tool)
            // or type-only (health — one per project).
            // insight_recurring_error is only stable when extract_error_identity
            // succeeded — if it fell through to normalized-description hash,
            // it should be treated as non-stable.
            let has_stable_key = matches!(
                insight.pattern_type.as_str(),
                "insight_fragile_code"
                    | "insight_untested"
                    | "insight_revert_cluster"
                    | "insight_stale_goal"
                    | "insight_churn_hotspot"
                    | "insight_health_degrading"
            ) || (insight.pattern_type == "insight_recurring_error"
                && extract_error_identity(&insight.description).is_some());

            // Skip if this pattern was previously dismissed.
            // Stable-key types skip this check — the upsert resets dismissed=0
            // so recurring issues with fresh evidence re-activate automatically.
            if !has_stable_key {
                let dismissed: bool = conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM behavior_patterns \
                         WHERE project_id = ? AND pattern_type = ? AND pattern_key IN (?, ?) AND dismissed = 1)",
                        params![project_id, insight.pattern_type, pattern_key, legacy_key],
                        |row| row.get(0),
                    )
                    .unwrap_or(false);
                if dismissed {
                    continue;
                }
            }

            // Intra-batch dedup for types WITHOUT stable keys.
            // Stable-key types rely on the DB upsert (same key → ON CONFLICT).
            // Non-stable types: limit 1 per pattern_type per batch — LLMs often
            // generate multiple prose variants of the same finding.
            // Exception: insight_session can have multiple distinct patterns from
            // the heuristic (short sessions, high churn, no summaries).
            if !has_stable_key && insight.pattern_type != "insight_session" {
                let type_already_in_batch = batch_descriptions
                    .iter()
                    .any(|(pt, _)| pt == &insight.pattern_type);

                if type_already_in_batch {
                    tracing::debug!(
                        "Skipping duplicate non-stable type in batch: {} ({})",
                        insight.pattern_type,
                        insight.description,
                    );
                    continue;
                }
            }

            // Cross-batch text similarity dedup — only for non-entity-aware types.
            // Entity-aware types skip this because similar filenames (login.rs vs
            // logout.rs) would exceed the threshold despite being distinct entities.

            if !has_stable_key {
                let mut stmt = conn
                    .prepare(
                        "SELECT json_extract(pattern_data, '$.description') \
                         FROM behavior_patterns \
                         WHERE project_id = ? AND pattern_type = ? AND pattern_key != ? AND dismissed = 0",
                    )
                    .map_err(|e| anyhow::anyhow!("Failed to prepare similarity check: {}", e))?;

                let existing_descriptions: Vec<String> = stmt
                    .query_map(
                        params![project_id, insight.pattern_type, pattern_key],
                        |row| row.get::<_, String>(0),
                    )
                    .map_err(|e| {
                        anyhow::anyhow!("Failed to query existing descriptions: {}", e)
                    })?
                    .filter_map(|r| r.ok())
                    .collect();

                let is_duplicate = existing_descriptions.iter().any(|existing_desc| {
                    text_similarity(&insight.description, existing_desc) > SIMILARITY_THRESHOLD
                });

                if is_duplicate {
                    tracing::debug!("Skipping near-duplicate insight: {}", insight.description);
                    continue;
                }
            }

            let pattern_data = serde_json::json!({
                "description": insight.description,
                "evidence": insight.evidence,
                "generated_by": "pondering",
            });

            // Upsert pattern - increment occurrence if exists, keep latest evidence
            let result = conn.execute(
                r#"
                INSERT INTO behavior_patterns
                    (project_id, pattern_type, pattern_key, pattern_data, confidence,
                     occurrence_count, last_triggered_at, first_seen_at, updated_at)
                VALUES (?, ?, ?, ?, ?, 1, datetime('now'), datetime('now'), datetime('now'))
                ON CONFLICT(project_id, pattern_type, pattern_key) DO UPDATE SET
                    occurrence_count = occurrence_count + 1,
                    confidence = (confidence + excluded.confidence) / 2,
                    pattern_data = excluded.pattern_data,
                    last_triggered_at = datetime('now'),
                    dismissed = 0
                "#,
                params![
                    project_id,
                    insight.pattern_type,
                    pattern_key,
                    pattern_data.to_string(),
                    insight.confidence,
                ],
            );

            match result {
                Ok(_) => {
                    stored += 1;
                    types_touched.insert(insight.pattern_type.clone());
                    batch_descriptions
                        .push((insight.pattern_type.clone(), insight.description.clone()));
                }
                Err(e) => tracing::warn!("Failed to store insight: {}", e),
            }
        }

        // Enforce per-type cap: keep the newest N, evict oldest
        for pattern_type in &types_touched {
            let cap = max_insights_for_type(pattern_type);
            let evicted = conn
                .execute(
                    r#"
                    DELETE FROM behavior_patterns
                    WHERE project_id = ? AND pattern_type = ?
                      AND id NOT IN (
                          SELECT id FROM behavior_patterns
                          WHERE project_id = ? AND pattern_type = ?
                          ORDER BY last_triggered_at DESC
                          LIMIT ?
                      )
                    "#,
                    params![project_id, pattern_type, project_id, pattern_type, cap,],
                )
                .unwrap_or(0);
            if evicted > 0 {
                tracing::info!(
                    "Evicted {} old '{}' insights (cap: {})",
                    evicted,
                    pattern_type,
                    cap,
                );
            }
        }

        Ok::<_, anyhow::Error>(stored)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_for_dedup() {
        // Numbers replaced with #
        assert_eq!(
            normalize_for_dedup("Module X had 3 reverts"),
            "module x had # reverts"
        );
        assert_eq!(
            normalize_for_dedup("Module X had 5 reverts"),
            "module x had # reverts"
        );

        // Multi-digit numbers
        assert_eq!(
            normalize_for_dedup("Goal 94 stale for 23 days"),
            "goal ## stale for ## days"
        );
        assert_eq!(
            normalize_for_dedup("Goal 94 stale for 45 days"),
            "goal ## stale for ## days"
        );

        // Whitespace collapsed
        assert_eq!(
            normalize_for_dedup("  extra   spaces   here  "),
            "extra spaces here"
        );
    }

    #[test]
    fn test_same_meaning_same_key() {
        // Generic type falls back to normalized description hash
        let key1 = compute_pattern_key(
            "insight_workflow",
            "Module background/ had 3 reverts in 24h",
        );
        let key2 = compute_pattern_key(
            "insight_workflow",
            "Module background/ had 7 reverts in 24h",
        );
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_different_meaning_different_key() {
        let key1 = compute_pattern_key("insight_workflow", "Module background/ had reverts");
        let key2 = compute_pattern_key("insight_workflow", "Module db/ had reverts");
        assert_ne!(key1, key2);
    }

    // ── Entity-aware dedup tests ─────────────────────────────────────

    #[test]
    fn test_extract_primary_entity() {
        assert_eq!(
            extract_primary_entity("File src/db/pool.rs has 148 modifications"),
            Some("src/db/pool.rs".to_string()),
        );
        assert_eq!(
            extract_primary_entity("Module background/ has high churn"),
            Some("background/".to_string()),
        );
        assert_eq!(extract_primary_entity("No paths here at all"), None,);
    }

    #[test]
    fn test_extract_quoted_text() {
        assert_eq!(
            extract_quoted_text(r#"Goal "Implement auth" is stale for 30 days"#),
            Some("Implement auth"),
        );
        assert_eq!(
            extract_quoted_text("Goal 'Quick Wins' has no progress"),
            Some("Quick Wins"),
        );
        assert_eq!(extract_quoted_text("No quotes here"), None);
    }

    #[test]
    fn test_entity_dedup_same_file_different_prose() {
        // Two different descriptions about the same file should produce the same key
        let key1 = compute_pattern_key(
            "insight_fragile_code",
            "src/db/factory.rs has 148 modifications and high revert rate",
        );
        let key2 = compute_pattern_key(
            "insight_fragile_code",
            "LLM provider abstraction src/db/factory.rs shows instability with 148 mods",
        );
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_entity_dedup_different_files() {
        let key1 = compute_pattern_key("insight_fragile_code", "src/db/factory.rs has high churn");
        let key2 = compute_pattern_key("insight_fragile_code", "src/db/pool.rs has high churn");
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_stale_goal_dedup_same_title() {
        let key1 = compute_pattern_key(
            "insight_stale_goal",
            r#"Goal "Implement auth" has been stale for 30 days"#,
        );
        let key2 = compute_pattern_key(
            "insight_stale_goal",
            r#"Goal "Implement auth" still in progress after 45 days with no updates"#,
        );
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_stale_goal_dedup_different_titles() {
        let key1 = compute_pattern_key("insight_stale_goal", r#"Goal "Implement auth" is stale"#);
        let key2 = compute_pattern_key("insight_stale_goal", r#"Goal "Add caching" is stale"#);
        assert_ne!(key1, key2);
    }

    // ── Punctuation robustness ────────────────────────────────────────

    #[test]
    fn test_extract_entity_trailing_punctuation() {
        // Trailing period, colon, etc. should be stripped
        assert_eq!(
            extract_primary_entity("File src/db/pool.rs."),
            Some("src/db/pool.rs".to_string()),
        );
        assert_eq!(
            extract_primary_entity("See src/db/pool.rs:"),
            Some("src/db/pool.rs".to_string()),
        );
        assert_eq!(
            extract_primary_entity("(src/db/pool.rs)"),
            Some("src/db/pool.rs".to_string()),
        );
    }

    #[test]
    fn test_entity_dedup_trailing_punctuation_same_key() {
        // src/db/pool.rs and src/db/pool.rs. should produce the same key
        let key1 =
            compute_pattern_key("insight_fragile_code", "File src/db/pool.rs has high churn");
        let key2 = compute_pattern_key(
            "insight_fragile_code",
            "File src/db/pool.rs. has high churn",
        );
        assert_eq!(key1, key2);
    }

    // ── Apostrophe handling in quoted text ──────────────────────────────

    #[test]
    fn test_extract_quoted_text_with_apostrophe() {
        // Apostrophe inside single-quoted text should not truncate
        assert_eq!(
            extract_quoted_text("Goal 'User's profile migration' has no progress"),
            Some("User's profile migration"),
        );
    }

    #[test]
    fn test_extract_quoted_text_apostrophe_before_title() {
        // Apostrophe in text BEFORE the quoted title should not be picked as opening
        assert_eq!(
            extract_quoted_text("There's a stale goal 'Add caching' with no progress"),
            Some("Add caching"),
        );
    }

    #[test]
    fn test_extract_quoted_text_apostrophe_after_closing() {
        // Apostrophe in text AFTER the closing quote should not extend the match
        assert_eq!(
            extract_quoted_text("Goal 'Add caching' users' feedback is stale"),
            Some("Add caching"),
        );
    }

    #[test]
    fn test_stale_goal_apostrophe_dedup() {
        // Goal titles with apostrophes should produce correct keys
        let key1 = compute_pattern_key(
            "insight_stale_goal",
            "Goal 'User's profile migration' is stale for 30 days",
        );
        let key2 = compute_pattern_key(
            "insight_stale_goal",
            "Goal 'User's profile migration' still in progress after 45 days",
        );
        assert_eq!(key1, key2);

        // Different title should produce different key
        let key3 = compute_pattern_key(
            "insight_stale_goal",
            "Goal 'Add caching' is stale for 30 days",
        );
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_stale_goal_apostrophe_before_title_dedup() {
        // Apostrophe before the quoted title should not affect key extraction
        let key1 = compute_pattern_key(
            "insight_stale_goal",
            "There's a stale goal 'Add caching' with no updates for 30 days",
        );
        let key2 = compute_pattern_key(
            "insight_stale_goal",
            "There's a stale goal 'Add caching' still in progress after 45 days",
        );
        assert_eq!(key1, key2);
    }

    // ── Legacy key dismissal ────────────────────────────────────────────

    /// Stable-key types bypass the dismissed check — the upsert resets dismissed=0.
    /// Legacy dismissed entries get reactivated when the issue recurs.
    #[tokio::test]
    async fn test_stable_key_reactivates_legacy_dismissed() {
        use crate::db::test_support::setup_test_pool;

        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/legacy-dismiss", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        let description = "src/db/factory.rs has 148 modifications and high revert rate";
        let pattern_type = "insight_fragile_code";

        // Simulate a legacy dismissed insight (keyed with old normalized-description hash)
        let legacy_key = hash_key(&normalize_for_dedup(description));
        pool.run({
            let legacy_key = legacy_key.clone();
            let pattern_type = pattern_type.to_string();
            move |conn| {
                conn.execute(
                    r#"INSERT INTO behavior_patterns
                        (project_id, pattern_type, pattern_key, pattern_data, confidence,
                         dismissed, last_triggered_at, first_seen_at, updated_at)
                       VALUES (?, ?, ?, '{}', 0.5, 1, datetime('now'), datetime('now'), datetime('now'))"#,
                    params![project_id, pattern_type, legacy_key],
                )?;
                Ok::<_, anyhow::Error>(())
            }
        })
        .await
        .unwrap();

        // Store the same insight — stable-key type bypasses dismissed check,
        // upsert creates a new row (different key: entity-based vs legacy)
        let insights = vec![PonderingInsight {
            pattern_type: pattern_type.to_string(),
            description: description.to_string(),
            confidence: 0.8,
            evidence: vec!["test".to_string()],
        }];

        let stored = store_insights(&pool, project_id, &insights).await.unwrap();
        assert_eq!(stored, 1, "stable-key type should bypass dismissed check");
    }

    /// Non-stable types should still be blocked by dismissed keys.
    #[tokio::test]
    async fn test_non_stable_blocked_by_dismissed() {
        use crate::db::test_support::setup_test_pool;

        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/dismiss-block", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        let description = "5 sessions lasted under 5 minutes — workflow fragmentation";
        let pattern_type = "insight_workflow"; // non-stable type

        // Dismiss the insight
        let pattern_key = hash_key(&normalize_for_dedup(description));
        pool.run({
            let pattern_key = pattern_key.clone();
            let pattern_type = pattern_type.to_string();
            move |conn| {
                conn.execute(
                    r#"INSERT INTO behavior_patterns
                        (project_id, pattern_type, pattern_key, pattern_data, confidence,
                         dismissed, last_triggered_at, first_seen_at, updated_at)
                       VALUES (?, ?, ?, '{}', 0.5, 1, datetime('now'), datetime('now'), datetime('now'))"#,
                    params![project_id, pattern_type, pattern_key],
                )?;
                Ok::<_, anyhow::Error>(())
            }
        })
        .await
        .unwrap();

        // Try to store — should be blocked
        let insights = vec![PonderingInsight {
            pattern_type: pattern_type.to_string(),
            description: description.to_string(),
            confidence: 0.7,
            evidence: vec!["test".to_string()],
        }];

        let stored = store_insights(&pool, project_id, &insights).await.unwrap();
        assert_eq!(stored, 0, "non-stable type should be blocked by dismissal");
    }

    // ── Churn hotspot entity-aware keying ──────────────────────────────

    #[test]
    fn test_churn_hotspot_same_file_different_prose() {
        let key1 = compute_pattern_key(
            "insight_churn_hotspot",
            "'src/mcp/mod.rs' touched in 56 sessions over 20 days — consider refactoring",
        );
        let key2 = compute_pattern_key(
            "insight_churn_hotspot",
            "src/mcp/mod.rs has been in continuous modification for 20+ days with 301 changes",
        );
        assert_eq!(key1, key2, "same file should produce same key");
    }

    #[test]
    fn test_churn_hotspot_different_files() {
        let key1 = compute_pattern_key(
            "insight_churn_hotspot",
            "'src/mcp/mod.rs' touched in 56 sessions",
        );
        let key2 = compute_pattern_key(
            "insight_churn_hotspot",
            "'src/mcp/requests.rs' touched in 53 sessions",
        );
        assert_ne!(key1, key2, "different files should produce different keys");
    }

    // ── Recurring error entity-aware keying ─────────────────────────────

    #[test]
    fn test_recurring_error_same_tool_same_error() {
        // Heuristic format — includes template via "resolution:" suffix
        let key1 = compute_pattern_key(
            "insight_recurring_error",
            "Error in 'Read' has occurred 10 times without resolution: file does not exist",
        );
        let key2 = compute_pattern_key(
            "insight_recurring_error",
            "Error in 'Read' has occurred 12 times without resolution: file does not exist",
        );
        assert_eq!(key1, key2, "same tool+error should produce same key");
    }

    #[test]
    fn test_recurring_error_different_errors_same_tool() {
        // Same tool, different error templates — should be distinct
        let key1 = compute_pattern_key(
            "insight_recurring_error",
            "Error in 'Read' has occurred 10 times without resolution: file does not exist",
        );
        let key2 = compute_pattern_key(
            "insight_recurring_error",
            "Error in 'Read' has occurred 8 times without resolution: permission denied",
        );
        assert_ne!(
            key1, key2,
            "different errors in same tool should produce different keys"
        );
    }

    #[test]
    fn test_recurring_error_llm_format_different_errors() {
        // LLM format: "'error' error in 'Tool' ..." — extracts error from first quotes
        let key_llm1 = compute_pattern_key(
            "insight_recurring_error",
            "'permission denied' error in 'Bash' has recurred 12 times across sessions",
        );
        let key_llm2 = compute_pattern_key(
            "insight_recurring_error",
            "'command not found' error in 'Bash' has recurred 8 times",
        );
        // Both extract tool+error: bash:permission denied vs bash:command not found
        assert_ne!(
            key_llm1, key_llm2,
            "LLM format should distinguish different errors for same tool"
        );
    }

    #[test]
    fn test_recurring_error_llm_format_same_error_paraphrased() {
        // Same error paraphrased differently by LLM — should produce same key
        let key1 = compute_pattern_key(
            "insight_recurring_error",
            "'permission denied' error in 'Bash' has recurred 12 times across sessions",
        );
        let key2 = compute_pattern_key(
            "insight_recurring_error",
            "'permission denied' error in 'Bash' keeps occurring — 12 occurrences noted",
        );
        assert_eq!(
            key1, key2,
            "same error+tool should produce same key regardless of surrounding text"
        );
    }

    #[test]
    fn test_recurring_error_llm_format_apostrophe_in_error() {
        // Error text contains apostrophe — should not truncate at it
        let key = compute_pattern_key(
            "insight_recurring_error",
            "'can't connect to server' error in 'Bash' has recurred 5 times",
        );
        let key_clean = compute_pattern_key(
            "insight_recurring_error",
            "'can't connect to server' error in 'Bash' keeps happening",
        );
        assert_eq!(key, key_clean, "apostrophe in error should not truncate");

        // Verify it's distinct from a different error
        let key_other = compute_pattern_key(
            "insight_recurring_error",
            "'permission denied' error in 'Bash' has recurred 5 times",
        );
        assert_ne!(
            key, key_other,
            "different errors should produce different keys"
        );
    }

    #[test]
    fn test_recurring_error_extra_quoted_spans_before_error() {
        // Extra quoted text before the error — should extract only the last quoted span
        let key = compute_pattern_key(
            "insight_recurring_error",
            "In module 'x', 'permission denied' error in 'Bash' has recurred 5 times",
        );
        let key_simple = compute_pattern_key(
            "insight_recurring_error",
            "'permission denied' error in 'Bash' has recurred 5 times",
        );
        assert_eq!(
            key, key_simple,
            "extra quoted spans before error should not pollute the key"
        );
    }

    #[test]
    fn test_recurring_error_trailing_in_quoted_after_tool() {
        // "in '/tmp/foo'" in the resolution text after the tool
        let id = extract_error_identity(
            "Error in 'Read' has occurred 10 times without resolution: no such file in '/tmp/foo'",
        );
        assert!(
            id.as_ref().unwrap().starts_with("read:"),
            "tool should be 'read', got: {:?}",
            id
        );

        let id_bar = extract_error_identity(
            "Error in 'Read' has occurred 10 times without resolution: no such file in '/tmp/bar'",
        );
        assert!(id_bar.as_ref().unwrap().starts_with("read:"));
        // Different paths → different resolution templates → different identity
        assert_ne!(id, id_bar);
    }

    #[test]
    fn test_recurring_error_llm_trailing_in_after_tool() {
        // LLM prose with "in 'module'" after the tool
        let id = extract_error_identity(
            "'permission denied' error in 'Bash' while in 'module x' across sessions",
        );
        assert!(
            id.as_ref().unwrap().starts_with("bash:"),
            "tool should be 'bash', got: {:?}",
            id
        );

        let id_simple =
            extract_error_identity("'permission denied' error in 'Bash' has recurred 5 times");
        assert_eq!(id, id_simple, "trailing context should not affect identity");
    }

    #[test]
    fn test_recurring_error_error_in_quoted_error_text() {
        // Error text inside double quotes contains "error in '...'" — skipped
        let id = extract_error_identity(
            "\"parse error in 'config'\" error in 'Bash' has recurred 5 times",
        );
        assert!(
            id.as_ref().unwrap().starts_with("bash:"),
            "tool should be 'bash', got: {:?}",
            id
        );
    }

    #[test]
    fn test_recurring_error_trailing_error_in_after_tool() {
        // "error in '...'" appears AFTER the real tool — should still pick the first
        let id = extract_error_identity(
            "'permission denied' error in 'Bash' has recurred; likely error in 'config' loader",
        );
        assert!(
            id.as_ref().unwrap().starts_with("bash:"),
            "tool should be 'bash', got: {:?}",
            id
        );

        // Heuristic format with "error in" in the resolution text
        let id2 = extract_error_identity(
            "Error in 'Read' has occurred 10 times without resolution: parse failure due to error in 'yaml' format",
        );
        assert!(
            id2.as_ref().unwrap().starts_with("read:"),
            "tool should be 'read', got: {:?}",
            id2
        );
    }

    #[test]
    fn test_recurring_error_earlier_in_quoted_span() {
        // "In 'module x'" before the actual tool — anchored "error in '" skips it
        let key = compute_pattern_key(
            "insight_recurring_error",
            "In 'module x', 'permission denied' error in 'Bash' has recurred 5 times",
        );
        let key_simple = compute_pattern_key(
            "insight_recurring_error",
            "'permission denied' error in 'Bash' has recurred 5 times",
        );
        assert_eq!(
            key, key_simple,
            "earlier 'in ...' should not steal the tool position"
        );
    }

    #[test]
    fn test_recurring_error_double_quotes() {
        // LLM uses double quotes around error
        let key_dq = compute_pattern_key(
            "insight_recurring_error",
            "\"permission denied\" error in 'Bash' has recurred 12 times",
        );
        let key_sq = compute_pattern_key(
            "insight_recurring_error",
            "'permission denied' error in 'Bash' has recurred 12 times",
        );
        assert_eq!(
            key_dq, key_sq,
            "double-quoted and single-quoted errors should produce the same key"
        );
    }

    #[test]
    fn test_recurring_error_mixed_quote_styles() {
        // Single-quoted module name + double-quoted error — should pick double-quoted
        // span (closer to "in '") not the single-quoted 'x'
        let key = compute_pattern_key(
            "insight_recurring_error",
            "In module 'x', \"permission denied\" error in 'Bash' has recurred 5 times",
        );
        let key_simple = compute_pattern_key(
            "insight_recurring_error",
            "'permission denied' error in 'Bash' has recurred 5 times",
        );
        assert_eq!(
            key, key_simple,
            "mixed quotes should extract the error span closest to 'in TOOL'"
        );
    }

    #[test]
    fn test_recurring_error_punctuation_after_closing_quote() {
        // Comma after closing quote instead of space
        let key = compute_pattern_key(
            "insight_recurring_error",
            "'permission denied', error in 'Bash' has recurred 5 times",
        );
        let key_simple = compute_pattern_key(
            "insight_recurring_error",
            "'permission denied' error in 'Bash' has recurred 5 times",
        );
        assert_eq!(
            key, key_simple,
            "punctuation after closing quote should still extract the error"
        );
    }

    #[test]
    fn test_recurring_error_different_tools() {
        let key1 = compute_pattern_key(
            "insight_recurring_error",
            "Error in 'Read' has occurred 10 times without resolution: file does not exist",
        );
        let key2 = compute_pattern_key(
            "insight_recurring_error",
            "Error in 'Bash' has occurred 10 times without resolution: permission denied",
        );
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_recurring_error_unicode_safety() {
        // İ (U+0130) lowercases to "i\u{0307}" (2 chars), shifting byte offsets.
        // extract_error_identity operates entirely on lowercased string to avoid this.
        let key = compute_pattern_key(
            "insight_recurring_error",
            "İrror in 'Bash' has occurred 5 times without resolution: timeout",
        );
        // Should still extract "bash" as the tool
        let key_plain = compute_pattern_key(
            "insight_recurring_error",
            "Error in 'Bash' has occurred 5 times without resolution: timeout",
        );
        assert_eq!(key, key_plain, "Unicode should not affect tool extraction");
    }

    // ── Health degrading: one per project ───────────────────────────────

    #[test]
    fn test_health_degrading_always_same_key() {
        let key1 = compute_pattern_key(
            "insight_health_degrading",
            "Codebase health degraded: avg debt score 42.0 → 55.0 (+31% change, 8 modules)",
        );
        let key2 = compute_pattern_key(
            "insight_health_degrading",
            "Health getting worse: score went from 40 to 60 across 10 modules",
        );
        assert_eq!(
            key1, key2,
            "health_degrading should always produce the same key per project"
        );
    }

    // ── Recurring error: stable key fallback ────────────────────────────

    #[test]
    fn test_recurring_error_no_quotes_falls_back_to_description_hash() {
        // No "in 'TOOL'" pattern → extract_error_identity returns None → description hash
        let key1 = compute_pattern_key(
            "insight_recurring_error",
            "Recurring errors detected in the build pipeline",
        );
        let key2 = compute_pattern_key(
            "insight_recurring_error",
            "Recurring errors detected in the build pipeline",
        );
        assert_eq!(key1, key2, "identical description should match");

        // Verify it's NOT a stable key (it's a description hash)
        assert!(
            extract_error_identity("Recurring errors detected in the build pipeline").is_none(),
            "no quotes → extract_error_identity should return None"
        );
    }

    // ── Intra-batch dedup ───────────────────────────────────────────────

    /// Non-stable, non-session types are limited to 1 per batch.
    #[tokio::test]
    async fn test_intra_batch_limits_non_session_non_stable() {
        use crate::db::test_support::setup_test_pool;

        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/intra-batch", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        // LLM generates 3 variants of a workflow insight — only 1 should survive
        let insights = vec![
            PonderingInsight {
                pattern_type: "insight_workflow".to_string(),
                description: "Developer frequently switches between unrelated tasks".to_string(),
                confidence: 0.7,
                evidence: vec!["task switching".to_string()],
            },
            PonderingInsight {
                pattern_type: "insight_workflow".to_string(),
                description: "High task switching frequency observed across sessions".to_string(),
                confidence: 0.7,
                evidence: vec!["pattern".to_string()],
            },
        ];

        let stored = store_insights(&pool, project_id, &insights).await.unwrap();
        assert_eq!(
            stored, 1,
            "Non-stable non-session type limited to 1 per batch, got {}",
            stored,
        );
    }

    /// Entity-aware types CAN have multiple entries per batch (one per entity).
    #[tokio::test]
    async fn test_intra_batch_allows_multiple_entity_types() {
        use crate::db::test_support::setup_test_pool;

        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/intra-batch-entity", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        // Two different files — both should be stored
        let insights = vec![
            PonderingInsight {
                pattern_type: "insight_churn_hotspot".to_string(),
                description:
                    "'src/mcp/mod.rs' touched in 56 sessions over 20 days — high churn area"
                        .to_string(),
                confidence: 0.7,
                evidence: vec!["56 sessions".to_string()],
            },
            PonderingInsight {
                pattern_type: "insight_churn_hotspot".to_string(),
                description:
                    "'src/mcp/requests.rs' touched in 53 sessions over 20 days — high churn area"
                        .to_string(),
                confidence: 0.7,
                evidence: vec!["53 sessions".to_string()],
            },
        ];

        let stored = store_insights(&pool, project_id, &insights).await.unwrap();
        assert_eq!(
            stored, 2,
            "Entity-aware types should allow multiple per batch (different entities)"
        );
    }

    /// Distinct session patterns (from heuristic) should coexist since they
    /// have different normalized hashes. LLM variants are limited by the
    /// intra-batch type dedup + per-type eviction cap.
    #[tokio::test]
    async fn test_distinct_session_patterns_coexist() {
        use crate::db::test_support::setup_test_pool;

        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/session-distinct", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        // 3 distinct heuristic session patterns
        let insights = vec![
            PonderingInsight {
                pattern_type: "insight_session".to_string(),
                description: "18 sessions in the last 7 days lasted less than 5 minutes"
                    .to_string(),
                confidence: 0.5,
                evidence: vec!["count: 18".to_string()],
            },
            PonderingInsight {
                pattern_type: "insight_session".to_string(),
                description: "122 sessions in the last 7 days — high context-switching frequency"
                    .to_string(),
                confidence: 0.5,
                evidence: vec!["count: 122".to_string()],
            },
            PonderingInsight {
                pattern_type: "insight_session".to_string(),
                description: "55 sessions in the last 7 days ended without a summary".to_string(),
                confidence: 0.5,
                evidence: vec!["count: 55".to_string()],
            },
        ];

        // insight_session is exempt from intra-batch type limit,
        // so all 3 distinct patterns survive in a single batch.
        store_insights(&pool, project_id, &insights).await.unwrap();

        let count: i64 = pool
            .run(move |conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM behavior_patterns \
                     WHERE project_id = ? AND pattern_type = 'insight_session'",
                    params![project_id],
                    |row| row.get(0),
                )
                .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .unwrap();
        assert_eq!(
            count, 3,
            "Distinct session patterns should each get their own row"
        );
    }

    /// Upsert should reset dismissed=0 so recurring issues re-activate.
    #[tokio::test]
    async fn test_upsert_resets_dismissed_flag() {
        use crate::db::test_support::setup_test_pool;

        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/dismiss-reset", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        // Store an insight
        let insights = vec![PonderingInsight {
            pattern_type: "insight_health_degrading".to_string(),
            description: "Health degrading: score 42 → 55".to_string(),
            confidence: 0.7,
            evidence: vec!["test".to_string()],
        }];
        store_insights(&pool, project_id, &insights).await.unwrap();

        // Simulate auto-dismiss
        pool.run(move |conn| {
            conn.execute(
                "UPDATE behavior_patterns SET dismissed = 1 \
                 WHERE project_id = ? AND pattern_type = 'insight_health_degrading'",
                params![project_id],
            )
            .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .unwrap();

        // Store again (issue recurs) — should un-dismiss via upsert
        let insights2 = vec![PonderingInsight {
            pattern_type: "insight_health_degrading".to_string(),
            description: "Health degrading: score 48 → 60".to_string(),
            confidence: 0.75,
            evidence: vec!["updated".to_string()],
        }];
        store_insights(&pool, project_id, &insights2).await.unwrap();

        let dismissed: bool = pool
            .run(move |conn| {
                conn.query_row(
                    "SELECT dismissed FROM behavior_patterns \
                     WHERE project_id = ? AND pattern_type = 'insight_health_degrading'",
                    params![project_id],
                    |row| row.get(0),
                )
                .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .unwrap();
        assert!(
            !dismissed,
            "Upsert should reset dismissed=0 when issue recurs"
        );
    }

    #[test]
    fn test_session_insight_falls_back_to_description() {
        // Session/workflow insights have no entity, so they use normalized description
        let key1 = compute_pattern_key("insight_session", "5 sessions lasted under 5 minutes");
        let key2 = compute_pattern_key("insight_session", "8 sessions lasted under 5 minutes");
        // Numbers are normalized to #, so these should match
        assert_eq!(key1, key2);
    }

    // ── Text similarity tests ────────────────────────────────────────────

    #[test]
    fn test_text_similarity_identical() {
        let text = "File src/db/pool.rs modified 8 times";
        assert_eq!(text_similarity(text, text), 1.0);
    }

    #[test]
    fn test_text_similarity_completely_different() {
        let a = "File src/db/pool.rs has issues";
        let b = "Goal Quick Wins needs attention";
        let sim = text_similarity(a, b);
        assert!(sim < 0.3, "Expected similarity < 0.3, got {}", sim);
    }

    #[test]
    fn test_text_similarity_near_duplicate() {
        // Very similar descriptions with minor wording differences
        let a = "File src/db/pool.rs modified 8 times and shows high churn";
        let b = "File src/db/pool.rs modified 12 times and shows high churn";
        let sim = text_similarity(a, b);
        assert!(
            sim > SIMILARITY_THRESHOLD,
            "Expected similarity > {}, got {}",
            SIMILARITY_THRESHOLD,
            sim
        );
    }

    #[test]
    fn test_text_similarity_different_entities() {
        let a = "File src/db/pool.rs has issues";
        let b = "File src/auth/login.rs has issues";
        let sim = text_similarity(a, b);
        assert!(
            sim < SIMILARITY_THRESHOLD,
            "Expected similarity < {}, got {}",
            SIMILARITY_THRESHOLD,
            sim
        );
    }

    #[test]
    fn test_text_similarity_empty_strings() {
        assert_eq!(text_similarity("", ""), 0.0);
        assert_eq!(text_similarity("nonempty", ""), 0.0);
        assert_eq!(text_similarity("", "nonempty"), 0.0);
    }

    #[tokio::test]
    async fn test_store_insights_skips_near_duplicates() {
        use crate::db::test_support::setup_test_pool;

        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/dedup-test", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        // Store the first insight (workflow type, no entity-based dedup)
        let insights1 = vec![PonderingInsight {
            pattern_type: "insight_workflow".to_string(),
            description: "Multiple sessions show rapid task switching pattern with 8 context changes per hour".to_string(),
            confidence: 0.8,
            evidence: vec!["test".to_string()],
        }];

        let stored = store_insights(&pool, project_id, &insights1).await.unwrap();
        assert_eq!(stored, 1, "First insight should be stored");

        // Try to store a near-duplicate (same pattern, minor wording change)
        let insights2 = vec![PonderingInsight {
            pattern_type: "insight_workflow".to_string(),
            description: "Multiple sessions show rapid task switching pattern with 12 context changes per hour".to_string(),
            confidence: 0.9,
            evidence: vec!["test2".to_string()],
        }];

        let stored = store_insights(&pool, project_id, &insights2).await.unwrap();
        assert_eq!(stored, 0, "Near-duplicate insight should be skipped");

        // Store a completely different insight (should succeed)
        let insights3 = vec![PonderingInsight {
            pattern_type: "insight_workflow".to_string(),
            description: "Sessions typically last under 5 minutes indicating quick fixes"
                .to_string(),
            confidence: 0.85,
            evidence: vec!["test3".to_string()],
        }];

        let stored = store_insights(&pool, project_id, &insights3).await.unwrap();
        assert_eq!(stored, 1, "Different insight should be stored");
    }

    /// Codex finding 1: recurring insights about the SAME entity must still
    /// reach the upsert path to refresh occurrence_count and last_triggered_at.
    #[tokio::test]
    async fn test_recurring_insight_updates_occurrence_count() {
        use crate::db::test_support::setup_test_pool;

        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/recurring-test", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        // Store initial insight about a specific file (entity-aware key)
        let insights1 = vec![PonderingInsight {
            pattern_type: "insight_fragile_code".to_string(),
            description: "src/db/pool.rs has 40% failure rate — 4 reverted, 2 follow-up fixes out of 15 changes".to_string(),
            confidence: 0.6,
            evidence: vec!["test".to_string()],
        }];

        let stored = store_insights(&pool, project_id, &insights1).await.unwrap();
        assert_eq!(stored, 1, "First insight should be stored");

        // Store updated version of the SAME entity (same pattern_key via entity extraction)
        let insights2 = vec![PonderingInsight {
            pattern_type: "insight_fragile_code".to_string(),
            description: "src/db/pool.rs has 50% failure rate — 6 reverted, 4 follow-up fixes out of 20 changes".to_string(),
            confidence: 0.7,
            evidence: vec!["updated".to_string()],
        }];

        let stored = store_insights(&pool, project_id, &insights2).await.unwrap();
        assert_eq!(
            stored, 1,
            "Recurring insight must reach upsert to update occurrence_count"
        );

        // Verify occurrence_count was incremented
        let count: i64 = pool
            .run(move |conn| {
                conn.query_row(
                    "SELECT occurrence_count FROM behavior_patterns \
                     WHERE project_id = ? AND pattern_type = 'insight_fragile_code'",
                    params![project_id],
                    |row| row.get(0),
                )
                .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .unwrap();
        assert_eq!(count, 2, "occurrence_count should be 2 after upsert");
    }

    /// Codex finding 2: insights about DIFFERENT entities with similar template
    /// wording should both be stored.
    #[tokio::test]
    async fn test_different_entities_similar_template_both_stored() {
        use crate::db::test_support::setup_test_pool;

        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/entity-dedup-test", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        // Store insight about file A
        let insights1 = vec![PonderingInsight {
            pattern_type: "insight_fragile_code".to_string(),
            description: "src/db/pool.rs has 40% failure rate — 4 reverted out of 10 changes"
                .to_string(),
            confidence: 0.6,
            evidence: vec!["test".to_string()],
        }];
        let stored = store_insights(&pool, project_id, &insights1).await.unwrap();
        assert_eq!(stored, 1);

        // Store insight about file B with similar template
        let insights2 = vec![PonderingInsight {
            pattern_type: "insight_fragile_code".to_string(),
            description: "src/auth/login.rs has 40% failure rate — 4 reverted out of 10 changes"
                .to_string(),
            confidence: 0.6,
            evidence: vec!["test".to_string()],
        }];
        let stored = store_insights(&pool, project_id, &insights2).await.unwrap();
        assert_eq!(
            stored, 1,
            "Different entity should be stored even with similar template"
        );

        // Verify both exist
        let count: i64 = pool
            .run(move |conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM behavior_patterns \
                     WHERE project_id = ? AND pattern_type = 'insight_fragile_code'",
                    params![project_id],
                    |row| row.get(0),
                )
                .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .unwrap();
        assert_eq!(count, 2, "Both entity-distinct insights should exist");
    }

    /// Codex finding 2 (refined): very similar filenames like login.rs vs logout.rs
    /// would exceed 0.8 bigram Jaccard. Entity-aware types must skip similarity check.
    #[tokio::test]
    async fn test_similar_filenames_not_suppressed() {
        use crate::db::test_support::setup_test_pool;

        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/similar-names", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        // Verify these descriptions actually exceed 0.8 similarity
        let sim = text_similarity(
            "src/auth/login.rs has 40% failure rate — 4 reverted out of 10 changes",
            "src/auth/logout.rs has 40% failure rate — 4 reverted out of 10 changes",
        );
        assert!(
            sim > 0.8,
            "Precondition: similarity should be > 0.8, got {}",
            sim
        );

        let insights1 = vec![PonderingInsight {
            pattern_type: "insight_fragile_code".to_string(),
            description: "src/auth/login.rs has 40% failure rate — 4 reverted out of 10 changes"
                .to_string(),
            confidence: 0.6,
            evidence: vec!["test".to_string()],
        }];
        let stored = store_insights(&pool, project_id, &insights1).await.unwrap();
        assert_eq!(stored, 1);

        let insights2 = vec![PonderingInsight {
            pattern_type: "insight_fragile_code".to_string(),
            description: "src/auth/logout.rs has 40% failure rate — 4 reverted out of 10 changes"
                .to_string(),
            confidence: 0.6,
            evidence: vec!["test".to_string()],
        }];
        let stored = store_insights(&pool, project_id, &insights2).await.unwrap();
        assert_eq!(
            stored, 1,
            "Entity-aware type must not use text similarity — login.rs vs logout.rs are distinct"
        );
    }

    #[test]
    fn test_extract_goal_id() {
        assert_eq!(
            extract_goal_id("Goal 94 (deadpool migration) has been in_progress"),
            Some(94)
        );
        assert_eq!(extract_goal_id("goal 42 is stale"), Some(42));
        assert_eq!(extract_goal_id("Goal #123 needs attention"), Some(123));
        assert_eq!(extract_goal_id("no goal here"), None);
        assert_eq!(extract_goal_id("Goal without number"), None);
        // Unicode case-fold: İ (U+0130) lowercases to 2 chars, must not drift offsets
        assert_eq!(extract_goal_id("İGoal 94 stale"), Some(94));
        // Word boundary: "subgoal" and "supergoal" should not match
        assert_eq!(extract_goal_id("subgoal 94 still blocked"), None);
        assert_eq!(extract_goal_id("supergoal #12 and Goal 94 stale"), Some(94));
        // Space after # should still work
        assert_eq!(extract_goal_id("Goal # 94 stale"), Some(94));
    }

    #[tokio::test]
    async fn test_stale_goal_unquoted_dedup() {
        use crate::db::test_support::setup_test_pool;

        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/goal-dedup", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        // Two rephrasings of the same stale goal, neither using quotes
        let insights1 = vec![PonderingInsight {
            pattern_type: "insight_stale_goal".to_string(),
            description:
                "Goal 94 (deadpool migration) has been in_progress 23 days with 0/3 milestones"
                    .to_string(),
            confidence: 0.6,
            evidence: vec!["stale".to_string()],
        }];
        let stored = store_insights(&pool, project_id, &insights1).await.unwrap();
        assert_eq!(stored, 1);

        // Same goal, different phrasing, still unquoted
        let insights2 = vec![PonderingInsight {
            pattern_type: "insight_stale_goal".to_string(),
            description: "Goal 94 has made no progress in 23 days — consider breaking it down"
                .to_string(),
            confidence: 0.65,
            evidence: vec!["stale".to_string()],
        }];
        let stored = store_insights(&pool, project_id, &insights2).await.unwrap();
        assert_eq!(
            stored, 1,
            "Same goal ID should upsert, not create a duplicate"
        );

        // Different goal should still be stored separately
        let insights3 = vec![PonderingInsight {
            pattern_type: "insight_stale_goal".to_string(),
            description: "Goal 95 (auth refactor) has been blocked for 14 days".to_string(),
            confidence: 0.6,
            evidence: vec!["stale".to_string()],
        }];
        let stored = store_insights(&pool, project_id, &insights3).await.unwrap();
        assert_eq!(
            stored, 1,
            "Different goal ID should be stored as a new insight"
        );
    }
}
