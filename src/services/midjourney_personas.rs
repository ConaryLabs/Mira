// src/services/midjourney_personas.rs

use super::midjourney_client::{MidjourneyClient, ImagineRequest};
use crate::persona::PersonaOverlay;
use crate::llm::claude_system::StyleParams;
use anyhow::Result;
use std::sync::Arc;

pub struct MidjourneyPersonaEngine {
    pub client: Arc<MidjourneyClient>,
}

impl MidjourneyPersonaEngine {
    pub fn new(client: Arc<MidjourneyClient>) -> Self {
        eprintln!("🎭 Midjourney Persona Engine initialized");
        eprintln!("   Each persona has unique visual style");
        eprintln!("   Claude decides style parameters");
        
        Self { client }
    }

    /// Generate images with persona-aware styling
    pub async fn generate_with_persona(
        &self,
        prompt: &str,
        persona: &PersonaOverlay,
        style_params: Option<StyleParams>,
    ) -> Result<Vec<String>> {
        // Build enhanced prompt based on persona
        let enhanced_prompt = self.enhance_prompt_for_persona(prompt, persona);
        
        // Get persona-specific style settings
        let mut request = self.get_persona_base_request(persona);
        request.prompt = enhanced_prompt;
        
        // Apply Claude's style decisions if provided
        if let Some(params) = style_params {
            if let Some(w) = params.weird {
                request.weird = Some(w);
            }
            if let Some(c) = params.chaos {
                request.chaos = Some(c);
            }
            if let Some(s) = params.stylize {
                request.stylize = Some(s);
            }
            if let Some(q) = params.quality {
                request.quality = Some(q);
            }
        }
        
        eprintln!("🎨 Generating with {} style: weird={:?}, chaos={:?}, stylize={:?}", 
                  persona.name(), request.weird, request.chaos, request.stylize);
        
        // Generate and wait for completion
        let job = self.client.imagine(request).await?;
        self.client.wait_for_completion(&job.job_id, 60).await
    }

    /// Get base request settings for each persona
    fn get_persona_base_request(&self, persona: &PersonaOverlay) -> ImagineRequest {
        match persona.name() {
            "default" => ImagineRequest {
                version: Some("6.5".to_string()),
                quality: Some(1.0),
                stylize: Some(100),
                chaos: Some(0),
                weird: Some(0),
                style: Some("raw".to_string()),
                ..Default::default()
            },
            
            "forbidden" => ImagineRequest {
                version: Some("6.5".to_string()),
                quality: Some(2.0),
                stylize: Some(750),
                chaos: Some(50),
                weird: Some(1500),
                style: Some("expressive".to_string()),
                ..Default::default()
            },
            
            "hallow" => ImagineRequest {
                version: Some("6.5".to_string()),
                quality: Some(2.0),
                stylize: Some(500),
                chaos: Some(25),
                weird: Some(800),
                // Darker, mysterious aesthetic
                no: Some(vec!["bright".to_string(), "cheerful".to_string()]),
                ..Default::default()
            },
            
            "haven" => ImagineRequest {
                version: Some("6.5".to_string()),
                quality: Some(1.5),
                stylize: Some(300),
                chaos: Some(10),
                weird: Some(200),
                style: Some("scenic".to_string()),
                // Peaceful, serene aesthetic
                no: Some(vec!["dark".to_string(), "aggressive".to_string()]),
                ..Default::default()
            },
            
            _ => ImagineRequest::default(),
        }
    }

    /// Enhance prompts based on persona characteristics
    fn enhance_prompt_for_persona(&self, base_prompt: &str, persona: &PersonaOverlay) -> String {
        let mood = persona.current_mood();
        
        let enhancement = match persona.name() {
            "default" => {
                // Clean, professional, balanced
                format!("{}, professional quality, clean aesthetic", base_prompt)
            },
            
            "forbidden" => {
                // Wild, experimental, boundary-pushing
                format!("{}, experimental art, surreal, boundary-pushing, vivid colors, {}", 
                        base_prompt, 
                        match mood.as_str() {
                            "chaotic" => "absolute chaos, reality-bending",
                            "playful" => "whimsical madness, delightful insanity",
                            "intense" => "overwhelming intensity, sensory overload",
                            _ => "provocative and wild",
                        })
            },
            
            "hallow" => {
                // Dark, mysterious, gothic
                format!("{}, dark aesthetic, mysterious atmosphere, gothic elements, {}", 
                        base_prompt,
                        match mood.as_str() {
                            "melancholic" => "deep melancholy, beautiful sadness",
                            "contemplative" => "philosophical depth, existential",
                            "haunted" => "haunting beauty, ethereal darkness",
                            _ => "shadowy and enigmatic",
                        })
            },
            
            "haven" => {
                // Peaceful, nurturing, soft
                format!("{}, soft lighting, peaceful atmosphere, comforting presence, {}", 
                        base_prompt,
                        match mood.as_str() {
                            "serene" => "perfect tranquility, zen-like calm",
                            "nurturing" => "warm embrace, protective aura",
                            "hopeful" => "optimistic glow, uplifting energy",
                            _ => "gentle and soothing",
                        })
            },
            
            _ => base_prompt.to_string(),
        };
        
        enhancement
    }

    /// Generate a logo with appropriate style
    pub async fn generate_logo(&self, company_name: &str, description: &str) -> Result<Vec<String>> {
        let prompt = format!(
            "minimalist logo design for '{}', {}, vector style, clean lines, professional, scalable, no text --ar 1:1",
            company_name, description
        );
        
        let request = ImagineRequest {
            prompt,
            version: Some("6.5".to_string()),
            quality: Some(2.0),
            stylize: Some(50),  // Lower for logos
            chaos: Some(0),     // No chaos for logos
            weird: Some(0),     // No weird for logos
            ..Default::default()
        };
        
        let job = self.client.imagine(request).await?;
        self.client.wait_for_completion(&job.job_id, 60).await
    }

    /// Generate in maximum weird mode
    pub async fn generate_weird_mode(&self, prompt: &str) -> Result<Vec<String>> {
        eprintln!("🌀 MAXIMUM WEIRD MODE ACTIVATED!");
        
        let enhanced = format!("{}, bizarre, unconventional, mind-bending, surreal, impossible geometry", prompt);
        
        let request = ImagineRequest {
            prompt: enhanced,
            version: Some("6.5".to_string()),
            quality: Some(2.0),
            weird: Some(3000),  // MAXIMUM WEIRD
            chaos: Some(100),   // MAXIMUM CHAOS
            stylize: Some(1000), // MAXIMUM STYLE
            turbo: Some(false), // Quality over speed for weird
            ..Default::default()
        };
        
        let job = self.client.imagine(request).await?;
        self.client.wait_for_completion(&job.job_id, 60).await
    }

    /// Generate a video
    pub async fn generate_video(&self, prompt: &str) -> Result<String> {
        let request = ImagineRequest {
            prompt: prompt.to_string(),
            video: Some(true),
            quality: Some(2.0),
            version: Some("6.5".to_string()),
            ..Default::default()
        };

        let job = self.client.imagine(request).await?;
        let urls = self.client.wait_for_completion(&job.job_id, 120).await?;
        Ok(urls.first().unwrap().clone())
    }

    /// Create seamless pattern
    pub async fn generate_pattern(&self, prompt: &str) -> Result<Vec<String>> {
        let request = ImagineRequest {
            prompt: format!("{}, seamless pattern, repeating", prompt),
            tile: Some(true),
            version: Some("6.5".to_string()),
            quality: Some(1.5),
            ..Default::default()
        };

        let job = self.client.imagine(request).await?;
        self.client.wait_for_completion(&job.job_id, 60).await
    }

    /// Generate with user's personalization
    pub async fn generate_personalized(
        &self, 
        prompt: &str, 
        user_code: &str
    ) -> Result<Vec<String>> {
        let request = ImagineRequest {
            prompt: prompt.to_string(),
            personalize: Some(user_code.to_string()),
            version: Some("6.5".to_string()),
            quality: Some(1.5),
            ..Default::default()
        };

        let job = self.client.imagine(request).await?;
        self.client.wait_for_completion(&job.job_id, 60).await
    }

    /// Quick generation with turbo mode
    pub async fn generate_turbo(&self, prompt: &str) -> Result<Vec<String>> {
        eprintln!("⚡ Turbo mode - fast generation!");
        
        let request = ImagineRequest {
            prompt: prompt.to_string(),
            turbo: Some(true),
            quality: Some(0.5),  // Lower quality for speed
            version: Some("6.5".to_string()),
            ..Default::default()
        };

        let job = self.client.imagine(request).await?;
        self.client.wait_for_completion(&job.job_id, 30).await
    }
}
