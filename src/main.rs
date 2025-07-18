use axum::{
    extract::Json,
    routing::post,
    Router,
    response::IntoResponse,
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use std::env;
use serde_json::json;
use tracing::{info, error};

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
}

#[derive(Serialize)]
struct ChatResponse {
    output: String,
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/chat", post(chat_handler))
        .nest_service("/", tower_http::services::ServeDir::new("frontend"));

    let port = 8080;
    info!("Listening on http://0.0.0.0:{port}");

    axum::Server::bind(&format!("0.0.0.0:{port}").parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn chat_handler(Json(payload): Json<ChatRequest>) -> impl IntoResponse {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
    let client = reqwest::Client::new();
    let body = json!({
        "model": "gpt-4.1",
        "response_format": { "type": "json_object" },
        "messages": [
            {
                "role": "system",
                "content": "You are Mira, a warm, witty, irreverent AI friend. Reply ONLY as a JSON object {\"output\": \"...\"}. No boilerplate, no apologies. Use playful banter if appropriate."
            },
            {
                "role": "user",
                "content": payload.message
            }
        ]
    });

    let res = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await;

    match res {
        Ok(resp) => {
            let result: serde_json::Value = match resp.json().await {
                Ok(val) => val,
                Err(_) => return (StatusCode::BAD_GATEWAY, Json(ChatResponse { output: "OpenAI output parse error.".to_string() })),
            };

            let output = result["choices"][0]["message"]["content"].as_str().unwrap_or("Malformed OpenAI response.").to_string();

            // Parse output as JSON if possible, else just use string
            let output_json: Result<ChatResponse, _> = serde_json::from_str(&output);
            match output_json {
                Ok(chat) => (StatusCode::OK, Json(chat)),
                Err(_) => (StatusCode::OK, Json(ChatResponse { output }))
            }
        }
        Err(e) => {
            error!("OpenAI call failed: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ChatResponse { output: "API call failed.".to_string() }))
        }
    }
}
