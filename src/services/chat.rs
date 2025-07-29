// src/services/chat.rs

use std::sync::Arc;
use anyhow::Result;
use serde::{Serialize, Deserialize};
use crate::llm::{OpenAIClient, emotional_weight};
use crate::persona::PersonaOverlay;
use crate::prompt::builder::build_system_prompt;
use crate::services::{MemoryService, ContextService};

#[derive(Clone)]
pub struct ChatService {
    llm_client: Arc<OpenAIClient>,
    memory_service: Arc<MemoryService>,
    context_service: Arc<ContextService>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: u8,
    pub summary: Option<String>,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: String,
    pub monologue: Option<String>,
    pub reasoning_summary: Option<String>,
    
    // For WebSocket streaming
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aside: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aside_intensity: Option<f32>,
}

impl ChatService {
    pub fn new(
        llm_client: Arc<OpenAIClient>,
        memory_service: Arc<MemoryService>,
        context_service: Arc<ContextService>,
    ) -> Self {
        Self {
            llm_client,
            memory_service,
            context_service,
        }
    }
    
    pub async fn process_message(
        &self,
        session_id: &str,
        content: &str,
        persona: &PersonaOverlay,
        project_id: Option<&str>,
    ) -> Result<ChatResponse> {
        eprintln!("🎭 ChatService processing message for session: {}", session_id);
        
        // 1. Get embedding for semantic search
        eprintln!("📊 Getting embedding for user message...");
        let user_embedding = match self.llm_client.get_embedding(content).await {
            Ok(emb) => {
                eprintln!("✅ Embedding generated successfully (length: {})", emb.len());
                Some(emb)
            },
            Err(e) => {
                eprintln!("❌ Failed to get embedding: {:?}", e);
                None
            }
        };
        
        // 2. Build context using both memory stores
        let recall_context = self.context_service
            .build_context(session_id, user_embedding.as_deref(), project_id)
            .await?;
        
        eprintln!("📚 Recall context: {} recent, {} semantic", 
            recall_context.recent.len(), 
            recall_context.semantic.len()
        );
        
        // Log recent messages for debugging
        eprintln!("📜 Recent messages in context:");
        for (i, msg) in recall_context.recent.iter().enumerate() {
            eprintln!("  {}. [{}] {} - {}", 
                i+1, 
                msg.role, 
                msg.timestamp.format("%H:%M:%S"),
                msg.content.chars().take(80).collect::<String>()
            );
        }
        
        // 3. Build system prompt with persona and memory context
        let system_prompt = build_system_prompt(persona, &recall_context);
        
        // 4. Moderate user message (log-only)
        let _ = self.llm_client.moderate(content).await;
        
        // 5. Get emotional weight for auto model routing
        let emotional_weight = match emotional_weight::classify(&self.llm_client, content).await {
            Ok(val) => {
                eprintln!("🎭 Emotional weight: {}", val);
                val
            },
            Err(e) => {
                eprintln!("Failed to classify emotional weight: {}", e);
                0.0
            }
        };
        
        let model = if emotional_weight > 0.95 {
            "o3"
        } else if emotional_weight > 0.6 {
            "o4-mini"
        } else {
            "gpt-4.1"
        };
        
        eprintln!("🤖 Using model: {}", model);
        
        // 6. Get structured chat completion
        eprintln!("💬 Calling LLM with structured output...");
        let mira_reply = self.llm_client
            .chat_with_custom_prompt(
                content,
                model,
                &system_prompt,
            )
            .await?;
        
        eprintln!("✅ Got LLM response: {} chars", mira_reply.output.len());
        eprintln!("   Mood: {}, Salience: {}/10", mira_reply.mood, mira_reply.salience);
        
        // 7. Save user message to memory
        self.memory_service
            .save_user_message(session_id, content, user_embedding.clone(), project_id)
            .await?;
        
        // 8. Evaluate and save Mira's response
        let evaluation = self.memory_service
            .evaluate_and_save_response(
                session_id,
                &mira_reply,
                project_id,
            )
            .await?;
        
        // 9. Build response with all fields
        Ok(ChatResponse {
            output: mira_reply.output,
            persona: persona.to_string(),
            mood: mira_reply.mood,
            salience: evaluation.salience,
            summary: evaluation.summary,
            memory_type: evaluation.memory_type.to_string(),
            tags: evaluation.tags,
            intent: mira_reply.intent,
            monologue: mira_reply.monologue.clone(),
            reasoning_summary: mira_reply.reasoning_summary,
            aside: mira_reply.monologue,
            aside_intensity: Some(evaluation.salience as f32 / 10.0),
        })
    }
}
