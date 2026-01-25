// crates/mira-server/src/experts/adaptation.rs
// Adaptive prompts and confidence calibration

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::ExpertRole;
use super::consultation::ExpertStats;
use super::patterns::get_top_patterns;

/// Prompt version with performance tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptVersion {
    pub id: Option<i64>,
    pub expert_role: ExpertRole,
    pub version: i32,
    pub prompt_additions: String,
    pub performance_metrics: PerformanceMetrics,
    pub adaptation_reason: Option<String>,
    pub consultation_count: i64,
    pub acceptance_rate: f64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PerformanceMetrics {
    pub total_consultations: i64,
    pub accepted_findings: i64,
    pub rejected_findings: i64,
    pub avg_confidence: f64,
    pub confidence_accuracy: f64, // How well confidence predicts acceptance
}

/// Get the current active prompt version for an expert
pub fn get_active_prompt_version(conn: &Connection, expert_role: ExpertRole) -> Result<Option<PromptVersion>> {
    let sql = r#"
        SELECT id, expert_role, version, prompt_additions, performance_metrics,
               adaptation_reason, consultation_count, acceptance_rate, is_active
        FROM expert_prompt_versions
        WHERE expert_role = ? AND is_active = 1
        ORDER BY version DESC
        LIMIT 1
    "#;

    let result = conn.query_row(sql, [expert_role.as_str()], |row| {
        let role_str: String = row.get(1)?;
        let metrics_json: String = row.get(4)?;

        Ok(PromptVersion {
            id: Some(row.get(0)?),
            expert_role: ExpertRole::from_str(&role_str).unwrap_or(ExpertRole::Architect),
            version: row.get(2)?,
            prompt_additions: row.get(3)?,
            performance_metrics: serde_json::from_str(&metrics_json).unwrap_or_default(),
            adaptation_reason: row.get(5)?,
            consultation_count: row.get(6)?,
            acceptance_rate: row.get(7)?,
            is_active: row.get(8)?,
        })
    });

    match result {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Create a new prompt version
pub fn create_prompt_version(
    conn: &Connection,
    expert_role: ExpertRole,
    prompt_additions: &str,
    adaptation_reason: &str,
) -> Result<i64> {
    // Get next version number
    let current_version: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM expert_prompt_versions WHERE expert_role = ?",
            [expert_role.as_str()],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let new_version = current_version + 1;

    // Deactivate previous versions
    conn.execute(
        "UPDATE expert_prompt_versions SET is_active = 0 WHERE expert_role = ?",
        [expert_role.as_str()],
    )?;

    // Insert new version
    let sql = r#"
        INSERT INTO expert_prompt_versions
        (expert_role, version, prompt_additions, performance_metrics, adaptation_reason, is_active)
        VALUES (?, ?, ?, '{}', ?, 1)
    "#;

    conn.execute(sql, rusqlite::params![
        expert_role.as_str(),
        new_version,
        prompt_additions,
        adaptation_reason,
    ])?;

    Ok(conn.last_insert_rowid())
}

/// Update performance metrics for a prompt version
pub fn update_prompt_metrics(
    conn: &Connection,
    version_id: i64,
    accepted: bool,
    confidence: f64,
) -> Result<()> {
    let sql = if accepted {
        r#"
            UPDATE expert_prompt_versions
            SET consultation_count = consultation_count + 1,
                acceptance_rate = (acceptance_rate * consultation_count + 1.0) / (consultation_count + 1),
                performance_metrics = json_set(
                    performance_metrics,
                    '$.total_consultations', json_extract(performance_metrics, '$.total_consultations') + 1,
                    '$.accepted_findings', json_extract(performance_metrics, '$.accepted_findings') + 1,
                    '$.avg_confidence', (json_extract(performance_metrics, '$.avg_confidence') * json_extract(performance_metrics, '$.total_consultations') + ?) / (json_extract(performance_metrics, '$.total_consultations') + 1)
                )
            WHERE id = ?
        "#
    } else {
        r#"
            UPDATE expert_prompt_versions
            SET consultation_count = consultation_count + 1,
                acceptance_rate = (acceptance_rate * consultation_count) / (consultation_count + 1),
                performance_metrics = json_set(
                    performance_metrics,
                    '$.total_consultations', json_extract(performance_metrics, '$.total_consultations') + 1,
                    '$.rejected_findings', json_extract(performance_metrics, '$.rejected_findings') + 1,
                    '$.avg_confidence', (json_extract(performance_metrics, '$.avg_confidence') * json_extract(performance_metrics, '$.total_consultations') + ?) / (json_extract(performance_metrics, '$.total_consultations') + 1)
                )
            WHERE id = ?
        "#
    };

    conn.execute(sql, rusqlite::params![confidence, version_id])?;
    Ok(())
}

/// Build adaptive prompt additions based on learned patterns
pub fn build_adaptive_prompt(
    conn: &Connection,
    expert_role: ExpertRole,
    stats: &ExpertStats,
    _context: &str,
) -> Result<String> {
    let mut additions = Vec::new();

    // Add performance context
    if stats.total_consultations >= 5 {
        additions.push(format!(
            "Based on {} prior consultations with {:.0}% acceptance rate.",
            stats.total_consultations,
            stats.acceptance_rate * 100.0
        ));
    }

    // Get relevant patterns
    let patterns = get_top_patterns(conn, expert_role, 5)?;
    if !patterns.is_empty() {
        let pattern_hints: Vec<String> = patterns
            .iter()
            .filter(|p| p.success_rate >= 0.6)
            .take(3)
            .map(|p| {
                format!(
                    "- {} ({}% success): {}",
                    p.pattern_signature,
                    (p.success_rate * 100.0) as i32,
                    p.successful_approaches.first().cloned().unwrap_or_default()
                )
            })
            .collect();

        if !pattern_hints.is_empty() {
            additions.push(format!(
                "Validated patterns for similar problems:\n{}",
                pattern_hints.join("\n")
            ));
        }
    }

    // Add confidence calibration hint
    if stats.acceptance_rate < 0.5 && stats.total_consultations >= 10 {
        additions.push(
            "Note: Historical acceptance rate is lower than average. Consider being more conservative with recommendations.".to_string()
        );
    } else if stats.acceptance_rate > 0.8 && stats.total_consultations >= 10 {
        additions.push(
            "Note: Historical acceptance rate is high. Continue with thorough, detailed analysis.".to_string()
        );
    }

    if additions.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("\n[Adaptive Context]\n{}\n", additions.join("\n\n")))
    }
}

/// Check if prompt adaptation is needed based on performance
pub fn should_adapt_prompt(conn: &Connection, expert_role: ExpertRole) -> Result<Option<AdaptationTrigger>> {
    let sql = r#"
        SELECT
            COUNT(*) as recent_count,
            AVG(CASE WHEN rf.status IN ('accepted', 'fixed') THEN 1.0 ELSE 0.0 END) as recent_acceptance,
            (SELECT acceptance_rate FROM expert_prompt_versions
             WHERE expert_role = ? AND is_active = 1) as baseline_acceptance
        FROM expert_consultations ec
        LEFT JOIN review_findings rf ON rf.expert_role = ec.expert_role
            AND rf.session_id = ec.session_id
        WHERE ec.expert_role = ?
          AND ec.created_at > datetime('now', '-7 days')
    "#;

    let result = conn.query_row(sql, [expert_role.as_str(), expert_role.as_str()], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<f64>>(1)?,
            row.get::<_, Option<f64>>(2)?,
        ))
    });

    match result {
        Ok((count, recent_acceptance, baseline)) => {
            if count < 5 {
                return Ok(None); // Not enough data
            }

            let recent = recent_acceptance.unwrap_or(0.5);
            let baseline = baseline.unwrap_or(0.5);

            // Check for significant performance drop
            if recent < baseline - 0.15 {
                return Ok(Some(AdaptationTrigger::PerformanceDrop {
                    baseline_rate: baseline,
                    current_rate: recent,
                    drop_percent: ((baseline - recent) / baseline * 100.0) as i32,
                }));
            }

            // Check for significant improvement (might want to capture what's working)
            if recent > baseline + 0.15 {
                return Ok(Some(AdaptationTrigger::PerformanceImprovement {
                    baseline_rate: baseline,
                    current_rate: recent,
                    improvement_percent: ((recent - baseline) / baseline * 100.0) as i32,
                }));
            }

            Ok(None)
        }
        Err(_) => Ok(None),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdaptationTrigger {
    PerformanceDrop {
        baseline_rate: f64,
        current_rate: f64,
        drop_percent: i32,
    },
    PerformanceImprovement {
        baseline_rate: f64,
        current_rate: f64,
        improvement_percent: i32,
    },
    NewPatternIdentified {
        pattern_description: String,
    },
    UserFeedbackTrend {
        feedback_summary: String,
    },
}

impl AdaptationTrigger {
    pub fn description(&self) -> String {
        match self {
            AdaptationTrigger::PerformanceDrop { drop_percent, .. } => {
                format!("Performance dropped {}% from baseline", drop_percent)
            }
            AdaptationTrigger::PerformanceImprovement { improvement_percent, .. } => {
                format!("Performance improved {}% from baseline", improvement_percent)
            }
            AdaptationTrigger::NewPatternIdentified { pattern_description } => {
                format!("New pattern identified: {}", pattern_description)
            }
            AdaptationTrigger::UserFeedbackTrend { feedback_summary } => {
                format!("User feedback trend: {}", feedback_summary)
            }
        }
    }
}
