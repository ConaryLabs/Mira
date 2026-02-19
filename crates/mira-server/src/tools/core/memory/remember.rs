// crates/mira-server/src/tools/core/memory/remember.rs
//! Memory storage (remember action) with rate limiting and security validation.

use crate::db::{StoreMemoryParams, store_memory_sync};
use crate::error::MiraError;
use crate::mcp::responses::Json;
use crate::mcp::responses::{MemoryData, MemoryOutput, RememberData};
use crate::tools::core::ToolContext;

use super::security::{detect_injection, detect_secret};

/// Maximum number of new memories allowed per session (rate limiting).
/// Prevents flooding attacks where a compromised or misbehaving agent
/// stores excessive memories to pollute the knowledge base.
const MAX_MEMORIES_PER_SESSION: i64 = 50;

/// Store a memory fact
pub async fn remember<C: ToolContext>(
    ctx: &C,
    content: String,
    key: Option<String>,
    fact_type: Option<String>,
    category: Option<String>,
    confidence: Option<f64>,
    scope: Option<String>,
) -> Result<Json<MemoryOutput>, MiraError> {
    // Input validation: reject oversized content (10KB limit)
    const MAX_MEMORY_BYTES: usize = 10 * 1024;
    if content.len() > MAX_MEMORY_BYTES {
        return Err(MiraError::InvalidInput(format!(
            "Memory content too large ({} bytes). Maximum allowed is {} bytes (10KB).",
            content.len(),
            MAX_MEMORY_BYTES
        )));
    }

    if content.trim().is_empty() {
        return Err(MiraError::InvalidInput(
            "Memory content cannot be empty or whitespace-only.".to_string(),
        ));
    }

    // Resolve identity fields early — needed by both rate-limit bypass and the insert.
    let project_id = ctx.project_id().await;
    let session_id = ctx.get_session_id().await;
    let user_id = ctx.get_user_identity();

    let fact_type = fact_type.unwrap_or_else(|| "general".to_string());
    let confidence = confidence.unwrap_or(0.8); // User explicitly asked to remember = high confidence
    // Default scope is "project" for backward compatibility
    let scope = scope.unwrap_or_else(|| "project".to_string());

    // Validate scope
    if !["personal", "project", "team"].contains(&scope.as_str()) {
        return Err(MiraError::InvalidInput(format!(
            "Invalid scope '{}'. Must be one of: personal, project, team",
            scope
        )));
    }

    // Personal scope requires user identity for access control
    if scope == "personal" && user_id.is_none() {
        return Err(MiraError::InvalidInput(
            "Cannot create personal memory: user identity not available".to_string(),
        ));
    }

    // Team scope: strict enforcement — must be in an active team
    let team_id: Option<i64> = if scope == "team" {
        let membership = ctx.get_team_membership();
        match membership {
            Some(m) => Some(m.team_id),
            None => {
                return Err(MiraError::InvalidInput(
                    "Cannot use scope='team': not in an active team. Use scope='project' instead."
                        .to_string(),
                ));
            }
        }
    } else {
        None
    };

    // Security: warn if content looks like it contains secrets
    if let Some(pattern_name) = detect_secret(&content) {
        return Err(MiraError::InvalidInput(format!(
            "Content appears to contain a secret ({pattern_name}). \
             Secrets should be stored in ~/.mira/.env, not in memories. \
             If this is a false positive, rephrase the content to avoid secret-like patterns."
        )));
    }

    // Security: detect prompt injection patterns. Store but flag as suspicious.
    let suspicious = if let Some(pattern_name) = detect_injection(&content) {
        tracing::warn!(
            "Memory flagged as suspicious (matched '{}'): {}",
            pattern_name,
            crate::utils::truncate(&content, 80)
        );
        true
    } else {
        false
    };

    // Get current branch for branch-aware memory
    let branch = ctx.get_branch().await;

    // Clone only what's needed after the closure; move the rest directly
    let content_for_later = content.clone(); // needed for entity extraction
    let key_for_later = key.clone(); // needed for response message
    // Atomic rate-limit check + insert inside a BEGIN IMMEDIATE transaction.
    // IMMEDIATE acquires a write lock before the count check, so concurrent
    // requests on other pooled connections block until this transaction commits.
    // This closes the TOCTOU gap: two requests cannot both see count < 50.
    let id: i64 = ctx
        .pool()
        .run_with_retry(move |conn| -> Result<i64, String> {
            let tx = rusqlite::Transaction::new_unchecked(
                conn,
                rusqlite::TransactionBehavior::Immediate,
            )
            .map_err(|e| e.to_string())?;

            // Rate limiting: max 50 new memories per session.
            // Skip for key-based upserts that update an existing row.
            // Fail-closed: DB errors are treated as limit reached.
            if let Some(ref sid) = session_id {
                // Bypass check uses the full upsert identity from store_memory_sync:
                // (key, project_id, scope, team_id, user_id).
                let is_keyed_update = key.as_ref().is_some_and(|k| {
                    tx.query_row(
                        "SELECT COUNT(*) FROM memory_facts
                         WHERE key = ?1 AND project_id IS ?2
                           AND COALESCE(scope, 'project') = ?3
                           AND COALESCE(team_id, 0) = COALESCE(?4, 0)
                           AND (?3 != 'personal' OR COALESCE(user_id, '') = COALESCE(?5, ''))",
                        rusqlite::params![k, project_id, &scope, team_id, user_id.as_deref()],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap_or(0)
                        > 0
                });

                if !is_keyed_update {
                    let count: i64 = tx
                        .query_row(
                            "SELECT COUNT(*) FROM memory_facts WHERE first_session_id = ?1",
                            [sid],
                            |row| row.get(0),
                        )
                        .unwrap_or(MAX_MEMORIES_PER_SESSION); // fail-closed
                    if count >= MAX_MEMORIES_PER_SESSION {
                        // tx drops here -> auto-rollback, releases write lock
                        return Err(format!(
                            "Rate limit exceeded: {} memories already created this session (max {}).",
                            count, MAX_MEMORIES_PER_SESSION
                        ));
                    }
                }
            }

            let params = StoreMemoryParams {
                project_id,
                key: key.as_deref(),
                content: &content,
                fact_type: &fact_type,
                category: category.as_deref(),
                confidence,
                session_id: session_id.as_deref(),
                user_id: user_id.as_deref(),
                scope: &scope,
                branch: branch.as_deref(),
                team_id,
                suspicious,
            };
            let id = store_memory_sync(&tx, params).map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(id)
        })
        .await?;

    // Extract and link entities in a separate transaction
    // If this fails, the fact is still stored — backfill will pick it up later
    {
        use crate::db::entities::{
            link_entity_to_fact_sync, mark_fact_has_entities_sync, upsert_entity_sync,
        };
        use crate::entities::extract_entities_heuristic;

        let entities = extract_entities_heuristic(&content_for_later);
        let pool_for_entities = ctx.pool().clone();
        if let Err(e) = pool_for_entities
            .run(move |conn| {
                let tx = conn.unchecked_transaction()?;
                for entity in &entities {
                    let entity_id = upsert_entity_sync(
                        &tx,
                        project_id,
                        &entity.canonical_name,
                        entity.entity_type.as_str(),
                        &entity.name,
                    )?;
                    link_entity_to_fact_sync(&tx, id, entity_id)?;
                }
                // Always mark as processed, even if zero entities found
                mark_fact_has_entities_sync(&tx, id)?;
                tx.commit()?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
        {
            tracing::warn!("Entity extraction failed for fact {}: {}", id, e);
        }
    }

    if let Some(cache) = ctx.fuzzy_cache() {
        cache.invalidate_memory(project_id).await;
    }

    let suspicious_note = if suspicious {
        " [WARNING: flagged as suspicious -- excluded from auto-injection and exports]"
    } else {
        ""
    };

    Ok(Json(MemoryOutput {
        action: "remember".into(),
        message: format!(
            "Stored memory (id: {}){}{}",
            id,
            if key_for_later.is_some() {
                " with key"
            } else {
                ""
            },
            suspicious_note,
        ),
        data: Some(MemoryData::Remember(RememberData { id })),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_constant_is_reasonable() {
        const { assert!(MAX_MEMORIES_PER_SESSION > 0) };
        const { assert!(MAX_MEMORIES_PER_SESSION <= 100) };
    }
}
