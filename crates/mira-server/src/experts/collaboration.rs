// crates/mira-server/src/experts/collaboration.rs
// Intelligent expert collaboration for complex problems

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::{CollaborationMode, ComplexityAssessment, ExpertRole};

/// Decision about how experts should collaborate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationDecision {
    pub mode: CollaborationMode,
    pub experts: Vec<ExpertRole>,
    pub rationale: String,
    pub estimated_benefit: f64,
}

/// Stored collaboration pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationPattern {
    pub id: Option<i64>,
    pub problem_domains: Vec<String>,
    pub complexity_threshold: f64,
    pub recommended_experts: Vec<ExpertRole>,
    pub collaboration_mode: CollaborationMode,
    pub synthesis_method: String,
    pub success_rate: f64,
    pub time_saved_percent: Option<f64>,
    pub occurrence_count: i64,
}

/// Decide how experts should collaborate based on problem analysis
pub fn decide_collaboration(
    conn: &Connection,
    complexity: &ComplexityAssessment,
    requested_experts: &[ExpertRole],
) -> Result<CollaborationDecision> {
    // Check for matching collaboration patterns
    if let Some(pattern) = find_matching_collaboration_pattern(conn, &complexity.domains_involved)?
    {
        if pattern.success_rate >= 0.6 {
            return Ok(CollaborationDecision {
                mode: pattern.collaboration_mode,
                experts: pattern.recommended_experts.clone(),
                rationale: format!(
                    "Using validated pattern with {:.0}% success rate",
                    pattern.success_rate * 100.0
                ),
                estimated_benefit: pattern.time_saved_percent.unwrap_or(0.0) / 100.0,
            });
        }
    }

    // Default decision based on complexity
    let decision = if complexity.score > 0.7 && complexity.domains_involved.len() > 2 {
        // High complexity, multiple domains - team collaboration
        let experts = select_experts_for_domains(&complexity.domains_involved);
        CollaborationDecision {
            mode: CollaborationMode::Parallel,
            experts,
            rationale: "High complexity with multiple domains requires parallel expert analysis"
                .to_string(),
            estimated_benefit: 0.3,
        }
    } else if complexity.score > 0.4 && complexity.domains_involved.len() > 1 {
        // Medium complexity - sequential consultation
        let experts = prioritize_experts_by_domain(&complexity.domains_involved);
        CollaborationDecision {
            mode: CollaborationMode::Sequential,
            experts,
            rationale: "Medium complexity benefits from sequential expert consultation".to_string(),
            estimated_benefit: 0.15,
        }
    } else if complexity.risk_level > 0.6 {
        // High risk - use security expert as lead
        let mut experts = vec![ExpertRole::Security];
        experts.extend(
            requested_experts
                .iter()
                .filter(|e| **e != ExpertRole::Security)
                .cloned(),
        );
        CollaborationDecision {
            mode: CollaborationMode::Hierarchical,
            experts,
            rationale: "High risk level requires security-led review".to_string(),
            estimated_benefit: 0.2,
        }
    } else {
        // Simple case - single expert
        CollaborationDecision {
            mode: CollaborationMode::Single,
            experts: requested_experts.to_vec(),
            rationale: "Low complexity allows single expert handling".to_string(),
            estimated_benefit: 0.0,
        }
    };

    Ok(decision)
}

/// Find a matching collaboration pattern from history
fn find_matching_collaboration_pattern(
    conn: &Connection,
    domains: &[String],
) -> Result<Option<CollaborationPattern>> {
    if domains.is_empty() {
        return Ok(None);
    }

    let domains_json = serde_json::to_string(domains).unwrap_or_default();

    let sql = r#"
        SELECT id, problem_domains, complexity_threshold, recommended_experts,
               collaboration_mode, synthesis_method, success_rate, time_saved_percent,
               occurrence_count
        FROM collaboration_patterns
        WHERE problem_domains = ?
          AND success_rate >= 0.5
        ORDER BY success_rate DESC, occurrence_count DESC
        LIMIT 1
    "#;

    let result = conn.query_row(sql, [&domains_json], |row| {
        let domains_str: String = row.get(1)?;
        let experts_str: String = row.get(3)?;
        let mode_str: String = row.get(4)?;

        let domains: Vec<String> = serde_json::from_str(&domains_str).unwrap_or_default();
        let experts: Vec<String> = serde_json::from_str(&experts_str).unwrap_or_default();
        let experts: Vec<ExpertRole> = experts
            .iter()
            .filter_map(|s| ExpertRole::from_str(s))
            .collect();

        let mode = match mode_str.as_str() {
            "parallel" => CollaborationMode::Parallel,
            "sequential" => CollaborationMode::Sequential,
            "hierarchical" => CollaborationMode::Hierarchical,
            _ => CollaborationMode::Single,
        };

        Ok(CollaborationPattern {
            id: Some(row.get(0)?),
            problem_domains: domains,
            complexity_threshold: row.get(2)?,
            recommended_experts: experts,
            collaboration_mode: mode,
            synthesis_method: row.get(5)?,
            success_rate: row.get(6)?,
            time_saved_percent: row.get(7)?,
            occurrence_count: row.get(8)?,
        })
    });

    match result {
        Ok(pattern) => Ok(Some(pattern)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Select experts based on problem domains
fn select_experts_for_domains(domains: &[String]) -> Vec<ExpertRole> {
    let mut experts = Vec::new();

    for domain in domains {
        let expert = match domain.to_lowercase().as_str() {
            "security" => ExpertRole::Security,
            "architecture" | "design" => ExpertRole::Architect,
            "performance" | "optimization" => ExpertRole::CodeReviewer,
            "requirements" | "scope" => ExpertRole::ScopeAnalyst,
            "planning" | "implementation" => ExpertRole::PlanReviewer,
            _ => ExpertRole::CodeReviewer,
        };

        if !experts.contains(&expert) {
            experts.push(expert);
        }
    }

    // Always include at least one expert
    if experts.is_empty() {
        experts.push(ExpertRole::CodeReviewer);
    }

    experts
}

/// Prioritize experts based on domain relevance
fn prioritize_experts_by_domain(domains: &[String]) -> Vec<ExpertRole> {
    let mut experts = select_experts_for_domains(domains);

    // Sort by typical priority: Security > Architect > PlanReviewer > ScopeAnalyst > CodeReviewer
    experts.sort_by_key(|e| match e {
        ExpertRole::Security => 0,
        ExpertRole::Architect => 1,
        ExpertRole::PlanReviewer => 2,
        ExpertRole::ScopeAnalyst => 3,
        ExpertRole::CodeReviewer => 4,
    });

    experts
}

/// Store a collaboration pattern
pub fn store_collaboration_pattern(
    conn: &Connection,
    pattern: &CollaborationPattern,
) -> Result<i64> {
    let domains_json = serde_json::to_string(&pattern.problem_domains).unwrap_or_default();
    let experts_json = serde_json::to_string(
        &pattern
            .recommended_experts
            .iter()
            .map(|e| e.as_str())
            .collect::<Vec<_>>(),
    )
    .unwrap_or_default();

    let sql = r#"
        INSERT INTO collaboration_patterns
        (problem_domains, complexity_threshold, recommended_experts, collaboration_mode,
         synthesis_method, success_rate, time_saved_percent, occurrence_count)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT DO UPDATE SET
            success_rate = (success_rate * occurrence_count + excluded.success_rate) / (occurrence_count + 1),
            occurrence_count = occurrence_count + 1,
            last_used_at = datetime('now')
    "#;

    conn.execute(
        sql,
        rusqlite::params![
            domains_json,
            pattern.complexity_threshold,
            experts_json,
            pattern.collaboration_mode.as_str(),
            pattern.synthesis_method,
            pattern.success_rate,
            pattern.time_saved_percent,
            pattern.occurrence_count,
        ],
    )?;

    Ok(conn.last_insert_rowid())
}

/// Update collaboration pattern success based on outcome
pub fn update_collaboration_success(
    conn: &Connection,
    pattern_id: i64,
    was_successful: bool,
) -> Result<()> {
    let success_value = if was_successful { 1.0 } else { 0.0 };

    let sql = r#"
        UPDATE collaboration_patterns
        SET success_rate = (success_rate * occurrence_count + ?) / (occurrence_count + 1),
            occurrence_count = occurrence_count + 1,
            last_used_at = datetime('now')
        WHERE id = ?
    "#;

    conn.execute(sql, rusqlite::params![success_value, pattern_id])?;
    Ok(())
}

/// Synthesize results from multiple experts
pub fn synthesize_expert_results(
    mode: CollaborationMode,
    results: &[(ExpertRole, String)],
) -> String {
    match mode {
        CollaborationMode::Parallel => {
            // Combine all results with headers
            let sections: Vec<String> = results
                .iter()
                .map(|(role, result)| format!("## {} Analysis\n\n{}", role.as_str(), result))
                .collect();
            sections.join("\n\n---\n\n")
        }
        CollaborationMode::Sequential => {
            // Present in order with flow
            let sections: Vec<String> = results
                .iter()
                .enumerate()
                .map(|(i, (role, result))| {
                    format!(
                        "## Step {}: {} Analysis\n\n{}",
                        i + 1,
                        role.as_str(),
                        result
                    )
                })
                .collect();
            sections.join("\n\n")
        }
        CollaborationMode::Hierarchical => {
            // Lead expert first, then supporting
            if let Some((lead, lead_result)) = results.first() {
                let supporting: Vec<String> = results[1..]
                    .iter()
                    .map(|(role, result)| {
                        format!("### Supporting: {}\n\n{}", role.as_str(), result)
                    })
                    .collect();

                format!(
                    "## Lead Analysis ({})\n\n{}\n\n{}",
                    lead.as_str(),
                    lead_result,
                    supporting.join("\n\n")
                )
            } else {
                String::new()
            }
        }
        CollaborationMode::Single => {
            // Just return the single result
            results
                .first()
                .map(|(_, result)| result.clone())
                .unwrap_or_default()
        }
    }
}
