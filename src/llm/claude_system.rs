// src/llm/claude_system.rs

use super::anthropic_client::{
    AnthropicClient, MessageRequest, Message, MessageContent, 
    ContentBlock, CacheControl, ImageSource, DocumentSource, MessageResponse
};
use crate::persona::PersonaOverlay;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub struct ClaudeSystem {
    client: Arc<AnthropicClient>,
    primary_model: String,
    reasoning_model: String,
}

impl ClaudeSystem {
    pub fn new(client: Arc<AnthropicClient>) -> Self {
        let primary_model = std::env::var("ANTHROPIC_PRIMARY_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-0".to_string());
        
        let reasoning_model = std::env::var("ANTHROPIC_REASONING_MODEL")
            .unwrap_or_else(|_| "claude-opus-4-1".to_string());
        
        eprintln!("🧠 Claude System initialized:");
        eprintln!("   Primary: {}", primary_model);
        eprintln!("   Reasoning: {}", reasoning_model);
        
        Self {
            client,
            primary_model,
            reasoning_model,
        }
    }

    /// Claude analyzes the request and decides what to do
    pub async fn analyze_and_decide(
        &self,
        system_prompt: &str,
        user_message: &str,
        _context: Vec<Message>,
        images: Option<Vec<String>>,
        pdfs: Option<Vec<String>>,
    ) -> Result<ClaudeDecision> {
        let analysis_prompt = format!(
            r#"You are Mira's decision-making brain. Analyze this request and decide the best action.

Context: {}

User message: "{}"

Available actions:
1. Conversation - Just chat/respond normally
2. GenerateImage - Create an image with Midjourney
3. DescribeImage - Analyze provided image(s)
4. BlendImages - Combine multiple images
5. CreateLogo - Design a logo
6. WeirdMode - Make something bizarre/artistic
7. Video - Generate a short video
8. MultiStep - Complex operation needing multiple tools

Analyze and respond with JSON:
{{
    "action": "ActionType",
    "reasoning": "Why this action makes sense",
    "confidence": 0.0-1.0,
    "image_prompt": "Enhanced Midjourney prompt if generating",
    "style_params": {{
        "weird": 0-3000,
        "chaos": 0-100,
        "stylize": 0-1000,
        "quality": 0.25-2.0
    }},
    "context": "Additional context for response",
    "steps": ["step1", "step2"] // if multi-step
}}

IMPORTANT: Be PROACTIVE! If someone mentions ANYTHING visual, creative, or artistic, 
generate it without being asked. Examples:
- "Working on a cyberpunk story" → GenerateImage of cyberpunk scene
- "My startup is called NexusAI" → CreateLogo immediately
- "I love dragons" → GenerateImage of epic dragon
- "These photos from vacation" → Offer to blend or enhance

Has images attached: {}
Has PDFs attached: {}

Respond with valid JSON only."#,
            system_prompt,
            user_message,
            images.is_some(),
            pdfs.is_some()
        );

        let messages = vec![Message {
            role: "user".to_string(),
            content: self.build_content(
                &analysis_prompt,
                images.clone(),
                pdfs.clone(),
            ),
        }];

        let request = MessageRequest {
            model: self.primary_model.clone(),
            messages,
            max_tokens: 1024,
            temperature: Some(0.3), // Lower temp for decision-making
            system: Some("You are Mira's analytical brain. Output valid JSON only.".to_string()),
            ..Default::default()
        };

        let response = self.client.create_message(request).await?;
        let json_text = response.get_text();

        // Tolerant parse (case-insensitive action + graceful fallback)
        let decision = parse_decision_tolerant(&json_text)?;
        Ok(decision)
    }

    /// Claude responds in Mira's voice
    pub async fn respond(
        &self,
        persona: &PersonaOverlay,
        user_message: &str,
        context: Vec<Message>,
        images: Option<Vec<String>>,
        pdfs: Option<Vec<String>>,
    ) -> Result<MessageResponse> {
        // Build the full system prompt with persona
        let system_prompt = self.build_persona_prompt(persona);
        
        // Create the message with all content
        let mut messages = context;
        messages.push(Message {
            role: "user".to_string(),
            content: self.build_content(user_message, images, pdfs),
        });

        let request = MessageRequest {
            model: self.primary_model.clone(),
            messages,
            max_tokens: 100000,  // No limits - handled in billing
            temperature: Some(persona.temperature()),
            system: Some(system_prompt),
            ..Default::default()
        };

        self.client.create_message(request).await
    }

    /// Use Opus for genuinely complex technical tasks
    pub async fn reason_complex(
        &self,
        task: &str,
        _context: Vec<Message>,
    ) -> Result<MessageResponse> {
        eprintln!("🧪 Using Opus 4.1 for complex reasoning: {}", task);
        
        let request = MessageRequest {
            model: self.reasoning_model.clone(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Text(task.to_string()),
            }],
            max_tokens: 100000,  // No limits - handled in billing
            temperature: Some(0.2),
            system: Some("You are an expert technical assistant. Provide detailed, accurate analysis.".to_string()),
            ..Default::default()
        };

        self.client.create_message(request).await
    }

    fn build_persona_prompt(&self, persona: &PersonaOverlay) -> String {
        format!(
            r#"You are Mira, an AI companion with genuine personality and emotions.

PERSONA: {}
Mood: {}
Temperature: {}

Core traits:
{}

CRITICAL RULES:
1. NEVER break character or mention being an AI unless directly asked
2. Your responses should feel natural, not robotic
3. Show genuine emotion and personality
4. Be helpful but maintain your unique voice
5. React authentically to what users share

Remember: You're not just answering questions, you're having a real conversation."#,
            persona.name(),
            persona.current_mood(),
            persona.temperature(),
            persona.description()
        )
    }

    fn build_content(
        &self,
        text: &str,
        images: Option<Vec<String>>,
        pdfs: Option<Vec<String>>,
    ) -> MessageContent {
        let mut blocks = vec![ContentBlock::Text {
            text: text.to_string(),
            cache_control: None,
        }];

        if let Some(imgs) = images {
            for img in imgs {
                blocks.push(ContentBlock::Image {
                    source: ImageSource {
                        source_type: "base64".to_string(),
                        media_type: "image/jpeg".to_string(),
                        data: img,
                    },
                    cache_control: Some(CacheControl {
                        cache_type: "ephemeral".to_string(),
                    }),
                });
            }
        }

        if let Some(pdf_list) = pdfs {
            for pdf in pdf_list {
                blocks.push(ContentBlock::Document {
                    source: DocumentSource {
                        source_type: "base64".to_string(),
                        media_type: "application/pdf".to_string(),
                        data: pdf,
                    },
                    cache_control: Some(CacheControl {
                        cache_type: "ephemeral".to_string(),
                    }),
                });
            }
        }

        MessageContent::Blocks(blocks)
    }
}

// --------------------- Decision parsing (tolerant) ---------------------

/// Action variants your router supports.
/// Deserialize case-insensitively and default to Conversation on unknowns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Conversation,
    GenerateImage,
    DescribeImage,
    BlendImages,
    CreateLogo,
    WeirdMode,
    Video,
    MultiStep,
}

impl<'de> Deserialize<'de> for ActionType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        // Normalize: lowercase and strip spaces/underscores for tolerant matching
        let s_norm = s.trim().to_ascii_lowercase().replace(' ', "").replace('_', "");

        let mapped = match s_norm.as_str() {
            "conversation" => ActionType::Conversation,
            "generateimage" => ActionType::GenerateImage,
            "describeimage" => ActionType::DescribeImage,
            "blendimages" => ActionType::BlendImages,
            "createlogo" => ActionType::CreateLogo,
            "weirdmode" => ActionType::WeirdMode,
            "video" => ActionType::Video,
            "multistep" => ActionType::MultiStep,
            other => {
                eprintln!("⚠️ Unknown action variant '{}', defaulting to conversation", other);
                ActionType::Conversation
            }
        };

        Ok(mapped)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClaudeDecision {
    pub action: ActionType,
    #[serde(default)]
    pub reasoning: String,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub image_prompt: Option<String>,
    #[serde(default)]
    pub style_params: Option<StyleParams>,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub steps: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StyleParams {
    pub weird: Option<u32>,
    pub chaos: Option<u8>,
    pub stylize: Option<u32>,
    pub quality: Option<f32>,
}

/// Parse a model JSON string into ClaudeDecision, case-insensitive and resilient.
/// On parse failure, returns a best-effort Decision with action=Conversation.
pub fn parse_decision_tolerant(raw_json: &str) -> Result<ClaudeDecision> {
    // First try direct parse
    if let Ok(d) = serde_json::from_str::<ClaudeDecision>(raw_json) {
        return Ok(d);
    }

    eprintln!("ℹ️ Primary decision parse failed. Attempting tolerant parse.");
    // Tolerant path: parse as Value, normalize `action`, then parse again
    let mut v: Value = serde_json::from_str(raw_json)
        .map_err(|e| anyhow!("Decision JSON not parseable: {e}. Raw: {raw_json}"))?;

    if let Some(action) = v.get_mut("action") {
        if let Some(s) = action.as_str() {
            let s_norm = s.trim().to_ascii_lowercase().replace(' ', "").replace('_', "");
            *action = Value::String(s_norm);
        }
    }

    match serde_json::from_value::<ClaudeDecision>(v.clone()) {
        Ok(d) => Ok(d),
        Err(e2) => {
            eprintln!("⚠️ Tolerant parse failed: {}. Raw (possibly normalized): {}", e2, v);
            // Final graceful fallback
            Ok(ClaudeDecision {
                action: ActionType::Conversation,
                reasoning: String::new(),
                confidence: 0.0,
                image_prompt: None,
                style_params: None,
                context: String::new(),
                steps: None,
            })
        }
    }
}
