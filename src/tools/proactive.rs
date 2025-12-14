// src/tools/proactive.rs
// Proactive context delivery - One unified tool that returns all relevant context

use sqlx::sqlite::SqlitePool;

use super::types::{GetProactiveContextRequest, FindSimilarFixesRequest, GetRelatedFilesRequest};
use super::semantic::SemanticSearch;
use super::corrections::{self, GetCorrectionsParams};
use super::goals;
use super::git_intel;
use super::code_intel;

/// Get all relevant context for the current work, combined into one response
pub async fn get_proactive_context(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    req: GetProactiveContextRequest,
    project_id: Option<i64>,
) -> anyhow::Result<serde_json::Value> {
    let limit = req.limit_per_category.unwrap_or(3) as i64;

    // Build context signals for relevance matching
    let file_context = req.files.as_ref().map(|f| f.join(", "));
    let topic_context = req.topics.as_ref().map(|t| t.join(", "));
    let combined_context = vec![
        file_context.as_deref(),
        topic_context.as_deref(),
        req.task.as_deref(),
        req.error.as_deref(),
    ].into_iter().flatten().collect::<Vec<_>>().join(" ");

    // 1. Get relevant corrections
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

    // 2. Get active goals
    let goals_result = goals::get_goal_progress(
        db,
        None, // goal_id
        project_id,
    ).await.unwrap_or_else(|_| serde_json::json!({"active_goals": []}));

    let active_goals = goals_result.get("active_goals")
        .and_then(|g| g.as_array())
        .map(|arr| arr.iter().take(limit as usize).cloned().collect::<Vec<_>>())
        .unwrap_or_default();

    // 3. Get rejected approaches (if files or topics specified)
    let rejected_approaches = get_rejected_approaches(db, &req, project_id, limit).await.unwrap_or_default();

    // 4. Get related decisions from memory
    let related_decisions = get_related_decisions(db, semantic, &combined_context, project_id, limit).await.unwrap_or_default();

    // 5. Get relevant memories (preferences, context)
    let relevant_memories = get_relevant_memories(db, semantic, &combined_context, project_id, limit).await.unwrap_or_default();

    // 6. Get similar errors if error signal provided
    let similar_errors = if let Some(error) = &req.error {
        git_intel::find_similar_fixes(
            db,
            semantic,
            FindSimilarFixesRequest {
                error: error.clone(),
                category: None,
                language: None,
                limit: Some(limit),
            },
        ).await.unwrap_or_default()
    } else {
        Vec::new()
    };

    // 7. Get code intelligence if files specified
    let code_context = if let Some(files) = &req.files {
        get_code_context(db, files, limit).await.unwrap_or_default()
    } else {
        serde_json::json!({})
    };

    // Check if code context has data
    let has_code_context = code_context.get("related_files")
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);

    // Build summary
    let summary_parts: Vec<String> = vec![
        if !corrections.is_empty() { Some(format!("{} corrections", corrections.len())) } else { None },
        if !active_goals.is_empty() { Some(format!("{} active goals", active_goals.len())) } else { None },
        if !rejected_approaches.is_empty() { Some(format!("{} rejected approaches", rejected_approaches.len())) } else { None },
        if !related_decisions.is_empty() { Some(format!("{} related decisions", related_decisions.len())) } else { None },
        if !relevant_memories.is_empty() { Some(format!("{} relevant memories", relevant_memories.len())) } else { None },
        if !similar_errors.is_empty() { Some(format!("{} similar errors", similar_errors.len())) } else { None },
        if has_code_context { Some("code context".to_string()) } else { None },
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

    Ok(serde_json::json!({
        "summary": summary,
        "has_critical_items": has_critical,
        "corrections": corrections,
        "active_goals": active_goals,
        "rejected_approaches": rejected_approaches,
        "related_decisions": related_decisions,
        "relevant_memories": relevant_memories,
        "similar_errors": similar_errors,
        "code_context": code_context,
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
        return Ok(Vec::new());
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

/// Get relevant memories (preferences, context)
async fn get_relevant_memories(
    db: &SqlitePool,
    semantic: &SemanticSearch,
    context: &str,
    project_id: Option<i64>,
    limit: i64,
) -> anyhow::Result<Vec<serde_json::Value>> {
    if context.is_empty() {
        // Return recent preferences if no context
        let results = sqlx::query_as::<_, (String, String, String, Option<String>, String)>(r#"
            SELECT id, key, value, category,
                   datetime(updated_at, 'unixepoch', 'localtime') as updated
            FROM memory_facts
            WHERE fact_type = 'preference'
              AND (project_id IS NULL OR project_id = $1)
            ORDER BY times_used DESC, updated_at DESC
            LIMIT $2
        "#)
        .bind(project_id)
        .bind(limit)
        .fetch_all(db)
        .await?;

        return Ok(results.into_iter().map(|(id, key, value, category, updated)| {
            serde_json::json!({
                "id": id,
                "key": key,
                "value": value,
                "category": category,
                "fact_type": "preference",
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

    Ok(serde_json::json!({
        "related_files": related_files,
        "key_symbols": key_symbols,
    }))
}
