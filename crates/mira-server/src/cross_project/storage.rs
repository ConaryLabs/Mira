// crates/mira-server/src/cross_project/storage.rs
// Cross-project pattern storage and retrieval

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::{
    AnonymizationLevel, AnonymizedPattern, CrossPatternType, SharingDirection,
    preferences::{consume_privacy_budget, get_preferences},
};

/// A cross-project pattern stored in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossProjectPattern {
    pub id: Option<i64>,
    pub pattern_type: CrossPatternType,
    pub pattern_hash: String,
    pub anonymized_data: serde_json::Value,
    pub category: Option<String>,
    pub confidence: f64,
    pub occurrence_count: i64,
    pub noise_added: f64,
    pub source_project_count: i64,
    pub min_projects_required: i64,
}

impl CrossProjectPattern {
    /// Check if this pattern meets k-anonymity requirements
    pub fn is_shareable(&self) -> bool {
        self.source_project_count >= self.min_projects_required
    }
}

/// Store an anonymized pattern in the cross-project database
pub fn store_pattern(
    conn: &Connection,
    project_id: i64,
    pattern: &AnonymizedPattern,
    k_anonymity: i64,
) -> Result<i64> {
    let tx = conn.unchecked_transaction()?;

    // Check preferences
    let prefs = get_preferences(&tx, project_id)?;
    if !prefs.can_export(pattern.pattern_type, pattern.noise_added) {
        anyhow::bail!("Export not allowed for this pattern type or privacy budget exhausted");
    }

    let data_json = serde_json::to_string(&pattern.anonymized_data)?;

    // Upsert the pattern and always return the affected row id.
    let sql = r#"
        INSERT INTO cross_project_patterns
        (pattern_type, pattern_hash, anonymized_data, category, confidence,
         noise_added, min_projects_required, source_project_count)
        VALUES (?, ?, ?, ?, ?, ?, ?, 1)
        ON CONFLICT(pattern_hash) DO UPDATE SET
            confidence = (cross_project_patterns.confidence * cross_project_patterns.source_project_count + excluded.confidence) / (cross_project_patterns.source_project_count + 1),
            occurrence_count = cross_project_patterns.occurrence_count + 1,
            source_project_count = cross_project_patterns.source_project_count + 1,
            last_updated_at = datetime('now')
        RETURNING id
    "#;

    let pattern_id: i64 = tx.query_row(
        sql,
        rusqlite::params![
            pattern.pattern_type.as_str(),
            pattern.pattern_hash,
            data_json,
            pattern.category,
            pattern.confidence,
            pattern.noise_added,
            k_anonymity,
        ],
        |row| row.get(0),
    )?;

    // Record provenance (anonymized)
    let contribution_hash = hash_contribution(project_id, &pattern.pattern_hash);
    let provenance_sql = r#"
        INSERT OR IGNORE INTO pattern_provenance
        (pattern_id, contribution_hash)
        VALUES (?, ?)
    "#;
    tx.execute(
        provenance_sql,
        rusqlite::params![pattern_id, contribution_hash],
    )?;

    // Consume privacy budget
    if !consume_privacy_budget(&tx, project_id, pattern.noise_added)? {
        anyhow::bail!("Privacy budget exhausted while storing pattern");
    }

    // Log the sharing event
    log_sharing_event(
        &tx,
        project_id,
        SharingDirection::Export,
        pattern.pattern_type,
        &pattern.pattern_hash,
        pattern.anonymization_level,
        pattern.noise_added,
    )?;

    tx.commit()?;
    Ok(pattern_id)
}

/// Get patterns that can be shared with a project
pub fn get_shareable_patterns(
    conn: &Connection,
    category: Option<&str>,
    pattern_type: Option<CrossPatternType>,
    min_confidence: f64,
    limit: i64,
) -> Result<Vec<CrossProjectPattern>> {
    let mut sql = r#"
        SELECT id, pattern_type, pattern_hash, anonymized_data, category,
               confidence, occurrence_count, noise_added, source_project_count,
               min_projects_required
        FROM cross_project_patterns
        WHERE source_project_count >= min_projects_required
          AND confidence >= ?
    "#
    .to_string();

    if category.is_some() {
        sql.push_str(" AND category = ?");
    }
    if pattern_type.is_some() {
        sql.push_str(" AND pattern_type = ?");
    }

    sql.push_str(" ORDER BY confidence DESC, occurrence_count DESC LIMIT ?");

    let mut stmt = conn.prepare(&sql)?;

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(min_confidence)];
    if let Some(cat) = category {
        params.push(Box::new(cat.to_string()));
    }
    if let Some(pt) = pattern_type {
        params.push(Box::new(pt.as_str().to_string()));
    }
    params.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let type_str: String = row.get(1)?;
        let data_str: String = row.get(3)?;

        Ok(CrossProjectPattern {
            id: Some(row.get(0)?),
            pattern_type: type_str
                .parse::<CrossPatternType>()
                .unwrap_or(CrossPatternType::Behavior),
            pattern_hash: row.get(2)?,
            anonymized_data: serde_json::from_str(&data_str).unwrap_or(serde_json::json!({})),
            category: row.get(4)?,
            confidence: row.get(5)?,
            occurrence_count: row.get(6)?,
            noise_added: row.get(7)?,
            source_project_count: row.get(8)?,
            min_projects_required: row.get(9)?,
        })
    })?;

    let patterns: Vec<CrossProjectPattern> = rows.filter_map(crate::db::log_and_discard).collect();
    Ok(patterns)
}

/// Import patterns to a project
pub fn import_pattern(
    conn: &Connection,
    project_id: i64,
    pattern: &CrossProjectPattern,
) -> Result<()> {
    // Check preferences
    let prefs = get_preferences(conn, project_id)?;
    if !prefs.can_import(pattern.pattern_type) {
        anyhow::bail!("Import not allowed for this pattern type");
    }

    // Log the import
    log_sharing_event(
        conn,
        project_id,
        SharingDirection::Import,
        pattern.pattern_type,
        &pattern.pattern_hash,
        AnonymizationLevel::Full, // Imported patterns are already anonymized
        pattern.noise_added,
    )?;

    Ok(())
}

/// Get patterns relevant for a specific project based on category
pub fn get_patterns_for_project(
    conn: &Connection,
    project_id: i64,
    category: Option<&str>,
    limit: i64,
) -> Result<Vec<CrossProjectPattern>> {
    let prefs = get_preferences(conn, project_id)?;

    if !prefs.sharing_enabled || !prefs.import_patterns {
        return Ok(vec![]);
    }

    // Get patterns that match the project's allowed types
    let patterns = get_shareable_patterns(
        conn,
        category,
        None,      // Get all types, filter by preferences
        0.5,       // Minimum confidence
        limit * 2, // Get more, then filter
    )?;

    // Filter by allowed pattern types
    let filtered: Vec<CrossProjectPattern> = patterns
        .into_iter()
        .filter(|p| prefs.is_pattern_type_allowed(p.pattern_type))
        .take(limit as usize)
        .collect();

    Ok(filtered)
}

/// Log a pattern sharing event
pub fn log_sharing_event(
    conn: &Connection,
    project_id: i64,
    direction: SharingDirection,
    pattern_type: CrossPatternType,
    pattern_hash: &str,
    anonymization_level: AnonymizationLevel,
    epsilon: f64,
) -> Result<i64> {
    let sql = r#"
        INSERT INTO pattern_sharing_log
        (project_id, direction, pattern_type, pattern_hash,
         anonymization_level, differential_privacy_epsilon)
        VALUES (?, ?, ?, ?, ?, ?)
    "#;

    conn.execute(
        sql,
        rusqlite::params![
            project_id,
            direction.as_str(),
            pattern_type.as_str(),
            pattern_hash,
            anonymization_level.as_str(),
            epsilon,
        ],
    )?;

    Ok(conn.last_insert_rowid())
}

/// Get sharing statistics for a project
pub fn get_sharing_stats(conn: &Connection, project_id: i64) -> Result<SharingStats> {
    let sql = r#"
        SELECT
            SUM(CASE WHEN direction = 'exported' THEN 1 ELSE 0 END) as exports,
            SUM(CASE WHEN direction = 'imported' THEN 1 ELSE 0 END) as imports,
            SUM(CASE WHEN direction = 'exported' THEN differential_privacy_epsilon ELSE 0 END) as epsilon_spent
        FROM pattern_sharing_log
        WHERE project_id = ?
    "#;

    let result = conn.query_row(sql, [project_id], |row| {
        Ok(SharingStats {
            exports: row.get::<_, Option<i64>>(0)?.unwrap_or(0),
            imports: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            epsilon_spent: row.get::<_, Option<f64>>(2)?.unwrap_or(0.0),
        })
    });

    match result {
        Ok(stats) => Ok(stats),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(SharingStats::default()),
        Err(e) => Err(e.into()),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SharingStats {
    pub exports: i64,
    pub imports: i64,
    pub epsilon_spent: f64,
}

/// Hash a project contribution for anonymous provenance tracking
fn hash_contribution(project_id: i64, pattern_hash: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    // Include a salt that changes over time to prevent correlation
    let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
    hasher.update(day.as_bytes());
    hasher.update(b":");
    hasher.update(project_id.to_string().as_bytes());
    hasher.update(b":");
    hasher.update(pattern_hash.as_bytes());

    format!("{:x}", hasher.finalize())[..16].to_string()
}

/// Extract and store patterns from a project's local patterns
pub fn extract_and_store_patterns(
    conn: &Connection,
    project_id: i64,
    config: &super::CrossProjectConfig,
) -> Result<usize> {
    use super::anonymizer::PatternAnonymizer;

    let prefs = get_preferences(conn, project_id)?;
    if !prefs.can_export(CrossPatternType::Behavior, config.epsilon) {
        return Ok(0);
    }

    let anonymizer = PatternAnonymizer::new(
        prefs.remaining_privacy_budget().min(config.epsilon),
        prefs.min_anonymization_level,
    );

    let mut stored_count = 0;

    // Extract file sequence patterns from behavior_patterns
    let file_sql = r#"
        SELECT pattern_data, confidence
        FROM behavior_patterns
        WHERE project_id = ? AND pattern_type = 'file_sequence' AND confidence >= ?
        LIMIT 50
    "#;

    let mut stmt = conn.prepare(file_sql)?;
    let file_rows = stmt.query_map(
        rusqlite::params![project_id, config.min_confidence],
        |row| {
            let data: String = row.get(0)?;
            let confidence: f64 = row.get(1)?;
            Ok((data, confidence))
        },
    )?;

    for row in file_rows.filter_map(crate::db::log_and_discard) {
        let (data, confidence) = row;
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data)
            && let Some(seq) = parsed.get("sequence").and_then(|s| s.as_array())
        {
            let files: Vec<String> = seq
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();

            if let Ok(anonymized) = anonymizer.anonymize_file_sequence(&files, confidence)
                && store_pattern(
                    conn,
                    project_id,
                    &anonymized,
                    config.k_anonymity_threshold as i64,
                )
                .is_ok()
            {
                stored_count += 1;
            }
        }
    }

    // Extract tool chain patterns
    let tool_sql = r#"
        SELECT pattern_data, confidence
        FROM behavior_patterns
        WHERE project_id = ? AND pattern_type = 'tool_chain' AND confidence >= ?
        LIMIT 50
    "#;

    let mut stmt = conn.prepare(tool_sql)?;
    let tool_rows = stmt.query_map(
        rusqlite::params![project_id, config.min_confidence],
        |row| {
            let data: String = row.get(0)?;
            let confidence: f64 = row.get(1)?;
            Ok((data, confidence))
        },
    )?;

    for row in tool_rows.filter_map(crate::db::log_and_discard) {
        let (data, confidence) = row;
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data)
            && let Some(tools) = parsed.get("tools").and_then(|t| t.as_array())
        {
            let tool_names: Vec<String> = tools
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();

            if let Ok(anonymized) = anonymizer.anonymize_tool_chain(&tool_names, confidence)
                && store_pattern(
                    conn,
                    project_id,
                    &anonymized,
                    config.k_anonymity_threshold as i64,
                )
                .is_ok()
            {
                stored_count += 1;
            }
        }
    }

    // Extract problem patterns from expert system
    let problem_sql = r#"
        SELECT expert_role, pattern_signature, pattern_description,
               successful_approaches, recommended_tools, success_rate
        FROM problem_patterns
        WHERE success_rate >= ?
        LIMIT 50
    "#;

    let mut stmt = conn.prepare(problem_sql)?;
    let problem_rows = stmt.query_map([config.min_confidence], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, f64>(5)?,
        ))
    })?;

    for row in problem_rows.filter_map(crate::db::log_and_discard) {
        let (expert_role, _signature, _description, approaches_json, tools_json, success_rate) =
            row;

        let approaches: Vec<String> = serde_json::from_str(&approaches_json).unwrap_or_default();
        let tools: Vec<String> = serde_json::from_str(&tools_json).unwrap_or_default();

        // Extract problem category from signature (format: "role:category")
        let category = _signature.split(':').nth(1).unwrap_or("general");

        if let Ok(anonymized) = anonymizer.anonymize_problem_pattern(
            &expert_role,
            category,
            &approaches,
            &tools,
            success_rate,
        ) && store_pattern(
            conn,
            project_id,
            &anonymized,
            config.k_anonymity_threshold as i64,
        )
        .is_ok()
        {
            stored_count += 1;
        }
    }

    Ok(stored_count)
}
