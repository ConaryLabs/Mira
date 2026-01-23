// crates/mira-server/src/proxy/server.rs
// Axum HTTP server for the proxy

use crate::db::Database;
use crate::proxy::{Backend, BackendConfig, ProxyConfig, UsageRecord};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared state for the proxy server
#[derive(Clone)]
pub struct ProxyServer {
    /// Proxy configuration
    pub config: ProxyConfig,
    /// Initialized backends (keyed by config name)
    pub backends: Arc<HashMap<String, Backend>>,
    /// Currently active backend name
    pub active_backend: Arc<RwLock<Option<String>>>,
    /// Database for usage persistence (optional)
    db: Option<Arc<Database>>,
}

impl ProxyServer {
    /// Create a new proxy server from config
    pub fn new(config: ProxyConfig) -> Self {
        Self::with_db(config, None)
    }

    /// Create a new proxy server with database for usage tracking
    pub fn with_db(config: ProxyConfig, db: Option<Arc<Database>>) -> Self {
        // Initialize backends from config
        let mut backends = HashMap::new();
        for (name, backend_config) in &config.backends {
            if backend_config.is_usable() {
                backends.insert(name.clone(), Backend::new(backend_config.clone()));
            }
        }

        // Set default active backend
        let active_backend = config.default_backend.clone();

        Self {
            config,
            backends: Arc::new(backends),
            active_backend: Arc::new(RwLock::new(active_backend)),
            db,
        }
    }

    /// Get the currently active backend
    pub async fn get_active_backend(&self) -> Option<Backend> {
        let active = self.active_backend.read().await;
        active
            .as_ref()
            .and_then(|name| self.backends.get(name).cloned())
    }

    /// Set the active backend by name
    pub async fn set_active_backend(&self, name: &str) -> Result<(), String> {
        if !self.backends.contains_key(name) {
            return Err(format!("Backend '{}' not found or not usable", name));
        }
        let mut active = self.active_backend.write().await;
        *active = Some(name.to_string());
        Ok(())
    }

    /// Get a backend by name (with optional override via header)
    pub async fn get_backend(&self, override_name: Option<&str>) -> Option<Backend> {
        if let Some(name) = override_name {
            return self.backends.get(name).cloned();
        }
        self.get_active_backend().await
    }

    /// List all available backends
    pub fn list_backends(&self) -> Vec<(&String, &BackendConfig)> {
        self.backends
            .iter()
            .map(|(name, backend)| (name, &backend.config))
            .collect()
    }

    /// Record a usage entry (persisted to database if available)
    pub async fn record_usage(&self, record: UsageRecord) {
        if let Some(ref db) = self.db {
            // Persist to database
            if let Err(e) = db.insert_proxy_usage(&record) {
                tracing::error!("Failed to record usage: {}", e);
            }
        } else {
            // No database - just log
            tracing::debug!(
                backend = %record.backend_name,
                input = record.input_tokens,
                output = record.output_tokens,
                "Usage recorded (no database)"
            );
        }
    }

    /// Start the proxy server
    pub async fn run(self) -> anyhow::Result<()> {
        use crate::proxy::routes;

        let addr = format!("{}:{}", self.config.host, self.config.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        tracing::info!("Mira proxy listening on {}", addr);

        let app = routes::create_router(self);
        axum::serve(listener, app).await?;

        Ok(())
    }
}
