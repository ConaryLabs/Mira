//! Mira Power Armor tools: task, goal, correction, store_decision, record_rejected_approach

use anyhow::Result;
use chrono::Utc;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

use crate::semantic::{SemanticSearch, COLLECTION_MEMORY};

/// Mira power armor tool implementations
pub struct MiraTools<'a> {
    pub cwd: &'a Path,
    pub semantic: &'a Option<Arc<SemanticSearch>>,
    pub db: &'a Option<SqlitePool>,
}

impl<'a> MiraTools<'a> {
    /// Get project_id from cwd
    async fn get_project_id(&self) -> Option<i64> {
        let db = self.db.as_ref()?;
        let project_path = self.cwd.to_string_lossy().to_string();
        sqlx::query_scalar("SELECT id FROM projects WHERE path = $1")
            .bind(&project_path)
            .fetch_optional(db)
            .await
            .ok()
            .flatten()
    }

    /// Task management - create, list, update, complete tasks
    pub async fn task(&self, args: &Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("list");
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let now = Utc::now().timestamp();

        match action {
            "create" => {
                let title = args["title"].as_str().unwrap_or("");
                if title.is_empty() {
                    return Ok("Error: title is required".into());
                }
                let description = args["description"].as_str();
                let priority = args["priority"].as_str().unwrap_or("medium");
                let parent_id = args["parent_id"].as_str();

                let id = Uuid::new_v4().to_string();

                let _ = sqlx::query(
                    r#"
                    INSERT INTO tasks (id, parent_id, title, description, status, priority, created_at, updated_at)
                    VALUES ($1, $2, $3, $4, 'pending', $5, $6, $6)
                "#,
                )
                .bind(&id)
                .bind(parent_id)
                .bind(title)
                .bind(description)
                .bind(priority)
                .bind(now)
                .execute(db)
                .await;

                Ok(json!({
                    "status": "created",
                    "task_id": id,
                    "title": title,
                    "priority": priority,
                })
                .to_string())
            }

            "list" => {
                let include_completed = args["include_completed"].as_bool().unwrap_or(false);
                let limit = args["limit"].as_i64().unwrap_or(20);

                let rows: Vec<(
                    String,
                    Option<String>,
                    String,
                    Option<String>,
                    String,
                    String,
                    String,
                    String,
                )> = sqlx::query_as(
                    r#"
                    SELECT id, parent_id, title, description, status, priority,
                           datetime(created_at, 'unixepoch', 'localtime') as created_at,
                           datetime(updated_at, 'unixepoch', 'localtime') as updated_at
                    FROM tasks
                    WHERE ($1 = 1 OR status != 'completed')
                    ORDER BY
                        CASE status WHEN 'in_progress' THEN 0 WHEN 'blocked' THEN 1 WHEN 'pending' THEN 2 ELSE 3 END,
                        CASE priority WHEN 'urgent' THEN 0 WHEN 'high' THEN 1 WHEN 'medium' THEN 2 ELSE 3 END,
                        created_at DESC
                    LIMIT $2
                "#,
                )
                .bind(if include_completed { 1 } else { 0 })
                .bind(limit)
                .fetch_all(db)
                .await
                .unwrap_or_default();

                let tasks: Vec<Value> = rows
                    .into_iter()
                    .map(
                        |(id, parent_id, title, desc, status, priority, created, updated)| {
                            json!({
                                "id": id,
                                "parent_id": parent_id,
                                "title": title,
                                "description": desc,
                                "status": status,
                                "priority": priority,
                                "created_at": created,
                                "updated_at": updated,
                            })
                        },
                    )
                    .collect();

                Ok(json!({
                    "tasks": tasks,
                    "count": tasks.len(),
                })
                .to_string())
            }

            "update" => {
                let task_id = args["task_id"].as_str().unwrap_or("");
                if task_id.is_empty() {
                    return Ok("Error: task_id is required".into());
                }

                let _ = sqlx::query(
                    r#"
                    UPDATE tasks
                    SET updated_at = $1,
                        title = COALESCE($2, title),
                        description = COALESCE($3, description),
                        status = COALESCE($4, status),
                        priority = COALESCE($5, priority)
                    WHERE id = $6 OR id LIKE $7
                "#,
                )
                .bind(now)
                .bind(args["title"].as_str())
                .bind(args["description"].as_str())
                .bind(args["status"].as_str())
                .bind(args["priority"].as_str())
                .bind(task_id)
                .bind(format!("{}%", task_id))
                .execute(db)
                .await;

                Ok(json!({
                    "status": "updated",
                    "task_id": task_id,
                })
                .to_string())
            }

            "complete" => {
                let task_id = args["task_id"].as_str().unwrap_or("");
                if task_id.is_empty() {
                    return Ok("Error: task_id is required".into());
                }
                let notes = args["notes"].as_str();

                let _ = sqlx::query(
                    r#"
                    UPDATE tasks
                    SET status = 'completed', completed_at = $1, updated_at = $1, completion_notes = $2
                    WHERE id = $3 OR id LIKE $4
                "#,
                )
                .bind(now)
                .bind(notes)
                .bind(task_id)
                .bind(format!("{}%", task_id))
                .execute(db)
                .await;

                Ok(json!({
                    "status": "completed",
                    "task_id": task_id,
                    "completed_at": Utc::now().to_rfc3339(),
                })
                .to_string())
            }

            "delete" => {
                let task_id = args["task_id"].as_str().unwrap_or("");
                if task_id.is_empty() {
                    return Ok("Error: task_id is required".into());
                }

                let _ = sqlx::query("DELETE FROM tasks WHERE id = $1 OR id LIKE $2")
                    .bind(task_id)
                    .bind(format!("{}%", task_id))
                    .execute(db)
                    .await;

                Ok(json!({
                    "status": "deleted",
                    "task_id": task_id,
                })
                .to_string())
            }

            _ => Ok(format!(
                "Unknown action: {}. Use create/list/update/complete/delete",
                action
            )),
        }
    }

    /// Goal management - create, list, update goals with milestones
    pub async fn goal(&self, args: &Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("list");
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let now = Utc::now().timestamp();

        match action {
            "create" => {
                let title = args["title"].as_str().unwrap_or("");
                if title.is_empty() {
                    return Ok("Error: title is required".into());
                }
                let description = args["description"].as_str();
                let priority = args["priority"].as_str().unwrap_or("medium");
                let success_criteria = args["success_criteria"].as_str();

                let id = format!("goal-{}", &Uuid::new_v4().to_string()[..8]);
                let project_id = self.get_project_id().await;

                let _ = sqlx::query(
                    r#"
                    INSERT INTO goals (id, title, description, success_criteria, status, priority, project_id, created_at, updated_at)
                    VALUES ($1, $2, $3, $4, 'planning', $5, $6, $7, $7)
                "#,
                )
                .bind(&id)
                .bind(title)
                .bind(description)
                .bind(success_criteria)
                .bind(priority)
                .bind(project_id)
                .bind(now)
                .execute(db)
                .await;

                Ok(json!({
                    "status": "created",
                    "goal_id": id,
                    "title": title,
                    "priority": priority,
                })
                .to_string())
            }

            "list" => {
                let include_finished = args["include_finished"].as_bool().unwrap_or(false);
                let limit = args["limit"].as_i64().unwrap_or(10);

                let rows: Vec<(String, String, Option<String>, String, String, i32, String)> =
                    if include_finished {
                        sqlx::query_as(
                            r#"
                        SELECT id, title, description, status, priority, progress_percent,
                               datetime(updated_at, 'unixepoch', 'localtime') as updated
                        FROM goals
                        ORDER BY
                            CASE status WHEN 'blocked' THEN 1 WHEN 'in_progress' THEN 2 WHEN 'planning' THEN 3 ELSE 4 END,
                            CASE priority WHEN 'critical' THEN 1 WHEN 'high' THEN 2 WHEN 'medium' THEN 3 ELSE 4 END,
                            updated_at DESC
                        LIMIT $1
                    "#,
                        )
                        .bind(limit)
                        .fetch_all(db)
                        .await
                        .unwrap_or_default()
                    } else {
                        sqlx::query_as(
                            r#"
                        SELECT id, title, description, status, priority, progress_percent,
                               datetime(updated_at, 'unixepoch', 'localtime') as updated
                        FROM goals
                        WHERE status IN ('planning', 'in_progress', 'blocked')
                        ORDER BY
                            CASE status WHEN 'blocked' THEN 1 WHEN 'in_progress' THEN 2 WHEN 'planning' THEN 3 END,
                            CASE priority WHEN 'critical' THEN 1 WHEN 'high' THEN 2 WHEN 'medium' THEN 3 ELSE 4 END,
                            updated_at DESC
                        LIMIT $1
                    "#,
                        )
                        .bind(limit)
                        .fetch_all(db)
                        .await
                        .unwrap_or_default()
                    };

                let goals: Vec<Value> = rows
                    .into_iter()
                    .map(|(id, title, desc, status, priority, progress, updated)| {
                        json!({
                            "id": id,
                            "title": title,
                            "description": desc,
                            "status": status,
                            "priority": priority,
                            "progress_percent": progress,
                            "updated_at": updated,
                        })
                    })
                    .collect();

                Ok(json!({
                    "goals": goals,
                    "count": goals.len(),
                })
                .to_string())
            }

            "update" => {
                let goal_id = args["goal_id"].as_str().unwrap_or("");
                if goal_id.is_empty() {
                    return Ok("Error: goal_id is required".into());
                }

                let _ = sqlx::query(
                    r#"
                    UPDATE goals
                    SET updated_at = $1,
                        title = COALESCE($2, title),
                        description = COALESCE($3, description),
                        status = COALESCE($4, status),
                        priority = COALESCE($5, priority),
                        progress_percent = COALESCE($6, progress_percent)
                    WHERE id = $7 OR id LIKE $8
                "#,
                )
                .bind(now)
                .bind(args["title"].as_str())
                .bind(args["description"].as_str())
                .bind(args["status"].as_str())
                .bind(args["priority"].as_str())
                .bind(args["progress_percent"].as_i64().map(|v| v as i32))
                .bind(goal_id)
                .bind(format!("{}%", goal_id))
                .execute(db)
                .await;

                Ok(json!({
                    "status": "updated",
                    "goal_id": goal_id,
                })
                .to_string())
            }

            "add_milestone" => {
                let goal_id = args["goal_id"].as_str().unwrap_or("");
                let title = args["title"].as_str().unwrap_or("");
                if goal_id.is_empty() || title.is_empty() {
                    return Ok("Error: goal_id and title are required".into());
                }

                let id = Uuid::new_v4().to_string();
                let weight = args["weight"].as_i64().unwrap_or(1) as i32;

                let _ = sqlx::query(
                    r#"
                    INSERT INTO milestones (id, goal_id, title, description, weight, created_at, updated_at)
                    VALUES ($1, $2, $3, $4, $5, $6, $6)
                "#,
                )
                .bind(&id)
                .bind(goal_id)
                .bind(title)
                .bind(args["description"].as_str())
                .bind(weight)
                .bind(now)
                .execute(db)
                .await;

                Ok(json!({
                    "status": "added",
                    "milestone_id": id,
                    "goal_id": goal_id,
                    "title": title,
                })
                .to_string())
            }

            "complete_milestone" => {
                let milestone_id = args["milestone_id"].as_str().unwrap_or("");
                if milestone_id.is_empty() {
                    return Ok("Error: milestone_id is required".into());
                }

                let _ = sqlx::query(
                    r#"
                    UPDATE milestones
                    SET status = 'completed', completed_at = $1, updated_at = $1
                    WHERE id = $2 OR id LIKE $3
                "#,
                )
                .bind(now)
                .bind(milestone_id)
                .bind(format!("{}%", milestone_id))
                .execute(db)
                .await;

                Ok(json!({
                    "status": "completed",
                    "milestone_id": milestone_id,
                })
                .to_string())
            }

            _ => Ok(format!(
                "Unknown action: {}. Use create/list/update/add_milestone/complete_milestone",
                action
            )),
        }
    }

    /// Correction management - record when user corrects the assistant
    pub async fn correction(&self, args: &Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("record");
        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let now = Utc::now().timestamp();

        match action {
            "record" => {
                let what_was_wrong = args["what_was_wrong"].as_str().unwrap_or("");
                let what_is_right = args["what_is_right"].as_str().unwrap_or("");
                if what_was_wrong.is_empty() || what_is_right.is_empty() {
                    return Ok("Error: what_was_wrong and what_is_right are required".into());
                }

                let correction_type = args["correction_type"].as_str().unwrap_or("approach");
                let rationale = args["rationale"].as_str();
                let scope = args["scope"].as_str().unwrap_or("project");
                let keywords = args["keywords"].as_str();

                let id = Uuid::new_v4().to_string();
                let project_id = self.get_project_id().await;

                let _ = sqlx::query(
                    r#"
                    INSERT INTO corrections (id, correction_type, what_was_wrong, what_is_right, rationale, scope, project_id, keywords, created_at, updated_at)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
                "#,
                )
                .bind(&id)
                .bind(correction_type)
                .bind(what_was_wrong)
                .bind(what_is_right)
                .bind(rationale)
                .bind(scope)
                .bind(project_id)
                .bind(keywords)
                .bind(now)
                .execute(db)
                .await;

                // Store in Qdrant for semantic matching
                if let Some(semantic) = self.semantic {
                    if semantic.is_available() {
                        let content = format!(
                            "Correction: {} -> {}. Rationale: {}",
                            what_was_wrong,
                            what_is_right,
                            rationale.unwrap_or("")
                        );
                        let mut metadata = HashMap::new();
                        metadata.insert("type".into(), json!("correction"));
                        metadata.insert("correction_type".into(), json!(correction_type));
                        metadata.insert("scope".into(), json!(scope));
                        metadata.insert("id".into(), json!(id));

                        let _ = semantic.store(COLLECTION_MEMORY, &id, &content, metadata).await;
                    }
                }

                Ok(json!({
                    "status": "recorded",
                    "correction_id": id,
                    "correction_type": correction_type,
                    "scope": scope,
                })
                .to_string())
            }

            "list" => {
                let limit = args["limit"].as_i64().unwrap_or(10);
                let correction_type = args["correction_type"].as_str();

                let rows: Vec<(String, String, String, String, Option<String>, String, f64, i64)> =
                    sqlx::query_as(
                        r#"
                    SELECT id, correction_type, what_was_wrong, what_is_right, rationale, scope, confidence, times_applied
                    FROM corrections
                    WHERE status = 'active'
                      AND ($1 IS NULL OR correction_type = $1)
                    ORDER BY confidence DESC, times_validated DESC
                    LIMIT $2
                "#,
                    )
                    .bind(correction_type)
                    .bind(limit)
                    .fetch_all(db)
                    .await
                    .unwrap_or_default();

                let corrections: Vec<Value> = rows
                    .into_iter()
                    .map(
                        |(id, ctype, wrong, right, rationale, scope, confidence, applied)| {
                            json!({
                                "id": id,
                                "correction_type": ctype,
                                "what_was_wrong": wrong,
                                "what_is_right": right,
                                "rationale": rationale,
                                "scope": scope,
                                "confidence": confidence,
                                "times_applied": applied,
                            })
                        },
                    )
                    .collect();

                Ok(json!({
                    "corrections": corrections,
                    "count": corrections.len(),
                })
                .to_string())
            }

            "validate" => {
                let correction_id = args["correction_id"].as_str().unwrap_or("");
                let outcome = args["outcome"].as_str().unwrap_or("validated");

                if correction_id.is_empty() {
                    return Ok("Error: correction_id is required".into());
                }

                match outcome {
                    "validated" => {
                        let _ = sqlx::query(
                            r#"
                            UPDATE corrections
                            SET times_validated = times_validated + 1,
                                confidence = MIN(1.0, confidence + 0.05),
                                updated_at = $1
                            WHERE id = $2 OR id LIKE $3
                        "#,
                        )
                        .bind(now)
                        .bind(correction_id)
                        .bind(format!("{}%", correction_id))
                        .execute(db)
                        .await;
                    }
                    "deprecated" => {
                        let _ = sqlx::query(
                            r#"
                            UPDATE corrections SET status = 'deprecated', updated_at = $1
                            WHERE id = $2 OR id LIKE $3
                        "#,
                        )
                        .bind(now)
                        .bind(correction_id)
                        .bind(format!("{}%", correction_id))
                        .execute(db)
                        .await;
                    }
                    _ => {}
                }

                Ok(json!({
                    "status": "validated",
                    "correction_id": correction_id,
                    "outcome": outcome,
                })
                .to_string())
            }

            _ => Ok(format!(
                "Unknown action: {}. Use record/list/validate",
                action
            )),
        }
    }

    /// Store an important decision with context
    pub async fn store_decision(&self, args: &Value) -> Result<String> {
        let key = args["key"].as_str().unwrap_or("");
        let decision = args["decision"].as_str().unwrap_or("");
        if key.is_empty() || decision.is_empty() {
            return Ok("Error: key and decision are required".into());
        }

        let category = args["category"].as_str();
        let context = args["context"].as_str();
        let now = Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();

        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let project_id = self.get_project_id().await;

        // Store in memory_facts with fact_type='decision'
        let _ = sqlx::query(
            r#"
            INSERT INTO memory_facts (id, fact_type, key, value, category, source, confidence, created_at, updated_at, project_id)
            VALUES ($1, 'decision', $2, $3, $4, $5, 1.0, $6, $6, $7)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                project_id = COALESCE(excluded.project_id, memory_facts.project_id),
                updated_at = excluded.updated_at
        "#,
        )
        .bind(&id)
        .bind(key)
        .bind(decision)
        .bind(category)
        .bind(context)
        .bind(now)
        .bind(project_id)
        .execute(db)
        .await;

        // Store in Qdrant for semantic search
        if let Some(semantic) = self.semantic {
            if semantic.is_available() {
                let mut metadata = HashMap::new();
                metadata.insert("fact_type".into(), json!("decision"));
                metadata.insert("key".into(), json!(key));
                if let Some(cat) = category {
                    metadata.insert("category".into(), json!(cat));
                }

                let _ = semantic.store(COLLECTION_MEMORY, &id, decision, metadata).await;
            }
        }

        Ok(json!({
            "status": "stored",
            "key": key,
            "decision": decision,
            "category": category,
        })
        .to_string())
    }

    /// Record a rejected approach to avoid re-suggesting it
    pub async fn record_rejected_approach(&self, args: &Value) -> Result<String> {
        let problem_context = args["problem_context"].as_str().unwrap_or("");
        let approach = args["approach"].as_str().unwrap_or("");
        let rejection_reason = args["rejection_reason"].as_str().unwrap_or("");

        if problem_context.is_empty() || approach.is_empty() || rejection_reason.is_empty() {
            return Ok(
                "Error: problem_context, approach, and rejection_reason are required".into(),
            );
        }

        let db = match &self.db {
            Some(db) => db,
            None => return Ok("Error: database not configured".into()),
        };

        let now = Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();
        let project_id = self.get_project_id().await;

        let related_files = args["related_files"].as_str();
        let related_topics = args["related_topics"].as_str();

        let _ = sqlx::query(
            r#"
            INSERT INTO rejected_approaches (id, project_id, problem_context, approach, rejection_reason, related_files, related_topics, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        )
        .bind(&id)
        .bind(project_id)
        .bind(problem_context)
        .bind(approach)
        .bind(rejection_reason)
        .bind(related_files)
        .bind(related_topics)
        .bind(now)
        .execute(db)
        .await;

        // Store in Qdrant for semantic matching
        if let Some(semantic) = self.semantic {
            if semantic.is_available() {
                let content = format!(
                    "Rejected approach for {}: {} - Reason: {}",
                    problem_context, approach, rejection_reason
                );
                let mut metadata = HashMap::new();
                metadata.insert("type".into(), json!("rejected_approach"));
                metadata.insert("id".into(), json!(id));

                let _ = semantic.store(COLLECTION_MEMORY, &id, &content, metadata).await;
            }
        }

        Ok(json!({
            "status": "recorded",
            "id": id,
            "problem_context": problem_context,
            "approach": approach,
        })
        .to_string())
    }
}
