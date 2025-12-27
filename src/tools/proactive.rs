// src/tools/proactive.rs
// Proactive context delivery - One unified tool that returns all relevant context

use sqlx::sqlite::SqlitePool;

use super::types::{GetProactiveContextRequest, FindSimilarFixesRequest, GetRelatedFilesRequest};
use crate::core::SemanticSearch;
use std::sync::Arc;
use super::corrections::{self, GetCorrectionsParams};
use super::goals;
use super::git_intel;
use super::code_intel;
use super::mcp_history;
use crate::core::ops::mcp_session::SessionPhase;
use crate::orchestrator::TaskType;

/// Phase-aware limits for different context categories
#[derive(Debug)]
struct PhaseLimits {
    goals: i64,
    decisions: i64,
    errors: i64,
    memories: i64,
    code: i64,
}

impl PhaseLimits {
    fn for_phase(phase: SessionPhase, base_limit: i64) -> Self {
        match phase {
            SessionPhase::Early => Self {
                goals: base_limit + 2,       // More goals in early phase
                decisions: base_limit + 1,   // Understand past decisions
                errors: 1.max(base_limit - 2), // Fewer errors early
                memories: base_limit,
                code: base_limit + 1,        // Understand structure
            },
            SessionPhase::Middle => Self {
                goals: base_limit,           // Track progress
                decisions: 1.max(base_limit - 1),
                errors: base_limit + 1,      // Fix as we go
                memories: 1.max(base_limit - 1),
                code: base_limit + 2,        // Focus on implementation
            },
            SessionPhase::Late => Self {
                goals: base_limit,           // Almost done?
                decisions: 1.max(base_limit - 1),
                errors: base_limit + 2,      // Fix remaining issues
                memories: 1.max(base_limit - 1),
                code: base_limit + 1,        // Refinement
            },
            SessionPhase::Wrapping => Self {
                goals: base_limit + 1,       // Completion status
                decisions: base_limit + 1,   // Record for next session
                errors: 1.max(base_limit - 1),
                memories: base_limit,        // Save learnings
                code: 1.max(base_limit - 1),
            },
        }
    }
}

/// Get all relevant context for the current work, combined into one response
pub async fn get_proactive_context(
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    req: GetProactiveContextRequest,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let base_limit = req.limit_per_category.unwrap_or(3) as i64;

    // Parse session phase and get phase-aware limits
    let phase = req.session_phase
        .as_ref()
        .and_then(|s| SessionPhase::from_str(s))
        .unwrap_or(SessionPhase::Middle);

    let limits = PhaseLimits::for_phase(phase, base_limit);
    let limit = base_limit; // Keep for backwards compat

    // Build context signals for relevance matching
    let file_context = req.files.as_ref().map(|f| f.join(", "));
    let topic_context = req.topics.as_ref().map(|t| t.join(", "));
    let combined_context = vec![
        file_context.as_deref(),
        topic_context.as_deref(),
        req.task.as_deref(),
        req.error.as_deref(),
    ].into_iter().flatten().collect::<Vec<_>>().join(" ");

    // 1. Get relevant corrections (always important, use base limit)
    let corrections = corrections::get_corrections(
        db,
        semantic,
        GetCorrectionsParams {
            file_path: req.files.as_ref().and_then(|f| f.first()).cloned(),
            topic: req.topics.as_ref().and_then(|t| t.first()).cloned(),
            context: if combined_context.is_empty() { None } else { Some(combined_context.clone()) },
            correction_type: None,
            limit: Some(limit),
        },
        project_id,
    ).await.unwrap_or_default();

    // 2. Get active goals (phase-aware: more in early/wrapping)
    let goals_result = goals::get_goal_progress(
        db,
        None, // goal_id
        project_id,
    ).await.unwrap_or_else(|_| serde_json::json!({"active_goals": []}));

    let active_goals = goals_result.get("active_goals")
        .and_then(|g| g.as_array())
        .map(|arr| arr.iter().take(limits.goals as usize).cloned().collect::<Vec<_>>())
        .unwrap_or_default();

    // 3. Get rejected approaches (if files or topics specified)
    let rejected_approaches = get_rejected_approaches(db, &req, project_id, limit).await.unwrap_or_default();

    // 4. Get related decisions from memory (phase-aware: more in early/wrapping)
    let related_decisions = get_related_decisions(db, semantic, &combined_context, project_id, limits.decisions).await.unwrap_or_default();

    // 5. Get relevant memories (preferences, context) (phase-aware)
    let relevant_memories = get_relevant_memories(db, semantic, &combined_context, project_id, limits.memories).await.unwrap_or_default();

    // 6. Get similar errors if error signal provided (phase-aware: more in middle/late)
    let similar_errors = if let Some(error) = &req.error {
        git_intel::find_similar_fixes(
            db,
            semantic,
            FindSimilarFixesRequest {
                error: error.clone(),
                category: None,
                language: None,
                limit: Some(limits.errors),
            },
        ).await.unwrap_or_default()
    } else {
        Vec::new()
    };

    // 7. Get code intelligence if files specified (phase-aware: more in middle)
    let code_context = if let Some(files) = &req.files {
        get_code_context(db, files, limits.code).await.unwrap_or_default()
    } else {
        serde_json::json!({})
    };

    // Check if code context has data
    let has_code_context = code_context.get("related_files")
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);

    let improvement_count = code_context.get("improvement_suggestions")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    // 8. Get call graph context for key symbols
    let call_graph = if let Some(files) = &req.files {
        get_call_graph_context(db, files, limit).await.unwrap_or_default()
    } else {
        Vec::new()
    };

    // 9. Get index freshness status
    let index_status = get_index_freshness(db, project_id).await.unwrap_or_default();

    // 10. Get relevant MCP history (semantic search if query available)
    let relevant_mcp_history = if !combined_context.is_empty() {
        mcp_history::semantic_search(db, semantic, &combined_context, project_id, limit as usize)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|h| serde_json::json!({
                "tool": h.tool_name,
                "summary": h.result_summary.unwrap_or_default(),
                "when": h.created_at,
            }))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    // Build summary
    let stale_count = index_status.get("stale_files")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let summary_parts: Vec<String> = vec![
        if !corrections.is_empty() { Some(format!("{} corrections", corrections.len())) } else { None },
        if !active_goals.is_empty() { Some(format!("{} active goals", active_goals.len())) } else { None },
        if !rejected_approaches.is_empty() { Some(format!("{} rejected approaches", rejected_approaches.len())) } else { None },
        if !related_decisions.is_empty() { Some(format!("{} related decisions", related_decisions.len())) } else { None },
        if !relevant_memories.is_empty() { Some(format!("{} relevant memories", relevant_memories.len())) } else { None },
        if !similar_errors.is_empty() { Some(format!("{} similar fixes", similar_errors.len())) } else { None },
        if !relevant_mcp_history.is_empty() { Some(format!("{} related tool calls", relevant_mcp_history.len())) } else { None },
        if has_code_context { Some("code context".to_string()) } else { None },
        if improvement_count > 0 { Some(format!("{} code improvements", improvement_count)) } else { None },
        if !call_graph.is_empty() { Some(format!("{} call relationships", call_graph.len())) } else { None },
        if stale_count > 0 { Some(format!("{} stale index files", stale_count)) } else { None },
    ].into_iter().flatten().collect();

    let summary = if summary_parts.is_empty() {
        "No relevant context found for the current work.".to_string()
    } else {
        format!("Found {} for current context.", summary_parts.join(", "))
    };

    // Calculate if any critical items need attention
    let has_critical = corrections.iter().any(|c| {
        c.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0) > 0.9
    }) || active_goals.iter().any(|g| {
        g.get("status").and_then(|s| s.as_str()) == Some("blocked")
    });

    // Detect task type from query/context
    let (task_type, task_confidence) = detect_task_type(&req, &similar_errors, &active_goals);

    // Get tool recommendations based on task type and phase
    let tool_recommendations = build_tool_recommendations(&task_type, phase);

    Ok(serde_json::json!({
        "summary": summary,
        "has_critical_items": has_critical,
        "session_context": {
            "phase": phase.as_str(),
            "task_type": task_type.as_str(),
            "task_confidence": task_confidence,
        },
        "tool_recommendations": tool_recommendations,
        "corrections": corrections,
        "active_goals": active_goals,
        "rejected_approaches": rejected_approaches,
        "related_decisions": related_decisions,
        "relevant_memories": relevant_memories,
        "similar_fixes": similar_errors,
        "related_tool_calls": relevant_mcp_history,
        "code_context": code_context,
        "call_graph": call_graph,
        "index_status": index_status,
    }))
}

/// Get rejected approaches relevant to current context
async fn get_rejected_approaches(
    db: &SqlitePool,
    req: &GetProactiveContextRequest,
    project_id: Option<i64>,
    limit: i64,
) -> anyhow::Result<Vec<serde_json::Value>> {
    // Build search terms
    let search_terms: Vec<String> = vec![
        req.files.clone().unwrap_or_default(),
        req.topics.clone().unwrap_or_default(),
    ].into_iter().flatten().collect();

    if search_terms.is_empty() && req.task.is_none() {
        return Ok(Vec::new());
    }

    // Get all rejected approaches for project
    let all_rejected = sqlx::query_as::<_, (String, String, String, String, Option<String>, Option<String>, String)>(r#"
        SELECT id, problem_context, approach, rejection_reason, related_files, related_topics,
               datetime(created_at, 'unixepoch', 'localtime') as created
        FROM rejected_approaches
        WHERE project_id IS NULL OR project_id = $1
        ORDER BY created_at DESC
        LIMIT 50
    "#)
    .bind(project_id)
    .fetch_all(db)
    .await?;

    // Filter by relevance
    let mut results = Vec::new();
    for (id, problem, approach, reason, files_json, topics_json, created) in all_rejected {
        let mut score = 0.0;

        // Check file relevance
        if let Some(files_str) = &files_json {
            if let Ok(files) = serde_json::from_str::<Vec<String>>(files_str) {
                if let Some(req_files) = &req.files {
                    for req_file in req_files {
                        if files.iter().any(|f| req_file.contains(f) || f.contains(req_file)) {
                            score += 2.0;
                        }
                    }
                }
            }
        }

        // Check topic relevance
        if let Some(topics_str) = &topics_json {
            if let Ok(topics) = serde_json::from_str::<Vec<String>>(topics_str) {
                if let Some(req_topics) = &req.topics {
                    for req_topic in req_topics {
                        let lower = req_topic.to_lowercase();
                        if topics.iter().any(|t| t.to_lowercase().contains(&lower) || lower.contains(&t.to_lowercase())) {
                            score += 1.5;
                        }
                    }
                }
            }
        }

        // Check problem context relevance
        if let Some(task) = &req.task {
            let task_lower = task.to_lowercase();
            let problem_lower = problem.to_lowercase();
            if problem_lower.contains(&task_lower) || task_lower.contains(&problem_lower) {
                score += 1.0;
            }
        }

        if score > 0.0 {
            results.push(serde_json::json!({
                "id": id,
                "problem_context": problem,
                "approach": approach,
                "rejection_reason": reason,
                "relevance_score": score,
                "created_at": created,
            }));
        }
    }

    // Sort by relevance and limit
    results.sort_by(|a, b| {
        let sa = a.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let sb = b.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit as usize);

    Ok(results)
}

/// Get related decisions from memory
async fn get_related_decisions(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    context: &str,
    project_id: Option<i64>,
    limit: i64,
) -> anyhow::Result<Vec<serde_json::Value>> {
    if context.is_empty() {
        // Return recent decisions when no context
        let results = sqlx::query_as::<_, (String, String, Option<String>, String)>(r#"
            SELECT key, value, category,
                   datetime(updated_at, 'unixepoch', 'localtime') as updated
            FROM memory_facts
            WHERE fact_type = 'decision'
              AND (project_id IS NULL OR project_id = $1)
              AND key NOT LIKE 'compaction-%'
            ORDER BY updated_at DESC
            LIMIT $2
        "#)
        .bind(project_id)
        .bind(limit)
        .fetch_all(db)
        .await?;

        return Ok(results.into_iter().map(|(key, value, category, updated)| {
            serde_json::json!({
                "key": key,
                "decision": value,
                "category": category,
                "updated_at": updated,
            })
        }).collect());
    }

    // Try semantic search first
    if semantic.is_available() {
        let filter = Some(qdrant_client::qdrant::Filter::must([
            qdrant_client::qdrant::Condition::matches("fact_type", "decision".to_string()),
        ]));

        if let Ok(results) = semantic.search("mira_conversation", context, limit as usize, filter).await {
            if !results.is_empty() {
                let mut decisions = Vec::new();
                for result in results {
                    // Get ID from metadata
                    let result_id = result.metadata.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    if result_id.is_empty() {
                        continue;
                    }
                    // Get full record from DB
                    if let Ok(Some(row)) = sqlx::query_as::<_, (String, String, Option<String>, String)>(r#"
                        SELECT key, value, category,
                               datetime(updated_at, 'unixepoch', 'localtime') as updated
                        FROM memory_facts
                        WHERE id = $1
                    "#)
                    .bind(result_id)
                    .fetch_optional(db)
                    .await {
                        decisions.push(serde_json::json!({
                            "key": row.0,
                            "decision": row.1,
                            "category": row.2,
                            "updated_at": row.3,
                            "relevance_score": result.score,
                        }));
                    }
                }
                return Ok(decisions);
            }
        }
    }

    // Fallback to text search
    let results = sqlx::query_as::<_, (String, String, Option<String>, String)>(r#"
        SELECT key, value, category,
               datetime(updated_at, 'unixepoch', 'localtime') as updated
        FROM memory_facts
        WHERE fact_type = 'decision'
          AND (project_id IS NULL OR project_id = $1)
          AND (value LIKE '%' || $2 || '%' OR key LIKE '%' || $2 || '%')
        ORDER BY updated_at DESC
        LIMIT $3
    "#)
    .bind(project_id)
    .bind(context.split_whitespace().next().unwrap_or(""))
    .bind(limit)
    .fetch_all(db)
    .await?;

    Ok(results.into_iter().map(|(key, value, category, updated)| {
        serde_json::json!({
            "key": key,
            "decision": value,
            "category": category,
            "updated_at": updated,
        })
    }).collect())
}

/// Get relevant memories (preferences, context, general)
async fn get_relevant_memories(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    context: &str,
    project_id: Option<i64>,
    limit: i64,
) -> anyhow::Result<Vec<serde_json::Value>> {
    if context.is_empty() {
        // Return recent non-decision memories when no context
        let results = sqlx::query_as::<_, (String, String, String, String, Option<String>, String)>(r#"
            SELECT id, key, value, fact_type, category,
                   datetime(updated_at, 'unixepoch', 'localtime') as updated
            FROM memory_facts
            WHERE fact_type != 'decision'
              AND (project_id IS NULL OR project_id = $1)
              AND key NOT LIKE 'compaction-%'
            ORDER BY times_used DESC, updated_at DESC
            LIMIT $2
        "#)
        .bind(project_id)
        .bind(limit)
        .fetch_all(db)
        .await?;

        return Ok(results.into_iter().map(|(id, key, value, fact_type, category, updated)| {
            serde_json::json!({
                "id": id,
                "key": key,
                "content": value,
                "fact_type": fact_type,
                "category": category,
                "updated_at": updated,
            })
        }).collect());
    }

    // Try semantic search
    if semantic.is_available() {
        if let Ok(results) = semantic.search("mira_conversation", context, limit as usize, None).await {
            if !results.is_empty() {
                let mut memories = Vec::new();
                for result in results {
                    // Skip if it's a decision (already covered)
                    if result.metadata.get("fact_type").and_then(|t| t.as_str()) == Some("decision") {
                        continue;
                    }

                    // Get ID from metadata
                    let result_id = result.metadata.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    if result_id.is_empty() {
                        continue;
                    }

                    if let Ok(Some(row)) = sqlx::query_as::<_, (String, String, String, String, Option<String>, String)>(r#"
                        SELECT id, key, value, fact_type, category,
                               datetime(updated_at, 'unixepoch', 'localtime') as updated
                        FROM memory_facts
                        WHERE id = $1
                    "#)
                    .bind(result_id)
                    .fetch_optional(db)
                    .await {
                        memories.push(serde_json::json!({
                            "id": row.0,
                            "key": row.1,
                            "value": row.2,
                            "fact_type": row.3,
                            "category": row.4,
                            "updated_at": row.5,
                            "relevance_score": result.score,
                        }));
                    }
                }
                return Ok(memories);
            }
        }
    }

    // Fallback to text search
    let results = sqlx::query_as::<_, (String, String, String, String, Option<String>, String)>(r#"
        SELECT id, key, value, fact_type, category,
               datetime(updated_at, 'unixepoch', 'localtime') as updated
        FROM memory_facts
        WHERE fact_type IN ('preference', 'context')
          AND (project_id IS NULL OR project_id = $1)
          AND (value LIKE '%' || $2 || '%' OR key LIKE '%' || $2 || '%')
        ORDER BY times_used DESC, updated_at DESC
        LIMIT $3
    "#)
    .bind(project_id)
    .bind(context.split_whitespace().next().unwrap_or(""))
    .bind(limit)
    .fetch_all(db)
    .await?;

    Ok(results.into_iter().map(|(id, key, value, fact_type, category, updated)| {
        serde_json::json!({
            "id": id,
            "key": key,
            "value": value,
            "fact_type": fact_type,
            "category": category,
            "updated_at": updated,
        })
    }).collect())
}

/// Get code intelligence context for specified files
async fn get_code_context(
    db: &SqlitePool,
    files: &[String],
    limit: i64,
) -> anyhow::Result<serde_json::Value> {
    let mut related_files = Vec::new();
    let mut key_symbols = Vec::new();

    for file_path in files.iter().take(3) {
        // Get related files (imports + cochange)
        if let Ok(related) = code_intel::get_related_files(
            db,
            GetRelatedFilesRequest {
                file_path: file_path.clone(),
                relation_type: Some("all".to_string()),
                limit: Some(limit),
            },
        ).await {
            // Extract cochange patterns (most useful for understanding impact)
            if let Some(cochange) = related.get("cochange_patterns").and_then(|c| c.as_array()) {
                for pattern in cochange.iter().take(3) {
                    if let Some(file) = pattern.get("file").and_then(|f| f.as_str()) {
                        let confidence = pattern.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.0);
                        if confidence > 0.3 {
                            related_files.push(serde_json::json!({
                                "file": file,
                                "related_to": file_path,
                                "relation": "cochange",
                                "confidence": confidence,
                            }));
                        }
                    }
                }
            }

            // Extract imports (dependencies)
            if let Some(imports) = related.get("imports").and_then(|i| i.as_array()) {
                for import in imports.iter().take(5) {
                    if let Some(path) = import.get("import_path").and_then(|p| p.as_str()) {
                        let is_external = import.get("is_external").and_then(|e| e.as_bool()).unwrap_or(true);
                        if !is_external {
                            related_files.push(serde_json::json!({
                                "file": path,
                                "related_to": file_path,
                                "relation": "import",
                            }));
                        }
                    }
                }
            }
        }

        // Get key symbols from the file (functions, structs, etc.)
        let symbols = sqlx::query_as::<_, (String, String, Option<String>, i64)>(r#"
            SELECT name, symbol_type, signature, start_line
            FROM code_symbols
            WHERE file_path LIKE $1
              AND symbol_type IN ('function', 'struct', 'class', 'trait', 'enum')
              AND (visibility IS NULL OR visibility != 'private')
            ORDER BY start_line
            LIMIT $2
        "#)
        .bind(format!("%{}", file_path))
        .bind(limit)
        .fetch_all(db)
        .await
        .unwrap_or_default();

        for (name, symbol_type, signature, line) in symbols {
            key_symbols.push(serde_json::json!({
                "name": name,
                "type": symbol_type,
                "signature": signature,
                "file": file_path,
                "line": line,
            }));
        }
    }

    // Deduplicate related files
    let mut seen = std::collections::HashSet::new();
    related_files.retain(|f| {
        let key = f.get("file").and_then(|f| f.as_str()).unwrap_or("");
        seen.insert(key.to_string())
    });

    // Get codebase style metrics and improvements if we have files to work with
    let (style_context, improvement_suggestions) = if let Some(first_file) = files.first() {
        // Determine project path from first file - try to find src directory for better accuracy
        let project_path = {
            let parts: Vec<&str> = first_file.split('/').collect();
            // Look for 'src' directory to exclude target/external code
            if let Some(src_idx) = parts.iter().position(|p| *p == "src") {
                parts[..=src_idx].join("/")
            } else {
                // Fallback to 4 segments (e.g., /home/user/project)
                parts.iter().take(4).cloned().collect::<Vec<_>>().join("/")
            }
        };
        match code_intel::analyze_codebase_style(db, &project_path).await {
            Ok(report) if report.total_functions > 0 => {
                // Get improvement suggestions for these files
                let improvements = code_intel::find_improvements(db, files, &report)
                    .await
                    .unwrap_or_default();

                let style = serde_json::json!({
                    "avg_function_length": report.avg_function_length,
                    "function_distribution": {
                        "short_pct": report.short_pct,
                        "medium_pct": report.medium_pct,
                        "long_pct": report.long_pct,
                    },
                    "abstraction_level": report.abstraction_level,
                    "suggested_max_length": report.suggested_max_length,
                    "guidance": format!(
                        "Keep functions under {} lines. This codebase averages {:.0} lines per function with {} abstraction.",
                        report.suggested_max_length,
                        report.avg_function_length,
                        report.abstraction_level
                    ),
                });

                let imps: Vec<serde_json::Value> = improvements.iter().map(|i| {
                    serde_json::json!({
                        "file_path": i.file_path,
                        "symbol_name": i.symbol_name,
                        "improvement_type": i.improvement_type,
                        "current_value": i.current_value,
                        "threshold": i.threshold,
                        "severity": i.severity,
                        "suggestion": i.suggestion,
                        "start_line": i.start_line,
                    })
                }).collect();

                (Some(style), imps)
            }
            _ => (None, Vec::new()),
        }
    } else {
        (None, Vec::new())
    };

    Ok(serde_json::json!({
        "related_files": related_files,
        "key_symbols": key_symbols,
        "codebase_style": style_context,
        "improvement_suggestions": improvement_suggestions,
    }))
}

/// Get call graph context for symbols in the specified files
async fn get_call_graph_context(
    db: &SqlitePool,
    files: &[String],
    limit: i64,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut call_refs = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Get symbols from the specified files
    for file_path in files.iter().take(3) {
        let symbols = sqlx::query_as::<_, (i64, String)>(
            r#"
            SELECT id, name FROM code_symbols
            WHERE file_path LIKE $1
              AND symbol_type IN ('function', 'method')
            ORDER BY start_line
            LIMIT 5
            "#,
        )
        .bind(format!("%{}", file_path))
        .fetch_all(db)
        .await
        .unwrap_or_default();

        for (symbol_id, symbol_name) in symbols {
            // Get callers (who calls this symbol)
            let callers = sqlx::query_as::<_, (String, String)>(
                r#"
                SELECT caller.name, caller.file_path
                FROM call_graph cg
                JOIN code_symbols caller ON cg.caller_id = caller.id
                WHERE cg.callee_id = $1
                LIMIT 5
                "#,
            )
            .bind(symbol_id)
            .fetch_all(db)
            .await
            .unwrap_or_default();

            for (caller_name, caller_file) in callers {
                let key = format!("{}->{}:{}", caller_name, symbol_name, caller_file);
                if seen.contains(&key) {
                    continue;
                }
                seen.insert(key);

                call_refs.push(serde_json::json!({
                    "caller": caller_name,
                    "callee": symbol_name,
                    "caller_file": caller_file,
                    "direction": "caller",
                }));
            }

            // Get callees (what this symbol calls)
            let callees = sqlx::query_as::<_, (String, String)>(
                r#"
                SELECT callee.name, callee.file_path
                FROM call_graph cg
                JOIN code_symbols callee ON cg.callee_id = callee.id
                WHERE cg.caller_id = $1
                LIMIT 5
                "#,
            )
            .bind(symbol_id)
            .fetch_all(db)
            .await
            .unwrap_or_default();

            for (callee_name, callee_file) in callees {
                let key = format!("{}->{}:{}", symbol_name, callee_name, callee_file);
                if seen.contains(&key) {
                    continue;
                }
                seen.insert(key);

                call_refs.push(serde_json::json!({
                    "caller": symbol_name,
                    "callee": callee_name,
                    "callee_file": callee_file,
                    "direction": "callee",
                }));
            }
        }
    }

    call_refs.truncate(limit as usize);
    Ok(call_refs)
}

/// Get index freshness status
async fn get_index_freshness(
    db: &SqlitePool,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    // Get last indexed time for the project
    let last_indexed: Option<(i64,)> = sqlx::query_as(
        r#"
        SELECT MAX(analyzed_at) FROM code_symbols
        WHERE project_id = $1 OR $1 IS NULL
        "#,
    )
    .bind(project_id)
    .fetch_optional(db)
    .await?;

    let last_indexed_ts = last_indexed.map(|r| r.0);

    // Get files modified since last index (using git)
    let stale_files = if let Some(_since_ts) = last_indexed_ts {
        // Query for indexed files that might be stale
        let indexed_files: Vec<(String, i64)> = sqlx::query_as(
            r#"
            SELECT DISTINCT file_path, MAX(analyzed_at) as last_analyzed
            FROM code_symbols
            WHERE project_id = $1 OR $1 IS NULL
            GROUP BY file_path
            ORDER BY last_analyzed DESC
            LIMIT 50
            "#,
        )
        .bind(project_id)
        .fetch_all(db)
        .await
        .unwrap_or_default();

        // Check file mtimes
        let mut stale = Vec::new();
        for (file_path, analyzed_at) in indexed_files {
            if let Ok(metadata) = std::fs::metadata(&file_path) {
                if let Ok(mtime) = metadata.modified() {
                    let mtime_ts = mtime
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);

                    if mtime_ts > analyzed_at {
                        stale.push(file_path);
                    }
                }
            }

            if stale.len() >= 10 {
                break;
            }
        }
        stale
    } else {
        Vec::new()
    };

    // Format last indexed as human-readable
    let last_indexed_str = last_indexed_ts.map(|ts| {
        chrono::DateTime::from_timestamp(ts, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "unknown".to_string())
    });

    Ok(serde_json::json!({
        "last_indexed": last_indexed_str,
        "stale_files": stale_files,
        "is_fresh": stale_files.is_empty(),
    }))
}

/// Detect task type from request context
///
/// Uses multiple signals:
/// 1. Explicit task description
/// 2. Error presence (suggests debugging)
/// 3. Active goals (suggests planning/new feature)
/// 4. Files being worked on
fn detect_task_type(
    req: &GetProactiveContextRequest,
    similar_errors: &[serde_json::Value],
    active_goals: &[serde_json::Value],
) -> (TaskType, f32) {
    // If error is explicitly provided, likely debugging
    if req.error.is_some() || !similar_errors.is_empty() {
        return (TaskType::Debugging, 0.9);
    }

    // If we have a task description, use keyword detection
    if let Some(task) = &req.task {
        return TaskType::detect_from_query(task);
    }

    // If we have files but no task, likely exploration
    if req.files.is_some() && req.task.is_none() {
        return (TaskType::Exploration, 0.7);
    }

    // If we have topics related to planning/goals
    if let Some(topics) = &req.topics {
        let topic_str = topics.join(" ").to_lowercase();
        if topic_str.contains("goal") || topic_str.contains("plan") || topic_str.contains("milestone") {
            return (TaskType::Planning, 0.7);
        }
    }

    // If there are active goals in progress, likely new feature work
    if !active_goals.is_empty() {
        let has_in_progress = active_goals.iter().any(|g| {
            g.get("status").and_then(|s| s.as_str()) == Some("in_progress")
        });
        if has_in_progress {
            return (TaskType::NewFeature, 0.6);
        }
    }

    // Default to exploration
    (TaskType::Exploration, 0.5)
}

/// Build tool recommendations based on task type and session phase
///
/// Returns a structured recommendation with:
/// - emphasized: Tools that are particularly useful for this context
/// - deemphasized: Tools that are less relevant (can still be used)
/// - model_hint: Recommended Gemini model for this context
fn build_tool_recommendations(task_type: &TaskType, phase: SessionPhase) -> serde_json::Value {
    let emphasized = task_type.emphasized_tools();
    let deemphasized = task_type.deemphasized_tools();

    // Adjust based on phase
    let mut phase_emphasis: Vec<&str> = Vec::new();
    let mut phase_deemphasis: Vec<&str> = Vec::new();

    match phase {
        SessionPhase::Early => {
            phase_emphasis.extend(&["get_session_context", "recall", "goal"]);
            phase_deemphasis.extend(&["store_session", "batch"]);
        }
        SessionPhase::Middle => {
            phase_emphasis.extend(&["get_proactive_context", "build", "semantic_code_search"]);
            phase_deemphasis.extend(&["store_session"]);
        }
        SessionPhase::Late => {
            phase_emphasis.extend(&["build", "find_similar_fixes", "correction"]);
            phase_deemphasis.extend(&["goal", "proposal"]);
        }
        SessionPhase::Wrapping => {
            phase_emphasis.extend(&["store_session", "remember", "goal"]);
            phase_deemphasis.extend(&["semantic_code_search", "index"]);
        }
    }

    // Combine task and phase recommendations
    let mut all_emphasized: Vec<&str> = emphasized.to_vec();
    all_emphasized.extend(phase_emphasis);
    all_emphasized.sort();
    all_emphasized.dedup();

    let mut all_deemphasized: Vec<&str> = deemphasized.to_vec();
    all_deemphasized.extend(phase_deemphasis);
    // Remove any that are also emphasized (task type takes precedence)
    all_deemphasized.retain(|t| !all_emphasized.contains(t));
    all_deemphasized.sort();
    all_deemphasized.dedup();

    serde_json::json!({
        "emphasized": all_emphasized,
        "deemphasized": all_deemphasized,
        "model_hint": task_type.recommended_model(),
        "thinking_level": task_type.recommended_thinking_level(),
        "rationale": format!(
            "Task type '{}' in '{}' phase: prioritize {} tools",
            task_type.as_str(),
            phase.as_str(),
            if emphasized.is_empty() { "general" } else { emphasized[0] }
        ),
    })
}
