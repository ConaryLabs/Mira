// background/diff_analysis/tests.rs

use super::format::format_change_markers;
use super::*;

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

    let result =
        compute_historical_risk(&conn, 1, &["src/lib.rs".into(), "src/main.rs".into()], 2);
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
    seed_pattern(
        &conn,
        1,
        "module_hotspot:tests",
        &data.to_string(),
        0.8,
        10,
    );

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
