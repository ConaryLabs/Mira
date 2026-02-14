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
                .unwrap()
                .is_alphanumeric();
        if before_ok {
            let after = lower[abs_pos + 4..].trim_start();
            let after = after.strip_prefix('#').map(|s| s.trim_start()).unwrap_or(after);
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

/// Compute a 16-hex-char pattern key, using the primary entity for
/// file/module-based insight types to avoid false-unique keys from
/// different LLM phrasings about the same entity.
fn compute_pattern_key(pattern_type: &str, description: &str) -> String {
    // For file/module insights, key on the entity not the prose
    if matches!(
        pattern_type,
        "insight_fragile_code" | "insight_untested" | "insight_revert_cluster"
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
    // Default: current behavior (normalized description hash)
    hash_key(&normalize_for_dedup(description))
}

/// Maximum number of insights to keep per pattern_type per project.
const MAX_INSIGHTS_PER_TYPE: i64 = 10;

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

        for insight in &insights_clone {
            // Generate pattern key using entity-aware hashing
            let pattern_key = compute_pattern_key(&insight.pattern_type, &insight.description);
            // Also compute legacy key (pre-entity-aware) so old dismissals are honoured
            let legacy_key = hash_key(&normalize_for_dedup(&insight.description));

            // Skip if this pattern was previously dismissed (check both key strategies)
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

            // Text similarity dedup — only for non-entity-aware pattern types.
            // Entity-aware types (fragile_code, untested, revert_cluster, stale_goal)
            // already have identity-preserving keys via compute_pattern_key(), so
            // same-entity recurrence is handled by the upsert and different entities
            // must not be suppressed (e.g., src/auth/login.rs vs src/auth/logout.rs
            // would exceed 0.8 bigram Jaccard despite being distinct entities).
            let is_entity_aware = matches!(
                insight.pattern_type.as_str(),
                "insight_fragile_code"
                    | "insight_untested"
                    | "insight_revert_cluster"
                    | "insight_stale_goal"
            );

            if !is_entity_aware {
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
                    text_similarity(&insight.description, existing_desc) > 0.8
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
                    last_triggered_at = datetime('now')
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
                }
                Err(e) => tracing::warn!("Failed to store insight: {}", e),
            }
        }

        // Enforce per-type cap: keep the newest MAX_INSIGHTS_PER_TYPE, evict oldest
        for pattern_type in &types_touched {
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
                    params![
                        project_id,
                        pattern_type,
                        project_id,
                        pattern_type,
                        MAX_INSIGHTS_PER_TYPE,
                    ],
                )
                .unwrap_or(0);
            if evicted > 0 {
                tracing::info!(
                    "Evicted {} old '{}' insights (cap: {})",
                    evicted,
                    pattern_type,
                    MAX_INSIGHTS_PER_TYPE,
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

    #[tokio::test]
    async fn test_legacy_dismissed_key_blocks_new_entity_key() {
        use crate::db::test_support::setup_test_pool;

        let pool = setup_test_pool().await;

        // Create a project
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

        // Now try to store the same insight — it should be blocked by the legacy dismissal
        let insights = vec![PonderingInsight {
            pattern_type: pattern_type.to_string(),
            description: description.to_string(),
            confidence: 0.8,
            evidence: vec!["test".to_string()],
        }];

        let stored = store_insights(&pool, project_id, &insights).await.unwrap();
        assert_eq!(
            stored, 0,
            "insight should be blocked by legacy dismissed key"
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
        assert!(sim > 0.8, "Expected similarity > 0.8, got {}", sim);
    }

    #[test]
    fn test_text_similarity_different_entities() {
        let a = "File src/db/pool.rs has issues";
        let b = "File src/auth/login.rs has issues";
        let sim = text_similarity(a, b);
        assert!(sim < 0.8, "Expected similarity < 0.8, got {}", sim);
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
        assert_eq!(extract_goal_id("Goal 94 (deadpool migration) has been in_progress"), Some(94));
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
            description: "Goal 94 (deadpool migration) has been in_progress 23 days with 0/3 milestones".to_string(),
            confidence: 0.6,
            evidence: vec!["stale".to_string()],
        }];
        let stored = store_insights(&pool, project_id, &insights1).await.unwrap();
        assert_eq!(stored, 1);

        // Same goal, different phrasing, still unquoted
        let insights2 = vec![PonderingInsight {
            pattern_type: "insight_stale_goal".to_string(),
            description: "Goal 94 has made no progress in 23 days — consider breaking it down".to_string(),
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
