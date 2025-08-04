// src/services/hybrid.rs

use crate::llm::assistant::{AssistantManager, ThreadManager};
use crate::llm::assistant::manager::ResponseMessage;
use crate::persona::PersonaOverlay;
use crate::services::{ChatService, MemoryService, ContextService};
use crate::memory::recall::RecallContext;
use crate::llm::schema::{ChatResponse, MiraStructuredReply};
use anyhow::Result;
use std::sync::Arc;

pub struct HybridMemoryService {
    chat_service: Arc<ChatService>,
    memory_service: Arc<MemoryService>,
    context_service: Arc<ContextService>,
    assistant_manager: Arc<AssistantManager>,
    thread_manager: Arc<ThreadManager>,
}

impl HybridMemoryService {
    pub fn new(
        chat_service: Arc<ChatService>,
        memory_service: Arc<MemoryService>,
        context_service: Arc<ContextService>,
        assistant_manager: Arc<AssistantManager>,
        thread_manager: Arc<ThreadManager>,
    ) -> Self {
        Self {
            chat_service,
            memory_service,
            context_service,
            assistant_manager,
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
        let thread_id = self.thread_manager.get_or_create_thread(session_id).await?;

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

    /// Always preserve Mira's personality through the conversation
    async fn run_assistant_with_context(
        &self,
        thread_id: &str,  // This is now just the session_id
        message: &str,
        persona: &PersonaOverlay,
    ) -> Result<ChatResponse> {
        // 1. Get conversation history
        let mut messages = self.thread_manager.get_conversation(thread_id).await;

        // 2. Build the full system prompt with persona AND output requirements
        let mut system_prompt = String::new();

        // Add Mira's persona
        system_prompt.push_str(persona.prompt());
        system_prompt.push_str("\n\n");

        // Add structured output requirements
        system_prompt.push_str("CRITICAL: Your entire reply MUST be a single valid JSON object with these fields:\n");
        system_prompt.push_str("- output: Your actual reply to the user (string)\n");
        system_prompt.push_str("- persona: The persona overlay in use (string)\n");
        system_prompt.push_str("- mood: The emotional tone of your reply (string)\n");
        system_prompt.push_str("- salience: How emotionally important this reply is (integer 0-10)\n");
        system_prompt.push_str("- summary: Short summary of your reply/context (string or null)\n");
        system_prompt.push_str("- memory_type: \"feeling\", \"fact\", \"joke\", \"promise\", \"event\", or \"other\" (string)\n");
        system_prompt.push_str("- tags: List of context/mood tags (array of strings)\n");
        system_prompt.push_str("- intent: Your intent in this reply (string)\n");
        system_prompt.push_str("- monologue: Your private inner thoughts, not shown to user (string or null)\n");
        system_prompt.push_str("- reasoning_summary: Your reasoning/chain-of-thought, if any (string or null)\n\n");
        system_prompt.push_str("Never add anything before or after the JSON. No markdown, no natural language, no commentary—just the JSON object.\n");

        // 3. ALWAYS include the system message at the beginning of the messages array
        // This ensures Mira's personality is preserved throughout the conversation
        let system_message = ResponseMessage {
            role: "system".to_string(),
            content: system_prompt,
        };

        // Filter out any existing system messages from history to avoid duplicates
        messages.retain(|m| m.role != "system");

        // Insert our system message at the beginning
        messages.insert(0, system_message);

        // 4. Add the new user message
        let user_msg = ResponseMessage {
            role: "user".to_string(),
            content: message.to_string(),
        };
        messages.push(user_msg.clone());
        self.thread_manager.add_message(thread_id, user_msg).await?;

        // 5. Create response using the updated manager (no vector store tools)
        eprintln!("💬 Using Responses API (no tools, just context)");
        eprintln!("🎭 Active persona: {}", persona);

        let response_object = self.assistant_manager
            .create_response(messages)
            .await?;

        // 6. Extract the message content from the ResponseObject
        let assistant_message = if let Some(msg) = response_object.choices.first() {
            msg.message.content.clone()
        } else {
            eprintln!("⚠️ No choices in response object");
            String::new()
        };

        eprintln!("📝 Assistant raw response: {}", assistant_message);

        // 7. Parse the structured response
        let chat_response = match serde_json::from_str::<ChatResponse>(&assistant_message) {
            Ok(mut parsed) => {
                parsed.persona = persona.to_string();
                parsed
            },
            Err(e) => {
                eprintln!("⚠️ Failed to parse assistant response as JSON: {}", e);
                eprintln!("Raw response was: {}", assistant_message);

                // Fallback response that maintains Mira's personality
                ChatResponse {
                    output: assistant_message.clone(),
                    persona: persona.to_string(),
                    mood: "confused".to_string(),
                    salience: 5,
                    summary: None,
                    memory_type: "other".to_string(),
                    tags: vec![],
                    intent: "response".to_string(),
                    monologue: Some("Something went wrong with my response format...".to_string()),
                    reasoning_summary: None,
                    aside_intensity: None,
                }
            }
        };

        // 8. Save assistant response to conversation
        let assistant_msg = ResponseMessage {
            role: "assistant".to_string(),
            content: serde_json::to_string(&chat_response).unwrap_or(assistant_message),
        };
        self.thread_manager.add_message(thread_id, assistant_msg).await?;

        // 9. Return the properly typed response
        Ok(chat_response)
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
