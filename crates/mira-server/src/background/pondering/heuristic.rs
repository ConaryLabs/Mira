// background/pondering/heuristic.rs
// Data-driven heuristic insight generation (no LLM needed)

use super::types::{PonderingInsight, ProjectInsightData};

/// Maximum insights to generate per category. Prevents insight floods when
/// a project has many files/modules matching a heuristic (e.g. untested hotspots).
const MAX_PER_CATEGORY: usize = 10;

/// Generate insights from project data without LLM.
/// Produces specific, actionable insights based on real project signals.
pub(super) fn generate_insights_heuristic(data: &ProjectInsightData) -> Vec<PonderingInsight> {
    let mut insights = Vec::new();

    // 1. Stale goals — goals stuck in_progress for >14 days
    for goal in data.stale_goals.iter().take(MAX_PER_CATEGORY) {
        if goal.days_since_update > 14 {
            let confidence = if goal.days_since_update > 21 {
                0.8
            } else {
                0.65
            };
            insights.push(PonderingInsight {
                pattern_type: "insight_stale_goal".to_string(),
                description: format!(
                    "Goal '{}' has been {} for {} days \u{2014} {}/{} milestones done",
                    goal.title,
                    goal.status,
                    goal.days_since_update,
                    goal.milestones_completed,
                    goal.milestones_total,
                ),
                confidence,
                evidence: vec![
                    format!("goal_id: {}", goal.goal_id),
                    format!("progress: {}%", goal.progress_percent),
                ],
            });
        }
    }

    // 2. Fragile modules — high revert/fix rate (minimum 10 changes to avoid small-sample noise)
    for module in data.fragile_modules.iter().take(MAX_PER_CATEGORY) {
        if module.total_changes >= 10 && module.bad_rate > 0.3 {
            let confidence = (module.bad_rate * 1.1).min(0.9);
            insights.push(PonderingInsight {
                pattern_type: "insight_fragile_code".to_string(),
                description: format!(
                    "Module '{}' has {:.0}% failure rate \u{2014} {} reverted, {} follow-up fixes out of {} changes",
                    module.module,
                    module.bad_rate * 100.0,
                    module.reverted,
                    module.follow_up_fixes,
                    module.total_changes,
                ),
                confidence,
                evidence: vec![
                    format!("reverted: {}", module.reverted),
                    format!("follow_up_fixes: {}", module.follow_up_fixes),
                    format!("total_changes: {}", module.total_changes),
                ],
            });
        }
    }

    // 3. Revert clusters — multiple reverts in short timespan
    for cluster in data.revert_clusters.iter().take(MAX_PER_CATEGORY) {
        if cluster.revert_count >= 2 {
            let confidence = if cluster.revert_count >= 3 {
                0.85
            } else {
                0.70
            };
            insights.push(PonderingInsight {
                pattern_type: "insight_revert_cluster".to_string(),
                description: format!(
                    "Module '{}' had {} reverts in {}h \u{2014} area may be unstable",
                    cluster.module, cluster.revert_count, cluster.timespan_hours,
                ),
                confidence,
                evidence: cluster.commits.clone(),
            });
        }
    }

    // 4. Recurring errors — unresolved errors with 3+ occurrences
    let mut error_count = 0;
    for error in &data.recurring_errors {
        if error_count >= MAX_PER_CATEGORY {
            break;
        }
        // Skip benign errors that are normal Claude Code behavior
        if is_benign_error(&error.tool_name, &error.error_template) {
            continue;
        }
        let confidence = if error.occurrence_count >= 10 {
            0.9
        } else if error.occurrence_count >= 5 {
            0.75
        } else {
            0.6
        };
        insights.push(PonderingInsight {
            pattern_type: "insight_recurring_error".to_string(),
            description: format!(
                "Error in '{}' has occurred {} times without resolution: {}",
                error.tool_name, error.occurrence_count, error.error_template,
            ),
            confidence,
            evidence: vec![
                format!("occurrences: {}", error.occurrence_count),
                format!("tool: {}", error.tool_name),
            ],
        });
        error_count += 1;
    }

    insights
}

/// Benign error patterns that are normal Claude Code behavior, not actual bugs.
/// These get recorded in error_patterns for data completeness but should not
/// generate pondering insights.
const BENIGN_ERRORS: &[(&str, &str)] = &[
    ("read", "file does not exist"),
    ("read", "not found"),
    ("glob", "no matches"),
    ("glob", "no files found"),
    ("grep", "no matches"),
    ("grep", "no results"),
];

/// Check if an error pattern is benign (expected normal behavior).
pub(super) fn is_benign_error(tool_name: &str, error_template: &str) -> bool {
    let tool_lower = tool_name.to_lowercase();
    let template_lower = error_template.to_lowercase();
    BENIGN_ERRORS
        .iter()
        .any(|(t, e)| tool_lower == *t && template_lower.contains(e))
}

#[cfg(test)]
mod tests {
    use super::super::types::*;
    use super::*;

    #[test]
    fn test_stale_goal_21_days() {
        let data = ProjectInsightData {
            stale_goals: vec![StaleGoal {
                goal_id: 94,
                title: "deadpool migration".to_string(),
                status: "in_progress".to_string(),
                progress_percent: 0,
                days_since_update: 23,
                milestones_total: 3,
                milestones_completed: 0,
            }],
            ..Default::default()
        };
        let insights = generate_insights_heuristic(&data);
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].pattern_type, "insight_stale_goal");
        assert_eq!(insights[0].confidence, 0.8);
        assert!(insights[0].description.contains("deadpool migration"));
        assert!(insights[0].description.contains("23 days"));
    }

    #[test]
    fn test_stale_goal_15_days() {
        let data = ProjectInsightData {
            stale_goals: vec![StaleGoal {
                goal_id: 1,
                title: "some goal".to_string(),
                status: "in_progress".to_string(),
                progress_percent: 50,
                days_since_update: 15,
                milestones_total: 2,
                milestones_completed: 1,
            }],
            ..Default::default()
        };
        let insights = generate_insights_heuristic(&data);
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].confidence, 0.65);
    }

    #[test]
    fn test_fragile_module() {
        let data = ProjectInsightData {
            fragile_modules: vec![FragileModule {
                module: "src/db".to_string(),
                total_changes: 10,
                reverted: 2,
                follow_up_fixes: 2,
                bad_rate: 0.4,
            }],
            ..Default::default()
        };
        let insights = generate_insights_heuristic(&data);
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].pattern_type, "insight_fragile_code");
        assert!((insights[0].confidence - 0.44).abs() < 0.01);
        assert!(insights[0].description.contains("src/db"));
    }

    #[test]
    fn test_fragile_module_below_min_changes() {
        // total_changes < 10 should be suppressed regardless of bad_rate
        let data = ProjectInsightData {
            fragile_modules: vec![FragileModule {
                module: "src/bad".to_string(),
                total_changes: 9,
                reverted: 5,
                follow_up_fixes: 2,
                bad_rate: 0.78,
            }],
            ..Default::default()
        };
        let insights = generate_insights_heuristic(&data);
        assert!(insights.is_empty());
    }

    #[test]
    fn test_fragile_module_high_rate_capped() {
        let data = ProjectInsightData {
            fragile_modules: vec![FragileModule {
                module: "src/bad".to_string(),
                total_changes: 10,
                reverted: 10,
                follow_up_fixes: 0,
                bad_rate: 1.0,
            }],
            ..Default::default()
        };
        let insights = generate_insights_heuristic(&data);
        assert_eq!(insights[0].confidence, 0.9); // capped at 0.9
    }

    #[test]
    fn test_revert_cluster() {
        let data = ProjectInsightData {
            revert_clusters: vec![RevertCluster {
                module: "background/".to_string(),
                revert_count: 3,
                timespan_hours: 24,
                commits: vec!["abc123".into(), "def456".into(), "ghi789".into()],
            }],
            ..Default::default()
        };
        let insights = generate_insights_heuristic(&data);
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].pattern_type, "insight_revert_cluster");
        assert_eq!(insights[0].confidence, 0.85);
    }

    #[test]
    fn test_revert_cluster_two() {
        let data = ProjectInsightData {
            revert_clusters: vec![RevertCluster {
                module: "src/tools".to_string(),
                revert_count: 2,
                timespan_hours: 12,
                commits: vec!["a".into(), "b".into()],
            }],
            ..Default::default()
        };
        let insights = generate_insights_heuristic(&data);
        assert_eq!(insights[0].confidence, 0.70);
    }

    #[test]
    fn test_recurring_error_high_count() {
        let data = ProjectInsightData {
            recurring_errors: vec![RecurringError {
                tool_name: "code_search".to_string(),
                error_template: "connection refused".to_string(),
                occurrence_count: 12,
                first_seen_session_id: None,
                last_seen_session_id: None,
            }],
            ..Default::default()
        };
        let insights = generate_insights_heuristic(&data);
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].pattern_type, "insight_recurring_error");
        assert_eq!(insights[0].confidence, 0.9);
        assert!(insights[0].description.contains("code_search"));
        assert!(insights[0].description.contains("12 times"));
    }

    #[test]
    fn test_recurring_error_medium_count() {
        let data = ProjectInsightData {
            recurring_errors: vec![RecurringError {
                tool_name: "memory".to_string(),
                error_template: "table not found".to_string(),
                occurrence_count: 5,
                first_seen_session_id: None,
                last_seen_session_id: None,
            }],
            ..Default::default()
        };
        let insights = generate_insights_heuristic(&data);
        assert_eq!(insights[0].confidence, 0.75);
    }

    #[test]
    fn test_recurring_error_low_count() {
        let data = ProjectInsightData {
            recurring_errors: vec![RecurringError {
                tool_name: "index".to_string(),
                error_template: "timeout".to_string(),
                occurrence_count: 3,
                first_seen_session_id: None,
                last_seen_session_id: None,
            }],
            ..Default::default()
        };
        let insights = generate_insights_heuristic(&data);
        assert_eq!(insights[0].confidence, 0.6);
    }

    #[test]
    fn test_benign_errors_filtered() {
        let data = ProjectInsightData {
            recurring_errors: vec![
                RecurringError {
                    tool_name: "Read".to_string(),
                    error_template: "file does not exist".to_string(),
                    occurrence_count: 15,
                    first_seen_session_id: None,
                    last_seen_session_id: None,
                },
                RecurringError {
                    tool_name: "Glob".to_string(),
                    error_template: "No files found".to_string(),
                    occurrence_count: 8,
                    first_seen_session_id: None,
                    last_seen_session_id: None,
                },
                RecurringError {
                    tool_name: "Grep".to_string(),
                    error_template: "no matches found".to_string(),
                    occurrence_count: 10,
                    first_seen_session_id: None,
                    last_seen_session_id: None,
                },
                // This one is NOT benign — should still generate an insight
                RecurringError {
                    tool_name: "code_search".to_string(),
                    error_template: "connection refused".to_string(),
                    occurrence_count: 5,
                    first_seen_session_id: None,
                    last_seen_session_id: None,
                },
            ],
            ..Default::default()
        };
        let insights = generate_insights_heuristic(&data);
        assert_eq!(insights.len(), 1);
        assert!(insights[0].description.contains("connection refused"));
    }

    #[test]
    fn test_empty_data_produces_no_insights() {
        let data = ProjectInsightData::default();
        let insights = generate_insights_heuristic(&data);
        assert!(insights.is_empty());
    }

    #[test]
    fn test_mixed_data() {
        let data = ProjectInsightData {
            stale_goals: vec![StaleGoal {
                goal_id: 1,
                title: "goal A".to_string(),
                status: "in_progress".to_string(),
                progress_percent: 0,
                days_since_update: 30,
                milestones_total: 5,
                milestones_completed: 1,
            }],
            fragile_modules: vec![FragileModule {
                module: "src/api".to_string(),
                total_changes: 20,
                reverted: 8,
                follow_up_fixes: 2,
                bad_rate: 0.5,
            }],
            revert_clusters: vec![],
            recurring_errors: vec![RecurringError {
                tool_name: "code".to_string(),
                error_template: "index missing".to_string(),
                occurrence_count: 7,
                first_seen_session_id: None,
                last_seen_session_id: None,
            }],
        };
        let insights = generate_insights_heuristic(&data);
        assert_eq!(insights.len(), 3); // stale goal + fragile module + recurring error
    }

}
