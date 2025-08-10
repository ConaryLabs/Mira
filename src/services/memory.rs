// src/services/memory.rs

use std::sync::Arc;
use anyhow::Result;
use chrono::{Utc, TimeZone};  // Add TimeZone import
use sqlx::Row;  // Add this import
use crate::llm::OpenAIClient;
use crate::llm::schema::{EvaluateMemoryRequest, MiraStructuredReply, EvaluateMemoryResponse, function_schema};
use crate::memory::sqlite::store::SqliteMemoryStore;
use crate::memory::qdrant::store::QdrantMemoryStore;
use crate::memory::traits::MemoryStore;
use crate::memory::types::{MemoryEntry, MemoryType};
use crate::memory::MemoryMessage;

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

    // Add methods needed by ChatService
    
    pub async fn store_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<()> {
        // Route to appropriate method based on role
        if role == "user" {
            // Get embedding for user messages if important enough
            let embedding = match self.llm_client.get_embedding(content).await {
                Ok(emb) => Some(emb),
                Err(_) => None,
            };
            self.save_user_message(session_id, content, embedding, project_id).await
        } else {
            // For assistant messages, create a structured reply
            let reply = MiraStructuredReply {
                salience: 5,  // Changed from 5.0 to 5 (u8)
                summary: Some(content.to_string()),  // Wrapped in Some
                memory_type: "conversation".to_string(),
                tags: vec!["assistant_message".to_string()],
                intent: "response".to_string(),
                mood: "present".to_string(),
                persona: "default".to_string(),
                output: content.to_string(),
                // Add missing fields
                aside_intensity: None,
                monologue: None,
                reasoning_summary: None,
            };
            self.evaluate_and_save_response(session_id, &reply, project_id).await?;
            Ok(())
        }
    }
    
    pub async fn get_recent_messages(
        &self,
        session_id: &str,
        limit: usize,
        _project_id: Option<&str>,
    ) -> Result<Vec<MemoryMessage>> {
        // Retrieve recent messages from SQLite store
        // For now, use a direct query since get_recent doesn't exist
        let query = r#"
            SELECT role, content, timestamp
            FROM chat_history
            WHERE session_id = ?
            ORDER BY timestamp DESC
            LIMIT ?
        "#;
        
        let rows = sqlx::query(query)
            .bind(session_id)
            .bind(limit as i64)
            .fetch_all(&self.sqlite_store.pool)
            .await?;
        
        Ok(rows.into_iter().map(|row| MemoryMessage {
            role: row.get("role"),
            content: row.get("content"),
            timestamp: Utc.from_utc_datetime(&row.get::<chrono::NaiveDateTime, _>("timestamp")),
        }).collect())
    }
    
    pub async fn store_memory(
        &self,
        session_id: &str,
        content: &str,
        memory_type: &str,
        salience: f32,
        _project_id: Option<&str>,
    ) -> Result<()> {
        // Convert string memory type to enum
        let mem_type = match memory_type {
            "feeling" => MemoryType::Feeling,
            "fact" => MemoryType::Fact,
            "joke" => MemoryType::Joke,
            "promise" => MemoryType::Promise,
            "event" => MemoryType::Event,
            _ => MemoryType::Other,
        };
        
        // Get embedding if important
        let embedding = if salience >= 7.0 {
            match self.llm_client.get_embedding(content).await {
                Ok(emb) => Some(emb),
                Err(_) => None,
            }
        } else {
            None
        };
        
        let entry = MemoryEntry {
            id: None,
            session_id: session_id.to_string(),
            role: "system".to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            embedding,
            salience: Some(salience),
            tags: Some(vec![memory_type.to_string()]),
            summary: None,
            memory_type: Some(mem_type),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };
        
        self.sqlite_store.save(&entry).await?;
        
        if entry.embedding.is_some() {
            let _ = self.qdrant_store.save(&entry).await;
        }
        
        Ok(())
    }
    
    pub async fn recall_memories(
        &self,
        session_id: &str,
        _query: &str,
        limit: usize,
        _project_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>> {
        // For now, just get recent memories from SQLite using direct query
        let query = r#"
            SELECT id, session_id, role, content, timestamp, salience, tags, 
                   summary, memory_type, moderation_flag, system_fingerprint
            FROM chat_history
            WHERE session_id = ?
            ORDER BY timestamp DESC
            LIMIT ?
        "#;
        
        let rows = sqlx::query(query)
            .bind(session_id)
            .bind(limit as i64)
            .fetch_all(&self.sqlite_store.pool)
            .await?;
        
        let mut entries = Vec::new();
        for row in rows {
            let tags_str: Option<String> = row.get("tags");
            let tags_vec = tags_str
                .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok());
            
            let memory_type_str: Option<String> = row.get("memory_type");
            let memory_type_enum = memory_type_str.and_then(|mt| match mt.as_str() {
                "Feeling" => Some(MemoryType::Feeling),
                "Fact" => Some(MemoryType::Fact),
                "Joke" => Some(MemoryType::Joke),
                "Promise" => Some(MemoryType::Promise),
                "Event" => Some(MemoryType::Event),
                _ => Some(MemoryType::Other),
            });
            
            entries.push(MemoryEntry {
                id: row.get("id"),
                session_id: row.get("session_id"),
                role: row.get("role"),
                content: row.get("content"),
                timestamp: Utc.from_utc_datetime(&row.get::<chrono::NaiveDateTime, _>("timestamp")),
                embedding: None,  // Skip for performance
                salience: row.get("salience"),
                tags: tags_vec,
                summary: row.get("summary"),
                memory_type: memory_type_enum,
                logprobs: None,
                moderation_flag: row.get("moderation_flag"),
                system_fingerprint: row.get("system_fingerprint"),
            });
        }
        
        Ok(entries)
    }
}
