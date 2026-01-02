// web/mcp_http.rs
// MCP over HTTP (Streamable HTTP transport)

use std::sync::Arc;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig,
    StreamableHttpService,
    session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;

use crate::mcp::MiraServer;
use crate::web::state::AppState;

/// Create the MCP HTTP service
pub fn create_mcp_service(
    state: AppState,
) -> StreamableHttpService<MiraServer, LocalSessionManager> {
    // Capture state for the factory closure
    let db = state.db.clone();
    let embeddings = state.embeddings.clone();
    let deepseek = state.deepseek.clone();
    let ws_tx = state.ws_tx.clone();
    let session_id = state.session_id.clone();

    // Service factory - creates a new MiraServer for each session
    let service_factory = move || {
        Ok(MiraServer::with_broadcaster(
            db.clone(),
            embeddings.clone(),
            deepseek.clone(),
            ws_tx.clone(),
            session_id.clone(),
        ))
    };

    // Session manager for managing MCP sessions
    let session_manager = Arc::new(LocalSessionManager::default());

    // Config for the HTTP transport
    let config = StreamableHttpServerConfig {
        sse_keep_alive: Some(std::time::Duration::from_secs(15)),
        stateful_mode: true,
        cancellation_token: CancellationToken::new(),
    };

    // Create and return the Streamable HTTP service
    StreamableHttpService::new(service_factory, session_manager, config)
}
