// src/services/hybrid.rs

use crate::llm::assistant::{AssistantManager, VectorStoreManager, ThreadManager};
use crate::llm::assistant::manager::ResponseMessage;
use crate::persona::PersonaOverlay;
use crate::services::{ChatService, MemoryService, ContextService};
use crate::memory::recall::RecallContext;
use crate::llm::schema::{ChatResponse, MiraStructuredReply};
use anyhow::{Result, Context as AnyhowContext};
use std::sync::Arc;
use reqwest::Method;

pub struct HybridMemoryService {
    chat_service: Arc<ChatService>,
    memory_service: Arc<MemoryService>,
    context_service: Arc<ContextService>,
    assistant_manager: Arc<AssistantManager>,
    vector_store_manager: Arc<VectorStoreManager>,
    thread_manager: Arc<ThreadManager>,
}

impl HybridMemoryService {
    pub fn new(
        chat_service: Arc<ChatService>,
        memory_service: Arc<MemoryService>,
        context_service: Arc<ContextService>,
        assistant_manager: Arc<AssistantManager>,
        vector_store_manager: Arc<VectorStoreManager>,
        thread_manager: Arc<ThreadManager>,
    ) -> Self {
        Self {
            chat_service,
            memory_service,
            context_service,
            assistant_manager,
            vector_store_manager,
            thread_manager,
        }
    }

    pub async fn process_with_hybrid_memory(
        &self,
        session_id: &str,
        content: &str,
        persona: &PersonaOverlay,
        project_id: Option<&str>,
    ) -> Result<ChatResponse> {
        let thread_id = if let Some(proj_id) = project_id {
            self.ensure_thread_with_vector_store(session_id, proj_id).await?
        } else {
            self.thread_manager.get_or_create_thread(session_id).await?
        };

        let embedding = self.chat_service.llm_client
            .get_embedding(content)
            .await
            .ok();

        let personal_context = self.context_service
            .build_context(session_id, embedding.as_deref(), project_id)
            .await?;

        let enriched_message = self.enrich_message_with_context(content, &personal_context);

        let assistant_response = self
            .run_assistant_with_context(&thread_id, &enriched_message, persona)
            .await?;

        self.sync_insights_to_personal_memory(
            session_id,
            &assistant_response,
            project_id,
        ).await?;

        Ok(assistant_response)
    }

    async fn ensure_thread_with_vector_store(
        &self,
        session_id: &str,
        project_id: &str,
    ) -> Result<String> {
        let vector_store_id = self.vector_store_manager
            .create_project_store(project_id)
            .await?;

        self.thread_manager
            .get_or_create_thread_with_tools(
                session_id,
                vec![vector_store_id],
            )
            .await
    }

    fn enrich_message_with_context(
        &self,
        content: &str,
        context: &RecallContext,
    ) -> String {
        let mut context_parts = Vec::new();
        
        if !context.recent.is_empty() {
            let recent_texts: Vec<String> = context.recent.iter()
                .take(3)
                .map(|m| format!("[{}]: {}", m.role, m.content.chars().take(100).collect::<String>()))
                .collect();
            context_parts.push(format!("Recent conversation:\n{}", recent_texts.join("\n")));
        }
        
        if !context.semantic.is_empty() {
            let semantic_texts: Vec<String> = context.semantic.iter()
                .take(3)
                .map(|m| format!("- {}", m.content.chars().take(100).collect::<String>()))
                .collect();
            context_parts.push(format!("Related memories:\n{}", semantic_texts.join("\n")));
        }
        
        if context_parts.is_empty() {
            content.to_string()
        } else {
            format!("{}\n\n[Personal Context:]\n{}", content, context_parts.join("\n\n"))
        }
    }

    /// Updated to use Responses API instead of deprecated Assistant API
    async fn run_assistant_with_context(
        &self,
        thread_id: &str,  // This is now just the session_id
        message: &str,
        _persona: &PersonaOverlay,
    ) -> Result<ChatResponse> {
        // 1. Get conversation history
        let mut messages = self.thread_manager.get_conversation(thread_id).await;
        
        // 2. Add system message if history is empty
        if messages.is_empty() {
            messages.push(ResponseMessage {
                role: "system".to_string(),
                content: "You are a helpful document search assistant. Search through available documents to provide accurate and relevant information.".to_string(),
            });
        }
        
        // 3. Add the new user message
        let user_msg = ResponseMessage {
            role: "user".to_string(),
            content: message.to_string(),
        };
        messages.push(user_msg.clone());
        self.thread_manager.add_message(thread_id, user_msg).await?;
        
        // 4. Get vector stores for this session
        let vector_store_ids = self.thread_manager.get_session_stores(thread_id).await;
        
        // 5. Create response
        let assistant_message = if !vector_store_ids.is_empty() {
            eprintln!("🔍 Using Responses API with {} vector stores", vector_store_ids.len());
            let response = self.assistant_manager
                .create_response_with_vector_stores(messages, vector_store_ids)
                .await?;
                
            response.choices
                .first()
                .map(|choice| choice.message.content.clone())
                .unwrap_or_else(|| "I couldn't generate a response.".to_string())
        } else {
            eprintln!("💬 Using regular chat completion");
            // If no vector stores, just use regular chat completion
            let req = serde_json::json!({
                "model": "gpt-4.1",
                "messages": messages,
                "temperature": 0.3,
            });
            
            let response = self.chat_service.llm_client
                .request(Method::POST, "chat/completions")
                .json(&req)
                .send()
                .await
                .context("Failed to send chat request")?;
                
            if !response.status().is_success() {
                let error_text = response.text().await?;
                return Err(anyhow::anyhow!("Chat API error: {}", error_text));
            }
            
            let result: serde_json::Value = response.json().await?;
            result["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("I couldn't generate a response.")
                .to_string()
        };
        
        // 6. Save assistant response to conversation
        let assistant_msg = ResponseMessage {
            role: "assistant".to_string(),
            content: assistant_message.clone(),
        };
        self.thread_manager.add_message(thread_id, assistant_msg).await?;
        
        // 7. Return as ChatResponse
        Ok(ChatResponse {
            output: assistant_message,
            persona: "Assistant".to_string(),
            mood: "neutral".to_string(),
            salience: 5,
            summary: None,
            memory_type: "general".to_string(),
            tags: vec![],
            intent: "response".to_string(),
            monologue: None,
            reasoning_summary: None,
            aside_intensity: None,
        })
    }

    async fn sync_insights_to_personal_memory(
        &self,
        session_id: &str,
        response: &ChatResponse,
        project_id: Option<&str>,
    ) -> Result<()> {
        if response.salience >= 7 {
            eprintln!("💡 High-salience response ({}), syncing insight to personal memory", response.salience);
            
            let insight_response = MiraStructuredReply {
                output: format!(
                    "Insight from project {}: {}",
                    project_id.unwrap_or("general"),
                    response.summary.as_ref().unwrap_or(&response.output)
                ),
                persona: "system".to_string(),
                mood: "neutral".to_string(),
                salience: response.salience,
                summary: Some("Synced insight from assistant interaction".to_string()),
                memory_type: "event".to_string(),
                tags: vec!["insight".to_string(), "synced".to_string()],
                intent: "system".to_string(),
                monologue: None,
                reasoning_summary: None,
                aside_intensity: None,
            };
            
            self.memory_service.evaluate_and_save_response(
                session_id,
                &insight_response,
                project_id,
            ).await?;
        }
        
        Ok(())
    }
}
