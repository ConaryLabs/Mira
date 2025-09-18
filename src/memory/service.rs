// src/memory/service.rs

//! Public API and orchestration for the memory service with unified message pipeline.

use crate::memory::features::embedding;
use crate::memory::features::message_pipeline;  // NEW - replaces analyzer + classifier
use crate::memory::features::recall_engine;
use crate::memory::features::session;
use crate::memory::features::summarization;
use crate::memory::cache::recent::RecentCache;  // NEW

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug};

use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::EmbeddingHead;
use crate::memory::{
    storage::sqlite::store::SqliteMemoryStore,
    storage::qdrant::multi_store::QdrantMultiStore,
    core::types::MemoryEntry,
    core::traits::MemoryStore,
};

// Re-export key types
pub use crate::memory::features::memory_types::{
    ScoredMemoryEntry, 
    MemoryServiceStats, 
    RoutingStats,
};

use embedding::EmbeddingManager;
use message_pipeline::MessagePipeline;  // FIXED - removed unused UnifiedAnalysis
use recall_engine::{RecallEngine, RecallContext, RecallConfig, SearchMode};
use session::SessionManager;
use summarization::SummarizationEngine;

/// Memory Service with unified analysis pipeline
pub struct MemoryService {
    // Core components
    llm_client: Arc<OpenAIClient>,
    sqlite_store: Arc<SqliteMemoryStore>,
    multi_store: Arc<QdrantMultiStore>,
    recent_cache: Option<Arc<RecentCache>>,  // NEW
    
    // Modular managers
    message_pipeline: Arc<MessagePipeline>,  // NEW - replaces analyzer + classifier
    embedding_mgr: Arc<EmbeddingManager>,
    recall_engine: Arc<RecallEngine>,
    session_mgr: Arc<SessionManager>,
    pub summarization_engine: Arc<SummarizationEngine>,
}

impl MemoryService {
    /// Creates a new memory service with all modules initialized
    pub fn new(
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        Self::new_with_cache(sqlite_store, multi_store, llm_client, None)
    }
    
    /// Creates a new memory service with optional cache support
    pub fn new_with_cache(
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
        llm_client: Arc<OpenAIClient>,
        recent_cache: Option<Arc<RecentCache>>,  // NEW parameter
    ) -> Self {
        info!("Initializing MemoryService with unified message pipeline");
        
        if recent_cache.is_some() {
            info!("Recent cache enabled for instant recall");
        } else {
            info!("Recent cache disabled - using direct SQLite queries");
        }
        
        // Initialize unified message pipeline (replaces analyzer + classifier)
        let message_pipeline = Arc::new(MessagePipeline::new(
            llm_client.clone(),
            sqlite_store.clone(),
        ));
        
        let embedding_mgr = Arc::new(EmbeddingManager::new(llm_client.clone())
            .expect("Failed to create embedding manager"));
        
        let recall_engine = Arc::new(RecallEngine::new(
            llm_client.clone(),
            sqlite_store.clone(),
            multi_store.clone(),
        ));
        
        let session_mgr = Arc::new(SessionManager::new());
        
        let summarization_engine = Arc::new(SummarizationEngine::new(
            llm_client.clone(),
            sqlite_store.clone(),
            multi_store.clone(),
        ));
        
        info!("All memory service modules initialized successfully");
        
        Self {
            llm_client,
            sqlite_store,
            multi_store,
            recent_cache,
            message_pipeline,
            embedding_mgr,
            recall_engine,
            session_mgr,
            summarization_engine,
        }
    }
    
    // ===== PRIMARY PUBLIC API =====
    
    /// Saves a user message with unified analysis and routing
    pub async fn save_user_message(
        &self,
        session_id: &str,
        content: &str,
        _project_id: Option<&str>,
    ) -> Result<String> {
        info!("Saving user message for session: {}", session_id);
        
        // Increment session counter
        let message_count = self.session_mgr.increment_counter(session_id).await;
        debug!("Session {} now has {} messages", session_id, message_count);
        
        // Create memory entry
        let entry = MemoryEntry::user_message(session_id.to_string(), content.to_string());
        
        // Process through unified pipeline
        let entry_id = self.process_and_save_entry(entry.clone(), "user").await?;
        
        // Update cache if enabled (NEW)
        if let Some(cache) = &self.recent_cache {
            let mut cached_entry = entry;
            cached_entry.id = Some(entry_id.parse::<i64>().unwrap_or(0));
            cache.add(session_id, cached_entry).await;
            debug!("Updated recent cache for session {}", session_id);
        }
        
        // Trigger async analysis for any remaining unprocessed messages
        self.trigger_background_processing(session_id).await;
        
        // Check for rolling summaries
        self.summarization_engine
            .check_and_process_summaries(session_id, message_count)
            .await?;
        
        Ok(entry_id)
    }
    
    /// Saves an assistant response with unified analysis and routing
    pub async fn save_assistant_response(
        &self,
        session_id: &str,
        response: &crate::llm::types::ChatResponse,
    ) -> Result<String> {
        info!("Saving assistant response for session: {}", session_id);
        
        // Increment session counter
        let message_count = self.session_mgr.increment_counter(session_id).await;
        
        // Create memory entry from ChatResponse
        let mut entry = MemoryEntry::assistant_message(
            session_id.to_string(), 
            response.output.clone()
        );
        entry.salience = Some(response.salience as f32);
        entry.summary = Some(response.summary.clone());
        
        // Process through unified pipeline
        let entry_id = self.process_and_save_entry(entry.clone(), "assistant").await?;
        
        // Update cache if enabled (NEW)
        if let Some(cache) = &self.recent_cache {
            entry.id = Some(entry_id.parse::<i64>().unwrap_or(0));
            cache.add(session_id, entry).await;
            debug!("Updated recent cache for session {}", session_id);
        }
        
        // Trigger async analysis
        self.trigger_background_processing(session_id).await;
        
        // Check for rolling summaries
        self.summarization_engine
            .check_and_process_summaries(session_id, message_count)
            .await?;
        
        Ok(entry_id)
    }
    
    // ... [All other public methods remain the same - delegating to engines] ...
    
    /// Creates a snapshot summary - DELEGATES TO ENGINE
    pub async fn create_snapshot_summary(
        &self,
        session_id: &str,
        max_tokens: Option<usize>,
    ) -> Result<String> {
        self.summarization_engine
            .create_snapshot_summary(session_id, max_tokens)
            .await
    }
    
    /// Creates a rolling summary - DELEGATES TO ENGINE
    pub async fn create_rolling_summary(
        &self,
        session_id: &str,
        window_size: usize,
    ) -> Result<String> {
        self.summarization_engine
            .create_rolling_summary(session_id, window_size)
            .await
    }
    
    /// Builds parallel recall context - DELEGATES TO ENGINE
    pub async fn parallel_recall_context(
        &self,
        session_id: &str,
        query_text: &str,
        recent_count: usize,
        semantic_count: usize,
    ) -> Result<RecallContext> {
        let config = RecallConfig {
            recent_count,
            semantic_count,
            ..Default::default()
        };
        
        // Try cache first for recent memories (NEW)
        let mut context = self.recall_engine
            .build_recall_context(session_id, query_text, Some(config))
            .await?;
        
        // Optimize with cache if available
        if let Some(cache) = &self.recent_cache {
            if let Some(cached_recent) = cache.get_recent(session_id, recent_count).await {
                context.recent = cached_recent;  // FIXED - was recent_memories
                debug!("Used cached recent memories for recall context");
            }
        }
        
        Ok(context)
    }
    
    /// Gets recent context - with cache support
    pub async fn get_recent_context(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        // Try cache first (NEW)
        if let Some(cache) = &self.recent_cache {
            if let Some(cached_entries) = cache.get_recent(session_id, limit).await {
                debug!("Cache HIT: Retrieved {} recent entries for session {}", 
                    cached_entries.len(), session_id);
                return Ok(cached_entries);
            }
            debug!("Cache MISS: Falling back to SQLite for session {}", session_id);
        }
        
        // Fallback to recall engine
        let results = self.recall_engine
            .search(session_id, SearchMode::Recent { limit })
            .await?;
        
        let entries: Vec<MemoryEntry> = results.into_iter().map(|s| s.entry).collect();
        
        // Populate cache for next time if we got results
        if let Some(cache) = &self.recent_cache {
            if !entries.is_empty() {
                // Preload more entries for better cache hit rate
                let preload_entries = self.sqlite_store
                    .load_recent(session_id, limit * 2)
                    .await
                    .unwrap_or_else(|_| entries.clone());
                
                cache.preload(session_id, preload_entries).await;
                debug!("Preloaded {} entries into cache for session {}", 
                    entries.len(), session_id);
            }
        }
        
        Ok(entries)
    }
    
    /// Search for similar memories - DELEGATES TO ENGINE
    pub async fn search_similar(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let results = self.recall_engine
            .search(session_id, SearchMode::Semantic { 
                query: query.to_string(), 
                limit 
            })
            .await?;
        
        Ok(results.into_iter().map(|s| s.entry).collect())
    }
    
    /// Gets memory service statistics
    pub async fn get_stats(&self, session_id: &str) -> Result<MemoryServiceStats> {
        let recent = if let Some(cache) = &self.recent_cache {
            // Try cache first for stats
            cache.get_recent(session_id, 100).await
                .unwrap_or_else(|| vec![])
        } else {
            self.sqlite_store.load_recent(session_id, 100).await?
        };
        
        Ok(MemoryServiceStats {
            total_messages: self.session_mgr.get_message_count(session_id).await,
            recent_messages: recent.len(),
            semantic_entries: 0,
            code_entries: 0,
            summary_entries: 0,
        })
    }
    
    /// Performs cleanup of old inactive sessions
    pub async fn cleanup_inactive_sessions(&self, max_age_hours: i64) -> Result<usize> {
        let cleaned = self.session_mgr.cleanup_inactive_sessions(max_age_hours).await;
        
        if cleaned > 0 {
            info!("Cleaned up {} inactive sessions", cleaned);
        }
        
        Ok(cleaned)
    }
    
    // ===== CACHE MANAGEMENT (NEW) =====
    
    /// Clear cache for a session
    pub async fn invalidate_session_cache(&self, session_id: &str) {
        if let Some(cache) = &self.recent_cache {
            cache.invalidate(session_id).await;
            info!("Invalidated cache for session {}", session_id);
        }
    }
    
    /// Get cache statistics
    pub async fn get_cache_stats(&self) -> Option<crate::memory::cache::recent::CacheStats> {
        if let Some(cache) = &self.recent_cache {
            Some(cache.get_stats().await)
        } else {
            None
        }
    }
    
    /// Preload cache for a session
    pub async fn preload_session_cache(&self, session_id: &str, message_count: usize) -> Result<()> {
        if let Some(cache) = &self.recent_cache {
            let entries = self.sqlite_store.load_recent(session_id, message_count).await?;
            if !entries.is_empty() {
                cache.preload(session_id, entries).await;
                info!("Preloaded {} messages into cache for session {}", message_count, session_id);
            }
        }
        Ok(())
    }
    
    // ===== INTERNAL PROCESSING =====
    
    /// Processes and saves an entry with unified analysis and routing
    async fn process_and_save_entry(
        &self,
        mut entry: MemoryEntry,
        role: &str,
    ) -> Result<String> {
        // UNIFIED ANALYSIS - single LLM call for everything!
        let analysis = self.message_pipeline
            .analyze_message(&entry.content, role, None)
            .await?;
        
        // Update entry with ALL analysis results
        entry.salience = Some(analysis.salience);
        entry.topics = Some(analysis.topics.clone());
        entry.contains_code = Some(analysis.is_code);
        entry.programming_lang = analysis.programming_lang.clone();
        // Additional fields from unified analysis
        entry.summary = entry.summary.or(analysis.summary.clone());
        
        // Save to SQLite
        let saved_entry = self.sqlite_store.save(&entry).await?;
        let entry_id = saved_entry.id.unwrap_or(0).to_string();
        
        // Check routing decision from analysis
        if !analysis.routing.should_embed {
            debug!("Skipping embedding: {}", 
                analysis.routing.skip_reason.unwrap_or_default());
            return Ok(entry_id);
        }
        
        // Generate embeddings and store in appropriate heads
        self.generate_and_store_embeddings(
            &saved_entry, 
            &analysis.routing.embedding_heads
        ).await?;
        
        info!("Processed entry {} -> {} heads", 
            entry_id, 
            analysis.routing.embedding_heads.len()
        );
        
        Ok(entry_id)
    }
    
    /// Triggers background processing of unanalyzed messages
    async fn trigger_background_processing(&self, session_id: &str) {
        let pipeline = self.message_pipeline.clone();
        let session_id = session_id.to_string();
        
        // Spawn background task for processing pending messages
        tokio::spawn(async move {
            if let Err(e) = pipeline.process_pending_messages(&session_id).await {
                debug!("Background processing error: {}", e);
            }
        });
    }
    
    /// Generates embeddings and stores in vector collections
    async fn generate_and_store_embeddings(
        &self,
        entry: &MemoryEntry,
        heads: &[EmbeddingHead],
    ) -> Result<()> {
        // Use batch embedding optimization
        let embeddings = self.embedding_mgr
            .generate_embeddings_for_heads(entry, heads)
            .await?;
        
        // Store in each head's collection
        for (head, chunks, chunk_embeddings) in embeddings {
            for (chunk_text, embedding) in chunks.iter().zip(chunk_embeddings.iter()) {
                let mut chunk_entry = entry.clone();
                chunk_entry.content = chunk_text.clone();
                chunk_entry.embedding = Some(embedding.clone());
                chunk_entry.embedding_heads = Some(vec![head.as_str().to_string()]);
                
                self.multi_store.save(head, &chunk_entry).await?;
            }
            
            debug!("Stored {} chunks in {} collection", chunks.len(), head.as_str());
        }
        
        Ok(())
    }
}
