// src/tools/corrections.rs
// Correction tracking - Learn from user corrections to avoid repeated mistakes

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use super::semantic::{SemanticSearch, COLLECTION_CONVERSATION};
use super::semantic_helpers::{MetadataBuilder, store_with_logging};

// === Parameter structs for consolidated correction tool ===

pub struct RecordCorrectionParams {
    pub correction_type: String,
    pub what_was_wrong: String,
    pub what_is_right: String,
    pub rationale: Option<String>,
    pub scope: Option<String>,
    pub keywords: Option<String>,
}

pub struct GetCorrectionsParams {
    pub file_path: Option<String>,
    pub topic: Option<String>,
    pub correction_type: Option<String>,
    pub context: Option<String>,
    pub limit: Option<i64>,
}

pub struct ListCorrectionsParams {
    pub correction_type: Option<String>,
    pub scope: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
}

/// Record a new correction when user corrects Claude's approach
pub async fn record_correction(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: RecordCorrectionParams,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();
    let id = Uuid::new_v4().to_string();
    let scope = req.scope.as_deref().unwrap_or("project");
    let keywords = normalize_json_array(&req.keywords);

    sqlx::query(r#"
        INSERT INTO corrections (id, correction_type, what_was_wrong, what_is_right, rationale,
                                scope, project_id, keywords, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
    "#)
    .bind(&id)
    .bind(&req.correction_type)
    .bind(&req.what_was_wrong)
    .bind(&req.what_is_right)
    .bind(&req.rationale)
    .bind(scope)
    .bind(if scope == "global" { None } else { project_id })
    .bind(&keywords)
    .bind(now)
    .execute(db)
    .await?;

    // Store in semantic search for fuzzy matching
    let content = format!(
        "Correction: {} -> {}. Rationale: {}",
        req.what_was_wrong,
        req.what_is_right,
        req.rationale.as_deref().unwrap_or("")
    );
    let metadata = MetadataBuilder::new("correction")
        .string("correction_type", &req.correction_type)
        .string("scope", scope)
        .string("id", &id)
        .project_id(project_id)
        .build();
    store_with_logging(semantic, COLLECTION_CONVERSATION, &id, &content, metadata).await;

    Ok(serde_json::json!({
        "status": "recorded",
        "correction_id": id,
        "correction_type": req.correction_type,
        "scope": scope,
    }))
}

/// Get corrections relevant to current context (file, topic, keywords)
pub async fn get_corrections(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: GetCorrectionsParams,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(10);
    let mut results = Vec::new();

    // SQL-based matching for precise matches
    let sql_results = sqlx::query_as::<_, (String, String, String, String, Option<String>, String, f64, i64, i64)>(r#"
        SELECT id, correction_type, what_was_wrong, what_is_right, rationale, scope,
               confidence, times_applied, times_validated
        FROM corrections
        WHERE status = 'active'
          AND (project_id IS NULL OR project_id = $1)
          AND ($2 IS NULL OR correction_type = $2)
        ORDER BY confidence DESC, times_validated DESC
        LIMIT $3
    "#)
    .bind(project_id)
    .bind(&req.correction_type)
    .bind(limit)
    .fetch_all(db)
    .await?;

    for (id, ctype, wrong, right, rationale, scope, confidence, applied, validated) in sql_results {
        let matches = check_correction_relevance(db, &id, &req).await?;
        if matches > 0.0 {
            results.push(serde_json::json!({
                "id": id,
                "correction_type": ctype,
                "what_was_wrong": wrong,
                "what_is_right": right,
                "rationale": rationale,
                "scope": scope,
                "confidence": confidence,
                "times_applied": applied,
                "times_validated": validated,
                "relevance_score": matches,
            }));
        }
    }

    // Supplement with semantic matches if available
    if semantic.is_available() {
        if let Some(context) = &req.context {
            if let Ok(semantic_results) = semantic.search(COLLECTION_CONVERSATION, context, limit as usize, None).await {
            for result in semantic_results {
                if result.metadata.get("type").and_then(|t| t.as_str()) == Some("correction") {
                    if let Some(correction_id) = result.metadata.get("id").and_then(|v| v.as_str()) {
                        if !results.iter().any(|r| r.get("id").and_then(|v| v.as_str()) == Some(correction_id)) {
                            if let Some(correction) = get_correction_by_id(db, correction_id).await? {
                                let mut c = correction;
                                c["relevance_score"] = serde_json::json!(result.score);
                                c["match_type"] = serde_json::json!("semantic");
                                results.push(c);
                            }
                        }
                    }
                }
            }
        }
        }
    }

    results.sort_by(|a, b| {
        let score_a = a.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let score_b = b.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit as usize);

    for result in &results {
        if let Some(id) = result.get("id").and_then(|v| v.as_str()) {
            let _ = sqlx::query("UPDATE corrections SET times_applied = times_applied + 1 WHERE id = $1")
                .bind(id)
                .execute(db)
                .await;
        }
    }

    Ok(results)
}

/// Validate a correction (mark as helpful or not)
pub async fn validate_correction(
    db: &SqlitePool,
    correction_id: &str,
    outcome: &str,
) -> anyhow::Result<serde_json::Value> {
    let now = Utc::now().timestamp();

    match outcome {
        "validated" => {
            sqlx::query(r#"
                UPDATE corrections
                SET times_validated = times_validated + 1,
                    confidence = MIN(1.0, confidence + 0.05),
                    updated_at = $2
                WHERE id = $1
            "#)
            .bind(correction_id)
            .bind(now)
            .execute(db)
            .await?;
        }
        "overridden" => {
            sqlx::query("UPDATE corrections SET updated_at = $2 WHERE id = $1")
                .bind(correction_id)
                .bind(now)
                .execute(db)
                .await?;
        }
        "deprecated" => {
            sqlx::query(r#"
                UPDATE corrections
                SET status = 'deprecated', updated_at = $2
                WHERE id = $1
            "#)
            .bind(correction_id)
            .bind(now)
            .execute(db)
            .await?;
        }
        _ => {
            return Err(anyhow::anyhow!("Invalid outcome: {}. Use 'validated', 'overridden', or 'deprecated'", outcome));
        }
    }

    sqlx::query(r#"
        INSERT INTO correction_applications (correction_id, outcome, applied_at)
        VALUES ($1, $2, $3)
    "#)
    .bind(correction_id)
    .bind(outcome)
    .bind(now)
    .execute(db)
    .await?;

    Ok(serde_json::json!({
        "status": "recorded",
        "correction_id": correction_id,
        "outcome": outcome,
    }))
}

/// List all corrections for a project
pub async fn list_corrections(
    db: &SqlitePool,
    req: ListCorrectionsParams,
    project_id: Option<i64>,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let limit = req.limit.unwrap_or(20);
    let status = req.status.as_deref().unwrap_or("active");

    let results = sqlx::query_as::<_, (String, String, String, String, Option<String>, String, f64, i64, i64, String)>(r#"
        SELECT id, correction_type, what_was_wrong, what_is_right, rationale, scope,
               confidence, times_applied, times_validated,
               datetime(created_at, 'unixepoch', 'localtime') as created
        FROM corrections
        WHERE status = $1
          AND (project_id IS NULL OR project_id = $2)
          AND ($3 IS NULL OR correction_type = $3)
          AND ($4 IS NULL OR scope = $4)
        ORDER BY created_at DESC
        LIMIT $5
    "#)
    .bind(status)
    .bind(project_id)
    .bind(&req.correction_type)
    .bind(&req.scope)
    .bind(limit)
    .fetch_all(db)
    .await?;

    Ok(results.into_iter().map(|(id, ctype, wrong, right, rationale, scope, confidence, applied, validated, created)| {
        serde_json::json!({
            "id": id,
            "correction_type": ctype,
            "what_was_wrong": wrong,
            "what_is_right": right,
            "rationale": rationale,
            "scope": scope,
            "confidence": confidence,
            "times_applied": applied,
            "times_validated": validated,
            "created_at": created,
        })
    }).collect())
}

// === Helper Functions ===

async fn get_correction_by_id(db: &SqlitePool, id: &str) -> anyhow::Result<Option<serde_json::Value>> {
    let result = sqlx::query_as::<_, (String, String, String, String, Option<String>, String, f64, i64, i64)>(r#"
        SELECT id, correction_type, what_was_wrong, what_is_right, rationale, scope,
               confidence, times_applied, times_validated
        FROM corrections
        WHERE id = $1 AND status = 'active'
    "#)
    .bind(id)
    .fetch_optional(db)
    .await?;

    Ok(result.map(|(id, ctype, wrong, right, rationale, scope, confidence, applied, validated)| {
        serde_json::json!({
            "id": id,
            "correction_type": ctype,
            "what_was_wrong": wrong,
            "what_is_right": right,
            "rationale": rationale,
            "scope": scope,
            "confidence": confidence,
            "times_applied": applied,
            "times_validated": validated,
        })
    }))
}

async fn check_correction_relevance(
    db: &SqlitePool,
    correction_id: &str,
    req: &GetCorrectionsParams,
) -> anyhow::Result<f64> {
    let correction = sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>, String)>(r#"
        SELECT file_patterns, topic_tags, keywords, scope
        FROM corrections WHERE id = $1
    "#)
    .bind(correction_id)
    .fetch_optional(db)
    .await?;

    let (file_patterns, topic_tags, keywords, scope) = match correction {
        Some(c) => c,
        None => return Ok(0.0),
    };

    let mut score = 0.0;

    if let (Some(patterns_json), Some(file_path)) = (&file_patterns, &req.file_path) {
        if let Ok(patterns) = serde_json::from_str::<Vec<String>>(patterns_json) {
            for pattern in patterns {
                if file_matches_pattern(file_path, &pattern) {
                    score += 3.0;
                    break;
                }
            }
        }
    }

    if let (Some(tags_json), Some(topic)) = (&topic_tags, &req.topic) {
        if let Ok(tags) = serde_json::from_str::<Vec<String>>(tags_json) {
            let topic_lower = topic.to_lowercase();
            for tag in tags {
                if topic_lower.contains(&tag.to_lowercase()) || tag.to_lowercase().contains(&topic_lower) {
                    score += 2.0;
                    break;
                }
            }
        }
    }

    if let (Some(keywords_json), Some(context)) = (&keywords, &req.context) {
        if let Ok(kws) = serde_json::from_str::<Vec<String>>(keywords_json) {
            let context_lower = context.to_lowercase();
            for kw in kws {
                if context_lower.contains(&kw.to_lowercase()) {
                    score += 1.0;
                }
            }
        }
    }

    if scope == "global" && score == 0.0 {
        score = 0.5;
    }

    if score == 0.0 && file_patterns.is_none() && topic_tags.is_none() && keywords.is_none() {
        score = 0.3;
    }

    Ok(score)
}

fn file_matches_pattern(file_path: &str, pattern: &str) -> bool {
    if pattern == "*" || pattern == "**" {
        return true;
    }
    if file_path == pattern {
        return true;
    }
    if pattern.starts_with("*.") {
        let ext = &pattern[1..];
        return file_path.ends_with(ext);
    }
    if pattern.ends_with("/*") {
        let prefix = &pattern[..pattern.len() - 1];
        return file_path.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix("**/") {
        if let Some(ext) = suffix.strip_prefix("*") {
            return file_path.ends_with(ext);
        } else {
            return file_path.ends_with(&format!("/{}", suffix)) || file_path == suffix;
        }
    }
    if file_path.contains(pattern) || pattern.contains(file_path) {
        return true;
    }
    false
}

fn normalize_json_array(input: &Option<String>) -> Option<String> {
    input.as_ref().map(|s| {
        if s.trim().starts_with('[') {
            s.clone()
        } else {
            let items: Vec<&str> = s.split(',').map(|x| x.trim()).filter(|x| !x.is_empty()).collect();
            serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
        }
    })
}
