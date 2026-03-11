//! Goal management bindings for Rhai scripts.
//!
//! Exposes `goal_create`, `goal_list`, `goal_get`, `goal_update`, `goal_delete`,
//! `goal_sessions`, `goal_bulk_create`, `goal_add_milestone`,
//! `goal_complete_milestone`, and `goal_delete_milestone` to Rhai scripts,
//! bridging them to the existing tool implementations in `tools/core/goals.rs`.

use crate::mcp::MiraServer;
use crate::mcp::requests::{GoalAction, GoalRequest};
use crate::scripting::bridge::call_async_json;
use crate::tools::core;
use rhai::{Dynamic, Engine, EvalAltResult, Map};

/// Build a GoalRequest with the given action and all optional fields as None.
fn make_request(action: GoalAction) -> GoalRequest {
    GoalRequest {
        action,
        goal_id: None,
        milestone_id: None,
        title: None,
        milestone_title: None,
        description: None,
        status: None,
        priority: None,
        progress_percent: None,
        weight: None,
        limit: None,
        goals: None,
        include_finished: None,
    }
}

pub fn register(engine: &mut Engine, server: MiraServer) {
    // goal_create(title) -> Map
    let srv = server.clone();
    engine.register_fn(
        "goal_create",
        move |title: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let mut req = make_request(GoalAction::Create);
            req.title = Some(title.to_string());
            call_async_json(async move { core::goal(&srv, req).await })
        },
    );

    // goal_create(title, priority) -> Map
    let srv = server.clone();
    engine.register_fn(
        "goal_create",
        move |title: &str, priority: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let mut req = make_request(GoalAction::Create);
            req.title = Some(title.to_string());
            req.priority = Some(priority.to_string());
            call_async_json(async move { core::goal(&srv, req).await })
        },
    );

    // goal_list() -> Array
    let srv = server.clone();
    engine.register_fn(
        "goal_list",
        move || -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let req = make_request(GoalAction::List);
            call_async_json(async move { core::goal(&srv, req).await })
        },
    );

    // goal_list(include_finished: bool) -> Array
    let srv = server.clone();
    engine.register_fn(
        "goal_list",
        move |include_finished: bool| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let mut req = make_request(GoalAction::List);
            req.include_finished = Some(include_finished);
            call_async_json(async move { core::goal(&srv, req).await })
        },
    );

    // goal_get(goal_id) -> Map
    let srv = server.clone();
    engine.register_fn(
        "goal_get",
        move |goal_id: i64| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let mut req = make_request(GoalAction::Get);
            req.goal_id = Some(goal_id);
            call_async_json(async move { core::goal(&srv, req).await })
        },
    );

    // goal_update(goal_id, fields: Map) -> Map
    let srv = server.clone();
    engine.register_fn(
        "goal_update",
        move |goal_id: i64, fields: Map| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let mut req = make_request(GoalAction::Update);
            req.goal_id = Some(goal_id);
            req.title = fields.get("title").and_then(|v| v.clone().try_cast::<String>());
            req.description = fields
                .get("description")
                .and_then(|v| v.clone().try_cast::<String>());
            req.status = fields.get("status").and_then(|v| v.clone().try_cast::<String>());
            req.priority = fields
                .get("priority")
                .and_then(|v| v.clone().try_cast::<String>());
            req.progress_percent = fields
                .get("progress_percent")
                .and_then(|v| v.clone().try_cast::<i64>())
                .map(|v| v as i32);
            call_async_json(async move { core::goal(&srv, req).await })
        },
    );

    // goal_delete(goal_id) -> Map
    let srv = server.clone();
    engine.register_fn(
        "goal_delete",
        move |goal_id: i64| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let mut req = make_request(GoalAction::Delete);
            req.goal_id = Some(goal_id);
            call_async_json(async move { core::goal(&srv, req).await })
        },
    );

    // goal_sessions(goal_id) -> Array
    let srv = server.clone();
    engine.register_fn(
        "goal_sessions",
        move |goal_id: i64| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let mut req = make_request(GoalAction::Sessions);
            req.goal_id = Some(goal_id);
            call_async_json(async move { core::goal(&srv, req).await })
        },
    );

    // goal_bulk_create(goals: Array) -> Array
    let srv = server.clone();
    engine.register_fn(
        "goal_bulk_create",
        move |goals: rhai::Array| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let mut req = make_request(GoalAction::BulkCreate);
            let goals_json = serde_json::to_string(
                &goals
                    .into_iter()
                    .map(crate::scripting::convert::dynamic_to_value)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_default();
            req.goals = Some(goals_json);
            call_async_json(async move { core::goal(&srv, req).await })
        },
    );

    // goal_add_milestone(goal_id, title) -> Map
    let srv = server.clone();
    engine.register_fn(
        "goal_add_milestone",
        move |goal_id: i64, title: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let mut req = make_request(GoalAction::AddMilestone);
            req.goal_id = Some(goal_id);
            req.milestone_title = Some(title.to_string());
            call_async_json(async move { core::goal(&srv, req).await })
        },
    );

    // goal_add_milestone(goal_id, title, weight) -> Map
    let srv = server.clone();
    engine.register_fn(
        "goal_add_milestone",
        move |goal_id: i64, title: &str, weight: i64| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let mut req = make_request(GoalAction::AddMilestone);
            req.goal_id = Some(goal_id);
            req.milestone_title = Some(title.to_string());
            req.weight = Some(weight as i32);
            call_async_json(async move { core::goal(&srv, req).await })
        },
    );

    // goal_complete_milestone(milestone_id) -> Map
    let srv = server.clone();
    engine.register_fn(
        "goal_complete_milestone",
        move |milestone_id: i64| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let mut req = make_request(GoalAction::CompleteMilestone);
            req.milestone_id = Some(milestone_id);
            call_async_json(async move { core::goal(&srv, req).await })
        },
    );

    // goal_delete_milestone(milestone_id) -> Map
    let srv = server.clone();
    engine.register_fn(
        "goal_delete_milestone",
        move |milestone_id: i64| -> Result<Dynamic, Box<EvalAltResult>> {
            let srv = srv.clone();
            let mut req = make_request(GoalAction::DeleteMilestone);
            req.milestone_id = Some(milestone_id);
            call_async_json(async move { core::goal(&srv, req).await })
        },
    );
}
