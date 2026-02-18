// background/diff_analysis/tests.rs

use super::format::format_change_markers;
use super::*;
use crate::db::DiffAnalysis;

#[test]
fn test_diff_stats_default() {
    let stats = DiffStats::default();
    assert_eq!(stats.files_changed, 0);
    assert_eq!(stats.lines_added, 0);
    assert_eq!(stats.lines_removed, 0);
    assert!(stats.files.is_empty());
}

#[test]
fn test_calculate_risk_level_low() {
    let flags: Vec<String> = vec![];
    let changes: Vec<SemanticChange> = vec![];
    assert_eq!(calculate_risk_level(&flags, &changes), "Low");
}

#[test]
fn test_calculate_risk_level_medium_with_flags() {
    let flags: Vec<String> = vec![
        "api_change".to_string(),
        "dependency_update".to_string(),
        "new_feature".to_string(),
        "config_change".to_string(),
    ];
    let changes: Vec<SemanticChange> = vec![];
    assert_eq!(calculate_risk_level(&flags, &changes), "Medium");
}

#[test]
fn test_calculate_risk_level_high_with_breaking() {
    let flags: Vec<String> = vec!["breaking_change".to_string(), "breaking".to_string()];
    let changes: Vec<SemanticChange> = vec![];
    assert_eq!(calculate_risk_level(&flags, &changes), "High");
}

#[test]
fn test_calculate_risk_level_high_with_breaking_change() {
    let flags: Vec<String> = vec![];
    let changes = vec![SemanticChange {
        change_type: "ModifiedFunction".to_string(),
        file_path: "src/lib.rs".to_string(),
        symbol_name: Some("parse".to_string()),
        description: "Changed function signature".to_string(),
        breaking: true,
        security_relevant: false,
    }];
    assert_eq!(calculate_risk_level(&flags, &changes), "High");
}

#[test]
fn test_calculate_risk_level_critical_with_security() {
    let flags: Vec<String> = vec!["security_issue".to_string()];
    let changes = vec![SemanticChange {
        change_type: "ModifiedFunction".to_string(),
        file_path: "src/auth.rs".to_string(),
        symbol_name: Some("validate_token".to_string()),
        description: "Changed auth logic".to_string(),
        breaking: true,
        security_relevant: true,
    }];
    assert_eq!(calculate_risk_level(&flags, &changes), "Critical");
}

#[test]
fn test_parse_llm_response_valid_json() {
    let content = r#"Here is the analysis:
{
    "changes": [
        {
            "change_type": "NewFunction",
            "file_path": "src/main.rs",
            "symbol_name": "process",
            "description": "Added new processing function",
            "breaking": false,
            "security_relevant": false
        }
    ],
    "summary": "Added a new function for processing",
    "risk_flags": ["new_feature"]
}
Some trailing text"#;

    let result = llm::parse_llm_response(content);
    assert!(result.is_ok());
    let (changes, summary, flags) = result.unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].change_type, "NewFunction");
    assert_eq!(summary, "Added a new function for processing");
    assert_eq!(flags, vec!["new_feature"]);
}

#[test]
fn test_parse_llm_response_no_json_fallback() {
    let content = "This is just plain text analysis without JSON.\nThe changes look good.";
    let result = llm::parse_llm_response(content);
    assert!(result.is_ok());
    let (changes, summary, flags) = result.unwrap();
    assert!(changes.is_empty());
    assert_eq!(summary, "This is just plain text analysis without JSON.");
    assert!(flags.is_empty());
}

#[test]
fn test_parse_llm_response_invalid_json_fallback() {
    let content = "Analysis: { broken json here }";
    let result = llm::parse_llm_response(content);
    assert!(result.is_ok());
    let (changes, summary, _) = result.unwrap();
    assert!(changes.is_empty());
    assert_eq!(summary, "Analysis: { broken json here }");
}

#[test]
fn test_format_change_markers_none() {
    let change = SemanticChange {
        change_type: "NewFunction".to_string(),
        file_path: "src/lib.rs".to_string(),
        symbol_name: None,
        description: "Added function".to_string(),
        breaking: false,
        security_relevant: false,
    };
    assert_eq!(format_change_markers(&change), "");
}

#[test]
fn test_format_change_markers_breaking_only() {
    let change = SemanticChange {
        change_type: "SignatureChange".to_string(),
        file_path: "src/lib.rs".to_string(),
        symbol_name: Some("parse".to_string()),
        description: "Changed signature".to_string(),
        breaking: true,
        security_relevant: false,
    };
    assert_eq!(format_change_markers(&change), " [BREAKING]");
}

#[test]
fn test_format_change_markers_security_only() {
    let change = SemanticChange {
        change_type: "ModifiedFunction".to_string(),
        file_path: "src/auth.rs".to_string(),
        symbol_name: Some("validate".to_string()),
        description: "Modified auth".to_string(),
        breaking: false,
        security_relevant: true,
    };
    assert_eq!(format_change_markers(&change), " [SECURITY]");
}

#[test]
fn test_format_change_markers_both() {
    let change = SemanticChange {
        change_type: "ModifiedFunction".to_string(),
        file_path: "src/auth.rs".to_string(),
        symbol_name: Some("validate".to_string()),
        description: "Changed auth signature".to_string(),
        breaking: true,
        security_relevant: true,
    };
    assert_eq!(format_change_markers(&change), " [BREAKING] [SECURITY]");
}

#[test]
fn test_format_diff_analysis_empty_changes() {
    let result = DiffAnalysisResult {
        from_ref: "abc123".to_string(),
        to_ref: "def456".to_string(),
        changes: vec![],
        impact: None,
        risk: RiskAssessment {
            overall: "Low".to_string(),
            flags: vec![],
        },
        summary: "No significant changes".to_string(),
        files: vec!["src/main.rs".to_string()],
        files_changed: 1,
        lines_added: 10,
        lines_removed: 5,
    };

    let output = format_diff_analysis(&result, None);
    assert!(output.contains("## Semantic Diff Analysis: abc123..def456"));
    assert!(output.contains("No significant changes"));
    assert!(output.contains("1 files changed, +10 -5"));
    assert!(output.contains("### Risk: Low"));
}

#[test]
fn test_format_diff_analysis_with_changes() {
    let result = DiffAnalysisResult {
        from_ref: "abc123".to_string(),
        to_ref: "def456".to_string(),
        changes: vec![
            SemanticChange {
                change_type: "NewFunction".to_string(),
                file_path: "src/main.rs".to_string(),
                symbol_name: Some("init".to_string()),
                description: "Added init function".to_string(),
                breaking: false,
                security_relevant: false,
            },
            SemanticChange {
                change_type: "ModifiedFunction".to_string(),
                file_path: "src/lib.rs".to_string(),
                symbol_name: Some("parse".to_string()),
                description: "Changed parse logic".to_string(),
                breaking: true,
                security_relevant: false,
            },
        ],
        impact: Some(ImpactAnalysis {
            affected_functions: vec![
                ("caller1".to_string(), "src/caller.rs".to_string(), 1),
                ("caller2".to_string(), "src/other.rs".to_string(), 2),
            ],
            affected_files: vec!["src/caller.rs".to_string(), "src/other.rs".to_string()],
        }),
        risk: RiskAssessment {
            overall: "High".to_string(),
            flags: vec!["breaking_change".to_string()],
        },
        summary: "Added new feature and modified existing function".to_string(),
        files: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
        files_changed: 2,
        lines_added: 50,
        lines_removed: 10,
    };

    let output = format_diff_analysis(&result, None);
    assert!(output.contains("### Changes (2)"));
    assert!(output.contains("**New Features**"));
    assert!(output.contains("Added init function"));
    assert!(output.contains("**Modifications**"));
    assert!(output.contains("[BREAKING]"));
    assert!(output.contains("### Impact"));
    assert!(output.contains("Directly affected: 1 functions"));
    assert!(output.contains("Transitively affected: 1 functions"));
    assert!(output.contains("### Risk: High"));
    assert!(output.contains("breaking_change"));
}

#[test]
fn test_semantic_change_serialization() {
    let change = SemanticChange {
        change_type: "NewFunction".to_string(),
        file_path: "src/main.rs".to_string(),
        symbol_name: Some("test_fn".to_string()),
        description: "Added test function".to_string(),
        breaking: false,
        security_relevant: true,
    };

    let json = serde_json::to_string(&change).unwrap();
    let deserialized: SemanticChange = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.change_type, "NewFunction");
    assert_eq!(deserialized.file_path, "src/main.rs");
    assert_eq!(deserialized.symbol_name, Some("test_fn".to_string()));
    assert!(deserialized.security_relevant);
}

#[test]
fn test_risk_assessment_serialization() {
    let risk = RiskAssessment {
        overall: "Medium".to_string(),
        flags: vec!["api_change".to_string(), "new_dependency".to_string()],
    };

    let json = serde_json::to_string(&risk).unwrap();
    let deserialized: RiskAssessment = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.overall, "Medium");
    assert_eq!(deserialized.flags.len(), 2);
}

#[test]
fn test_impact_analysis_serialization() {
    let impact = ImpactAnalysis {
        affected_functions: vec![
            ("fn1".to_string(), "file1.rs".to_string(), 1),
            ("fn2".to_string(), "file2.rs".to_string(), 2),
        ],
        affected_files: vec!["file1.rs".to_string(), "file2.rs".to_string()],
    };

    let json = serde_json::to_string(&impact).unwrap();
    let deserialized: ImpactAnalysis = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.affected_functions.len(), 2);
    assert_eq!(deserialized.affected_files.len(), 2);
}

// =========================================================================
// Historical Risk Tests
// =========================================================================

fn setup_patterns_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE behavior_patterns (
            id INTEGER PRIMARY KEY,
            project_id INTEGER,
            pattern_type TEXT NOT NULL,
            pattern_key TEXT NOT NULL,
            pattern_data TEXT NOT NULL,
            confidence REAL DEFAULT 0.5,
            occurrence_count INTEGER DEFAULT 1,
            last_triggered_at TEXT,
            first_seen_at TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(project_id, pattern_type, pattern_key)
        );",
    )
    .unwrap();
    conn
}

fn seed_pattern(
    conn: &rusqlite::Connection,
    project_id: i64,
    key: &str,
    data: &str,
    confidence: f64,
    count: i64,
) {
    conn.execute(
        "INSERT INTO behavior_patterns (project_id, pattern_type, pattern_key, pattern_data, confidence, occurrence_count)
         VALUES (?, 'change_pattern', ?, ?, ?, ?)",
        rusqlite::params![project_id, key, data, confidence, count],
    )
    .unwrap();
}

#[test]
fn test_historical_risk_no_patterns() {
    let conn = setup_patterns_db();
    let result = compute_historical_risk(&conn, 1, &["src/main.rs".into()], 1);
    assert!(
        result.is_none(),
        "Should return None when no patterns exist"
    );
}

#[test]
fn test_historical_risk_module_hotspot_match() {
    let conn = setup_patterns_db();
    let data = serde_json::json!({
        "type": "change_pattern",
        "files": [],
        "module": "src",
        "pattern_subtype": "module_hotspot",
        "outcome_stats": { "total": 10, "clean": 5, "reverted": 3, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(&conn, 1, "module_hotspot:src", &data.to_string(), 0.7, 10);

    let result = compute_historical_risk(&conn, 1, &["src/lib.rs".into(), "src/main.rs".into()], 2);
    assert!(result.is_some());
    let hr = result.unwrap();
    assert_eq!(hr.risk_delta, "elevated");
    assert_eq!(hr.matching_patterns.len(), 1);
    assert_eq!(hr.matching_patterns[0].pattern_subtype, "module_hotspot");
    assert!((hr.matching_patterns[0].bad_rate - 0.5).abs() < 0.01);
}

#[test]
fn test_historical_risk_module_hotspot_no_match() {
    let conn = setup_patterns_db();
    let data = serde_json::json!({
        "type": "change_pattern",
        "files": [],
        "module": "tests",
        "pattern_subtype": "module_hotspot",
        "outcome_stats": { "total": 10, "clean": 3, "reverted": 5, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(&conn, 1, "module_hotspot:tests", &data.to_string(), 0.8, 10);

    // Files are in "src", not "tests"
    let result = compute_historical_risk(&conn, 1, &["src/main.rs".into()], 1);
    assert!(result.is_none());
}

#[test]
fn test_historical_risk_co_change_gap_match() {
    let conn = setup_patterns_db();
    let data = serde_json::json!({
        "type": "change_pattern",
        "files": ["src/schema.rs", "src/migrations.rs"],
        "module": null,
        "pattern_subtype": "co_change_gap",
        "outcome_stats": { "total": 5, "clean": 1, "reverted": 2, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(
        &conn,
        1,
        "co_change_gap:src/schema.rs|src/migrations.rs",
        &data.to_string(),
        0.65,
        5,
    );

    // schema.rs changed WITHOUT migrations.rs
    let result =
        compute_historical_risk(&conn, 1, &["src/schema.rs".into(), "src/lib.rs".into()], 2);
    assert!(result.is_some());
    let hr = result.unwrap();
    assert_eq!(hr.matching_patterns.len(), 1);
    assert_eq!(hr.matching_patterns[0].pattern_subtype, "co_change_gap");
    assert!(hr.matching_patterns[0].description.contains("schema.rs"));
    assert!(
        hr.matching_patterns[0]
            .description
            .contains("migrations.rs")
    );
}

#[test]
fn test_historical_risk_co_change_gap_both_present() {
    let conn = setup_patterns_db();
    let data = serde_json::json!({
        "type": "change_pattern",
        "files": ["src/schema.rs", "src/migrations.rs"],
        "module": null,
        "pattern_subtype": "co_change_gap",
        "outcome_stats": { "total": 5, "clean": 1, "reverted": 2, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(
        &conn,
        1,
        "co_change_gap:src/schema.rs|src/migrations.rs",
        &data.to_string(),
        0.65,
        5,
    );

    // Both files present — no gap, no match
    let result = compute_historical_risk(
        &conn,
        1,
        &["src/schema.rs".into(), "src/migrations.rs".into()],
        2,
    );
    assert!(result.is_none());
}

#[test]
fn test_historical_risk_size_risk_match() {
    let conn = setup_patterns_db();
    let data = serde_json::json!({
        "type": "change_pattern",
        "files": [],
        "module": null,
        "pattern_subtype": "size_risk",
        "outcome_stats": { "total": 8, "clean": 3, "reverted": 3, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(&conn, 1, "size_risk:large", &data.to_string(), 0.6, 8);

    // 15 files = "large" bucket
    let files: Vec<String> = (0..15).map(|i| format!("src/file{}.rs", i)).collect();
    let result = compute_historical_risk(&conn, 1, &files, 15);
    assert!(result.is_some());
    let hr = result.unwrap();
    assert_eq!(hr.matching_patterns.len(), 1);
    assert_eq!(hr.matching_patterns[0].pattern_subtype, "size_risk");
    assert!(hr.matching_patterns[0].description.contains("LARGE"));
}

#[test]
fn test_historical_risk_size_bucket_no_match() {
    let conn = setup_patterns_db();
    let data = serde_json::json!({
        "type": "change_pattern",
        "files": [],
        "module": null,
        "pattern_subtype": "size_risk",
        "outcome_stats": { "total": 8, "clean": 3, "reverted": 3, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(&conn, 1, "size_risk:large", &data.to_string(), 0.6, 8);

    // 2 files = "small" bucket, pattern is for "large"
    let result = compute_historical_risk(&conn, 1, &["src/a.rs".into(), "src/b.rs".into()], 2);
    assert!(result.is_none());
}

#[test]
fn test_historical_risk_multiple_matches() {
    let conn = setup_patterns_db();

    // Module hotspot
    let hotspot = serde_json::json!({
        "type": "change_pattern",
        "files": [],
        "module": "src",
        "pattern_subtype": "module_hotspot",
        "outcome_stats": { "total": 10, "clean": 5, "reverted": 3, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(
        &conn,
        1,
        "module_hotspot:src",
        &hotspot.to_string(),
        0.7,
        10,
    );

    // Size risk for medium
    let size = serde_json::json!({
        "type": "change_pattern",
        "files": [],
        "module": null,
        "pattern_subtype": "size_risk",
        "outcome_stats": { "total": 6, "clean": 2, "reverted": 2, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(&conn, 1, "size_risk:medium", &size.to_string(), 0.55, 6);

    // 5 files in src = matches both module_hotspot:src AND size_risk:medium
    let files: Vec<String> = (0..5).map(|i| format!("src/file{}.rs", i)).collect();
    let result = compute_historical_risk(&conn, 1, &files, 5);
    assert!(result.is_some());
    let hr = result.unwrap();
    assert_eq!(hr.matching_patterns.len(), 2);
    assert_eq!(hr.risk_delta, "elevated");
    // Confidence should be average of 0.7 and 0.55
    assert!((hr.overall_confidence - 0.625).abs() < 0.01);
}

#[test]
fn test_historical_risk_low_confidence_normal() {
    let conn = setup_patterns_db();
    let data = serde_json::json!({
        "type": "change_pattern",
        "files": [],
        "module": "src",
        "pattern_subtype": "module_hotspot",
        "outcome_stats": { "total": 3, "clean": 1, "reverted": 1, "follow_up_fix": 1 },
        "sample_commits": []
    });
    // Low confidence (0.4) — should match but risk_delta = "normal"
    seed_pattern(&conn, 1, "module_hotspot:src", &data.to_string(), 0.4, 3);

    let result = compute_historical_risk(&conn, 1, &["src/main.rs".into()], 1);
    assert!(result.is_some());
    let hr = result.unwrap();
    assert_eq!(hr.risk_delta, "normal");
}

#[test]
fn test_historical_risk_wrong_project() {
    let conn = setup_patterns_db();
    let data = serde_json::json!({
        "type": "change_pattern",
        "files": [],
        "module": "src",
        "pattern_subtype": "module_hotspot",
        "outcome_stats": { "total": 10, "clean": 5, "reverted": 3, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(&conn, 1, "module_hotspot:src", &data.to_string(), 0.7, 10);

    // Query for project 2 — shouldn't match project 1's patterns
    let result = compute_historical_risk(&conn, 2, &["src/main.rs".into()], 1);
    assert!(result.is_none());
}

#[test]
fn test_historical_risk_serialization_roundtrip() {
    let hr = HistoricalRisk {
        risk_delta: "elevated".to_string(),
        matching_patterns: vec![MatchedPattern {
            pattern_subtype: "module_hotspot".to_string(),
            description: "Module 'src' has 50% bad outcome rate".to_string(),
            confidence: 0.7,
            bad_rate: 0.5,
        }],
        overall_confidence: 0.7,
    };

    let json = serde_json::to_string(&hr).unwrap();
    let deserialized: HistoricalRisk = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.risk_delta, "elevated");
    assert_eq!(deserialized.matching_patterns.len(), 1);
    assert!((deserialized.overall_confidence - 0.7).abs() < 0.001);
}

#[test]
fn test_format_diff_analysis_with_historical_risk() {
    let result = DiffAnalysisResult {
        from_ref: "abc123".to_string(),
        to_ref: "def456".to_string(),
        changes: vec![],
        impact: None,
        risk: RiskAssessment {
            overall: "Medium".to_string(),
            flags: vec![],
        },
        summary: "Test".to_string(),
        files: vec!["src/main.rs".to_string()],
        files_changed: 1,
        lines_added: 5,
        lines_removed: 2,
    };

    let hr = HistoricalRisk {
        risk_delta: "elevated".to_string(),
        matching_patterns: vec![MatchedPattern {
            pattern_subtype: "module_hotspot".to_string(),
            description: "Module 'src' has 50% bad outcome rate".to_string(),
            confidence: 0.7,
            bad_rate: 0.5,
        }],
        overall_confidence: 0.7,
    };

    let output = format_diff_analysis(&result, Some(&hr));
    assert!(output.contains("### Historical Risk: ELEVATED"));
    assert!(output.contains("1 matching pattern(s)"));
    assert!(output.contains("module_hotspot"));
    assert!(output.contains("50% bad outcome rate"));
}

#[test]
fn test_format_diff_analysis_without_historical_risk() {
    let result = DiffAnalysisResult {
        from_ref: "abc123".to_string(),
        to_ref: "def456".to_string(),
        changes: vec![],
        impact: None,
        risk: RiskAssessment {
            overall: "Low".to_string(),
            flags: vec![],
        },
        summary: "Test".to_string(),
        files: vec![],
        files_changed: 0,
        lines_added: 0,
        lines_removed: 0,
    };

    let output = format_diff_analysis(&result, None);
    assert!(!output.contains("Historical Risk"));
}

// =========================================================================
// result_from_cache Tests
// =========================================================================

fn make_cached(overrides: impl FnOnce(&mut DiffAnalysis)) -> DiffAnalysis {
    let mut cached = DiffAnalysis {
        id: 1,
        project_id: Some(1),
        from_commit: "aaa".to_string(),
        to_commit: "bbb".to_string(),
        analysis_type: "commit".to_string(),
        changes_json: Some("[]".to_string()),
        impact_json: None,
        risk_json: None,
        summary: Some("Test summary".to_string()),
        files_changed: Some(3),
        lines_added: Some(10),
        lines_removed: Some(5),
        status: "complete".to_string(),
        created_at: "2026-01-01".to_string(),
        files_json: Some(r#"["src/a.rs","src/b.rs"]"#.to_string()),
    };
    overrides(&mut cached);
    cached
}

#[test]
fn test_result_from_cache_all_fields_present() {
    let changes = vec![SemanticChange {
        change_type: "NewFunction".to_string(),
        file_path: "src/main.rs".to_string(),
        symbol_name: Some("init".to_string()),
        description: "Added init".to_string(),
        breaking: false,
        security_relevant: false,
    }];
    let impact = ImpactAnalysis {
        affected_functions: vec![("caller".to_string(), "src/caller.rs".to_string(), 1)],
        affected_files: vec!["src/caller.rs".to_string()],
    };
    let risk = RiskAssessment {
        overall: "High".to_string(),
        flags: vec!["breaking_change".to_string()],
    };

    let cached = make_cached(|c| {
        c.changes_json = Some(serde_json::to_string(&changes).unwrap());
        c.impact_json = Some(serde_json::to_string(&impact).unwrap());
        c.risk_json = Some(serde_json::to_string(&risk).unwrap());
    });

    let result = super::result_from_cache(cached, "from".to_string(), "to".to_string());
    assert_eq!(result.from_ref, "from");
    assert_eq!(result.to_ref, "to");
    assert_eq!(result.changes.len(), 1);
    assert_eq!(result.changes[0].change_type, "NewFunction");
    assert!(result.impact.is_some());
    assert_eq!(result.impact.unwrap().affected_files.len(), 1);
    assert_eq!(result.risk.overall, "High");
    assert_eq!(result.summary, "Test summary");
    assert_eq!(result.files_changed, 3);
    assert_eq!(result.lines_added, 10);
    assert_eq!(result.lines_removed, 5);
    assert_eq!(result.files.len(), 2);
}

#[test]
fn test_result_from_cache_all_optional_fields_none() {
    let cached = make_cached(|c| {
        c.changes_json = None;
        c.impact_json = None;
        c.risk_json = None;
        c.summary = None;
        c.files_changed = None;
        c.lines_added = None;
        c.lines_removed = None;
        c.files_json = None;
    });

    let result = super::result_from_cache(cached, "from".to_string(), "to".to_string());
    assert!(result.changes.is_empty());
    assert!(result.impact.is_none());
    assert_eq!(result.risk.overall, "Unknown");
    assert!(result.risk.flags.is_empty());
    assert!(result.summary.is_empty());
    assert_eq!(result.files_changed, 0);
    assert_eq!(result.lines_added, 0);
    assert_eq!(result.lines_removed, 0);
    assert!(result.files.is_empty());
}

#[test]
fn test_result_from_cache_malformed_changes_json() {
    let cached = make_cached(|c| {
        c.changes_json = Some("{not valid json}".to_string());
    });

    let result = super::result_from_cache(cached, "a".to_string(), "b".to_string());
    // Malformed JSON should fall back to empty vec via unwrap_or_default
    assert!(result.changes.is_empty());
}

#[test]
fn test_result_from_cache_malformed_impact_json() {
    let cached = make_cached(|c| {
        c.impact_json = Some("{broken}".to_string());
    });

    let result = super::result_from_cache(cached, "a".to_string(), "b".to_string());
    // Malformed impact JSON should result in None
    assert!(result.impact.is_none());
}

#[test]
fn test_result_from_cache_malformed_risk_json() {
    let cached = make_cached(|c| {
        c.risk_json = Some("{broken}".to_string());
    });

    let result = super::result_from_cache(cached, "a".to_string(), "b".to_string());
    // Malformed risk JSON falls back to "Unknown" with empty flags
    assert_eq!(result.risk.overall, "Unknown");
    assert!(result.risk.flags.is_empty());
}

#[test]
fn test_result_from_cache_malformed_files_json() {
    let cached = make_cached(|c| {
        c.files_json = Some("{not an array}".to_string());
    });

    let result = super::result_from_cache(cached, "a".to_string(), "b".to_string());
    // Malformed files_json should fall back to empty vec
    assert!(result.files.is_empty());
}

#[test]
fn test_result_from_cache_empty_string_jsons() {
    let cached = make_cached(|c| {
        c.changes_json = Some("".to_string());
        c.impact_json = Some("".to_string());
        c.risk_json = Some("".to_string());
        c.files_json = Some("".to_string());
        c.summary = Some("".to_string());
    });

    let result = super::result_from_cache(cached, "a".to_string(), "b".to_string());
    assert!(result.changes.is_empty());
    assert!(result.impact.is_none());
    assert_eq!(result.risk.overall, "Unknown");
    assert!(result.files.is_empty());
    assert!(result.summary.is_empty());
}

// =========================================================================
// compute_historical_risk Edge Cases
// =========================================================================

#[test]
fn test_historical_risk_empty_files_list() {
    let conn = setup_patterns_db();
    let data = serde_json::json!({
        "type": "change_pattern",
        "files": [],
        "module": "src",
        "pattern_subtype": "module_hotspot",
        "outcome_stats": { "total": 10, "clean": 5, "reverted": 3, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(&conn, 1, "module_hotspot:src", &data.to_string(), 0.7, 10);

    // Empty files list -> no modules to match
    let result = compute_historical_risk(&conn, 1, &[], 0);
    assert!(result.is_none(), "Empty files should produce no matches");
}

#[test]
fn test_historical_risk_co_change_gap_single_file_pattern() {
    let conn = setup_patterns_db();
    // Pattern with only 1 file -- the guard pattern_files.len() >= 2 should skip it
    let data = serde_json::json!({
        "type": "change_pattern",
        "files": ["src/schema.rs"],
        "module": null,
        "pattern_subtype": "co_change_gap",
        "outcome_stats": { "total": 5, "clean": 1, "reverted": 2, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(
        &conn,
        1,
        "co_change_gap:src/schema.rs",
        &data.to_string(),
        0.65,
        5,
    );

    let result = compute_historical_risk(&conn, 1, &["src/schema.rs".into()], 1);
    assert!(
        result.is_none(),
        "co_change_gap with only 1 file in pattern should not match"
    );
}

#[test]
fn test_historical_risk_co_change_gap_empty_pattern_files() {
    let conn = setup_patterns_db();
    // Pattern with empty files list
    let data = serde_json::json!({
        "type": "change_pattern",
        "files": [],
        "module": null,
        "pattern_subtype": "co_change_gap",
        "outcome_stats": { "total": 5, "clean": 1, "reverted": 2, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(&conn, 1, "co_change_gap:empty", &data.to_string(), 0.65, 5);

    let result = compute_historical_risk(&conn, 1, &["src/anything.rs".into()], 1);
    assert!(
        result.is_none(),
        "co_change_gap with empty pattern files should not match"
    );
}

#[test]
fn test_historical_risk_files_without_slash() {
    let conn = setup_patterns_db();
    // Module hotspot for a root-level file (no '/' separator)
    let data = serde_json::json!({
        "type": "change_pattern",
        "files": [],
        "module": "Cargo.toml",
        "pattern_subtype": "module_hotspot",
        "outcome_stats": { "total": 10, "clean": 3, "reverted": 5, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(
        &conn,
        1,
        "module_hotspot:Cargo.toml",
        &data.to_string(),
        0.7,
        10,
    );

    // File without '/' -> module is the full filename
    let result = compute_historical_risk(&conn, 1, &["Cargo.toml".into()], 1);
    assert!(result.is_some());
    let hr = result.unwrap();
    assert_eq!(hr.matching_patterns.len(), 1);
    assert_eq!(hr.matching_patterns[0].pattern_subtype, "module_hotspot");
}

#[test]
fn test_historical_risk_size_buckets_boundary() {
    let conn = setup_patterns_db();

    // Small bucket pattern
    let small_data = serde_json::json!({
        "type": "change_pattern",
        "files": [],
        "module": null,
        "pattern_subtype": "size_risk",
        "outcome_stats": { "total": 5, "clean": 1, "reverted": 2, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(&conn, 1, "size_risk:small", &small_data.to_string(), 0.6, 5);

    // 3 files -> "small" bucket (boundary: <= 3)
    let result =
        compute_historical_risk(&conn, 1, &["a.rs".into(), "b.rs".into(), "c.rs".into()], 3);
    assert!(result.is_some());
    assert_eq!(
        result.unwrap().matching_patterns[0].pattern_subtype,
        "size_risk"
    );

    // 4 files -> "medium" bucket (boundary: > 3), should NOT match "small"
    let result = compute_historical_risk(
        &conn,
        1,
        &["a.rs".into(), "b.rs".into(), "c.rs".into(), "d.rs".into()],
        4,
    );
    assert!(result.is_none(), "4 files is medium bucket, not small");
}

#[test]
fn test_historical_risk_outcome_stats_zero_total() {
    let conn = setup_patterns_db();
    // Pattern with total=0 -- bad_rate should be 0.0, not NaN/panic
    let data = serde_json::json!({
        "type": "change_pattern",
        "files": [],
        "module": "src",
        "pattern_subtype": "module_hotspot",
        "outcome_stats": { "total": 0, "clean": 0, "reverted": 0, "follow_up_fix": 0 },
        "sample_commits": []
    });
    seed_pattern(&conn, 1, "module_hotspot:src", &data.to_string(), 0.7, 1);

    let result = compute_historical_risk(&conn, 1, &["src/main.rs".into()], 1);
    assert!(result.is_some());
    let hr = result.unwrap();
    assert_eq!(hr.matching_patterns.len(), 1);
    assert!((hr.matching_patterns[0].bad_rate - 0.0).abs() < 0.01);
}

#[test]
fn test_historical_risk_unknown_pattern_subtype() {
    let conn = setup_patterns_db();
    // Pattern with an unknown subtype -- should be skipped
    let data = serde_json::json!({
        "type": "change_pattern",
        "files": [],
        "module": "src",
        "pattern_subtype": "unknown_future_type",
        "outcome_stats": { "total": 10, "clean": 5, "reverted": 3, "follow_up_fix": 2 },
        "sample_commits": []
    });
    seed_pattern(
        &conn,
        1,
        "unknown_future_type:src",
        &data.to_string(),
        0.7,
        10,
    );

    let result = compute_historical_risk(&conn, 1, &["src/main.rs".into()], 1);
    assert!(
        result.is_none(),
        "Unknown pattern subtype should be skipped by match"
    );
}
