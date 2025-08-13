// src/services/memory.rs
// Phase 4: Memory service with GPT-5 Functions API integration

use std::sync::Arc;
use anyhow::Result;
use chrono::Utc;

use crate::llm::client::OpenAIClient;
use crate::llm::schema::{
    create_evaluation_request, 
    EvaluateMemoryResponse, 
    MiraStructuredReply,
    MemoryType as LLMMemoryType
};
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::types::{MemoryEntry, MemoryType};

pub struct EvaluationResult {
    pub salience: u8,
    pub tags: Vec<String>,
    pub summary: Option<String>,
    pub memory_type: LLMMemoryType,
}

#[derive(Clone)]
pub struct MemoryService {
    sqlite_store: Arc<SqliteMemoryStore>,
    qdrant_store: Arc<QdrantMemoryStore>,
    llm_client: Arc<OpenAIClient>,
}

impl MemoryService {
    pub fn new(
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
        llm_client: Arc<OpenAIClient>,
    ) -> Self {
        Self {
            sqlite_store,
            qdrant_store,
            llm_client,
        }
    }

    /// Save a user message to memory stores
    pub async fn save_user_message(
        &self,
        session_id: &str,
        content: &str,
        _project_id: Option<&str>,
    ) -> Result<()> {
        eprintln!("💬 Saving user message to memory...");

        // Determine if we should embed this message
        let want_embed = self.should_embed_text("user", content, None);
        let mut embedding: Option<Vec<f32>> = None;

        if want_embed {
            eprintln!("🧲 Embedding user message...");
            match self.llm_client.get_embedding(content).await {
                Ok(vec) => {
                    // Check for near-duplicates to avoid redundant storage
                    if self.is_near_duplicate(session_id, "user", &vec).await.unwrap_or(false) {
                        eprintln!("🔁 Near-duplicate detected, skipping Qdrant storage");
                        embedding = None;
                    } else {
                        embedding = Some(vec);
                    }
                }
                Err(e) => eprintln!("⚠️ Failed to get embedding: {:?}", e),
            }
        }

        // Create memory entry
        let entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "user".to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            embedding: embedding.clone(),
            salience: Some(5.0), // Default salience for user messages
            tags: Some(vec!["user_message".to_string()]),
            summary: None,
            memory_type: Some(MemoryType::Other),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };

        // Always save to SQLite
        self.sqlite_store.save(&entry).await?;
        eprintln!("✅ User message saved to SQLite");

        // Save to Qdrant if embedding exists
        if entry.embedding.is_some() {
            match self.qdrant_store.save(&entry).await {
                Ok(_) => eprintln!("✅ User message saved to Qdrant"),
                Err(e) => eprintln!("⚠️ Failed to save to Qdrant (non-fatal): {:?}", e),
            }
        }

        Ok(())
    }

    /// Evaluate and save an assistant response using GPT-5 Functions API
    pub async fn evaluate_and_save_response(
        &self,
        session_id: &str,
        response: &MiraStructuredReply,
        _project_id: Option<&str>,
    ) -> Result<EvaluationResult> {
        eprintln!("🧠 Evaluating response for memory importance using Functions API...");

        // Create evaluation request with the new helper function
        let eval_request = create_evaluation_request(response.output.clone());

        // Call GPT-5 with Functions API for evaluation
        let evaluation = match self.llm_client.evaluate_memory(eval_request).await {
            Ok(eval) => {
                eprintln!("✅ Memory evaluation successful via Functions API");
                eval
            }
            Err(e) => {
                eprintln!("❌ Memory evaluation failed: {:?}, using fallback defaults", e);
                // Fallback evaluation based on response characteristics
                EvaluateMemoryResponse {
                    salience: response.salience.min(10).max(1),
                    tags: response.tags.clone(),
                    memory_type: self.infer_memory_type(&response.memory_type),
                    summary: response.summary.clone(),
                }
            }
        };

        eprintln!("📊 Memory evaluation results:");
        eprintln!("   Salience: {}/10", evaluation.salience);
        eprintln!("   Type: {:?}", evaluation.memory_type);
        eprintln!("   Tags: {:?}", evaluation.tags);
        if let Some(ref summary) = evaluation.summary {
            eprintln!("   Summary: {}", summary);
        }

        // Determine if we should embed this response
        let want_embed = self.should_embed_text("assistant", &response.output, Some(evaluation.salience));
        let mut embedding: Option<Vec<f32>> = None;

        if want_embed {
            eprintln!("🧲 Embedding assistant response...");
            match self.llm_client.get_embedding(&response.output).await {
                Ok(vec) => {
                    if self.is_near_duplicate(session_id, "assistant", &vec).await.unwrap_or(false) {
                        eprintln!("🔁 Near-duplicate detected, skipping Qdrant storage");
                        embedding = None;
                    } else {
                        embedding = Some(vec);
                    }
                }
                Err(e) => eprintln!("⚠️ Failed to get embedding: {:?}", e),
            }
        } else {
            eprintln!("📉 Skipping embedding (low salience or short message)");
        }

        // Create memory entry with evaluation results
        let entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "assistant".to_string(),
            content: response.output.clone(),
            timestamp: Utc::now(),
            embedding: embedding.clone(),
            salience: Some(evaluation.salience as f32),
            tags: Some(evaluation.tags.clone()),
            summary: evaluation.summary.clone(),
            memory_type: Some(self.convert_memory_type(&evaluation.memory_type)),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };

        // Always save to SQLite
        self.sqlite_store.save(&entry).await?;
        eprintln!("✅ Assistant response saved to SQLite");

        // Save to Qdrant if embedding exists and salience is high enough
        if entry.embedding.is_some() && evaluation.salience >= 6 {
            match self.qdrant_store.save(&entry).await {
                Ok(_) => eprintln!("✅ Assistant response saved to Qdrant (high salience)"),
                Err(e) => eprintln!("⚠️ Failed to save to Qdrant (non-fatal): {:?}", e),
            }
        }

        Ok(EvaluationResult {
            salience: evaluation.salience,
            tags: evaluation.tags,
            summary: evaluation.summary,
            memory_type: evaluation.memory_type,
        })
    }

    /// Store a message directly (used by ChatService fallback)
    pub async fn store_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        if role == "user" {
            self.save_user_message(session_id, content, project_id).await
        } else {
            // For assistant messages without structured reply, create a default one
            let reply = MiraStructuredReply {
                output: content.to_string(),
                persona: "assistant".to_string(),
                mood: "neutral".to_string(),
                salience: 5,
                summary: None,
                memory_type: "other".to_string(),
                tags: vec![],
                intent: "response".to_string(),
                monologue: None,
                reasoning_summary: None,
                aside_intensity: None,
            };
            self.evaluate_and_save_response(session_id, &reply, project_id).await?;
            Ok(())
        }
    }

    /// Check if an embedding is a near-duplicate of recent messages
    async fn is_near_duplicate(
        &self,
        session_id: &str,
        role: &str,
        embedding: &[f32],
    ) -> Result<bool> {
        // Simple cosine similarity check against recent messages
        // This could be optimized with a proper vector index
        let recent = self.sqlite_store
            .get_recent_messages(session_id, 5)
            .await?;
        
        for msg in recent {
            if msg.role == role {
                if let Some(ref stored_embedding) = msg.embedding {
                    let similarity = self.cosine_similarity(embedding, stored_embedding);
                    if similarity > 0.95 {
                        return Ok(true);
                    }
                }
            }
        }
        
        Ok(false)
    }

    /// Calculate cosine similarity between two vectors
    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }
        
        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        
        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }
        
        dot_product / (norm_a * norm_b)
    }

    /// Determine if text should be embedded based on role and characteristics
    fn should_embed_text(&self, role: &str, content: &str, salience: Option<u8>) -> bool {
        // Skip very short messages
        if content.len() < 20 {
            return false;
        }
        
        // Always embed high-salience messages
        if let Some(s) = salience {
            if s >= 7 {
                return true;
            }
        }
        
        // User messages: embed most things except very short
        if role == "user" {
            return content.len() >= 30;
        }
        
        // Assistant messages: be more selective
        content.len() >= 50 && salience.unwrap_or(5) >= 5
    }

    /// Convert from LLM MemoryType to storage MemoryType
    fn convert_memory_type(&self, llm_type: &LLMMemoryType) -> MemoryType {
        match llm_type {
            LLMMemoryType::Feeling => MemoryType::Feeling,
            LLMMemoryType::Fact => MemoryType::Fact,
            LLMMemoryType::Joke => MemoryType::Joke,
            LLMMemoryType::Promise => MemoryType::Promise,
            LLMMemoryType::Event => MemoryType::Event,
            LLMMemoryType::Other => MemoryType::Other,
        }
    }

    /// Infer memory type from a string representation
    fn infer_memory_type(&self, type_str: &str) -> LLMMemoryType {
        match type_str.to_lowercase().as_str() {
            "feeling" => LLMMemoryType::Feeling,
            "fact" => LLMMemoryType::Fact,
            "joke" => LLMMemoryType::Joke,
            "promise" => LLMMemoryType::Promise,
            "event" => LLMMemoryType::Event,
            _ => LLMMemoryType::Other,
        }
    }

    /// Get recent messages for a session (public interface)
    pub async fn get_recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        self.sqlite_store.get_recent_messages(session_id, limit).await
    }

    /// Search for semantically similar memories
    pub async fn search_similar(
        &self,
        session_id: &str,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        self.qdrant_store.search_similar(session_id, embedding, limit).await
    }
}
