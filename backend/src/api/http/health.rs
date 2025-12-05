// backend/src/api/http/health.rs
//
// Health check and readiness endpoints for load balancers and Kubernetes probes.

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use std::sync::Arc;

use crate::state::AppState;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    db: &'static str,
    qdrant: &'static str,
}

#[derive(Serialize)]
struct ReadyResponse {
    status: &'static str,
    migrations: &'static str,
}

/// Health check endpoint for load balancers.
/// Returns 200 if all services are healthy, 503 if any service is down.
///
/// GET /health
pub async fn health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Check database connectivity
    let db_ok = sqlx::query("SELECT 1")
        .fetch_one(&state.sqlite_pool)
        .await
        .is_ok();

    // Check Qdrant connectivity via memory_service multi_store
    let multi_store = state.memory_service.get_multi_store();
    let qdrant_ok = multi_store.health_check().await.unwrap_or(false);

    let response = HealthResponse {
        status: if db_ok && qdrant_ok { "healthy" } else { "unhealthy" },
        db: if db_ok { "ok" } else { "error" },
        qdrant: if qdrant_ok { "ok" } else { "error" },
    };

    if db_ok && qdrant_ok {
        (StatusCode::OK, Json(response))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(response))
    }
}

/// Readiness probe for Kubernetes startup.
/// Returns 200 if the application is ready to accept traffic.
///
/// GET /ready
pub async fn readiness_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Check that migrations have been applied by querying a known table
    let migrations_ok = sqlx::query("SELECT 1 FROM users LIMIT 1")
        .fetch_optional(&state.sqlite_pool)
        .await
        .is_ok();

    let response = ReadyResponse {
        status: if migrations_ok { "ready" } else { "not_ready" },
        migrations: if migrations_ok { "applied" } else { "pending" },
    };

    if migrations_ok {
        (StatusCode::OK, Json(response))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(response))
    }
}

/// Liveness probe - simple ping to verify the server is running.
///
/// GET /live
pub async fn liveness_check() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({"status": "alive"})))
}
