// src/services/context.rs
// Provides a service for building and retrieving conversation context

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug};
use crate::memory::RecallContext;
use crate::memory::MemoryService;
use crate::config::CONFIG;

/// Service responsible for constructing context for chat interactions
#[derive(Clone)]
pub struct ContextService {
    memory_service: Arc<MemoryService>,
}

impl ContextService {
    pub fn new(memory_service: Arc<MemoryService>) -> Self {
        info!("Initializing ContextService in robust mode");
        Self { 
            memory_service,
        }
    }
    
    /// Builds the RecallContext for a given session and user query
    pub async fn build_context_with_text(
        &self,
        session_id: &str,
        user_text: &str,
        _project_id: Option<&str>,
    ) -> Result<RecallContext> {
        debug!("Building context with text for session {}", session_id);
        
        info!("Using MemoryService parallel recall for session: {}", session_id);
        self.memory_service.parallel_recall_context(
            session_id,
            user_text,
            CONFIG.context_recent_messages,
            CONFIG.context_semantic_matches,
        ).await
    }
}
