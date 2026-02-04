// crates/mira-server/src/proactive/feedback.rs
// Feedback loop - learns from user responses to improve predictions

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::patterns::update_pattern_confidence;
use super::{InterventionType, UserResponse};

/// Parameters for recording an intervention
pub struct InterventionParams<'a> {
    pub project_id: i64,
    pub session_id: &'a str,
    pub intervention_type: InterventionType,
    pub trigger_pattern_id: Option<i64>,
    pub trigger_context: &'a str,
    pub suggestion_content: &'a str,
    pub confidence: f64,
}

/// Record an intervention that was shown to the user
pub fn record_intervention(conn: &Connection, p: &InterventionParams) -> Result<i64> {
    let sql = r#"
        INSERT INTO proactive_interventions
        (project_id, session_id, intervention_type, trigger_pattern_id, trigger_context, suggestion_content, confidence)
        VALUES (?, ?, ?, ?, ?, ?, ?)
    "#;

    conn.execute(
        sql,
        rusqlite::params![
            p.project_id,
            p.session_id,
            p.intervention_type.as_str(),
            p.trigger_pattern_id,
            p.trigger_context,
            p.suggestion_content,
            p.confidence,
        ],
    )?;

    Ok(conn.last_insert_rowid())
}

/// Record user response to an intervention
pub fn record_response(
    conn: &Connection,
    intervention_id: i64,
    response: UserResponse,
    response_time_ms: Option<i64>,
) -> Result<()> {
    // Calculate effectiveness score based on response
    let effectiveness = response.effectiveness_multiplier();

    let sql = r#"
        UPDATE proactive_interventions
        SET user_response = ?,
            response_time_ms = ?,
            effectiveness_score = ?,
            responded_at = datetime('now')
        WHERE id = ?
    "#;

    conn.execute(
        sql,
        rusqlite::params![
            response.as_str(),
            response_time_ms,
            effectiveness,
            intervention_id,
        ],
    )?;

    // Update the source pattern's confidence if there is one
    let pattern_id: Option<i64> = conn
        .query_row(
            "SELECT trigger_pattern_id FROM proactive_interventions WHERE id = ?",
            [intervention_id],
            |row| row.get(0),
        )
        .ok();

    if let Some(pattern_id) = pattern_id {
        update_pattern_confidence(conn, pattern_id, effectiveness)?;
    }

    Ok(())
}

/// Get pending interventions (those without responses)
pub fn get_pending_interventions(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<PendingIntervention>> {
    let sql = r#"
        SELECT id, intervention_type, suggestion_content, confidence, created_at
        FROM proactive_interventions
        WHERE session_id = ? AND user_response IS NULL
        ORDER BY created_at DESC
        LIMIT 10
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([session_id], |row| {
        Ok(PendingIntervention {
            id: row.get(0)?,
            intervention_type: row.get(1)?,
            suggestion_content: row.get(2)?,
            confidence: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;

    let interventions: Vec<PendingIntervention> = rows.flatten().collect();
    Ok(interventions)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingIntervention {
    pub id: i64,
    pub intervention_type: String,
    pub suggestion_content: String,
    pub confidence: f64,
    pub created_at: String,
}

/// Calculate overall effectiveness for a project
pub fn get_effectiveness_stats(conn: &Connection, project_id: i64) -> Result<EffectivenessStats> {
    let sql = r#"
        SELECT
            COUNT(*) as total,
            SUM(CASE WHEN user_response = 'accepted' THEN 1 ELSE 0 END) as accepted,
            SUM(CASE WHEN user_response = 'dismissed' THEN 1 ELSE 0 END) as dismissed,
            SUM(CASE WHEN user_response = 'acted_upon' THEN 1 ELSE 0 END) as acted_upon,
            SUM(CASE WHEN user_response = 'ignored' THEN 1 ELSE 0 END) as ignored,
            AVG(effectiveness_score) as avg_effectiveness
        FROM proactive_interventions
        WHERE project_id = ? AND user_response IS NOT NULL
    "#;

    let result = conn.query_row(sql, [project_id], |row| {
        Ok(EffectivenessStats {
            total: row.get(0)?,
            accepted: row.get(1)?,
            dismissed: row.get(2)?,
            acted_upon: row.get(3)?,
            ignored: row.get(4)?,
            avg_effectiveness: row.get::<_, Option<f64>>(5)?.unwrap_or(0.0),
        })
    })?;

    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectivenessStats {
    pub total: i64,
    pub accepted: i64,
    pub dismissed: i64,
    pub acted_upon: i64,
    pub ignored: i64,
    pub avg_effectiveness: f64,
}

impl EffectivenessStats {
    pub fn acceptance_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.accepted + self.acted_upon) as f64 / self.total as f64
        }
    }
}

/// Run background learning to improve pattern confidence
/// Call this periodically (e.g., at session end)
pub fn run_learning_update(conn: &Connection, project_id: i64) -> Result<usize> {
    // Find patterns with enough feedback to update
    let sql = r#"
        SELECT bp.id, AVG(pi.effectiveness_score) as avg_eff, COUNT(*) as count
        FROM behavior_patterns bp
        JOIN proactive_interventions pi ON pi.trigger_pattern_id = bp.id
        WHERE bp.project_id = ?
          AND pi.user_response IS NOT NULL
          AND pi.created_at > datetime('now', '-7 days')
        GROUP BY bp.id
        HAVING COUNT(*) >= 3
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([project_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
    })?;

    let mut updates = 0;
    for row in rows.flatten() {
        let (pattern_id, avg_effectiveness) = row;

        // Update pattern confidence based on average effectiveness
        // Use weighted update: new_confidence = 0.7 * old + 0.3 * feedback
        let update_sql = r#"
            UPDATE behavior_patterns
            SET confidence = confidence * 0.7 + ? * 0.3,
                updated_at = datetime('now')
            WHERE id = ?
        "#;

        conn.execute(update_sql, rusqlite::params![avg_effectiveness, pattern_id])?;
        updates += 1;
    }

    Ok(updates)
}

/// Mark old pending interventions as ignored
pub fn mark_stale_as_ignored(conn: &Connection, timeout_minutes: i64) -> Result<usize> {
    let sql = r#"
        UPDATE proactive_interventions
        SET user_response = 'ignored',
            effectiveness_score = 0.0,
            responded_at = datetime('now')
        WHERE user_response IS NULL
          AND created_at < datetime('now', ? || ' minutes')
    "#;

    let updated = conn.execute(sql, [format!("-{}", timeout_minutes)])?;
    Ok(updated)
}
