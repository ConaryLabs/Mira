// crates/mira-server/src/db/insights_tests.rs
// Tests for unified insights digest — pondering, doc gaps, and health trends

use super::insights::*;
use super::test_support::setup_test_connection;
use rusqlite::params;

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════
    // Test Helpers
    // ═══════════════════════════════════════

    fn create_test_project(conn: &rusqlite::Connection) -> i64 {
        super::super::get_or_create_project_sync(conn, "/test/insights", Some("test"))
            .unwrap()
            .0
    }

    fn insert_behavior_pattern(
        conn: &rusqlite::Connection,
        project_id: i64,
        pattern_type: &str,
        pattern_data: &str,
        confidence: f64,
        triggered_at: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO behavior_patterns (project_id, pattern_type, pattern_key, pattern_data, confidence, last_triggered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                project_id,
                pattern_type,
                format!("key_{}", pattern_type),
                pattern_data,
                confidence,
                triggered_at
            ],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn insert_health_snapshot(
        conn: &rusqlite::Connection,
        project_id: i64,
        avg_score: f64,
        max_score: f64,
        module_count: i64,
        snapshot_at: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO health_snapshots (project_id, avg_debt_score, max_debt_score, tier_distribution, module_count, snapshot_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                project_id,
                avg_score,
                max_score,
                r#"{"A":5,"B":3}"#,
                module_count,
                snapshot_at
            ],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn insert_doc_task(
        conn: &rusqlite::Connection,
        project_id: i64,
        doc_type: &str,
        category: &str,
        path: &str,
        priority: &str,
    ) {
        conn.execute(
            "INSERT INTO documentation_tasks (project_id, doc_type, doc_category, target_doc_path, priority, status)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
            params![project_id, doc_type, category, path, priority],
        )
        .unwrap();
    }

    /// Helper to get a timestamp N days ago from now, formatted for SQLite.
    fn days_ago(n: i64) -> String {
        let now = chrono::Utc::now().naive_utc();
        let past = now - chrono::Duration::days(n);
        past.format("%Y-%m-%d %H:%M:%S").to_string()
    }

    /// Helper to get a timestamp N days in the future.
    fn days_from_now(n: i64) -> String {
        let now = chrono::Utc::now().naive_utc();
        let future = now + chrono::Duration::days(n);
        future.format("%Y-%m-%d %H:%M:%S").to_string()
    }

    // ═══════════════════════════════════════
    // compute_age_days Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_compute_age_days_recent_timestamp() {
        let ts = days_ago(5);
        let age = compute_age_days(&ts);
        // Should be approximately 5 days (within 0.1 tolerance for test timing)
        assert!((age - 5.0).abs() < 0.1, "Expected ~5.0 days, got {}", age);
    }

    #[test]
    fn test_compute_age_days_old_timestamp() {
        let ts = days_ago(30);
        let age = compute_age_days(&ts);
        assert!((age - 30.0).abs() < 0.1, "Expected ~30.0 days, got {}", age);
    }

    #[test]
    fn test_compute_age_days_malformed_timestamp() {
        let age = compute_age_days("not-a-date");
        assert_eq!(age, 0.0);
    }

    #[test]
    fn test_compute_age_days_empty_string() {
        let age = compute_age_days("");
        assert_eq!(age, 0.0);
    }

    #[test]
    fn test_compute_age_days_future_timestamp() {
        let ts = days_from_now(3);
        let age = compute_age_days(&ts);
        // Future timestamps produce negative age
        assert!(
            age < 0.0,
            "Expected negative age for future timestamp, got {}",
            age
        );
    }

    #[test]
    fn test_compute_age_days_just_now() {
        let ts = days_ago(0);
        let age = compute_age_days(&ts);
        // Should be close to zero
        assert!(
            age.abs() < 0.01,
            "Expected ~0.0 days for 'just now', got {}",
            age
        );
    }

    // ═══════════════════════════════════════
    // humanize_insight_type Tests
    // (Private fn — tested via get_unified_insights_sync source_type field)
    // ═══════════════════════════════════════

    #[test]
    fn test_humanize_known_types_via_unified_insights() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        let known_types = vec![
            ("insight_revert_cluster", "Revert Pattern"),
            ("insight_fragile_code", "Fragile Code"),
            ("insight_stale_goal", "Stale Goal"),
            ("insight_untested", "Untested Code"),
            ("insight_recurring_error", "Recurring Error"),
            ("insight_churn_hotspot", "Code Churn"),
            ("insight_health_degrading", "Health Degradation"),
            ("insight_session", "Session Pattern"),
            ("insight_workflow", "Workflow"),
        ];

        for (i, (pattern_type, expected_label)) in known_types.iter().enumerate() {
            // Use unique pattern_key to avoid UNIQUE constraint
            conn.execute(
                "INSERT INTO behavior_patterns (project_id, pattern_type, pattern_key, pattern_data, confidence, last_triggered_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    project_id,
                    pattern_type,
                    format!("key_{}", i),
                    r#"{"description":"test"}"#,
                    0.9,
                    now
                ],
            )
            .unwrap();

            let results =
                get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100)
                    .unwrap();

            let matching = results.iter().find(|r| r.source_type == *expected_label);
            assert!(
                matching.is_some(),
                "Expected source_type '{}' for pattern_type '{}', got types: {:?}",
                expected_label,
                pattern_type,
                results.iter().map(|r| &r.source_type).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn test_humanize_unknown_type_strips_prefix() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_custom_thing",
            r#"{"description":"custom insight"}"#,
            0.9,
            &now,
        );

        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100).unwrap();

        assert_eq!(results.len(), 1);
        // "insight_custom_thing" → strip "insight_" → "custom_thing" → replace _ with space → "custom thing"
        assert_eq!(results[0].source_type, "custom thing");
    }

    // ═══════════════════════════════════════
    // Scoring/Decay Logic Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_chronic_inverse_decay_14_days() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let ts = days_ago(14);

        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_stale_goal",
            r#"{"description":"stale goal test"}"#,
            0.9,
            &ts,
        );

        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100).unwrap();

        assert_eq!(results.len(), 1);
        // decay = (1.0 + 14/14).min(2.0) = 2.0
        // type_weight for insight_stale_goal = 0.9
        // priority_score = 0.9 * 0.9 * 2.0 = 1.62
        let expected = 0.9 * 0.9 * 2.0;
        assert!(
            (results[0].priority_score - expected).abs() < 0.05,
            "Expected priority_score ~{}, got {}",
            expected,
            results[0].priority_score
        );
    }

    #[test]
    fn test_acute_normal_decay_7_days() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let ts = days_ago(7);

        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_session",
            r#"{"description":"session pattern test"}"#,
            0.8,
            &ts,
        );

        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100).unwrap();

        assert_eq!(results.len(), 1);
        // decay = (1.0 - 7/14).max(0.3) = 0.5
        // type_weight for insight_session = 0.75
        // priority_score = 0.8 * 0.75 * 0.5 = 0.3
        let expected = 0.8 * 0.75 * 0.5;
        assert!(
            (results[0].priority_score - expected).abs() < 0.05,
            "Expected priority_score ~{}, got {}",
            expected,
            results[0].priority_score
        );
    }

    #[test]
    fn test_acute_decay_floor() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        // 30 days old — acute insight
        let ts = days_ago(13); // Within 14-day window for query, but old enough for decay

        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_workflow",
            r#"{"description":"old workflow"}"#,
            0.8,
            &ts,
        );

        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100).unwrap();

        assert_eq!(results.len(), 1);
        // decay = (1.0 - 13/14).max(0.3) = max(0.071, 0.3) = 0.3 (floor)
        // type_weight for insight_workflow = 0.7
        // priority_score = 0.8 * 0.7 * 0.3 = 0.168
        let expected = 0.8 * 0.7 * 0.3;
        assert!(
            (results[0].priority_score - expected).abs() < 0.05,
            "Expected priority_score ~{} (decay floor at 0.3), got {}",
            expected,
            results[0].priority_score
        );
    }

    #[test]
    fn test_chronic_decay_cap() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        // Very old chronic insight — within the query window
        let ts = days_ago(29);

        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_fragile_code",
            r#"{"description":"very old fragile code"}"#,
            0.9,
            &ts,
        );

        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100).unwrap();

        assert_eq!(results.len(), 1);
        // decay = (1.0 + 29/14).min(2.0) = min(3.07, 2.0) = 2.0 (cap)
        // type_weight for insight_fragile_code = 0.95
        // priority_score = 0.9 * 0.95 * 2.0 = 1.71
        let expected = 0.9 * 0.95 * 2.0;
        assert!(
            (results[0].priority_score - expected).abs() < 0.05,
            "Expected priority_score ~{} (decay cap at 2.0), got {}",
            expected,
            results[0].priority_score
        );
    }

    #[test]
    fn test_fresh_insight_no_decay() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let ts = days_ago(0);

        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_revert_cluster",
            r#"{"description":"recent revert"}"#,
            0.95,
            &ts,
        );

        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100).unwrap();

        assert_eq!(results.len(), 1);
        // decay = (1.0 - 0/14).max(0.3) ≈ 1.0 for acute type (insight_revert_cluster)
        // type_weight = 1.0
        // priority_score = 0.95 * 1.0 * 1.0 = 0.95
        let expected = 0.95 * 1.0 * 1.0;
        assert!(
            (results[0].priority_score - expected).abs() < 0.05,
            "Expected priority_score ~{}, got {}",
            expected,
            results[0].priority_score
        );
    }

    // ═══════════════════════════════════════
    // get_unified_insights_sync Merge Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_merge_all_sources() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        // Insert pondering insight
        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_stale_goal",
            r#"{"description":"a stale goal"}"#,
            0.9,
            &now,
        );

        // Insert doc gap
        insert_doc_task(&conn, project_id, "api", "endpoint", "/docs/api.md", "high");

        // Insert health snapshots (need 2 for trend, with >10% delta)
        insert_health_snapshot(&conn, project_id, 50.0, 80.0, 10, &days_ago(7));
        insert_health_snapshot(&conn, project_id, 60.0, 90.0, 10, &now);

        let results = get_unified_insights_sync(&conn, project_id, None, 0.0, 30, 100).unwrap();

        let sources: Vec<&str> = results.iter().map(|r| r.source.as_str()).collect();
        assert!(
            sources.contains(&"pondering"),
            "Missing pondering source, got: {:?}",
            sources
        );
        assert!(
            sources.contains(&"doc_gap"),
            "Missing doc_gap source, got: {:?}",
            sources
        );
        assert!(
            sources.contains(&"health_trend"),
            "Missing health_trend source, got: {:?}",
            sources
        );
    }

    #[test]
    fn test_min_confidence_filters_on_confidence() {
        // min_confidence filters on the confidence field, not priority_score.
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        // High confidence (0.95) — should pass min_confidence=0.5
        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_revert_cluster",
            r#"{"description":"high confidence"}"#,
            0.95,
            &now,
        );

        // Low confidence (0.3) — should be filtered out by min_confidence=0.5
        conn.execute(
            "INSERT INTO behavior_patterns (project_id, pattern_type, pattern_key, pattern_data, confidence, last_triggered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                project_id,
                "insight_workflow",
                "key_low",
                r#"{"description":"low confidence"}"#,
                0.3,
                now
            ],
        )
        .unwrap();

        // min_confidence=0.5 should filter out the 0.3 confidence insight
        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.5, 30, 100).unwrap();

        assert!(
            !results.is_empty(),
            "Expected at least 1 result after filtering"
        );
        // All surviving results must have confidence >= 0.5
        assert!(
            results.iter().all(|r| r.confidence >= 0.5),
            "All results should have confidence >= 0.5, got: {:?}",
            results.iter().map(|r| r.confidence).collect::<Vec<_>>()
        );
        // The low-confidence one (0.3) must NOT be present
        assert!(
            results
                .iter()
                .all(|r| !r.description.contains("low confidence")),
            "Low-confidence insight should have been filtered out"
        );
    }

    #[test]
    fn test_sorting_by_priority_score_then_timestamp() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        // Insert insights with different scores
        // High confidence chronic (will get high priority_score due to inverse decay)
        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_stale_goal",
            r#"{"description":"stale goal high"}"#,
            0.95,
            &days_ago(10),
        );

        // Low confidence acute (low priority_score)
        conn.execute(
            "INSERT INTO behavior_patterns (project_id, pattern_type, pattern_key, pattern_data, confidence, last_triggered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                project_id,
                "insight_workflow",
                "key_sort_low",
                r#"{"description":"workflow low"}"#,
                0.5,
                days_ago(1)
            ],
        )
        .unwrap();

        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100).unwrap();

        assert!(results.len() >= 2);
        // Should be sorted by priority_score descending
        for i in 1..results.len() {
            assert!(
                results[i - 1].priority_score >= results[i].priority_score,
                "Results not sorted: {} < {} at index {}",
                results[i - 1].priority_score,
                results[i].priority_score,
                i
            );
        }
    }

    #[test]
    fn test_limit_truncates_results() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        for i in 0..10 {
            conn.execute(
                "INSERT INTO behavior_patterns (project_id, pattern_type, pattern_key, pattern_data, confidence, last_triggered_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    project_id,
                    "insight_session",
                    format!("key_limit_{}", i),
                    format!(r#"{{"description":"insight {}"}}"#, i),
                    0.9,
                    now
                ],
            )
            .unwrap();
        }

        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 3).unwrap();

        assert_eq!(results.len(), 3, "Limit should cap results at 3");
    }

    #[test]
    fn test_filter_source_pondering_only() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_stale_goal",
            r#"{"description":"pondering insight"}"#,
            0.9,
            &now,
        );
        insert_doc_task(&conn, project_id, "api", "endpoint", "/docs/api.md", "high");

        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100).unwrap();

        assert!(
            results.iter().all(|r| r.source == "pondering"),
            "filter_source='pondering' should exclude doc_gap"
        );
    }

    #[test]
    fn test_filter_source_doc_gap_only() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_session",
            r#"{"description":"pondering"}"#,
            0.9,
            &now,
        );
        insert_doc_task(
            &conn,
            project_id,
            "guide",
            "setup",
            "/docs/setup.md",
            "medium",
        );

        let results =
            get_unified_insights_sync(&conn, project_id, Some("doc_gap"), 0.0, 30, 100).unwrap();

        assert!(
            results.iter().all(|r| r.source == "doc_gap"),
            "filter_source='doc_gap' should exclude pondering"
        );
        assert!(!results.is_empty(), "Should have doc_gap results");
    }

    #[test]
    fn test_filter_source_health_trend_only() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_session",
            r#"{"description":"pondering"}"#,
            0.9,
            &now,
        );
        // Two snapshots with >10% delta for health trend
        insert_health_snapshot(&conn, project_id, 40.0, 60.0, 5, &days_ago(3));
        insert_health_snapshot(&conn, project_id, 50.0, 70.0, 5, &now);

        let results =
            get_unified_insights_sync(&conn, project_id, Some("health_trend"), 0.0, 30, 100)
                .unwrap();

        assert!(
            results.iter().all(|r| r.source == "health_trend"),
            "filter_source='health_trend' should exclude pondering"
        );
        assert!(!results.is_empty(), "Should have health_trend results");
    }

    #[test]
    fn test_empty_insights() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        let results = get_unified_insights_sync(&conn, project_id, None, 0.0, 30, 100).unwrap();

        assert!(
            results.is_empty(),
            "Empty project should return no insights"
        );
    }

    #[test]
    fn test_dismissed_insights_excluded() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        let row_id = insert_behavior_pattern(
            &conn,
            project_id,
            "insight_session",
            r#"{"description":"dismissed"}"#,
            0.9,
            &now,
        );

        // Manually dismiss
        conn.execute(
            "UPDATE behavior_patterns SET dismissed = 1 WHERE id = ?1",
            params![row_id],
        )
        .unwrap();

        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100).unwrap();

        assert!(results.is_empty(), "Dismissed insights should be excluded");
    }

    #[test]
    fn test_insight_description_from_json() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_stale_goal",
            r#"{"description":"My custom description","evidence":"some evidence here"}"#,
            0.9,
            &now,
        );

        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].description, "My custom description");
        assert_eq!(results[0].evidence, Some("some evidence here".to_string()));
    }

    #[test]
    fn test_insight_description_non_json_fallback() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_session",
            "plain text data, not JSON",
            0.8,
            &now,
        );

        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100).unwrap();

        assert_eq!(results.len(), 1);
        // Falls back to raw pattern_data as description
        assert_eq!(results[0].description, "plain text data, not JSON");
        assert!(results[0].evidence.is_none());
    }

    #[test]
    fn test_insight_evidence_from_array() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_recurring_error",
            r#"{"description":"errors","evidence":["file1.rs","file2.rs","file3.rs"]}"#,
            0.9,
            &now,
        );

        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].evidence,
            Some("file1.rs; file2.rs; file3.rs".to_string())
        );
    }

    // ═══════════════════════════════════════
    // fetch_health_trend_insights Tests
    // (Private fn — tested via get_unified_insights_sync)
    // ═══════════════════════════════════════

    #[test]
    fn test_health_trend_less_than_2_snapshots() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        // Only 1 snapshot
        insert_health_snapshot(&conn, project_id, 50.0, 80.0, 10, &days_ago(1));

        let results =
            get_unified_insights_sync(&conn, project_id, Some("health_trend"), 0.0, 30, 100)
                .unwrap();

        assert!(
            results.is_empty(),
            "Less than 2 snapshots should return no health trend"
        );
    }

    #[test]
    fn test_health_trend_small_delta_no_insight() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        // Two snapshots with < 10% delta
        insert_health_snapshot(&conn, project_id, 50.0, 80.0, 10, &days_ago(3));
        insert_health_snapshot(&conn, project_id, 52.0, 82.0, 10, &days_ago(0));

        let results =
            get_unified_insights_sync(&conn, project_id, Some("health_trend"), 0.0, 30, 100)
                .unwrap();

        assert!(
            results.is_empty(),
            "Delta < 10% should not produce a health trend insight"
        );
    }

    #[test]
    fn test_health_trend_degraded() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        // prev_avg=40.0, current_avg=50.0 → delta = +25% → degraded
        insert_health_snapshot(&conn, project_id, 40.0, 60.0, 8, &days_ago(5));
        insert_health_snapshot(&conn, project_id, 50.0, 70.0, 8, &now);

        let results =
            get_unified_insights_sync(&conn, project_id, Some("health_trend"), 0.0, 30, 100)
                .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].trend, Some("degraded".to_string()));
        assert!(results[0].description.contains("degraded"));
        assert_eq!(results[0].source, "health_trend");
        assert_eq!(results[0].source_type, "Health Trend");
    }

    #[test]
    fn test_health_trend_improved() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        // prev_avg=60.0, current_avg=40.0 → delta = -33% → improved
        insert_health_snapshot(&conn, project_id, 60.0, 80.0, 12, &days_ago(5));
        insert_health_snapshot(&conn, project_id, 40.0, 60.0, 12, &now);

        let results =
            get_unified_insights_sync(&conn, project_id, Some("health_trend"), 0.0, 30, 100)
                .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].trend, Some("improved".to_string()));
        assert!(results[0].description.contains("improved"));
    }

    #[test]
    fn test_health_trend_prev_avg_zero_with_current() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        // prev_avg=0.0 with current_avg>0 → returns a "baseline" trend
        insert_health_snapshot(&conn, project_id, 0.0, 0.0, 5, &days_ago(3));
        insert_health_snapshot(&conn, project_id, 50.0, 70.0, 5, &now);

        let results =
            get_unified_insights_sync(&conn, project_id, Some("health_trend"), 0.0, 30, 100)
                .unwrap();

        assert_eq!(
            results.len(),
            1,
            "prev_avg=0 with current>0 should return baseline insight"
        );
        assert_eq!(results[0].trend, Some("baseline".to_string()));
        assert!(results[0].description.contains("baseline"));
    }

    #[test]
    fn test_health_trend_prev_avg_zero_both_zero() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        // prev_avg=0.0 and current_avg=0.0 → empty (nothing to report)
        insert_health_snapshot(&conn, project_id, 0.0, 0.0, 5, &days_ago(3));
        insert_health_snapshot(&conn, project_id, 0.0, 0.0, 5, &now);

        let results =
            get_unified_insights_sync(&conn, project_id, Some("health_trend"), 0.0, 30, 100)
                .unwrap();

        assert!(
            results.is_empty(),
            "Both prev_avg and current_avg=0 should return empty"
        );
    }

    #[test]
    fn test_health_trend_7day_average_evidence() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        // Insert multiple snapshots within 7 days for averaging
        insert_health_snapshot(&conn, project_id, 40.0, 60.0, 10, &days_ago(6));
        insert_health_snapshot(&conn, project_id, 42.0, 62.0, 10, &days_ago(4));
        insert_health_snapshot(&conn, project_id, 55.0, 75.0, 10, &now);

        let results =
            get_unified_insights_sync(&conn, project_id, Some("health_trend"), 0.0, 30, 100)
                .unwrap();

        assert_eq!(results.len(), 1);
        // Evidence should contain 7-day average info
        assert!(
            results[0].evidence.is_some(),
            "Should have 7-day average evidence"
        );
        let evidence = results[0].evidence.as_ref().unwrap();
        assert!(
            evidence.contains("7-day avg"),
            "Evidence should mention 7-day avg: {}",
            evidence
        );
    }

    #[test]
    fn test_health_trend_confidence_large_delta() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        // delta > 25% → confidence = 0.85
        insert_health_snapshot(&conn, project_id, 30.0, 50.0, 5, &days_ago(3));
        insert_health_snapshot(&conn, project_id, 50.0, 70.0, 5, &now);

        let results =
            get_unified_insights_sync(&conn, project_id, Some("health_trend"), 0.0, 30, 100)
                .unwrap();

        assert_eq!(results.len(), 1);
        assert!(
            (results[0].confidence - 0.85).abs() < 0.01,
            "Large delta should have confidence=0.85, got {}",
            results[0].confidence
        );
    }

    #[test]
    fn test_health_trend_confidence_moderate_delta() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        // 10% < delta < 25% → confidence = 0.7
        // prev=50.0, current=56.0 → delta = 12%
        insert_health_snapshot(&conn, project_id, 50.0, 70.0, 5, &days_ago(3));
        insert_health_snapshot(&conn, project_id, 56.0, 76.0, 5, &now);

        let results =
            get_unified_insights_sync(&conn, project_id, Some("health_trend"), 0.0, 30, 100)
                .unwrap();

        assert_eq!(results.len(), 1);
        assert!(
            (results[0].confidence - 0.7).abs() < 0.01,
            "Moderate delta should have confidence=0.7, got {}",
            results[0].confidence
        );
    }

    #[test]
    fn test_health_trend_change_summary() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        insert_health_snapshot(&conn, project_id, 40.0, 60.0, 5, &days_ago(3));
        insert_health_snapshot(&conn, project_id, 55.0, 75.0, 5, &now);

        let results =
            get_unified_insights_sync(&conn, project_id, Some("health_trend"), 0.0, 30, 100)
                .unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].change_summary.is_some());
        let summary = results[0].change_summary.as_ref().unwrap();
        assert!(
            summary.contains("40.0") && summary.contains("55.0"),
            "Change summary should show prev→current: {}",
            summary
        );
    }

    // ═══════════════════════════════════════
    // auto_dismiss_stale_insights Tests
    // (Private fn — tested via get_unified_insights_sync side effects)
    // ═══════════════════════════════════════

    #[test]
    fn test_auto_dismiss_acute_older_than_14_days() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        // Acute insight older than 14 days
        let row_id = insert_behavior_pattern(
            &conn,
            project_id,
            "insight_session",
            r#"{"description":"old acute"}"#,
            0.8,
            &days_ago(20),
        );

        // Calling get_unified_insights triggers auto_dismiss
        let _results = get_unified_insights_sync(&conn, project_id, None, 0.0, 30, 100).unwrap();

        // Verify the row is dismissed
        let dismissed: i64 = conn
            .query_row(
                "SELECT dismissed FROM behavior_patterns WHERE id = ?1",
                params![row_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            dismissed, 1,
            "Acute insight older than 14 days should be auto-dismissed"
        );
    }

    #[test]
    fn test_auto_dismiss_chronic_not_dismissed() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        // Chronic insight (stale_goal) older than 14 days — should NOT be dismissed
        let row_id = insert_behavior_pattern(
            &conn,
            project_id,
            "insight_stale_goal",
            r#"{"description":"old chronic"}"#,
            0.8,
            &days_ago(20),
        );

        let _results = get_unified_insights_sync(&conn, project_id, None, 0.0, 30, 100).unwrap();

        let dismissed: i64 = conn
            .query_row(
                "SELECT COALESCE(dismissed, 0) FROM behavior_patterns WHERE id = ?1",
                params![row_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            dismissed, 0,
            "Chronic insight (insight_stale_goal) should NOT be auto-dismissed"
        );
    }

    #[test]
    fn test_auto_dismiss_chronic_types_preserved() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        // All chronic types should be preserved
        let chronic_types = [
            "insight_stale_goal",
            "insight_fragile_code",
            "insight_untested",
            "insight_recurring_error",
            "insight_health_degrading",
        ];

        let mut row_ids = Vec::new();
        for (i, chronic_type) in chronic_types.iter().enumerate() {
            let row_id = conn
                .execute(
                    "INSERT INTO behavior_patterns (project_id, pattern_type, pattern_key, pattern_data, confidence, last_triggered_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        project_id,
                        chronic_type,
                        format!("chronic_key_{}", i),
                        r#"{"description":"chronic"}"#,
                        0.8,
                        days_ago(20)
                    ],
                )
                .map(|_| conn.last_insert_rowid())
                .unwrap();
            row_ids.push((chronic_type.to_string(), row_id));
        }

        let _results = get_unified_insights_sync(&conn, project_id, None, 0.0, 30, 100).unwrap();

        for (pattern_type, row_id) in &row_ids {
            let dismissed: i64 = conn
                .query_row(
                    "SELECT COALESCE(dismissed, 0) FROM behavior_patterns WHERE id = ?1",
                    params![row_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                dismissed, 0,
                "Chronic type '{}' (id={}) should NOT be auto-dismissed",
                pattern_type, row_id
            );
        }
    }

    #[test]
    fn test_auto_dismiss_recent_not_dismissed() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        // Recent acute insight — should NOT be dismissed
        let row_id = insert_behavior_pattern(
            &conn,
            project_id,
            "insight_session",
            r#"{"description":"recent acute"}"#,
            0.8,
            &days_ago(3),
        );

        let _results = get_unified_insights_sync(&conn, project_id, None, 0.0, 30, 100).unwrap();

        let dismissed: i64 = conn
            .query_row(
                "SELECT COALESCE(dismissed, 0) FROM behavior_patterns WHERE id = ?1",
                params![row_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dismissed, 0, "Recent insight should NOT be auto-dismissed");
    }

    #[test]
    fn test_auto_dismiss_already_dismissed_stays_dismissed() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        let row_id = insert_behavior_pattern(
            &conn,
            project_id,
            "insight_session",
            r#"{"description":"already dismissed"}"#,
            0.8,
            &days_ago(20),
        );

        // Pre-dismiss
        conn.execute(
            "UPDATE behavior_patterns SET dismissed = 1 WHERE id = ?1",
            params![row_id],
        )
        .unwrap();

        let _results = get_unified_insights_sync(&conn, project_id, None, 0.0, 30, 100).unwrap();

        let dismissed: i64 = conn
            .query_row(
                "SELECT dismissed FROM behavior_patterns WHERE id = ?1",
                params![row_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            dismissed, 1,
            "Already dismissed insight should stay dismissed"
        );
    }

    // ═══════════════════════════════════════
    // dismiss_insight_sync Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_dismiss_valid_insight() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        let row_id = insert_behavior_pattern(
            &conn,
            project_id,
            "insight_stale_goal",
            r#"{"description":"dismiss me"}"#,
            0.9,
            &now,
        );

        let result = dismiss_insight_sync(&conn, project_id, row_id).unwrap();
        assert!(result, "Should return true for valid dismissal");

        // Verify dismissed
        let dismissed: i64 = conn
            .query_row(
                "SELECT dismissed FROM behavior_patterns WHERE id = ?1",
                params![row_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dismissed, 1);
    }

    #[test]
    fn test_dismiss_wrong_project_id() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        let row_id = insert_behavior_pattern(
            &conn,
            project_id,
            "insight_session",
            r#"{"description":"test"}"#,
            0.8,
            &now,
        );

        // Use wrong project_id
        let result = dismiss_insight_sync(&conn, project_id + 999, row_id).unwrap();
        assert!(!result, "Should return false when project_id doesn't match");

        // Verify not dismissed
        let dismissed: i64 = conn
            .query_row(
                "SELECT COALESCE(dismissed, 0) FROM behavior_patterns WHERE id = ?1",
                params![row_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dismissed, 0);
    }

    #[test]
    fn test_dismiss_non_insight_pattern_type() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        // Insert a non-insight pattern (no "insight_" prefix)
        conn.execute(
            "INSERT INTO behavior_patterns (project_id, pattern_type, pattern_key, pattern_data, confidence, last_triggered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                project_id,
                "file_sequence",
                "key_non_insight",
                "{}",
                0.5,
                now
            ],
        )
        .unwrap();
        let row_id = conn.last_insert_rowid();

        let result = dismiss_insight_sync(&conn, project_id, row_id).unwrap();
        assert!(!result, "Should return false for non-insight pattern_type");
    }

    #[test]
    fn test_dismiss_already_dismissed() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        let row_id = insert_behavior_pattern(
            &conn,
            project_id,
            "insight_workflow",
            r#"{"description":"test"}"#,
            0.8,
            &now,
        );

        // First dismiss
        let result1 = dismiss_insight_sync(&conn, project_id, row_id).unwrap();
        assert!(result1, "First dismiss should succeed");

        // Second dismiss — already dismissed
        let result2 = dismiss_insight_sync(&conn, project_id, row_id).unwrap();
        assert!(!result2, "Should return false when already dismissed");
    }

    #[test]
    fn test_dismiss_nonexistent_id() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        let result = dismiss_insight_sync(&conn, project_id, 99999).unwrap();
        assert!(!result, "Should return false for nonexistent ID");
    }

    // ═══════════════════════════════════════
    // Doc Gap Insights Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_doc_gap_insights_basic() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        insert_doc_task(&conn, project_id, "api", "endpoint", "/docs/api.md", "high");
        insert_doc_task(
            &conn,
            project_id,
            "guide",
            "setup",
            "/docs/setup.md",
            "medium",
        );

        let results =
            get_unified_insights_sync(&conn, project_id, Some("doc_gap"), 0.0, 30, 100).unwrap();

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.source == "doc_gap"));
        // High priority should come first
        assert!(
            results[0].priority_score >= results[1].priority_score,
            "Should be sorted by priority_score desc"
        );
    }

    #[test]
    fn test_doc_gap_completed_excluded() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        insert_doc_task(&conn, project_id, "api", "endpoint", "/docs/api.md", "high");

        // Mark as completed
        conn.execute(
            "UPDATE documentation_tasks SET status = 'completed' WHERE project_id = ?1",
            params![project_id],
        )
        .unwrap();

        let results =
            get_unified_insights_sync(&conn, project_id, Some("doc_gap"), 0.0, 30, 100).unwrap();

        assert!(
            results.is_empty(),
            "Completed doc tasks should not appear as insights"
        );
    }

    #[test]
    fn test_doc_gap_description_format() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        insert_doc_task(
            &conn,
            project_id,
            "architecture",
            "module",
            "/docs/arch.md",
            "urgent",
        );

        let results =
            get_unified_insights_sync(&conn, project_id, Some("doc_gap"), 0.0, 30, 100).unwrap();

        assert_eq!(results.len(), 1);
        assert!(
            results[0].description.contains("module"),
            "Description should include category"
        );
        assert!(
            results[0].description.contains("/docs/arch.md"),
            "Description should include target path"
        );
        assert!(
            results[0].description.contains("architecture"),
            "Description should include doc_type"
        );
    }

    #[test]
    fn test_doc_gap_priority_scores() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        insert_doc_task(&conn, project_id, "api", "a", "/a.md", "urgent");
        insert_doc_task(&conn, project_id, "api", "b", "/b.md", "high");
        insert_doc_task(&conn, project_id, "api", "c", "/c.md", "medium");
        insert_doc_task(&conn, project_id, "api", "d", "/d.md", "low");

        let results =
            get_unified_insights_sync(&conn, project_id, Some("doc_gap"), 0.0, 30, 100).unwrap();

        assert_eq!(results.len(), 4);
        // Verify priority scores match the priority_score function
        let scores: Vec<f64> = results.iter().map(|r| r.priority_score).collect();
        assert!(
            scores[0] >= scores[1] && scores[1] >= scores[2] && scores[2] >= scores[3],
            "Scores should be descending: {:?}",
            scores
        );
    }

    // ═══════════════════════════════════════
    // Project Isolation Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_insights_project_isolation() {
        let conn = setup_test_connection();
        let project1 = create_test_project(&conn);
        let (project2, _) =
            super::super::get_or_create_project_sync(&conn, "/test/other", Some("other")).unwrap();

        let now = days_ago(0);

        insert_behavior_pattern(
            &conn,
            project1,
            "insight_session",
            r#"{"description":"project 1 insight"}"#,
            0.9,
            &now,
        );
        insert_behavior_pattern(
            &conn,
            project2,
            "insight_workflow",
            r#"{"description":"project 2 insight"}"#,
            0.9,
            &now,
        );

        let results1 =
            get_unified_insights_sync(&conn, project1, Some("pondering"), 0.0, 30, 100).unwrap();
        let results2 =
            get_unified_insights_sync(&conn, project2, Some("pondering"), 0.0, 30, 100).unwrap();

        assert_eq!(results1.len(), 1);
        assert_eq!(results2.len(), 1);
        assert!(results1[0].description.contains("project 1"));
        assert!(results2[0].description.contains("project 2"));
    }

    // ═══════════════════════════════════════
    // Row ID Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_pondering_insight_has_row_id() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        let inserted_id = insert_behavior_pattern(
            &conn,
            project_id,
            "insight_session",
            r#"{"description":"test row_id"}"#,
            0.9,
            &now,
        );

        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].row_id, Some(inserted_id));
    }

    #[test]
    fn test_doc_gap_insight_has_no_row_id() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        insert_doc_task(&conn, project_id, "api", "endpoint", "/docs/api.md", "high");

        let results =
            get_unified_insights_sync(&conn, project_id, Some("doc_gap"), 0.0, 30, 100).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].row_id, None);
    }

    #[test]
    fn test_health_trend_insight_has_no_row_id() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);
        let now = days_ago(0);

        insert_health_snapshot(&conn, project_id, 40.0, 60.0, 5, &days_ago(3));
        insert_health_snapshot(&conn, project_id, 55.0, 75.0, 5, &now);

        let results =
            get_unified_insights_sync(&conn, project_id, Some("health_trend"), 0.0, 30, 100)
                .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].row_id, None);
    }

    // ═══════════════════════════════════════
    // days_back Filter Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_days_back_filters_old_pondering() {
        let conn = setup_test_connection();
        let project_id = create_test_project(&conn);

        // Insert insight 20 days ago (chronic type so it won't be auto-dismissed)
        insert_behavior_pattern(
            &conn,
            project_id,
            "insight_stale_goal",
            r#"{"description":"old insight"}"#,
            0.9,
            &days_ago(20),
        );

        // Query with days_back=10 — should exclude the 20-day-old insight
        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 10, 100).unwrap();

        assert!(
            results.is_empty(),
            "Insight older than days_back should be excluded"
        );

        // Query with days_back=30 — should include it
        let results =
            get_unified_insights_sync(&conn, project_id, Some("pondering"), 0.0, 30, 100).unwrap();

        assert_eq!(
            results.len(),
            1,
            "Insight within days_back should be included"
        );
    }
}
