use axum::{
    routing::post,
    Router,
    extract::Extension,
};
use tokio::net::TcpListener;
use std::sync::Arc;
use tracing::info;

mod persona;
mod prompt;
mod llm;
mod session;
mod handlers;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    // Init session store and pool
    // Use a local path relative to the current directory
    let session_store = Arc::new(session::SessionStore::new("sqlite://mira.db").await?);

    let app = Router::new()
        .route("/chat", post(handlers::chat_handler))
        .nest_service("/", tower_http::services::ServeDir::new("frontend"))
        .layer(Extension(session_store.clone()));

    let port = 8080;
    let addr = format!("0.0.0.0:{port}");
    info!("Listening on http://{addr}");

    let listener = TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
    Ok(())
}
