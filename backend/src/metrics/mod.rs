// backend/src/metrics/mod.rs
// Prometheus metrics for Mira backend

use axum::{http::StatusCode, response::IntoResponse};
use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;
use tracing::info;

/// Global Prometheus handle for metrics rendering
static PROMETHEUS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Active WebSocket connections counter
static ACTIVE_CONNECTIONS: AtomicU64 = AtomicU64::new(0);

/// Initialize the Prometheus metrics exporter
pub fn init_metrics() {
    let handle = PrometheusBuilder::new()
        .install_recorder()
        .expect("Failed to install Prometheus recorder");

    PROMETHEUS_HANDLE
        .set(handle)
        .expect("Prometheus handle already initialized");

    info!("Prometheus metrics initialized");
}

/// GET /metrics - Prometheus metrics endpoint
pub async fn metrics_handler() -> impl IntoResponse {
    match PROMETHEUS_HANDLE.get() {
        Some(handle) => (StatusCode::OK, handle.render()),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Metrics not initialized".to_string(),
        ),
    }
}

/// Record a request (counter)
pub fn record_request(request_type: &str) {
    counter!("mira_requests_total", "type" => request_type.to_string()).increment(1);
}

/// Record request duration (histogram)
pub fn record_request_duration(request_type: &str, duration_seconds: f64) {
    histogram!("mira_request_duration_seconds", "type" => request_type.to_string())
        .record(duration_seconds);
}

/// Record an LLM API call
pub fn record_llm_call(model: &str, success: bool) {
    let status = if success { "success" } else { "error" };
    counter!("mira_llm_calls_total", "model" => model.to_string(), "status" => status)
        .increment(1);
}

/// Record LLM cache hit/miss
pub fn record_cache_result(hit: bool) {
    let result = if hit { "hit" } else { "miss" };
    counter!("mira_llm_cache_total", "result" => result).increment(1);
}

/// Update budget usage gauge
pub fn set_budget_used(daily_usd: f64, monthly_usd: f64) {
    gauge!("mira_budget_daily_used_usd").set(daily_usd);
    gauge!("mira_budget_monthly_used_usd").set(monthly_usd);
}

/// Track active WebSocket connections
pub fn connection_opened() {
    let count = ACTIVE_CONNECTIONS.fetch_add(1, Ordering::SeqCst) + 1;
    gauge!("mira_active_connections").set(count as f64);
}

/// Track closed WebSocket connections
pub fn connection_closed() {
    let count = ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::SeqCst) - 1;
    gauge!("mira_active_connections").set(count as f64);
}

/// Record tokens used in an LLM response
pub fn record_tokens(prompt_tokens: u64, completion_tokens: u64, reasoning_tokens: u64) {
    counter!("mira_llm_tokens_total", "type" => "prompt").increment(prompt_tokens);
    counter!("mira_llm_tokens_total", "type" => "completion").increment(completion_tokens);
    counter!("mira_llm_tokens_total", "type" => "reasoning").increment(reasoning_tokens);
}

/// Record tool execution
pub fn record_tool_execution(tool_name: &str, success: bool, duration_seconds: f64) {
    let status = if success { "success" } else { "error" };
    counter!("mira_tool_executions_total", "tool" => tool_name.to_string(), "status" => status)
        .increment(1);
    histogram!("mira_tool_execution_duration_seconds", "tool" => tool_name.to_string())
        .record(duration_seconds);
}

/// Helper for timing operations
pub struct RequestTimer {
    start: Instant,
    request_type: String,
}

impl RequestTimer {
    pub fn new(request_type: &str) -> Self {
        record_request(request_type);
        Self {
            start: Instant::now(),
            request_type: request_type.to_string(),
        }
    }
}

impl Drop for RequestTimer {
    fn drop(&mut self) {
        let duration = self.start.elapsed().as_secs_f64();
        record_request_duration(&self.request_type, duration);
    }
}
