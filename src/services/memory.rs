// src/services/memory.rs

use std::sync::Arc;
use anyhow::Result;
use chrono::Utc;
use crate::llm::OpenAIClient;
use crate::llm::schema::{EvaluateMemoryRequest, MiraStructuredReply, EvaluateMemoryResponse, function_schema};
use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::traits::MemoryStore;
use crate::memory::types::{MemoryEntry, MemoryType};

#[derive(Clone)]
pub struct MemoryService {
    sqlite_store: Arc<SqliteMemoryStore>,
    qdrant_store: Arc<QdrantMemoryStore>,
    llm_client: Arc<OpenAIClient>,
}

#[derive(Debug)]
pub struct EvaluationResult {
    pub salience: u8,
    pub tags: Vec<String>,
    pub summary: Option<String>,
    pub memory_type: crate::llm::schema::MemoryType,
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
    
    pub async fn save_user_message(
        &self,
        session_id: &str,
        content: &str,
        embedding: Option<Vec<f32>>,
        _project_id: Option<&str>,
    ) -> Result<()> {
        eprintln!("💾 Saving user message to memory stores...");
        
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
            memory_type: None,
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };
        
        // Always save to SQLite
        self.sqlite_store.save(&entry).await?;
        eprintln!("✅ User message saved to SQLite");
        
        // Save to Qdrant if embedding exists
        if embedding.is_some() {
            match self.qdrant_store.save(&entry).await {
                Ok(_) => eprintln!("✅ User message saved to Qdrant"),
                Err(e) => eprintln!("⚠️ Failed to save to Qdrant (non-fatal): {:?}", e),
            }
        }
        
        Ok(())
    }
    
    pub async fn evaluate_and_save_response(
        &self,
        session_id: &str,
        response: &MiraStructuredReply,
        _project_id: Option<&str>,
    ) -> Result<EvaluationResult> {
        eprintln!("🧠 Evaluating Mira's response for memory importance...");
        
        // Evaluate memory importance
        let eval_request = EvaluateMemoryRequest {
            content: response.output.clone(),
            function_schema: function_schema(),
        };
        
        let evaluation = self.llm_client
            .evaluate_memory(&eval_request)
            .await
            .unwrap_or_else(|e| {
                eprintln!("❌ Memory evaluation failed: {:?}, using defaults", e);
                EvaluateMemoryResponse {
                    salience: 5,
                    tags: vec!["response".to_string()],
                    memory_type: crate::llm::schema::MemoryType::Other,
                    summary: None,
                }
            });
        
        eprintln!("📊 Memory evaluation: salience={}/10, type={:?}, tags={:?}", 
            evaluation.salience, evaluation.memory_type, evaluation.tags
        );
        
        // Get embedding if high salience
        let embedding = if evaluation.salience >= 7 {
            eprintln!("🚀 High salience ({}/10), getting embedding...", evaluation.salience);
            match self.llm_client.get_embedding(&response.output).await {
                Ok(emb) => Some(emb),
                Err(e) => {
                    eprintln!("❌ Failed to get embedding for Mira's response: {:?}", e);
                    None
                }
            }
        } else {
            eprintln!("📉 Low salience ({}/10), skipping embedding", evaluation.salience);
            None
        };
        
        // Create memory entry
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
        eprintln!("💾 Saving Mira's response to SQLite...");
        self.sqlite_store.save(&entry).await?;
        eprintln!("✅ Mira's response saved to SQLite");
        
        // Save to Qdrant if embedding exists
        if embedding.is_some() {
            eprintln!("🔍 Saving Mira's response to Qdrant...");
            match self.qdrant_store.save(&entry).await {
                Ok(_) => eprintln!("✅ Mira's response saved to Qdrant"),
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
    
    fn convert_memory_type(&self, llm_type: &crate::llm::schema::MemoryType) -> MemoryType {
        match llm_type {
            crate::llm::schema::MemoryType::Feeling => MemoryType::Feeling,
            crate::llm::schema::MemoryType::Fact => MemoryType::Fact,
            crate::llm::schema::MemoryType::Joke => MemoryType::Joke,
            crate::llm::schema::MemoryType::Promise => MemoryType::Promise,
            crate::llm::schema::MemoryType::Event => MemoryType::Event,
            crate::llm::schema::MemoryType::Other => MemoryType::Other,
        }
    }
}
