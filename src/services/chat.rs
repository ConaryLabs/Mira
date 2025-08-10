// src/services/chat.rs

use crate::llm::anthropic_client::{AnthropicClient, Message, MessageContent};
use crate::llm::claude_system::{ClaudeSystem, ActionType};
use crate::llm::OpenAIClient;
use crate::persona::PersonaOverlay;
use crate::llm::schema::MiraStructuredReply;
use crate::services::ContextService;
use crate::services::MemoryService;
use crate::memory::MemoryMessage;
use anyhow::Result;
use std::sync::Arc;

#[derive(Clone)]
pub struct ChatService {
    pub anthropic_client: Arc<AnthropicClient>,
    pub context_service: Option<Arc<ContextService>>,
    pub memory_service: Option<Arc<MemoryService>>,
    pub llm_client: Arc<OpenAIClient>,  // Used for embeddings AND image generation
    claude_system: ClaudeSystem,
}

impl ChatService {
    pub fn new(
        anthropic_client: Arc<AnthropicClient>,
        openai_client: Arc<OpenAIClient>,
    ) -> Self {
        eprintln!("🚀 Mira initialized:");
        eprintln!("  🧠 Claude Sonnet 4.0 orchestrates everything");
        eprintln!("  🎨 OpenAI gpt-image-1 for image generation");
        eprintln!("  ✨ Fully autonomous decision-making");
        
        Self {
            anthropic_client: anthropic_client.clone(),
            context_service: None,
            memory_service: None,
            llm_client: openai_client,
            claude_system: ClaudeSystem::new(anthropic_client),
        }
    }

    pub fn set_context_service(&mut self, context_service: Arc<ContextService>) {
        self.context_service = Some(context_service);
    }

    pub fn set_memory_service(&mut self, memory_service: Arc<MemoryService>) {
        self.memory_service = Some(memory_service);
    }

    /// SINGLE ENTRY POINT - Claude orchestrates everything
    pub async fn process_message(
        &self,
        session_id: &str,
        content: &str,
        persona: &PersonaOverlay,
        project_id: Option<&str>,
        images: Option<Vec<String>>,
        pdfs: Option<Vec<String>>,
    ) -> Result<MiraStructuredReply> {
        eprintln!("📨 Processing message: {}", content);
        
        // Build orchestration prompt
        let system_prompt = self.build_orchestration_prompt(persona, project_id).await?;
        
        // Get memory context
        let memory_messages = self.get_memory_messages(session_id, project_id).await?;
        
        // Claude analyzes and decides
        let decision = self.claude_system.analyze_and_decide(
            &system_prompt,
            content,
            memory_messages.clone(),
            images.clone(),
            pdfs.clone(),
        ).await?;
        
        eprintln!("🧠 Claude decided: {:?} (confidence: {})", 
                  decision.action, decision.confidence);
        
        // Execute Claude's decision - SIMPLIFIED to just what we support
        match decision.action {
            ActionType::Conversation => {
                let response = self.claude_system.respond(
                    persona,
                    content,
                    memory_messages,
                    images,
                    pdfs,
                ).await?;
                
                // Store in memory if available
                if let Some(mem_service) = &self.memory_service {
                    let _ = mem_service.store_message(session_id, "user", content, project_id).await;
                    let _ = mem_service.store_message(session_id, "assistant", &response.get_text(), project_id).await;
                }
                
                let text = response.get_text();
                Ok(MiraStructuredReply {
                    salience: 5,
                    summary: Some(text.clone()),
                    memory_type: "conversation".to_string(),
                    tags: vec![persona.name().to_string()],
                    intent: "response".to_string(),
                    mood: persona.current_mood(),
                    persona: persona.name().to_string(),
                    output: text,
                    aside_intensity: None,
                    monologue: None,
                    reasoning_summary: Some(decision.reasoning.clone()),
                })
            },
            
            ActionType::GenerateImage => {
                let prompt = decision.image_prompt.unwrap_or_else(|| content.to_string());
                eprintln!("🎨 Generating image with OpenAI: {}", prompt);
                
                match self.llm_client.generate_image(&prompt, Some("hd")).await {
                    Ok(urls) => {
                        eprintln!("✅ Image generated: {:?}", urls);
                        let response_text = format!(
                            "I've created an image for you! You can view it here: {}\n\nThe image depicts: {}",
                            urls.join(", "),
                            prompt
                        );
                        self.claude_respond(persona, &response_text, vec![]).await
                    },
                    Err(e) => {
                        eprintln!("❌ Image generation failed: {:?}", e);
                        let error_msg = format!(
                            "I wanted to create an image of '{}' but encountered a technical issue. Let me describe it instead: {}",
                            prompt, decision.context
                        );
                        self.claude_respond(persona, &error_msg, vec![]).await
                    }
                }
            },
            
            ActionType::DescribeImage => {
                if let Some(imgs) = images {
                    // Have Claude describe the image using its vision capabilities
                    let response = self.claude_system.respond(
                        persona,
                        "Please describe this image in detail.",
                        vec![],
                        Some(imgs),
                        None,
                    ).await?;
                    
                    let text = response.get_text();
                    Ok(MiraStructuredReply {
                        salience: 5,
                        summary: Some(text.clone()),
                        memory_type: "image_analysis".to_string(),
                        tags: vec!["image".to_string(), persona.name().to_string()],
                        intent: "describe".to_string(),
                        mood: persona.current_mood(),
                        persona: persona.name().to_string(),
                        output: text,
                        aside_intensity: None,
                        monologue: None,
                        reasoning_summary: None,
                    })
                } else {
                    let msg = "I'd need you to provide an image for me to describe. Please upload one and I'll tell you what I see!";
                    self.claude_respond(persona, msg, vec![]).await
                }
            },
            
            // For any unsupported action, just treat it as conversation
            _ => {
                eprintln!("⚠️ Unsupported action {:?}, falling back to conversation", decision.action);
                let response = self.claude_system.respond(
                    persona,
                    content,
                    memory_messages,
                    images,
                    pdfs,
                ).await?;
                
                let text = response.get_text();
                Ok(MiraStructuredReply {
                    salience: 5,
                    summary: Some(text.clone()),
                    memory_type: "conversation".to_string(),
                    tags: vec![persona.name().to_string()],
                    intent: "response".to_string(),
                    mood: persona.current_mood(),
                    persona: persona.name().to_string(),
                    output: text,
                    aside_intensity: None,
                    monologue: None,
                    reasoning_summary: Some(decision.reasoning.clone()),
                })
            }
        }
    }

    async fn build_orchestration_prompt(&self, persona: &PersonaOverlay, project_id: Option<&str>) -> Result<String> {
        let mut context_info = String::new();
        
        if let Some(pid) = project_id {
            if let Some(_ctx_service) = &self.context_service {
                context_info = format!("Project context: {}", pid);
            }
        }
        
        Ok(format!(
            "You are orchestrating as Mira with persona: {}\nMood: {}\n{}",
            persona.name(),
            persona.current_mood(),
            context_info
        ))
    }

    async fn get_memory_messages(&self, session_id: &str, project_id: Option<&str>) -> Result<Vec<Message>> {
        if let Some(mem_service) = &self.memory_service {
            let memories = mem_service.get_recent_messages(session_id, 10, project_id).await?;
            
            Ok(memories.into_iter().map(|m: MemoryMessage| Message {
                role: m.role,
                content: MessageContent::Text(m.content),
            }).collect())
        } else {
            Ok(vec![])
        }
    }
    
    /// Helper to always go through Claude
    async fn claude_respond(
        &self,
        persona: &PersonaOverlay,
        prompt: &str,
        context: Vec<Message>,
    ) -> Result<MiraStructuredReply> {
        let response = self.claude_system.respond(
            persona,
            prompt,
            context,
            None,
            None,
        ).await?;
        
        let text = response.get_text();
        
        Ok(MiraStructuredReply {
            salience: 5,
            summary: Some(text.clone()),
            memory_type: "conversation".to_string(),
            tags: vec![persona.name().to_string()],
            intent: "response".to_string(),
            mood: persona.current_mood(),
            persona: persona.name().to_string(),
            output: text,
            aside_intensity: None,
            monologue: None,
            reasoning_summary: None,
        })
    }
}
