// background/pondering/heuristic.rs
// Data-driven heuristic insight generation (no LLM needed)

use super::types::{ProjectInsightData, PonderingInsight};

/// Generate insights from project data without LLM.
/// Produces specific, actionable insights based on real project signals.
pub(super) fn generate_insights_heuristic(
    data: &ProjectInsightData,
) -> Vec<PonderingInsight> {
    let mut insights = Vec::new();

    // 1. Stale goals — goals stuck in_progress for >14 days
    for goal in &data.stale_goals {
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

    // 2. Fragile modules — high revert/fix rate
    for module in &data.fragile_modules {
        if module.bad_rate > 0.3 {
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
    for cluster in &data.revert_clusters {
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

    // 4. Untested hotspots — frequently modified without test updates
    for file in &data.untested_hotspots {
        if file.modification_count >= 5 {
            insights.push(PonderingInsight {
                pattern_type: "insight_untested".to_string(),
                description: format!(
                    "'{}' modified {} times across {} sessions with no test updates",
                    file.file_path, file.modification_count, file.sessions_involved,
                ),
                confidence: 0.6,
                evidence: vec![
                    format!("modifications: {}", file.modification_count),
                    format!("sessions: {}", file.sessions_involved),
                ],
            });
        }
    }

    // 5. Session patterns — use description directly
    for pattern in &data.session_patterns {
        insights.push(PonderingInsight {
            pattern_type: "insight_session".to_string(),
            description: pattern.description.clone(),
            confidence: 0.5,
            evidence: vec![format!("count: {}", pattern.count)],
        });
    }

    insights
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::*;

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
    fn test_fragile_module_high_rate_capped() {
        let data = ProjectInsightData {
            fragile_modules: vec![FragileModule {
                module: "src/bad".to_string(),
                total_changes: 5,
                reverted: 5,
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
    fn test_untested_hotspot() {
        let data = ProjectInsightData {
            untested_hotspots: vec![UntestedFile {
                file_path: "src/db/pool.rs".to_string(),
                modification_count: 8,
                sessions_involved: 3,
            }],
            ..Default::default()
        };
        let insights = generate_insights_heuristic(&data);
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].pattern_type, "insight_untested");
        assert_eq!(insights[0].confidence, 0.6);
        assert!(insights[0].description.contains("pool.rs"));
    }

    #[test]
    fn test_untested_hotspot_below_threshold() {
        let data = ProjectInsightData {
            untested_hotspots: vec![UntestedFile {
                file_path: "src/main.rs".to_string(),
                modification_count: 4,
                sessions_involved: 2,
            }],
            ..Default::default()
        };
        let insights = generate_insights_heuristic(&data);
        assert!(insights.is_empty());
    }

    #[test]
    fn test_session_pattern() {
        let data = ProjectInsightData {
            session_patterns: vec![SessionPattern {
                description: "5 sessions in the last 7 days lasted less than 5 minutes".to_string(),
                count: 5,
            }],
            ..Default::default()
        };
        let insights = generate_insights_heuristic(&data);
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].pattern_type, "insight_session");
        assert_eq!(insights[0].confidence, 0.5);
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
            untested_hotspots: vec![UntestedFile {
                file_path: "src/lib.rs".to_string(),
                modification_count: 10,
                sessions_involved: 4,
            }],
            session_patterns: vec![],
        };
        let insights = generate_insights_heuristic(&data);
        assert_eq!(insights.len(), 3); // stale goal + fragile module + untested
    }
}
