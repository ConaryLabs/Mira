// src/llm/chat_service/context.rs

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, warn};
use crate::config::CONFIG;
use crate::memory::recall::RecallContext;
use super::config::ChatConfig;
use crate::memory::MemoryService;

#[derive(Clone)]
pub struct ContextBuilder {
    memory_service: Arc<MemoryService>,
    config: ChatConfig,
}

#[derive(Debug, Clone)]
pub struct ContextStats {
    pub total_messages: usize,
    pub recent_messages: usize,
    pub semantic_matches: usize,
    pub rolling_summaries: usize,
    pub compression_ratio: f64,
}

impl ContextBuilder {
    pub fn new(memory_service: Arc<MemoryService>, config: ChatConfig) -> Self {
        info!("ContextBuilder initialized with robust MemoryService");
        Self {
            memory_service,
            config,
        }
    }
    
    pub async fn build_context_with_fallbacks(
        &self,
        session_id: &str,
        user_text: &str,
    ) -> Result<RecallContext> {
        let recent_count = self.config.history_message_cap();
        let semantic_count = if self.config.enable_vector_search() {
            self.config.max_vector_search_results()
        } else {
            0
        };
        
        let context_result = self
            .memory_service
            .parallel_recall_context(session_id, user_text, recent_count, semantic_count)
            .await;
            
        match context_result {
            Ok(context) => Ok(context),
            Err(e) => {
                warn!("Failed to build context with parallel recall: {}. Falling back to minimal context.", e);
                self.build_minimal_context(session_id).await
            }
        }
    }
    
    pub async fn build_minimal_context(&self, session_id: &str) -> Result<RecallContext> {
        info!("Building minimal context (recent messages only) for session: {}", session_id);
        
        let recent = self
            .memory_service
            .get_recent_context(session_id, self.config.history_message_cap())
            .await?;
            
        Ok(RecallContext::new(recent, Vec::new()))
    }
    
    pub async fn get_context_stats(&self, session_id: &str) -> Result<ContextStats> {
        let memory_stats = self.memory_service.get_service_stats(session_id).await?;
        
        Ok(ContextStats {
            total_messages: memory_stats.total_messages,
            recent_messages: memory_stats.recent_messages,
            semantic_matches: memory_stats.semantic_entries,
            rolling_summaries: if CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100 {
                memory_stats.total_messages / 10
            } else {
                0
            },
            compression_ratio: if memory_stats.total_messages > 0 {
                (memory_stats.semantic_entries as f64) / (memory_stats.total_messages as f64)
            } else {
                0.0
            },
        })
    }
    
    pub fn can_use_vector_search(&self) -> bool {
        self.config.enable_vector_search()
    }
    
    pub fn config(&self) -> &ChatConfig {
        &self.config
    }
}
