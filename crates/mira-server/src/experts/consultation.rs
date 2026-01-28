// crates/mira-server/src/experts/consultation.rs
// Consultation logging and history tracking

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::ExpertRole;

/// Record of an expert consultation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsultationRecord {
    pub id: Option<i64>,
    pub expert_role: ExpertRole,
    pub project_id: i64,
    pub session_id: Option<String>,
    pub context_hash: String,
    pub problem_category: Option<String>,
    pub context_summary: String,
    pub tools_used: Vec<String>,
    pub tool_call_count: i32,
    pub consultation_duration_ms: Option<i64>,
    pub initial_confidence: Option<f64>,
    pub calibrated_confidence: Option<f64>,
    pub prompt_version: i32,
}

impl ConsultationRecord {
    pub fn new(expert_role: ExpertRole, project_id: i64, context: &str) -> Self {
        let context_hash = hash_context(context);
        let context_summary = summarize_context(context);
        let problem_category = categorize_problem(context);

        Self {
            id: None,
            expert_role,
            project_id,
            session_id: None,
            context_hash,
            problem_category,
            context_summary,
            tools_used: vec![],
            tool_call_count: 0,
            consultation_duration_ms: None,
            initial_confidence: None,
            calibrated_confidence: None,
            prompt_version: 1,
        }
    }

    pub fn with_session(mut self, session_id: &str) -> Self {
        self.session_id = Some(session_id.to_string());
        self
    }

    pub fn with_tools(mut self, tools: Vec<String>, call_count: i32) -> Self {
        self.tools_used = tools;
        self.tool_call_count = call_count;
        self
    }

    pub fn with_duration(mut self, duration_ms: i64) -> Self {
        self.consultation_duration_ms = Some(duration_ms);
        self
    }

    pub fn with_confidence(mut self, initial: f64, calibrated: f64) -> Self {
        self.initial_confidence = Some(initial);
        self.calibrated_confidence = Some(calibrated);
        self
    }
}

/// Hash context for pattern matching
fn hash_context(context: &str) -> String {
    let mut hasher = Sha256::new();
    // Normalize context before hashing
    let normalized = context
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string()
}

/// Create a brief summary of the context
fn summarize_context(context: &str) -> String {
    let words: Vec<&str> = context.split_whitespace().collect();
    if words.len() <= 50 {
        context.to_string()
    } else {
        words[..50].join(" ") + "..."
    }
}

/// Categorize the problem based on context keywords
fn categorize_problem(context: &str) -> Option<String> {
    let lower = context.to_lowercase();

    if lower.contains("security") || lower.contains("vulnerab") || lower.contains("auth") {
        Some("security".to_string())
    } else if lower.contains("performance") || lower.contains("optim") || lower.contains("slow") {
        Some("performance".to_string())
    } else if lower.contains("bug") || lower.contains("error") || lower.contains("fix") {
        Some("bug_fix".to_string())
    } else if lower.contains("refactor") || lower.contains("clean") {
        Some("refactoring".to_string())
    } else if lower.contains("design") || lower.contains("architect") || lower.contains("pattern") {
        Some("architecture".to_string())
    } else if lower.contains("test") || lower.contains("coverage") {
        Some("testing".to_string())
    } else if lower.contains("document") || lower.contains("readme") {
        Some("documentation".to_string())
    } else if lower.contains("feature") || lower.contains("implement") || lower.contains("add") {
        Some("new_feature".to_string())
    } else {
        Some("general".to_string())
    }
}

/// Log a consultation to the database
pub fn log_consultation(conn: &Connection, record: &ConsultationRecord) -> Result<i64> {
    let sql = r#"
        INSERT INTO expert_consultations
        (expert_role, project_id, session_id, context_hash, problem_category,
         context_summary, tools_used, tool_call_count, consultation_duration_ms,
         initial_confidence, calibrated_confidence, prompt_version)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    "#;

    let tools_json = serde_json::to_string(&record.tools_used).unwrap_or_default();

    conn.execute(
        sql,
        rusqlite::params![
            record.expert_role.as_str(),
            record.project_id,
            record.session_id,
            record.context_hash,
            record.problem_category,
            record.context_summary,
            tools_json,
            record.tool_call_count,
            record.consultation_duration_ms,
            record.initial_confidence,
            record.calibrated_confidence,
            record.prompt_version,
        ],
    )?;

    Ok(conn.last_insert_rowid())
}

/// Get recent consultations for an expert
pub fn get_recent_consultations(
    conn: &Connection,
    expert_role: ExpertRole,
    project_id: i64,
    limit: i64,
) -> Result<Vec<ConsultationRecord>> {
    let sql = r#"
        SELECT id, expert_role, project_id, session_id, context_hash, problem_category,
               context_summary, tools_used, tool_call_count, consultation_duration_ms,
               initial_confidence, calibrated_confidence, prompt_version
        FROM expert_consultations
        WHERE expert_role = ? AND project_id = ?
        ORDER BY created_at DESC
        LIMIT ?
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(
        rusqlite::params![expert_role.as_str(), project_id, limit],
        |row| {
            let tools_json: String = row.get(7)?;
            let tools: Vec<String> = serde_json::from_str(&tools_json).unwrap_or_default();
            let role_str: String = row.get(1)?;

            Ok(ConsultationRecord {
                id: Some(row.get(0)?),
                expert_role: ExpertRole::from_str(&role_str).unwrap_or(ExpertRole::Architect),
                project_id: row.get(2)?,
                session_id: row.get(3)?,
                context_hash: row.get(4)?,
                problem_category: row.get(5)?,
                context_summary: row.get(6)?,
                tools_used: tools,
                tool_call_count: row.get(8)?,
                consultation_duration_ms: row.get(9)?,
                initial_confidence: row.get(10)?,
                calibrated_confidence: row.get(11)?,
                prompt_version: row.get(12)?,
            })
        },
    )?;

    let consultations: Vec<ConsultationRecord> = rows.flatten().collect();
    Ok(consultations)
}

/// Get consultation statistics for an expert
pub fn get_expert_stats(
    conn: &Connection,
    expert_role: ExpertRole,
    project_id: i64,
) -> Result<ExpertStats> {
    let sql = r#"
        SELECT
            COUNT(*) as total_consultations,
            AVG(tool_call_count) as avg_tool_calls,
            AVG(consultation_duration_ms) as avg_duration_ms,
            AVG(initial_confidence) as avg_confidence
        FROM expert_consultations
        WHERE expert_role = ? AND project_id = ?
    "#;

    let result = conn.query_row(
        sql,
        [expert_role.as_str(), &project_id.to_string()],
        |row| {
            Ok(ExpertStats {
                total_consultations: row.get(0)?,
                avg_tool_calls: row.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                avg_duration_ms: row.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
                avg_confidence: row.get::<_, Option<f64>>(3)?.unwrap_or(0.5),
                acceptance_rate: 0.0, // Will be filled from findings
            })
        },
    )?;

    // Get acceptance rate from review_findings
    let acceptance_sql = r#"
        SELECT
            CAST(SUM(CASE WHEN status IN ('accepted', 'fixed') THEN 1 ELSE 0 END) AS REAL) /
            NULLIF(COUNT(*), 0) as acceptance_rate
        FROM review_findings
        WHERE expert_role = ? AND project_id = ?
    "#;

    let acceptance_rate: f64 = conn
        .query_row(
            acceptance_sql,
            [expert_role.as_str(), &project_id.to_string()],
            |row| row.get::<_, Option<f64>>(0),
        )
        .ok()
        .flatten()
        .unwrap_or(0.5);

    Ok(ExpertStats {
        acceptance_rate,
        ..result
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertStats {
    pub total_consultations: i64,
    pub avg_tool_calls: f64,
    pub avg_duration_ms: f64,
    pub avg_confidence: f64,
    pub acceptance_rate: f64,
}

impl ExpertStats {
    /// Calculate calibrated confidence based on historical accuracy
    pub fn calibrate_confidence(&self, stated_confidence: f64) -> f64 {
        if self.total_consultations < 5 {
            // Not enough data, trust stated confidence
            stated_confidence
        } else {
            // Weighted average: 70% stated, 30% historical
            stated_confidence * 0.7 + self.acceptance_rate * 0.3
        }
    }
}
