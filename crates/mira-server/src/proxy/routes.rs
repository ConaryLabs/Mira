// crates/mira-server/src/proxy/routes.rs
// HTTP route handlers for the proxy

use axum::{
    Router,
    body::Body,
    routing::{get, post},
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    Json,
};
use futures::StreamExt;
use serde::Serialize;
use serde_json::Value;

use crate::proxy::ProxyServer;

/// Header name for backend override
const X_BACKEND_HEADER: &str = "x-mira-backend";

/// Create the axum router with all proxy routes
pub fn create_router(server: ProxyServer) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/v1/backends", get(list_backends))
        .route("/v1/messages", post(proxy_messages))
        .with_state(server)
}

/// Health check endpoint
async fn health_check() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
}

/// List available backends
async fn list_backends(State(server): State<ProxyServer>) -> impl IntoResponse {
    let active = server.active_backend.read().await.clone();
    let backends: Vec<BackendInfo> = server
        .list_backends()
        .into_iter()
        .map(|(name, config)| BackendInfo {
            name: name.clone(),
            display_name: config.name.clone(),
            base_url: config.base_url.clone(),
            active: active.as_ref() == Some(name),
        })
        .collect();

    Json(BackendsResponse { backends })
}

#[derive(Serialize)]
struct BackendInfo {
    name: String,
    display_name: String,
    base_url: String,
    active: bool,
}

#[derive(Serialize)]
struct BackendsResponse {
    backends: Vec<BackendInfo>,
}

/// Proxy /v1/messages to the selected backend
async fn proxy_messages(
    State(server): State<ProxyServer>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ProxyError> {
    // Check for backend override header
    let backend_override = headers
        .get(X_BACKEND_HEADER)
        .and_then(|v| v.to_str().ok());

    // Get the appropriate backend
    let backend = server
        .get_backend(backend_override)
        .await
        .ok_or(ProxyError::NoBackend)?;

    // Get API key
    let api_key = backend
        .config
        .get_api_key()
        .ok_or(ProxyError::NoApiKey)?;

    // Check if streaming is requested
    let is_streaming = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    // Build the target URL
    let target_url = format!("{}/v1/messages", backend.config.base_url);

    // Forward the request
    let response = backend
        .client
        .post(&target_url)
        .header("content-type", "application/json")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .map_err(|e| ProxyError::RequestFailed(e.to_string()))?;

    let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::OK);

    if is_streaming {
        // Stream the response body through
        let stream = response.bytes_stream().map(|result| {
            result.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
        });

        let body = Body::from_stream(stream);

        Ok(Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::CONNECTION, "keep-alive")
            .body(body)
            .unwrap())
    } else {
        // Non-streaming: parse and return JSON
        let response_body: Value = response
            .json()
            .await
            .map_err(|e| ProxyError::InvalidResponse(e.to_string()))?;

        Ok((status, Json(response_body)).into_response())
    }
}

/// Proxy error types
#[derive(Debug)]
enum ProxyError {
    NoBackend,
    NoApiKey,
    RequestFailed(String),
    InvalidResponse(String),
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ProxyError::NoBackend => (
                StatusCode::SERVICE_UNAVAILABLE,
                "No backend configured or available",
            ),
            ProxyError::NoApiKey => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Backend API key not configured",
            ),
            ProxyError::RequestFailed(ref e) => (
                StatusCode::BAD_GATEWAY,
                e.as_str(),
            ),
            ProxyError::InvalidResponse(ref e) => (
                StatusCode::BAD_GATEWAY,
                e.as_str(),
            ),
        };

        let body = serde_json::json!({
            "type": "error",
            "error": {
                "type": "proxy_error",
                "message": message
            }
        });

        (status, Json(body)).into_response()
    }
}
