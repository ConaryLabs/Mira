//! Proactive Organization System - Proposal Extraction & Management
//!
//! Implements GPT-5.2's "shadow organizer" approach:
//! 1. Extract goals/tasks/decisions from conversation automatically
//! 2. Store as proposals with confidence scores
//! 3. Auto-commit high confidence, batch-review the rest
//! 4. Dedupe via embeddings to prevent DB trash

use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use crate::core::{CoreError, CoreResult, OpContext};

/// Proposal types that can be extracted
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalType {
    Goal,
    Task,
    Decision,
    Summary,
}

impl std::fmt::Display for ProposalType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProposalType::Goal => write!(f, "goal"),
            ProposalType::Task => write!(f, "task"),
            ProposalType::Decision => write!(f, "decision"),
            ProposalType::Summary => write!(f, "summary"),
        }
    }
}

impl std::str::FromStr for ProposalType {
    type Err = CoreError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "goal" => Ok(ProposalType::Goal),
            "task" => Ok(ProposalType::Task),
            "decision" => Ok(ProposalType::Decision),
            "summary" => Ok(ProposalType::Summary),
            _ => Err(CoreError::InvalidArgument(format!("Unknown proposal type: {}", s))),
        }
    }
}

/// Proposal status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Pending,
    Confirmed,
    Rejected,
    AutoCommitted,
}

impl std::fmt::Display for ProposalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProposalStatus::Pending => write!(f, "pending"),
            ProposalStatus::Confirmed => write!(f, "confirmed"),
            ProposalStatus::Rejected => write!(f, "rejected"),
            ProposalStatus::AutoCommitted => write!(f, "auto_committed"),
        }
    }
}

impl std::str::FromStr for ProposalStatus {
    type Err = CoreError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(ProposalStatus::Pending),
            "confirmed" => Ok(ProposalStatus::Confirmed),
            "rejected" => Ok(ProposalStatus::Rejected),
            "auto_committed" => Ok(ProposalStatus::AutoCommitted),
            _ => Err(CoreError::InvalidArgument(format!("Unknown proposal status: {}", s))),
        }
    }
}

/// A proposal extracted from conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub proposal_type: ProposalType,
    pub content: String,
    pub title: Option<String>,
    pub confidence: f64,
    pub evidence: Option<String>,
    pub status: ProposalStatus,
    pub source_tool: Option<String>,
    pub source_context: Option<String>,
    pub project_path: Option<String>,
    pub created_at: i64,
    pub processed_at: Option<i64>,
    pub promoted_to: Option<String>,
}

/// Extraction pattern from database
#[derive(Debug, Clone)]
pub struct ExtractionPattern {
    pub id: i64,
    pub pattern_type: ProposalType,
    pub pattern: String,
    pub confidence_boost: f64,
    pub description: Option<String>,
    #[allow(dead_code)]
    pub compiled: Option<Regex>,
}

/// Result of running extraction on text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionMatch {
    pub proposal_type: ProposalType,
    pub matched_text: String,
    pub full_context: String,
    pub confidence: f64,
    pub pattern_id: i64,
}

// ============================================================================
// Pattern Loading & Extraction
// ============================================================================

/// Load all enabled extraction patterns from database
pub async fn load_patterns(ctx: &OpContext) -> CoreResult<Vec<ExtractionPattern>> {
    let db = ctx.require_db()?;

    let rows: Vec<(i64, String, String, f64, Option<String>)> = sqlx::query_as(
        r#"
        SELECT id, pattern_type, pattern, confidence_boost, description
        FROM extraction_patterns
        WHERE enabled = TRUE
        ORDER BY confidence_boost DESC
        "#,
    )
    .fetch_all(db)
    .await?;

    let mut patterns = Vec::new();
    for (id, ptype, pattern_str, boost, desc) in rows {
        let proposal_type: ProposalType = ptype.parse().unwrap_or(ProposalType::Task);
        let compiled = Regex::new(&pattern_str).ok();

        patterns.push(ExtractionPattern {
            id,
            pattern_type: proposal_type,
            pattern: pattern_str,
            confidence_boost: boost,
            description: desc,
            compiled,
        });
    }

    Ok(patterns)
}

/// Extract proposals from text using heuristic patterns
pub async fn extract_from_text(
    ctx: &OpContext,
    text: &str,
    base_confidence: f64,
) -> CoreResult<Vec<ExtractionMatch>> {
    let patterns = load_patterns(ctx).await?;
    let mut matches = Vec::new();

    for pattern in patterns {
        if let Some(ref regex) = pattern.compiled {
            for cap in regex.find_iter(text) {
                // Get surrounding context (up to 100 chars each side)
                let start = cap.start().saturating_sub(100);
                let end = (cap.end() + 100).min(text.len());
                let context = &text[start..end];

                let confidence = (base_confidence + pattern.confidence_boost).min(1.0);

                matches.push(ExtractionMatch {
                    proposal_type: pattern.pattern_type,
                    matched_text: cap.as_str().to_string(),
                    full_context: context.to_string(),
                    confidence,
                    pattern_id: pattern.id,
                });
            }
        }
    }

    // Dedupe matches by context (avoid multiple matches for same content)
    let mut seen: HashMap<String, ExtractionMatch> = HashMap::new();
    for m in matches {
        let key = format!("{:?}:{}", m.proposal_type, m.full_context);
        seen.entry(key)
            .and_modify(|existing| {
                if m.confidence > existing.confidence {
                    *existing = m.clone();
                }
            })
            .or_insert(m);
    }

    Ok(seen.into_values().collect())
}

// ============================================================================
// Proposal CRUD
// ============================================================================

/// Create a new proposal
pub async fn create_proposal(
    ctx: &OpContext,
    proposal_type: ProposalType,
    content: &str,
    title: Option<&str>,
    confidence: f64,
    evidence: Option<&str>,
    source_tool: Option<&str>,
    source_context: Option<&str>,
) -> CoreResult<Proposal> {
    let db = ctx.require_db()?;

    let id = format!("prop-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let now = chrono::Utc::now().timestamp();
    let content_hash = hash_content(content);
    let project_path = if ctx.project_path.is_empty() {
        None
    } else {
        Some(ctx.project_path.as_str())
    };

    // Determine initial status based on confidence threshold
    let status = if confidence >= 0.8 {
        ProposalStatus::AutoCommitted
    } else {
        ProposalStatus::Pending
    };

    sqlx::query(
        r#"
        INSERT INTO proposals (
            id, proposal_type, content, title, confidence, evidence, status,
            content_hash, source_tool, source_context, project_path, created_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        "#,
    )
    .bind(&id)
    .bind(proposal_type.to_string())
    .bind(content)
    .bind(title)
    .bind(confidence)
    .bind(evidence)
    .bind(status.to_string())
    .bind(&content_hash)
    .bind(source_tool)
    .bind(source_context)
    .bind(project_path)
    .bind(now)
    .execute(db)
    .await?;

    // Update pattern match count
    if let Some(evidence_json) = evidence {
        if let Ok(evidence_data) = serde_json::from_str::<serde_json::Value>(evidence_json) {
            if let Some(pattern_id) = evidence_data.get("pattern_id").and_then(|v| v.as_i64()) {
                let _ = sqlx::query(
                    "UPDATE extraction_patterns SET times_matched = times_matched + 1 WHERE id = $1",
                )
                .bind(pattern_id)
                .execute(db)
                .await;
            }
        }
    }

    // Store embedding for semantic duplicate detection (async, non-blocking)
    store_proposal_embedding(ctx, &id, content).await;

    Ok(Proposal {
        id,
        proposal_type,
        content: content.to_string(),
        title: title.map(String::from),
        confidence,
        evidence: evidence.map(String::from),
        status,
        source_tool: source_tool.map(String::from),
        source_context: source_context.map(String::from),
        project_path: project_path.map(String::from),
        created_at: now,
        processed_at: None,
        promoted_to: None,
    })
}

/// List proposals with optional filters
pub async fn list_proposals(
    ctx: &OpContext,
    status: Option<ProposalStatus>,
    proposal_type: Option<ProposalType>,
    limit: i64,
) -> CoreResult<Vec<Proposal>> {
    let db = ctx.require_db()?;

    let mut sql = String::from(
        r#"
        SELECT id, proposal_type, content, title, confidence, evidence, status,
               source_tool, source_context, project_path, created_at, processed_at, promoted_to
        FROM proposals
        WHERE 1=1
        "#,
    );

    if status.is_some() {
        sql.push_str(" AND status = $1");
    }
    if proposal_type.is_some() {
        sql.push_str(if status.is_some() {
            " AND proposal_type = $2"
        } else {
            " AND proposal_type = $1"
        });
    }
    sql.push_str(" ORDER BY created_at DESC LIMIT ");
    sql.push_str(&limit.to_string());

    // This is ugly but sqlx doesn't have great dynamic query support
    let rows: Vec<(
        String,
        String,
        String,
        Option<String>,
        f64,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        Option<i64>,
        Option<String>,
    )> = match (status, proposal_type) {
        (Some(s), Some(t)) => {
            sqlx::query_as(&sql)
                .bind(s.to_string())
                .bind(t.to_string())
                .fetch_all(db)
                .await?
        }
        (Some(s), None) => sqlx::query_as(&sql).bind(s.to_string()).fetch_all(db).await?,
        (None, Some(t)) => sqlx::query_as(&sql).bind(t.to_string()).fetch_all(db).await?,
        (None, None) => sqlx::query_as(&sql).fetch_all(db).await?,
    };

    Ok(rows
        .into_iter()
        .map(|r| Proposal {
            id: r.0,
            proposal_type: r.1.parse().unwrap_or(ProposalType::Task),
            content: r.2,
            title: r.3,
            confidence: r.4,
            evidence: r.5,
            status: match r.6.as_str() {
                "confirmed" => ProposalStatus::Confirmed,
                "rejected" => ProposalStatus::Rejected,
                "auto_committed" => ProposalStatus::AutoCommitted,
                _ => ProposalStatus::Pending,
            },
            source_tool: r.7,
            source_context: r.8,
            project_path: r.9,
            created_at: r.10,
            processed_at: r.11,
            promoted_to: r.12,
        })
        .collect())
}

/// Confirm a proposal and promote it to actual goal/task/decision
pub async fn confirm_proposal(ctx: &OpContext, proposal_id: &str) -> CoreResult<Option<String>> {
    let db = ctx.require_db()?;

    // Get the proposal
    let proposal: Option<(String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT proposal_type, content, title, status FROM proposals WHERE id = $1",
    )
    .bind(proposal_id)
    .fetch_optional(db)
    .await?;

    let (ptype, content, title, status): (String, String, Option<String>, String) = match proposal {
        Some(p) => p,
        None => return Ok(None),
    };

    if status != "pending" {
        return Ok(Some(format!("Proposal already processed: {}", status)));
    }

    let now = chrono::Utc::now().timestamp();
    let promoted_id: Option<String>;

    // Create the actual item based on type
    match ptype.as_str() {
        "task" => {
            let task_title = title.unwrap_or_else(|| truncate(&content, 100));
            let input = super::tasks::CreateTaskInput {
                title: task_title,
                description: Some(content.clone()),
                priority: None,
                parent_id: None,
            };
            let output = super::tasks::create_task(ctx, input).await?;
            promoted_id = Some(output.task_id);
        }
        "goal" => {
            let goal_title = title.unwrap_or_else(|| truncate(&content, 100));
            let input = super::goals::CreateGoalInput {
                title: goal_title,
                description: Some(content.clone()),
                success_criteria: None,
                priority: None,
                project_id: None,
            };
            let output = super::goals::create_goal(ctx, input).await?;
            promoted_id = Some(output.goal_id);
        }
        "decision" => {
            let key = format!("decision-{}", &uuid::Uuid::new_v4().to_string()[..8]);
            let input = super::decisions::StoreDecisionInput {
                key: key.clone(),
                decision: content.clone(),
                category: None,
                context: None,
                project_id: None,
            };
            super::decisions::store_decision(ctx, input).await?;
            promoted_id = Some(key);
        }
        _ => {
            promoted_id = None;
        }
    }

    // Update proposal status
    sqlx::query(
        "UPDATE proposals SET status = 'confirmed', processed_at = $1, promoted_to = $2 WHERE id = $3",
    )
    .bind(now)
    .bind(&promoted_id)
    .bind(proposal_id)
    .execute(db)
    .await?;

    // Update pattern confirmation count
    let evidence: Option<(Option<String>,)> =
        sqlx::query_as("SELECT evidence FROM proposals WHERE id = $1")
            .bind(proposal_id)
            .fetch_optional(db)
            .await?;

    if let Some((Some(evidence_json),)) = evidence {
        if let Ok(evidence_data) = serde_json::from_str::<serde_json::Value>(&evidence_json) {
            if let Some(pattern_id) = evidence_data.get("pattern_id").and_then(|v| v.as_i64()) {
                let _ = sqlx::query(
                    "UPDATE extraction_patterns SET times_confirmed = times_confirmed + 1 WHERE id = $1",
                )
                .bind(pattern_id)
                .execute(db)
                .await;
            }
        }
    }

    Ok(Some(format!(
        "Confirmed {} → {}",
        proposal_id,
        promoted_id.unwrap_or_default()
    )))
}

/// Reject a proposal
pub async fn reject_proposal(ctx: &OpContext, proposal_id: &str) -> CoreResult<Option<String>> {
    let db = ctx.require_db()?;

    let now = chrono::Utc::now().timestamp();

    let result = sqlx::query("UPDATE proposals SET status = 'rejected', processed_at = $1 WHERE id = $2 AND status = 'pending'")
        .bind(now)
        .bind(proposal_id)
        .execute(db)
        .await?;

    if result.rows_affected() > 0 {
        Ok(Some(format!("Rejected proposal: {}", proposal_id)))
    } else {
        Ok(None)
    }
}

/// Get pending proposals for review (for session_start lazy confirmation)
pub async fn get_pending_review(ctx: &OpContext, limit: i64) -> CoreResult<Vec<Proposal>> {
    list_proposals(ctx, Some(ProposalStatus::Pending), None, limit).await
}

/// Similarity threshold for semantic duplicate detection (0.0 - 1.0)
/// 0.85 is fairly strict - catches "Add REST API" vs "Implement REST interface"
/// but not "Add REST API" vs "Add GraphQL API"
const SEMANTIC_DUPLICATE_THRESHOLD: f32 = 0.85;

/// Check for duplicate proposal using semantic similarity (preferred) or content hash (fallback)
pub async fn find_duplicate(ctx: &OpContext, content: &str) -> CoreResult<Option<String>> {
    let db = ctx.require_db()?;

    // Try semantic similarity first if available
    if let Some(semantic) = ctx.semantic.as_ref() {
        if semantic.is_available() {
            // Search for similar proposals in the conversation collection
            // (proposals are stored there with type="proposal")
            use qdrant_client::qdrant::{Condition, Filter};

            let filter = Filter::must([Condition::matches("type", "proposal".to_string())]);

            match semantic.search(
                crate::core::primitives::semantic::COLLECTION_CONVERSATION,
                content,
                5,
                Some(filter),
            ).await {
                Ok(results) => {
                    // Check if any result exceeds our similarity threshold
                    for result in results {
                        if result.score >= SEMANTIC_DUPLICATE_THRESHOLD {
                            if let Some(id) = result.metadata.get("proposal_id") {
                                if let Some(id_str) = id.as_str() {
                                    tracing::debug!(
                                        "Found semantic duplicate: {} (score: {:.2})",
                                        id_str, result.score
                                    );
                                    return Ok(Some(id_str.to_string()));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    // Log but don't fail - fall through to hash-based
                    tracing::warn!("Semantic duplicate search failed: {}, falling back to hash", e);
                }
            }
        }
    }

    // Fallback to hash-based dedup (exact match)
    let hash = hash_content(content);
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM proposals WHERE content_hash = $1 AND status IN ('pending', 'confirmed', 'auto_committed')",
    )
    .bind(&hash)
    .fetch_optional(db)
    .await?;

    Ok(existing.map(|(id,)| id))
}

/// Store proposal embedding in Qdrant for future similarity searches
async fn store_proposal_embedding(ctx: &OpContext, proposal_id: &str, content: &str) {
    if let Some(semantic) = ctx.semantic.as_ref() {
        if semantic.is_available() {
            use std::collections::HashMap;

            let mut metadata = HashMap::new();
            metadata.insert("type".to_string(), serde_json::Value::String("proposal".to_string()));
            metadata.insert("proposal_id".to_string(), serde_json::Value::String(proposal_id.to_string()));

            if let Err(e) = semantic.store(
                crate::core::primitives::semantic::COLLECTION_CONVERSATION,
                &format!("proposal-{}", proposal_id),
                content,
                metadata,
            ).await {
                tracing::warn!("Failed to store proposal embedding: {}", e);
            }
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.to_lowercase().as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string()
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_content() {
        let hash1 = hash_content("hello world");
        let hash2 = hash_content("HELLO WORLD");
        assert_eq!(hash1, hash2); // Case insensitive

        let hash3 = hash_content("different content");
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("this is a longer string", 10), "this is...");
    }
}
