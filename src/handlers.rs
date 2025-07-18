use axum::{
    extract::{Extension, Json},
    http::{HeaderMap, header::SET_COOKIE, StatusCode},
};
use std::sync::Arc;
use crate::session;
use crate::prompt;
use crate::llm;
use crate::llm::ChatIntent;

#[derive(serde::Deserialize)]
pub struct ChatRequest {
    pub message: String,
}

#[axum::debug_handler]
pub async fn chat_handler(
    Extension(session_store): Extension<Arc<session::SessionStore>>,
    headers: HeaderMap,
    Json(payload): Json<ChatRequest>,
) -> (HeaderMap, axum::http::StatusCode, axum::Json<ChatIntent>) {
    // 1. Try to get session ID from cookie, else generate new
    let session_id = headers
        .get(axum::http::header::COOKIE)
        .and_then(|c| c.to_str().ok())
        .and_then(|cookie_str| {
            cookie_str.split(';').find_map(|pair| {
                let mut kv = pair.trim().splitn(2, '=');
                match (kv.next(), kv.next()) {
                    (Some(k), Some(v)) if k == "mira_session" => Some(v.to_string()),
                    _ => None,
                }
            })
        })
        .unwrap_or_else(session::generate_session_id);

    // 2. Load chat history for this session (last 15 messages)
    let history = match session_store.load_history(&session_id, 15).await {
        Ok(h) => h,
        Err(_) => vec![],
    };

    // 3. Format as GPT "messages"
    let mut gpt_messages = history
        .into_iter()
        .map(|(role, content)| {
            let role = if role == "assistant" { "assistant" } else { "user" };
            serde_json::json!({ "role": role, "content": content })
        })
        .collect::<Vec<_>>();

    // 4. Add the new user message
    gpt_messages.push(serde_json::json!({
        "role": "user",
        "content": &payload.message
    }));

    // 5. Build system prompt as before
    let prompt_context = prompt::PromptContext::new();
    let system_prompt = prompt::build_system_prompt(&prompt_context);

    let mut messages = vec![serde_json::json!({
        "role": "system",
        "content": system_prompt
    })];
    messages.extend(gpt_messages);

    let function_schema = llm::chat_intent_function_schema();

    let llm_result = llm::call_openai_with_function(
        &system_prompt,
        &payload.message,
        &function_schema,
    ).await;

    let (chat, status): (ChatIntent, axum::http::StatusCode) = match llm_result {
        Ok(raw) => {
            let chat = llm::ChatIntent::from_function_result(&raw);
            (chat, axum::http::StatusCode::OK)
        }
        Err(e) => (
            ChatIntent {
                output: e,
                persona: "Default".to_string(),
                mood: "neutral".to_string(),
            },
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        ),
    };

    // 6. Save the user message and Mira's reply to session DB
    let _ = session_store.save_message(&session_id, "user", &payload.message).await;
    let _ = session_store.save_message(&session_id, "assistant", &chat.output).await;

    // 7. Set session cookie (so browser keeps same ID on next load)
    let mut header_map = HeaderMap::new();
    header_map.insert(
        SET_COOKIE,
        format!("mira_session={}; Path=/; HttpOnly; SameSite=Lax", session_id)
            .parse()
            .unwrap(),
    );

    (header_map, status, axum::Json(chat))
}
