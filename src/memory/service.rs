// src/memory/service.rs

//! Public API and orchestration for the memory service with message analysis.

use crate::memory::features::classification;
use crate::memory::features::embedding;
use crate::memory::features::message_analyzer;
use crate::memory::features::scoring;
use crate::memory::features::session;
use crate::memory::features::summarization;

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, debug};

use crate::config::CONFIG;
use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::EmbeddingHead;
use crate::memory::{
    sqlite::store::SqliteMemoryStore,
    qdrant::multi_store::QdrantMultiStore,
    types::MemoryEntry,
    recall::RecallContext,
    traits::MemoryStore,
};

// Re-export key types
pub use crate::memory::features::memory_types::{
    ScoredMemoryEntry, 
    MemoryServiceStats, 
    RoutingStats,
    SummaryRequest,
    SummaryType,
};

use classification::MessageClassifier;
use embedding::EmbeddingManager;
use message_analyzer::AnalysisService;
use scoring::MemoryScorer;
use session::SessionManager;
use summarization::SummarizationEngine;

/// Memory Service with complete analysis pipeline
pub struct MemoryService {
    // Core components
    llm_client: Arc<OpenAIClient>,
    sqlite_store: Arc<SqliteMemoryStore>,
    multi_store: Arc<QdrantMultiStore>,
    
    // Modular managers
    analysis_service: Arc<AnalysisService>,
    classifier: Arc<MessageClassifier>,
    embedding_mgr: Arc<EmbeddingManager>,
    scorer: Arc<MemoryScorer>,
    session_mgr: Arc<SessionManager>,
    summarization_engine: Arc<SummarizationEngine>,
}

impl MemoryService {
    /// Creates a new memory service with all modules initialized
    pub fn new(
        sqlite_store: Arc<SqliteMemoryStore>,
        multi_store: Arc<QdrantMultiStore>,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        info!("Initializing MemoryService with complete analysis pipeline");
        
        // Initialize all modules
        let analysis_service = Arc::new(AnalysisService::new(
            llm_client.clone(),
            sqlite_store.clone(),
        ));
        let classifier = Arc::new(MessageClassifier::new(llm_client.clone()));
        let embedding_mgr = Arc::new(EmbeddingManager::new(llm_client.clone()).expect("Failed to create embedding manager"));
        let scorer = Arc::new(MemoryScorer::new());
        let session_mgr = Arc::new(SessionManager::new());
        let summarization_engine = Arc::new(SummarizationEngine::new(llm_client.clone()));
        
        info!("All memory service modules initialized successfully");
        
        Self {
            llm_client,
            sqlite_store,
            multi_store,
            analysis_service,
            classifier,
            embedding_mgr,
            scorer,
            session_mgr,
            summarization_engine,
        }
    }
    
    // ===== PRIMARY PUBLIC API =====
    
    /// Saves a user message with analysis, classification and routing
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
        let entry = MemoryEntry::from_user_message(session_id.to_string(), content.to_string());
        
        // Save and process with analysis
        let entry_id = self.process_and_save_entry(entry, "user").await?;
        
        // Trigger async analysis for enrichment
        self.trigger_analysis(session_id).await;
        
        // Check for rolling summaries
        self.check_rolling_summaries(session_id, message_count).await?;
        
        Ok(entry_id)
    }
    
    /// Saves an assistant response with analysis, classification and routing
    pub async fn save_assistant_response(
        &self,
        session_id: &str,
        response: &crate::llm::types::ChatResponse,
    ) -> Result<String> {
        info!("Saving assistant response for session: {}", session_id);
        
        // Increment session counter
        let message_count = self.session_mgr.increment_counter(session_id).await;
        
        // Create memory entry from ChatResponse
        let mut entry = MemoryEntry::from_user_message(
            session_id.to_string(), 
            response.output.clone()
        );
        entry.role = "assistant".to_string();
        entry.salience = Some(response.salience as f32);
        entry.summary = Some(response.summary.clone());
        
        // Save and process with analysis
        let entry_id = self.process_and_save_entry(entry, "assistant").await?;
        
        // Trigger async analysis
        self.trigger_analysis(session_id).await;
        
        // Check for rolling summaries
        self.check_rolling_summaries(session_id, message_count).await?;
        
        Ok(entry_id)
    }
    
    /// Triggers background analysis of unanalyzed messages
    async fn trigger_analysis(&self, session_id: &str) {
        let analysis_service = self.analysis_service.clone();
        let session_id = session_id.to_string();
        
        // Spawn background task for analysis
        tokio::spawn(async move {
            if let Err(e) = analysis_service.process_pending_messages(&session_id).await {
                debug!("Background analysis error: {}", e);
            }
        });
    }
    
    /// Builds parallel recall context with multi-head search
    pub async fn parallel_recall_context(
        &self,
        session_id: &str,
        query_text: &str,
        recent_count: usize,
        semantic_count: usize,
    ) -> Result<RecallContext> {
        info!("Building parallel recall context for session: {}", session_id);
        
        // Parallel retrieval using tokio::join!
        let (embedding_result, recent_result) = tokio::join!(
            self.llm_client.get_embedding(query_text),
            self.sqlite_store.load_recent(session_id, recent_count)
        );
        
        let embedding = embedding_result?;
        let recent = recent_result?;
        
        // Multi-head search
        let k_per_head = std::cmp::max(10, semantic_count / 3);
        let multi_results = self.multi_store.search_all(session_id, &embedding, k_per_head).await?;
        
        // Score and rank results
        let now = chrono::Utc::now();
        let scored = self.scorer.score_entries(multi_results, &embedding, now);
        
        // Take top semantic results
        let semantic: Vec<MemoryEntry> = scored.into_iter()
            .take(semantic_count)
            .map(|s| s.entry)
            .collect();
        
        Ok(RecallContext {
            recent,
            semantic,
        })
    }
    
    /// Gets recent context including summaries
    pub async fn get_recent_context(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        self.sqlite_store.load_recent(session_id, limit).await
    }
    
    /// Creates an on-demand snapshot summary
    pub async fn create_snapshot_summary(
        &self,
        session_id: &str,
        max_tokens: Option<usize>,
    ) -> Result<String> {
        info!("Creating snapshot summary for session: {}", session_id);
        
        let messages = self.sqlite_store.load_recent(session_id, 50).await?;
        let summary_entry = self.summarization_engine
            .create_snapshot_summary(session_id, &messages, max_tokens)
            .await?;
        
        // Save the summary
        let saved = self.sqlite_store.save(&summary_entry).await?;
        
        // Embed and store in Summary head
        self.embed_and_store_summary(saved).await?;
        
        Ok(summary_entry.content)
    }
    
    /// Gets memory service statistics
    pub async fn get_stats(&self, session_id: &str) -> Result<MemoryServiceStats> {
        let recent = self.sqlite_store.load_recent(session_id, 100).await?;
        
        // For now, return basic stats
        Ok(MemoryServiceStats {
            total_messages: self.session_mgr.get_message_count(session_id).await,
            recent_messages: recent.len(),
            semantic_entries: 0,
            code_entries: 0,
            summary_entries: 0,
        })
    }
    
    /// Alias for get_stats for backward compatibility
    pub async fn get_service_stats(&self, session_id: &str) -> Result<MemoryServiceStats> {
        self.get_stats(session_id).await
    }
    
    /// Search for similar memories
    pub async fn search_similar(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let embedding = self.llm_client.get_embedding(query).await?;
        let results = self.multi_store.search_all(session_id, &embedding, limit).await?;
        
        let now = chrono::Utc::now();
        let scored = self.scorer.score_entries(results, &embedding, now);
        
        Ok(scored.into_iter()
            .take(limit)
            .map(|s| s.entry)
            .collect())
    }
    
    /// Trigger a rolling summary manually
    pub async fn trigger_rolling_summary(
        &self,
        session_id: &str,
        window_size: usize,
    ) -> Result<String> {
        let request = SummaryRequest {
            session_id: session_id.to_string(),
            window_size,
            summary_type: if window_size == 100 { 
                SummaryType::Rolling100 
            } else { 
                SummaryType::Rolling10 
            },
        };
        
        self.create_and_store_rolling_summary(request).await?;
        Ok(format!("Created {}-message rolling summary", window_size))
    }
    
    /// Trigger a snapshot summary
    pub async fn trigger_snapshot_summary(&self, session_id: &str) -> Result<String> {
        self.create_snapshot_summary(session_id, None).await
    }
    
    // ===== INTERNAL PROCESSING =====
    
    /// Processes and saves an entry with classification and routing
    async fn process_and_save_entry(
        &self,
        mut entry: MemoryEntry,
        role: &str,
    ) -> Result<String> {
        // Classify the content
        let classification = self.classifier.classify_message(&entry.content).await?;
        entry = entry.with_classification(classification.clone());
        
        // Save to SQLite
        let saved_entry = self.sqlite_store.save(&entry).await?;
        let entry_id = saved_entry.id.unwrap_or(0).to_string();
        
        // Determine routing
        let routing_decision = self.classifier
            .make_routing_decision(&entry.content, role, saved_entry.salience)
            .await?;
        
        if !routing_decision.should_embed {
            debug!("Skipping embedding: {}", 
                routing_decision.skip_reason.unwrap_or_default());
            return Ok(entry_id);
        }
        
        // Generate embeddings and store in appropriate heads
        self.generate_and_store_embeddings(&saved_entry, &routing_decision.heads).await?;
        
        info!("Processed entry {} -> {} heads", entry_id, routing_decision.heads.len());
        
        Ok(entry_id)
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
                chunk_entry.head = Some(head.as_str().to_string());
                
                self.multi_store.save(head, &chunk_entry).await?;
            }
            
            debug!("Stored {} chunks in {} collection", chunks.len(), head.as_str());
        }
        
        Ok(())
    }
    
    /// Checks and triggers rolling summaries if needed
    async fn check_rolling_summaries(
        &self,
        session_id: &str,
        message_count: usize,
    ) -> Result<()> {
        if !CONFIG.rolling_summaries_enabled() {
            return Ok(());
        }
        
        // Check if summaries should be triggered
        if let Some(summary_request) = self.summarization_engine
            .check_and_trigger_rolling_summaries(session_id, message_count)
            .await? 
        {
            // Create the summary
            self.create_and_store_rolling_summary(summary_request).await?;
        }
        
        Ok(())
    }
    
    /// Creates and stores a rolling summary
    async fn create_and_store_rolling_summary(
        &self,
        request: SummaryRequest,
    ) -> Result<()> {
        info!("Creating {}-message rolling summary for session {}", 
              request.window_size, request.session_id);
        
        // Load messages for summarization
        let messages = self.sqlite_store
            .load_recent(&request.session_id, request.window_size)
            .await?;
        
        if messages.len() < request.window_size / 2 {
            debug!("Not enough messages for summary");
            return Ok(());
        }
        
        // Generate summary
        let summary_entry = self.summarization_engine
            .create_rolling_summary(&messages, request.window_size)
            .await?;
        
        // Save to SQLite
        let saved = self.sqlite_store.save(&summary_entry).await?;
        
        // Embed and store in Summary head
        self.embed_and_store_summary(saved).await?;
        
        // Update session metadata
        self.session_mgr.increment_summary_count(&request.session_id).await;
        
        Ok(())
    }
    
    /// Embeds and stores a summary in the Summary collection
    async fn embed_and_store_summary(&self, summary: MemoryEntry) -> Result<()> {
        if !CONFIG.embed_heads.contains("summary") {
            return Ok(());
        }
        
        let embedding = self.llm_client.get_embedding(&summary.content).await?;
        
        let mut embedded_summary = summary;
        embedded_summary.embedding = Some(embedding);
        embedded_summary.head = Some("summary".to_string());
        
        self.multi_store.save(EmbeddingHead::Summary, &embedded_summary).await?;
        
        info!("Stored summary in Summary collection");
        
        Ok(())
    }
    
    /// Performs cleanup of old inactive sessions
    pub async fn cleanup_inactive_sessions(&self, max_age_hours: i64) -> Result<usize> {
        let cleaned = self.session_mgr.cleanup_inactive_sessions(max_age_hours).await;
        
        if cleaned > 0 {
            info!("Cleaned up {} inactive sessions", cleaned);
        }
        
        Ok(cleaned)
    }
}
