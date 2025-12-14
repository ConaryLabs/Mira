// src/studio/handlers.rs
// HTTP handlers for conversations and workspace events

use axum::{
    extract::{State, Path, Query},
    response::{
        sse::{Event, Sse},
        Json,
    },
    http::StatusCode,
};
use futures::stream::Stream;
use std::{convert::Infallible, time::Duration};
use tracing::debug;

use super::types::{StudioState, WorkspaceEvent, ConversationInfo, MessageInfo, MessagesQuery};

pub async fn status_handler(
    State(state): State<StudioState>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "anthropic_configured": state.anthropic_key.is_some(),
        "semantic_search": state.semantic.is_available(),
    }))
}

/// SSE stream of workspace events for the terminal panel
pub async fn workspace_events_handler(
    State(state): State<StudioState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.workspace_tx.subscribe();

    // Send initial connection event
    let _ = state.workspace_tx.send(WorkspaceEvent::Info {
        message: "Terminal connected".to_string(),
    });

    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Ok(json) = serde_json::to_string(&event) {
                        yield Ok(Event::default().data(json));
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    // Missed some events, continue
                    debug!("Workspace event stream lagged by {} events", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("ping")
    )
}

/// List recent conversations
pub async fn list_conversations(
    State(state): State<StudioState>,
) -> Result<Json<Vec<ConversationInfo>>, (StatusCode, String)> {
    let conversations = sqlx::query_as::<_, (String, Option<String>, i64, i64)>(r#"
        SELECT c.id, c.title, c.created_at, c.updated_at
        FROM studio_conversations c
        ORDER BY c.updated_at DESC
        LIMIT 20
    "#)
    .fetch_all(state.db.as_ref())
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut result = Vec::new();
    for (id, title, created_at, updated_at) in conversations {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM studio_messages WHERE conversation_id = $1"
        )
        .bind(&id)
        .fetch_one(state.db.as_ref())
        .await
        .unwrap_or((0,));

        result.push(ConversationInfo {
            id,
            title,
            created_at,
            updated_at,
            message_count: count,
        });
    }

    Ok(Json(result))
}

/// Create a new conversation
pub async fn create_conversation(
    State(state): State<StudioState>,
) -> Result<Json<ConversationInfo>, (StatusCode, String)> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO studio_conversations (id, created_at, updated_at) VALUES ($1, $2, $2)"
    )
    .bind(&id)
    .bind(now)
    .execute(state.db.as_ref())
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ConversationInfo {
        id,
        title: None,
        created_at: now,
        updated_at: now,
        message_count: 0,
    }))
}

/// Get conversation info
pub async fn get_conversation(
    State(state): State<StudioState>,
    Path(id): Path<String>,
) -> Result<Json<ConversationInfo>, (StatusCode, String)> {
    let conv = sqlx::query_as::<_, (String, Option<String>, i64, i64)>(
        "SELECT id, title, created_at, updated_at FROM studio_conversations WHERE id = $1"
    )
    .bind(&id)
    .fetch_optional(state.db.as_ref())
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or((StatusCode::NOT_FOUND, "Conversation not found".to_string()))?;

    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM studio_messages WHERE conversation_id = $1"
    )
    .bind(&id)
    .fetch_one(state.db.as_ref())
    .await
    .unwrap_or((0,));

    Ok(Json(ConversationInfo {
        id: conv.0,
        title: conv.1,
        created_at: conv.2,
        updated_at: conv.3,
        message_count: count,
    }))
}

/// Get messages for a conversation (paginated)
pub async fn get_messages(
    State(state): State<StudioState>,
    Path(id): Path<String>,
    Query(query): Query<MessagesQuery>,
) -> Result<Json<Vec<MessageInfo>>, (StatusCode, String)> {
    let messages = if let Some(before_id) = query.before {
        // Get the created_at of the before message
        let before_time: Option<(i64,)> = sqlx::query_as(
            "SELECT created_at FROM studio_messages WHERE id = $1"
        )
        .bind(&before_id)
        .fetch_optional(state.db.as_ref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if let Some((before_time,)) = before_time {
            sqlx::query_as::<_, (String, String, String, i64)>(r#"
                SELECT id, role, content, created_at
                FROM studio_messages
                WHERE conversation_id = $1 AND created_at < $2
                ORDER BY created_at DESC
                LIMIT $3
            "#)
            .bind(&id)
            .bind(before_time)
            .bind(query.limit)
            .fetch_all(state.db.as_ref())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        } else {
            vec![]
        }
    } else {
        sqlx::query_as::<_, (String, String, String, i64)>(r#"
            SELECT id, role, content, created_at
            FROM studio_messages
            WHERE conversation_id = $1
            ORDER BY created_at DESC
            LIMIT $2
        "#)
        .bind(&id)
        .bind(query.limit)
        .fetch_all(state.db.as_ref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };

    // Reverse to get chronological order
    let result: Vec<MessageInfo> = messages.into_iter().rev().map(|(id, role, content, created_at)| {
        MessageInfo { id, role, content, created_at }
    }).collect();

    Ok(Json(result))
}
