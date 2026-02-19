// crates/mira-server/src/tools/core/memory/recall.rs
//! Memory recall with semantic search, fuzzy fallback, and keyword fallback.

use crate::error::MiraError;
use crate::mcp::responses::Json;
use crate::mcp::responses::{MemoryData, MemoryItem, MemoryOutput, RecallData};
use crate::search::embedding_to_bytes;
use crate::tools::core::{ToolContext, get_project_info};
use crate::utils::{prepare_recall_query, truncate};
use mira_types::MemoryFact;

/// Strong distance threshold for semantic recall -- only high-confidence matches.
const STRONG_THRESHOLD: f32 = 0.7;

/// Weak distance threshold for semantic recall -- used when no strong matches exist.
/// Stricter threshold (0.80) is used in hooks (see hooks/recall.rs).
const WEAK_THRESHOLD: f32 = 0.85;

/// Common interface for recall result types (MemoryFact, FuzzyMemoryResult)
pub(super) trait RecallResult {
    fn id(&self) -> i64;
    fn content(&self) -> &str;
    fn fact_type(&self) -> &str;
    fn category(&self) -> Option<&str>;
    fn score(&self) -> Option<f32> {
        None
    }
}

impl RecallResult for MemoryFact {
    fn id(&self) -> i64 {
        self.id
    }
    fn content(&self) -> &str {
        &self.content
    }
    fn fact_type(&self) -> &str {
        &self.fact_type
    }
    fn category(&self) -> Option<&str> {
        self.category.as_deref()
    }
}

impl RecallResult for crate::fuzzy::FuzzyMemoryResult {
    fn id(&self) -> i64 {
        self.id
    }
    fn content(&self) -> &str {
        &self.content
    }
    fn fact_type(&self) -> &str {
        &self.fact_type
    }
    fn category(&self) -> Option<&str> {
        self.category.as_deref()
    }
    fn score(&self) -> Option<f32> {
        Some(self.score)
    }
}

/// Filter recall results by category and fact_type, applying limit
fn filter_results<T: RecallResult>(
    results: Vec<T>,
    category: &Option<String>,
    fact_type: &Option<String>,
    limit: usize,
) -> Vec<T> {
    if category.is_none() && fact_type.is_none() {
        return results.into_iter().take(limit).collect();
    }
    results
        .into_iter()
        .filter(|m| {
            let ft_ok = fact_type
                .as_ref()
                .is_none_or(|f| f.as_str() == m.fact_type());
            let cat_ok = category
                .as_ref()
                .is_none_or(|c| m.category() == Some(c.as_str()));
            ft_ok && cat_ok
        })
        .take(limit)
        .collect()
}

/// Fire-and-forget recording of memory access for evidence-based tracking.
/// Spawns a background task that records each memory ID as accessed in the given session.
fn spawn_record_access(
    pool: std::sync::Arc<crate::db::pool::DatabasePool>,
    ids: Vec<i64>,
    session_id: String,
) {
    use crate::db::record_memory_access_sync;
    tokio::spawn(async move {
        pool.try_interact("record memory access", move |conn| {
            for id in ids {
                if let Err(e) = record_memory_access_sync(conn, id, &session_id) {
                    tracing::warn!("Failed to record memory access: {}", e);
                }
            }
            Ok(())
        })
        .await;
    });
}

/// Record access, format response, and build MemoryOutput from any RecallResult type.
///
/// `override_scores` allows callers to supply synthetic scores (e.g. for keyword results
/// that have no inherent relevance score). When provided, these take precedence over
/// `RecallResult::score()`.
fn build_recall_output<T: RecallResult>(
    results: &[T],
    context_header: &str,
    label: &str,
    session_id: &Option<String>,
    pool: &std::sync::Arc<crate::db::pool::DatabasePool>,
    override_scores: Option<&[f32]>,
) -> Json<MemoryOutput> {
    // Record access
    if let Some(sid) = session_id {
        let ids: Vec<i64> = results.iter().map(|m| m.id()).collect();
        spawn_record_access(pool.clone(), ids, sid.clone());
    }

    let items: Vec<MemoryItem> = results
        .iter()
        .enumerate()
        .map(|(i, mem)| {
            let score = override_scores
                .and_then(|s| s.get(i).copied())
                .or_else(|| mem.score());
            MemoryItem {
                id: mem.id(),
                content: mem.content().to_string(),
                score,
                fact_type: Some(mem.fact_type().to_string()),
                branch: None,
            }
        })
        .collect();
    let total = items.len();
    let mut response = format!("{}Found {} memories{}:\n", context_header, total, label);
    for (i, mem) in results.iter().enumerate() {
        let score = override_scores
            .and_then(|s| s.get(i).copied())
            .or_else(|| mem.score());
        let preview = truncate(mem.content(), 100);
        if let Some(s) = score {
            response.push_str(&format!(
                "  [{}] (score: {:.2}) ({}) {}\n",
                mem.id(),
                s,
                mem.fact_type(),
                preview
            ));
        } else {
            response.push_str(&format!(
                "  [{}] ({}) {}\n",
                mem.id(),
                mem.fact_type(),
                preview
            ));
        }
    }

    Json(MemoryOutput {
        action: "recall".into(),
        message: response,
        data: Some(MemoryData::Recall(RecallData {
            memories: items,
            total,
        })),
    })
}

/// Search memories using semantic similarity or keyword fallback
pub async fn recall<C: ToolContext>(
    ctx: &C,
    query: String,
    limit: Option<i64>,
    category: Option<String>,
    fact_type: Option<String>,
) -> Result<Json<MemoryOutput>, MiraError> {
    use crate::db::search_memories_sync;

    // Empty query guard: reject queries that are too short to be meaningful
    if query.trim().len() < 2 {
        return Ok(Json(MemoryOutput {
            action: "recall".into(),
            message: "Query too short for recall (minimum 2 characters).".into(),
            data: Some(MemoryData::Recall(RecallData {
                memories: vec![],
                total: 0,
            })),
        }));
    }

    let pi = get_project_info(ctx).await;
    let project_id = pi.id;
    let session_id = ctx.get_session_id().await;
    let user_id = ctx.get_user_identity();
    let current_branch = ctx.get_branch().await;
    let context_header = pi.header;
    let has_filters = category.is_some() || fact_type.is_some();

    // Get team_id if in a team (for team-scoped memory visibility)
    let team_id: Option<i64> = ctx.get_team_membership().map(|m| m.team_id);

    // Over-fetch when filters are set since some results will be filtered out
    let limit = (limit.unwrap_or(10).clamp(1, 100)) as usize;
    let fetch_limit = if has_filters { limit * 3 } else { limit };

    // Extract entities from query for entity-based recall boost
    let query_entity_names: Vec<String> = {
        use crate::entities::extract_entities_heuristic;
        extract_entities_heuristic(&query)
            .into_iter()
            .map(|e| e.canonical_name)
            .collect()
    };

    // Expand short queries for better embedding quality
    let expanded_query = prepare_recall_query(&query);

    // Try semantic search first if embeddings available (with branch-aware + entity boosting)
    if let Some(embeddings) = ctx.embeddings()
        && let Ok(query_embedding) = embeddings.embed(&expanded_query).await
    {
        let embedding_bytes = embedding_to_bytes(&query_embedding);
        let user_id_for_query = user_id.clone();
        let branch_for_query = current_branch.clone();
        let entity_names_for_query = query_entity_names.clone();

        // Run vector search via connection pool with branch + entity + team boosting
        // Graceful degradation: if vector search fails, fall through to fuzzy/SQL
        let vec_result: Result<Vec<crate::db::RecallRow>, _> = ctx
            .pool()
            .run(move |conn| {
                crate::db::recall_semantic_with_entity_boost_sync(
                    conn,
                    &embedding_bytes,
                    project_id,
                    user_id_for_query.as_deref(),
                    team_id,
                    branch_for_query.as_deref(),
                    &entity_names_for_query,
                    fetch_limit,
                )
            })
            .await;

        let results = match vec_result {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Semantic recall failed, falling back to fuzzy/SQL: {}", e);
                vec![]
            }
        };

        // Adaptive two-tier threshold: prefer strong matches, fall back to weak
        // matches rather than dropping to keyword search immediately.
        // (Hook thresholds in hooks/recall.rs are stricter: weak = 0.80)
        let strong: Vec<_> = results
            .iter()
            .filter(|r| r.distance < STRONG_THRESHOLD)
            .cloned()
            .collect();
        let results = if !strong.is_empty() {
            strong
        } else {
            // No strong matches -- use weak matches rather than falling to keyword
            results
                .into_iter()
                .filter(|r| r.distance < WEAK_THRESHOLD)
                .collect()
        };

        if !results.is_empty() {
            // Apply category/fact_type filters if requested (using inline metadata)
            let results = if has_filters {
                let cat = category.clone();
                let ft = fact_type.clone();
                results
                    .into_iter()
                    .filter(|r| {
                        let ft_ok = ft.as_ref().is_none_or(|f| f == &r.fact_type);
                        let cat_ok = cat.as_ref().is_none_or(|c| r.category.as_ref() == Some(c));
                        ft_ok && cat_ok
                    })
                    .take(limit)
                    .collect::<Vec<_>>()
            } else {
                results
            };

            if !results.is_empty() {
                // Record memory access for evidence-based tracking
                if let Some(ref sid) = session_id {
                    let ids: Vec<i64> = results.iter().map(|r| r.id).collect();
                    spawn_record_access(ctx.pool().clone(), ids, sid.clone());
                }

                let items: Vec<MemoryItem> = results
                    .iter()
                    .map(|r| MemoryItem {
                        id: r.id,
                        content: r.content.clone(),
                        score: Some(1.0 - r.distance),
                        fact_type: Some(r.fact_type.clone()),
                        branch: r.branch.clone(),
                    })
                    .collect();
                let total = items.len();
                let mut response = format!(
                    "{}Found {} memories (semantic search):\n",
                    context_header, total
                );
                for r in &results {
                    let score = 1.0 - r.distance;
                    let preview = truncate(&r.content, 100);
                    let branch_tag = r
                        .branch
                        .as_ref()
                        .map(|b| format!(" [{}]", b))
                        .unwrap_or_default();
                    response.push_str(&format!(
                        "  [{}] (score: {:.2}){} {}\n",
                        r.id, score, branch_tag, preview
                    ));
                }
                return Ok(Json(MemoryOutput {
                    action: "recall".into(),
                    message: response,
                    data: Some(MemoryData::Recall(RecallData {
                        memories: items,
                        total,
                    })),
                }));
            }
        }
    }

    // Fall back to fuzzy search if enabled
    if let Some(cache) = ctx.fuzzy_cache()
        && let Ok(mut results) = cache
            .search_memories(
                ctx.pool(),
                project_id,
                user_id.as_deref(),
                team_id,
                &query,
                fetch_limit,
            )
            .await
        && !results.is_empty()
    {
        // Apply entity boost to fuzzy results
        if !query_entity_names.is_empty() {
            let fact_ids: Vec<i64> = results.iter().map(|r| r.id).collect();
            if !fact_ids.is_empty() {
                let entity_names_for_fuzzy = query_entity_names.clone();
                if let Ok(entity_counts) = ctx
                    .pool()
                    .run(move |conn| {
                        crate::db::entities::get_entity_match_counts_sync(
                            conn,
                            project_id,
                            &entity_names_for_fuzzy,
                        )
                    })
                    .await
                {
                    for r in &mut results {
                        let count = entity_counts.get(&r.id).copied().unwrap_or(0);
                        if count > 0 {
                            r.score *= (1.0 + 0.15 * count as f32).min(1.27);
                        }
                    }
                    results.sort_by(|a, b| {
                        b.score
                            .partial_cmp(&a.score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
            }
        }
        let results = filter_results(results, &category, &fact_type, limit);
        if !results.is_empty() {
            return Ok(build_recall_output(
                &results,
                &context_header,
                " (fuzzy)",
                &session_id,
                ctx.pool(),
                None,
            ));
        }
    }

    // Fall back to SQL search via connection pool
    let query_clone = query.clone();
    let user_id_clone = user_id.clone();
    let mut results: Vec<MemoryFact> = ctx
        .pool()
        .run(move |conn| {
            search_memories_sync(
                conn,
                project_id,
                &query_clone,
                user_id_clone.as_deref(),
                team_id,
                fetch_limit,
            )
        })
        .await?;

    // Apply entity boost to keyword results: reorder by entity match count,
    // preserving original order for equal counts (stable sort)
    if !query_entity_names.is_empty() && !results.is_empty() {
        let entity_names_for_kw = query_entity_names.clone();
        if let Ok(entity_counts) = ctx
            .pool()
            .run(move |conn| {
                crate::db::entities::get_entity_match_counts_sync(
                    conn,
                    project_id,
                    &entity_names_for_kw,
                )
            })
            .await
        {
            results.sort_by(|a, b| {
                let ca = entity_counts.get(&a.id).copied().unwrap_or(0);
                let cb = entity_counts.get(&b.id).copied().unwrap_or(0);
                cb.cmp(&ca)
            });
        }
    }

    let results = filter_results(results, &category, &fact_type, limit);

    if results.is_empty() {
        return Ok(Json(MemoryOutput {
            action: "recall".into(),
            message: format!("{}No memories found.", context_header),
            data: Some(MemoryData::Recall(RecallData {
                memories: vec![],
                total: 0,
            })),
        }));
    }

    // Synthetic position-based scores for keyword results
    let keyword_scores: Vec<f32> = (0..results.len())
        .map(|i| 0.8_f32 - (i as f32 * 0.08).min(0.5))
        .collect();

    Ok(build_recall_output(
        &results,
        &context_header,
        " (keyword fallback)",
        &session_id,
        ctx.pool(),
        Some(&keyword_scores),
    ))
}
