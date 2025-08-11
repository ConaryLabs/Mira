use crate::llm::OpenAIClient;
use crate::persona::PersonaOverlay;
use crate::llm::schema::MiraStructuredReply;
use crate::services::ContextService;
use crate::services::MemoryService;
use crate::memory::MemoryMessage;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[derive(Clone)]
pub struct ChatService {
    pub context_service: Option<Arc<ContextService>>,
    pub memory_service: Option<Arc<MemoryService>>,
    pub llm_client: Arc<OpenAIClient>, // embeddings, images, and GPT-5 chat
}

impl ChatService {
    pub fn new(openai_client: Arc<OpenAIClient>) -> Self {
        Self {
            context_service: None,
            memory_service: None,
            llm_client: openai_client,
        }
    }

    pub fn set_context_service(&mut self, context_service: Arc<ContextService>) {
        self.context_service = Some(context_service);
    }

    pub fn set_memory_service(&mut self, memory_service: Arc<MemoryService>) {
        self.memory_service = Some(memory_service);
    }

    /// Compatibility shim for existing call sites.
    pub async fn process_message(
        &self,
        session_id: &str,
        content: &str,
        persona: &PersonaOverlay,
        project_id: Option<&str>,
        images: Option<Vec<String>>,
        pdfs: Option<Vec<String>>,
    ) -> Result<MiraStructuredReply> {
        self.process_message_gpt5(session_id, content, persona, project_id, images, pdfs).await
    }

    /// GPT-5 path: persona + memory -> Responses API (model gpt-5) -> persist -> reply
    pub async fn process_message_gpt5(
        &self,
        session_id: &str,
        content: &str,
        persona: &PersonaOverlay,
        project_id: Option<&str>,
        _images: Option<Vec<String>>,
        _pdfs: Option<Vec<String>>,
    ) -> Result<MiraStructuredReply> {
        // Persist + embed the user turn with simple exponential backoff
        if let Some(mem_service) = &self.memory_service {
            let mut attempt = 0u32;
            let max_attempts = 6u32;
            let embedding = loop {
                match self.llm_client.get_embedding(content).await {
                    Ok(v) => break Some(v),
                    Err(_) => {
                        attempt += 1;
                        if attempt >= max_attempts {
                            break None; // never drop the turn
                        }
                        let delay_ms = (300u64 * (1u64 << (attempt - 1))).min(5_000);
                        sleep(Duration::from_millis(delay_ms)).await;
                    }
                }
            };
            let _ = mem_service
                .save_user_message(session_id, content, embedding, project_id)
                .await;
        }

        // Persona instructions come STRICTLY from src/persona
        let instructions: String = persona.prompt().to_string();

        // Pull recent messages from memory
        let memory_messages = self.get_memory_messages(session_id, project_id).await?;

        // Assemble Responses API input (prior turns + current user turn; persona is in `instructions`)
        let input = self.build_gpt5_input(&memory_messages, content);

        // Call GPT-5 via OpenAIClient
        let llm_resp = self
            .llm_client
            .respond_gpt5(
                input,
                Some(instructions.as_str()),
                None,
                Some("medium"),
                Some("medium"),
                None,
                None,
            )
            .await?;

        let cleaned = extract_user_facing_text(&llm_resp.text);

        // Persist assistant once
        self.store_assistant(session_id, &cleaned, project_id).await;

        Ok(MiraStructuredReply {
            salience: 5,
            summary: Some(cleaned.clone()),
            memory_type: "conversation".to_string(),
            tags: vec![persona.name().to_string()],
            intent: "response".to_string(),
            mood: persona.current_mood(),
            persona: persona.name().to_string(),
            output: cleaned,
            aside_intensity: None,
            monologue: None,
            reasoning_summary: None,
        })
    }

    async fn get_memory_messages(
        &self,
        session_id: &str,
        project_id: Option<&str>,
    ) -> Result<Vec<MemoryMessage>> {
        if let Some(mem_service) = &self.memory_service {
            Ok(mem_service.get_recent_messages(session_id, 20, project_id).await?)
        } else {
            Ok(vec![])
        }
    }

    /// Build the Responses API `input` array with correct content-part types.
    /// - user turns -> input_text
    /// - assistant turns -> output_text
    /// Persona is provided via `instructions`.
    fn build_gpt5_input(
        &self,
        memory_messages: &Vec<MemoryMessage>,
        user_content: &str,
    ) -> serde_json::Value {
        let mut msgs: Vec<serde_json::Value> = Vec::with_capacity(memory_messages.len() + 1);

        // Prior turns
        for m in memory_messages {
            let (role, part_type) = if m.role == "assistant" {
                ("assistant", "output_text")
            } else {
                ("user", "input_text")
            };

            msgs.push(serde_json::json!({
                "role": role,
                "content": [ { "type": part_type, "text": m.content } ]
            }));
        }

        // Current user turn
        msgs.push(serde_json::json!({
            "role": "user",
            "content": [ { "type": "input_text", "text": user_content } ]
        }));

        Value::Array(msgs)
    }

    async fn store_assistant(
        &self,
        session_id: &str,
        assistant_text: &str,
        project_id: Option<&str>,
    ) {
        if let Some(mem_service) = &self.memory_service {
            let _ = mem_service
                .store_message(session_id, "assistant", assistant_text, project_id)
                .await;
        }
    }
}

/// Strip `json { ... }`, ```json blocks, and return user-facing text.
fn extract_user_facing_text(raw: &str) -> String {
    let mut s = raw.trim().to_string();

    if s.starts_with("```") {
        if let Some(start) = s.find('\n') {
            if let Some(end) = s.rfind("```") {
                s = s[start + 1..end].trim().to_string();
            }
        }
    }

    if s.to_ascii_lowercase().starts_with("json ") {
        s = s[4..].trim().to_string();
    }

    if let Ok(v) = serde_json::from_str::<Value>(&s) {
        if let Some(resp) = v.get("response").and_then(|x| x.as_str()) {
            return resp.to_string();
        }
    }

    s
}
