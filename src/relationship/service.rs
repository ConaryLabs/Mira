// src/relationship/service.rs

use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::info;

use crate::relationship::{
    storage::RelationshipStorage,
    pattern_engine::PatternEngine,
    context_loader::ContextLoader,
};

/// Main relationship service - coordinates all relationship functionality
pub struct RelationshipService {
    pub storage: Arc<RelationshipStorage>,
    pub pattern_engine: Arc<PatternEngine>,
    pub context_loader: Arc<ContextLoader>,
}

impl RelationshipService {
    /// Create a new relationship service
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        info!("Initializing RelationshipService");

        let storage = Arc::new(RelationshipStorage::new(pool));
        let pattern_engine = Arc::new(PatternEngine::new(storage.clone()));
        let context_loader = Arc::new(ContextLoader::new(
            storage.clone(),
            pattern_engine.clone(),
        ));

        Self {
            storage,
            pattern_engine,
            context_loader,
        }
    }

    /// Get storage layer
    pub fn storage(&self) -> Arc<RelationshipStorage> {
        self.storage.clone()
    }

    /// Get pattern engine
    pub fn pattern_engine(&self) -> Arc<PatternEngine> {
        self.pattern_engine.clone()
    }

    /// Get context loader
    pub fn context_loader(&self) -> Arc<ContextLoader> {
        self.context_loader.clone()
    }
}
