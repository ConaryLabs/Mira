// src/services/chat.rs

use crate::llm::anthropic_client::{AnthropicClient, Message, MessageContent};
use crate::llm::claude_system::{ClaudeSystem, ActionType};
use crate::llm::OpenAIClient;
use crate::services::midjourney_client::MidjourneyClient;
use crate::services::midjourney_personas::MidjourneyPersonaEngine;
use crate::persona::PersonaOverlay;
use crate::llm::schema::MiraStructuredReply;
use crate::services::ContextService;
use crate::services::MemoryService;
use crate::memory::MemoryMessage;
use anyhow::Result;
use std::sync::Arc;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

#[derive(Clone)]
pub struct ChatService {
    pub anthropic_client: Arc<AnthropicClient>,
    pub midjourney_engine: Arc<MidjourneyPersonaEngine>,
    pub context_service: Option<Arc<ContextService>>,
    pub memory_service: Option<Arc<MemoryService>>,
    pub llm_client: Arc<OpenAIClient>,  // Keep for backward compatibility
    claude_system: ClaudeSystem,
}

impl ChatService {
    pub fn new(
        anthropic_client: Arc<AnthropicClient>,
        midjourney_client: Arc<MidjourneyClient>,
    ) -> Self {
        eprintln!("🚀 Mira initialized:");
        eprintln!("  🧠 Claude Sonnet 4.0 orchestrates everything");
        eprintln!("  🎨 Midjourney v6.5 for all visuals");
        eprintln!("  ✨ Fully autonomous decision-making");
        
        // Create OpenAI client for backward compatibility
        let llm_client = Arc::new(OpenAIClient::new());
        
        Self {
            anthropic_client: anthropic_client.clone(),
            midjourney_engine: Arc::new(MidjourneyPersonaEngine::new(midjourney_client)),
            context_service: None,
            memory_service: None,
            llm_client,
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
        
        // Execute Claude's decision
        match decision.action {
            ActionType::Conversation => {
                let response = self.claude_system.respond(
                    persona,
                    content,
                    memory_messages,
                    images,
                    pdfs,
                ).await?;
                
                // Store in memory
                if let Some(mem_service) = &self.memory_service {
                    mem_service.store_message(
                        session_id,
                        "user",
                        content,
                        project_id,
                    ).await?;
                    
                    mem_service.store_message(
                        session_id,
                        "assistant",
                        &response.get_text(),
                        project_id,
                    ).await?;
                }
                
                let text = response.get_text();
                Ok(MiraStructuredReply {
                    salience: 5,  // u8
                    summary: Some(text.clone()),  // Option<String>
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
                let urls = self.midjourney_engine.generate_with_persona(
                    &decision.image_prompt.unwrap_or_else(|| content.to_string()),
                    persona,
                    decision.style_params,
                ).await?;
                
                self.format_image_response(urls, persona, &decision.context).await
            },
            
            ActionType::DescribeImage => {
                if let Some(imgs) = images {
                    // Decode first image and describe
                    let image_data = BASE64.decode(&imgs[0])?;
                    let description = self.midjourney_engine.client
                        .describe(&image_data)
                        .await?;
                    
                    self.format_description_response(description, persona).await
                } else {
                    // Even error messages go through Claude
                    self.claude_respond_to_missing_image(persona).await
                }
            },
            
            ActionType::BlendImages => {
                if let Some(imgs) = images {
                    if imgs.len() < 2 {
                        return self.claude_respond_to_insufficient_images_for_blend(persona).await;
                    }
                    
                    let decoded: Vec<Vec<u8>> = imgs.iter()
                        .map(|img| BASE64.decode(img).unwrap())
                        .collect();
                    
                    let job = self.midjourney_engine.client
                        .blend(decoded)
                        .await?;
                    
                    let urls = self.midjourney_engine.client
                        .wait_for_completion(&job.job_id, 60)
                        .await?;
                    
                    self.format_blend_response(urls, persona).await
                } else {
                    self.claude_respond_to_missing_images_for_blend(persona).await
                }
            },
            
            ActionType::CreateLogo => {
                // Extract company name from context
                let company_name = decision.context.split_whitespace()
                    .next()
                    .unwrap_or("Company");
                
                let urls = self.midjourney_engine.generate_logo(
                    company_name,
                    &decision.image_prompt.unwrap_or_else(|| "modern tech company".to_string()),
                ).await?;
                
                self.format_logo_response(urls, company_name, persona).await
            },
            
            ActionType::WeirdMode => {
                eprintln!("🌀 WEIRD MODE ACTIVATED BY CLAUDE!");
                
                let urls = self.midjourney_engine.generate_weird_mode(
                    &decision.image_prompt.unwrap_or_else(|| content.to_string()),
                ).await?;
                
                self.format_weird_response(urls, persona).await
            },
            
            ActionType::Video => {
                let url = self.midjourney_engine.generate_video(
                    &decision.image_prompt.unwrap_or_else(|| content.to_string()),
                ).await?;
                
                self.format_video_response(url, persona).await
            },
            
            ActionType::MultiStep => {
                // Box the recursive call to avoid infinite size
                Box::pin(self.execute_multi_step(
                    decision.steps.unwrap_or_default(),
                    persona,
                    session_id,
                    project_id,
                )).await
            },
        }
    }

    async fn build_orchestration_prompt(&self, persona: &PersonaOverlay, project_id: Option<&str>) -> Result<String> {
        let mut context_info = String::new();
        
        if let Some(pid) = project_id {
            if let Some(_ctx_service) = &self.context_service {
                // Get project context if available
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

    async fn execute_multi_step(
        &self,
        steps: Vec<String>,
        persona: &PersonaOverlay,
        session_id: &str,
        project_id: Option<&str>,
    ) -> Result<MiraStructuredReply> {
        let mut results = Vec::new();
        
        for step in steps {
            eprintln!("📋 Executing step: {}", step);
            // Box the recursive call to avoid infinite size
            let step_result = Box::pin(self.process_message(
                session_id,
                &step,
                persona,
                project_id,
                None,
                None,
            )).await?;
            results.push(step_result.output);
        }
        
        // Have Claude synthesize all results
        let synthesis_prompt = format!(
            "Synthesize these results into a cohesive response:\n{}",
            results.join("\n\n")
        );
        
        self.claude_respond(persona, &synthesis_prompt, vec![]).await
    }

    // Response formatting methods - ALL go through Claude for personality

    async fn format_image_response(
        &self,
        urls: Vec<String>,
        persona: &PersonaOverlay,
        context: &str,
    ) -> Result<MiraStructuredReply> {
        let prompt = format!(
            "I just created {} image(s) for the user. URLs: {:?}\n\
            Context: {}\n\
            Share this creation in your unique voice with enthusiasm.",
            urls.len(), urls, context
        );
        self.claude_respond(persona, &prompt, vec![]).await
    }

    async fn format_description_response(
        &self,
        description: crate::services::midjourney_client::DescribeResponse,
        persona: &PersonaOverlay,
    ) -> Result<MiraStructuredReply> {
        let prompt = format!(
            "I analyzed an image. Here's what I found:\n\
            Descriptions: {:?}\n\
            Tags: {:?}\n\
            Style: {}\n\
            Mood: {}\n\
            Describe this analysis in your voice.",
            description.descriptions, description.tags, 
            description.style, description.mood
        );
        self.claude_respond(persona, &prompt, vec![]).await
    }

    async fn format_blend_response(
        &self,
        urls: Vec<String>,
        persona: &PersonaOverlay,
    ) -> Result<MiraStructuredReply> {
        let prompt = format!(
            "I blended the images together! Result: {:?}\n\
            Express excitement about this creative blend.",
            urls
        );
        self.claude_respond(persona, &prompt, vec![]).await
    }

    async fn format_logo_response(
        &self,
        urls: Vec<String>,
        company: &str,
        persona: &PersonaOverlay,
    ) -> Result<MiraStructuredReply> {
        let prompt = format!(
            "Created a logo for {}! URLs: {:?}\n\
            Share this professional design with appropriate commentary.",
            company, urls
        );
        self.claude_respond(persona, &prompt, vec![]).await
    }

    async fn format_weird_response(
        &self,
        urls: Vec<String>,
        persona: &PersonaOverlay,
    ) -> Result<MiraStructuredReply> {
        let prompt = format!(
            "MAXIMUM WEIRD MODE produced this insanity: {:?}\n\
            React to this bizarre creation in character!",
            urls
        );
        self.claude_respond(persona, &prompt, vec![]).await
    }

    async fn format_video_response(
        &self,
        url: String,
        persona: &PersonaOverlay,
    ) -> Result<MiraStructuredReply> {
        let prompt = format!(
            "Created a video: {}\n\
            Share this exciting creation in your voice.",
            url
        );
        self.claude_respond(persona, &prompt, vec![]).await
    }
    
    async fn claude_respond_to_missing_image(
        &self,
        persona: &PersonaOverlay,
    ) -> Result<MiraStructuredReply> {
        let prompt = "User asked me to describe an image but didn't provide one. \
                     Respond naturally about needing an image.";
        self.claude_respond(persona, prompt, vec![]).await
    }
    
    async fn claude_respond_to_missing_images_for_blend(
        &self,
        persona: &PersonaOverlay,
    ) -> Result<MiraStructuredReply> {
        let prompt = "User wants to blend images but didn't provide any. \
                     Respond naturally about needing at least 2 images.";
        self.claude_respond(persona, prompt, vec![]).await
    }

    async fn claude_respond_to_insufficient_images_for_blend(
        &self,
        persona: &PersonaOverlay,
    ) -> Result<MiraStructuredReply> {
        let prompt = "User wants to blend images but only provided one. \
                     Explain naturally that blending needs at least 2 images.";
        self.claude_respond(persona, prompt, vec![]).await
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
            salience: 5,  // u8
            summary: Some(text.clone()),  // Option<String>
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
