//! Mira Power Armor tools: task, goal, correction, store_decision, record_rejected_approach
//!
//! Thin wrapper delegating to core::ops::mira for shared implementation with MCP.

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::Path;
use std::sync::Arc;

use crate::core::SemanticSearch;

use crate::core::ops::mira as core_mira;
use crate::core::OpContext;

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

    /// Create OpContext from our fields
    fn make_context(&self) -> OpContext {
        let mut ctx = OpContext::new(self.cwd.to_path_buf());
        if let Some(db) = self.db.as_ref() {
            ctx = ctx.with_db(db.clone());
        }
        if let Some(semantic) = self.semantic.as_ref() {
            ctx = ctx.with_semantic(semantic.clone());
        }
        ctx
    }

    /// Task management - create, list, update, complete tasks
    pub async fn task(&self, args: &Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("list");
        if self.db.is_none() {
            return Ok("Error: database not configured".into());
        }

        let ctx = self.make_context();

        match action {
            "create" => {
                let title = args["title"].as_str().unwrap_or("");
                if title.is_empty() {
                    return Ok("Error: title is required".into());
                }

                let input = core_mira::CreateTaskInput {
                    title: title.to_string(),
                    description: args["description"].as_str().map(String::from),
                    priority: args["priority"].as_str().map(String::from),
                    parent_id: args["parent_id"].as_str().map(String::from),
                };

                match core_mira::create_task(&ctx, input).await {
                    Ok(output) => Ok(json!({
                        "status": "created",
                        "task_id": output.task_id,
                        "title": output.title,
                        "priority": output.priority,
                    }).to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "list" => {
                let input = core_mira::ListTasksInput {
                    status: args["status"].as_str().map(String::from),
                    parent_id: args["parent_id"].as_str().map(String::from),
                    include_completed: args["include_completed"].as_bool().unwrap_or(false),
                    limit: args["limit"].as_i64().unwrap_or(20),
                };

                match core_mira::list_tasks(&ctx, input).await {
                    Ok(tasks) => {
                        let tasks_json: Vec<Value> = tasks.into_iter().map(|t| {
                            json!({
                                "id": t.id,
                                "parent_id": t.parent_id,
                                "title": t.title,
                                "description": t.description,
                                "status": t.status,
                                "priority": t.priority,
                                "created_at": t.created_at,
                                "updated_at": t.updated_at,
                            })
                        }).collect();
                        let count = tasks_json.len();
                        Ok(json!({
                            "tasks": tasks_json,
                            "count": count,
                        }).to_string())
                    }
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "get" => {
                let task_id = args["task_id"].as_str().unwrap_or("");
                if task_id.is_empty() {
                    return Ok("Error: task_id is required".into());
                }

                match core_mira::get_task(&ctx, task_id).await {
                    Ok(Some(t)) => {
                        let subtasks: Vec<Value> = t.subtasks.into_iter().map(|s| {
                            json!({
                                "id": s.id,
                                "title": s.title,
                                "status": s.status,
                                "priority": s.priority,
                            })
                        }).collect();
                        Ok(json!({
                            "id": t.id,
                            "parent_id": t.parent_id,
                            "title": t.title,
                            "description": t.description,
                            "status": t.status,
                            "priority": t.priority,
                            "subtasks": subtasks,
                            "created_at": t.created_at,
                            "updated_at": t.updated_at,
                        }).to_string())
                    }
                    Ok(None) => Ok(json!({"error": "Task not found"}).to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "update" => {
                let task_id = args["task_id"].as_str().unwrap_or("");
                if task_id.is_empty() {
                    return Ok("Error: task_id is required".into());
                }

                let input = core_mira::UpdateTaskInput {
                    task_id: task_id.to_string(),
                    title: args["title"].as_str().map(String::from),
                    description: args["description"].as_str().map(String::from),
                    status: args["status"].as_str().map(String::from),
                    priority: args["priority"].as_str().map(String::from),
                };

                match core_mira::update_task(&ctx, input).await {
                    Ok(true) => Ok(json!({
                        "status": "updated",
                        "task_id": task_id,
                    }).to_string()),
                    Ok(false) => Ok(json!({"error": "Task not found"}).to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "complete" => {
                let task_id = args["task_id"].as_str().unwrap_or("");
                if task_id.is_empty() {
                    return Ok("Error: task_id is required".into());
                }
                let notes = args["notes"].as_str().map(String::from);

                match core_mira::complete_task(&ctx, task_id, notes).await {
                    Ok(Some(output)) => Ok(json!({
                        "status": "completed",
                        "task_id": output.task_id,
                        "title": output.title,
                        "completed_at": output.completed_at,
                    }).to_string()),
                    Ok(None) => Ok(json!({"error": "Task not found"}).to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "delete" => {
                let task_id = args["task_id"].as_str().unwrap_or("");
                if task_id.is_empty() {
                    return Ok("Error: task_id is required".into());
                }

                match core_mira::delete_task(&ctx, task_id).await {
                    Ok(Some(title)) => Ok(json!({
                        "status": "deleted",
                        "task_id": task_id,
                        "title": title,
                    }).to_string()),
                    Ok(None) => Ok(json!({"error": "Task not found"}).to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            _ => Ok(format!(
                "Unknown action: {}. Use create/list/get/update/complete/delete",
                action
            )),
        }
    }

    /// Goal management - create, list, update goals with milestones
    pub async fn goal(&self, args: &Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("list");
        if self.db.is_none() {
            return Ok("Error: database not configured".into());
        }

        let ctx = self.make_context();
        let project_id = self.get_project_id().await;

        match action {
            "create" => {
                let title = args["title"].as_str().unwrap_or("");
                if title.is_empty() {
                    return Ok("Error: title is required".into());
                }

                let input = core_mira::CreateGoalInput {
                    title: title.to_string(),
                    description: args["description"].as_str().map(String::from),
                    success_criteria: args["success_criteria"].as_str().map(String::from),
                    priority: args["priority"].as_str().map(String::from),
                    project_id,
                };

                match core_mira::create_goal(&ctx, input).await {
                    Ok(output) => Ok(json!({
                        "status": "created",
                        "goal_id": output.goal_id,
                        "title": output.title,
                        "priority": output.priority,
                    }).to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "list" => {
                let input = core_mira::ListGoalsInput {
                    status: args["status"].as_str().map(String::from),
                    include_finished: args["include_finished"].as_bool().unwrap_or(false),
                    limit: args["limit"].as_i64().unwrap_or(10),
                    project_id,
                };

                match core_mira::list_goals(&ctx, input).await {
                    Ok(goals) => {
                        let goals_json: Vec<Value> = goals.into_iter().map(|g| {
                            json!({
                                "id": g.id,
                                "title": g.title,
                                "description": g.description,
                                "status": g.status,
                                "priority": g.priority,
                                "progress_percent": g.progress_percent,
                                "updated_at": g.updated_at,
                            })
                        }).collect();
                        let count = goals_json.len();
                        Ok(json!({
                            "goals": goals_json,
                            "count": count,
                        }).to_string())
                    }
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "update" => {
                let goal_id = args["goal_id"].as_str().unwrap_or("");
                if goal_id.is_empty() {
                    return Ok("Error: goal_id is required".into());
                }

                let input = core_mira::UpdateGoalInput {
                    goal_id: goal_id.to_string(),
                    title: args["title"].as_str().map(String::from),
                    description: args["description"].as_str().map(String::from),
                    status: args["status"].as_str().map(String::from),
                    priority: args["priority"].as_str().map(String::from),
                    progress_percent: args["progress_percent"].as_i64().map(|v| v as i32),
                };

                match core_mira::update_goal(&ctx, input).await {
                    Ok(true) => Ok(json!({
                        "status": "updated",
                        "goal_id": goal_id,
                    }).to_string()),
                    Ok(false) => Ok(json!({"error": "Goal not found"}).to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "add_milestone" => {
                let goal_id = args["goal_id"].as_str().unwrap_or("");
                let title = args["title"].as_str().unwrap_or("");
                if goal_id.is_empty() || title.is_empty() {
                    return Ok("Error: goal_id and title are required".into());
                }

                let input = core_mira::AddMilestoneInput {
                    goal_id: goal_id.to_string(),
                    title: title.to_string(),
                    description: args["description"].as_str().map(String::from),
                    weight: args["weight"].as_i64().map(|v| v as i32),
                };

                match core_mira::add_milestone(&ctx, input).await {
                    Ok(output) => Ok(json!({
                        "status": "added",
                        "milestone_id": output.milestone_id,
                        "goal_id": output.goal_id,
                        "title": output.title,
                    }).to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "complete_milestone" => {
                let milestone_id = args["milestone_id"].as_str().unwrap_or("");
                if milestone_id.is_empty() {
                    return Ok("Error: milestone_id is required".into());
                }

                match core_mira::complete_milestone(&ctx, milestone_id).await {
                    Ok(Some(output)) => Ok(json!({
                        "status": "completed",
                        "milestone_id": output.milestone_id,
                        "goal_id": output.goal_id,
                        "goal_progress_percent": output.goal_progress_percent,
                    }).to_string()),
                    Ok(None) => Ok(json!({"error": "Milestone not found"}).to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "progress" => {
                let goal_id = args["goal_id"].as_str().map(String::from);

                // Get goal progress summary
                let input = core_mira::ListGoalsInput {
                    status: None,
                    include_finished: false,
                    limit: 20,
                    project_id,
                };

                match core_mira::list_goals(&ctx, input).await {
                    Ok(goals) => {
                        if let Some(gid) = goal_id {
                            // Return specific goal
                            if let Some(g) = goals.into_iter().find(|g| g.id == gid) {
                                return Ok(json!({
                                    "id": g.id,
                                    "title": g.title,
                                    "status": g.status,
                                    "progress_percent": g.progress_percent,
                                    "milestones_completed": g.milestones_completed,
                                    "milestones_total": g.milestones_total,
                                }).to_string());
                            }
                            return Ok(json!({"error": "Goal not found"}).to_string());
                        }

                        // Return all goals summary
                        let total_active = goals.len();
                        let blocked_count = goals.iter().filter(|g| g.status == "blocked").count();
                        let goals_json: Vec<Value> = goals.into_iter().map(|g| {
                            json!({
                                "id": g.id,
                                "title": g.title,
                                "status": g.status,
                                "progress_percent": g.progress_percent,
                            })
                        }).collect();

                        Ok(json!({
                            "active_goals": goals_json,
                            "total_active": total_active,
                            "blocked_count": blocked_count,
                        }).to_string())
                    }
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            _ => Ok(format!(
                "Unknown action: {}. Use create/list/update/add_milestone/complete_milestone/progress",
                action
            )),
        }
    }

    /// Correction management - record when user corrects the assistant
    pub async fn correction(&self, args: &Value) -> Result<String> {
        let action = args["action"].as_str().unwrap_or("record");
        if self.db.is_none() {
            return Ok("Error: database not configured".into());
        }

        let ctx = self.make_context();
        let project_id = self.get_project_id().await;

        match action {
            "record" => {
                let what_was_wrong = args["what_was_wrong"].as_str().unwrap_or("");
                let what_is_right = args["what_is_right"].as_str().unwrap_or("");
                if what_was_wrong.is_empty() || what_is_right.is_empty() {
                    return Ok("Error: what_was_wrong and what_is_right are required".into());
                }

                let input = core_mira::RecordCorrectionInput {
                    correction_type: args["correction_type"].as_str().unwrap_or("approach").to_string(),
                    what_was_wrong: what_was_wrong.to_string(),
                    what_is_right: what_is_right.to_string(),
                    rationale: args["rationale"].as_str().map(String::from),
                    scope: args["scope"].as_str().map(String::from),
                    keywords: args["keywords"].as_str().map(String::from),
                    project_id,
                };

                match core_mira::record_correction(&ctx, input).await {
                    Ok(output) => Ok(json!({
                        "status": "recorded",
                        "correction_id": output.correction_id,
                        "correction_type": output.correction_type,
                        "scope": output.scope,
                    }).to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "list" => {
                let input = core_mira::ListCorrectionsInput {
                    correction_type: args["correction_type"].as_str().map(String::from),
                    scope: args["scope"].as_str().map(String::from),
                    status: Some("active".to_string()),
                    limit: args["limit"].as_i64().unwrap_or(10),
                    project_id,
                };

                match core_mira::list_corrections(&ctx, input).await {
                    Ok(corrections) => {
                        let corrections_json: Vec<Value> = corrections.into_iter().map(|c| {
                            json!({
                                "id": c.id,
                                "correction_type": c.correction_type,
                                "what_was_wrong": c.what_was_wrong,
                                "what_is_right": c.what_is_right,
                                "rationale": c.rationale,
                                "scope": c.scope,
                                "confidence": c.confidence,
                                "times_applied": c.times_applied,
                            })
                        }).collect();
                        let count = corrections_json.len();
                        Ok(json!({
                            "corrections": corrections_json,
                            "count": count,
                        }).to_string())
                    }
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }

            "validate" => {
                let correction_id = args["correction_id"].as_str().unwrap_or("");
                let outcome = args["outcome"].as_str().unwrap_or("validated");

                if correction_id.is_empty() {
                    return Ok("Error: correction_id is required".into());
                }

                match core_mira::validate_correction(&ctx, correction_id, outcome).await {
                    Ok(_) => Ok(json!({
                        "status": "validated",
                        "correction_id": correction_id,
                        "outcome": outcome,
                    }).to_string()),
                    Err(e) => Ok(format!("Error: {}", e)),
                }
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

        if self.db.is_none() {
            return Ok("Error: database not configured".into());
        }

        let ctx = self.make_context();
        let project_id = self.get_project_id().await;

        let input = core_mira::StoreDecisionInput {
            key: key.to_string(),
            decision: decision.to_string(),
            category: args["category"].as_str().map(String::from),
            context: args["context"].as_str().map(String::from),
            project_id,
        };

        match core_mira::store_decision(&ctx, input).await {
            Ok(()) => Ok(json!({
                "status": "stored",
                "key": key,
                "decision": decision,
                "category": args["category"].as_str(),
            }).to_string()),
            Err(e) => Ok(format!("Error: {}", e)),
        }
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

        if self.db.is_none() {
            return Ok("Error: database not configured".into());
        }

        let ctx = self.make_context();
        let project_id = self.get_project_id().await;

        let input = core_mira::RecordRejectedApproachInput {
            problem_context: problem_context.to_string(),
            approach: approach.to_string(),
            rejection_reason: rejection_reason.to_string(),
            related_files: args["related_files"].as_str().map(String::from),
            related_topics: args["related_topics"].as_str().map(String::from),
            project_id,
        };

        match core_mira::record_rejected_approach(&ctx, input).await {
            Ok(output) => Ok(json!({
                "status": "recorded",
                "id": output.id,
                "problem_context": output.problem_context,
                "approach": output.approach,
            }).to_string()),
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }
}
