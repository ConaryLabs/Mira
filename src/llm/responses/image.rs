// src/llm/responses/image.rs - Image generation using Responses API with gpt-image-1

use crate::llm::client::OpenAIClient;
use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use std::sync::Arc;

/// Image generation manager that integrates with your existing Responses API
pub struct ImageGenerationManager {
    client: Arc<OpenAIClient>,
}

impl ImageGenerationManager {
    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self { client }
    }

    /// Generate images using the Responses API with gpt-image-1
    pub async fn generate_images(
        &self,
        prompt: &str,
        options: ImageGenerationOptions,
    ) -> Result<ImageGenerationResponse> {
        let request = self.build_request(prompt, options);
        
        eprintln!("🎨 Generating images via Responses API with gpt-image-1");
        
        let response = self.client
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.client.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("Failed to send image generation request")?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            eprintln!("❌ Image generation failed: {}", error_text);
            return Err(anyhow::anyhow!("Image generation failed: {}", error_text));
        }

        let response_json: Value = response.json().await
            .context("Failed to parse image generation response")?;
        
        // Parse the response into our structure
        let parsed_response = self.parse_response(response_json)?;
        
        eprintln!("✅ Successfully generated {} image(s)", parsed_response.images.len());
        if !parsed_response.images.is_empty() {
            eprintln!("⚠️  Image URLs are valid for 60 minutes only");
        }
        
        Ok(parsed_response)
    }

    /// Build a Responses API request for image generation
    fn build_request(&self, prompt: &str, options: ImageGenerationOptions) -> Value {
        json!({
            "model": "gpt-image-1",
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "modalities": ["text", "image"],
            "image_generation": {
                "n": options.n.unwrap_or(1),
                "size": options.size.unwrap_or_else(|| "1024x1024".to_string()),
                "quality": options.quality.unwrap_or_else(|| "standard".to_string()),
                "style": options.style.unwrap_or_else(|| "vivid".to_string()),
                "response_format": options.response_format.unwrap_or_else(|| "url".to_string()),
            }
        })
    }

    /// Parse the Responses API response to extract images
    fn parse_response(&self, response: Value) -> Result<ImageGenerationResponse> {
        let mut images = Vec::new();
        let mut text_content = None;
        
        if let Some(choices) = response["choices"].as_array() {
            for choice in choices {
                if let Some(message) = choice.get("message") {
                    if let Some(content) = message["content"].as_array() {
                        for item in content {
                            match item["type"].as_str() {
                                Some("image_url") => {
                                    if let Some(url) = item["image_url"]["url"].as_str() {
                                        images.push(GeneratedImage {
                                            url: Some(url.to_string()),
                                            b64_json: None,
                                            revised_prompt: None,
                                        });
                                    }
                                },
                                Some("text") => {
                                    if let Some(text) = item["text"].as_str() {
                                        text_content = Some(text.to_string());
                                    }
                                },
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        
        Ok(ImageGenerationResponse {
            images,
            text_content,
            model: response["model"].as_str().unwrap_or("gpt-image-1").to_string(),
            usage: response["usage"].as_object().map(|u| ImageUsage {
                prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
                total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
            }),
        })
    }

    /// Simplified method for common use case
    pub async fn generate_single_image(
        &self,
        prompt: &str,
        quality: Option<&str>,
    ) -> Result<String> {
        let options = ImageGenerationOptions {
            n: Some(1),
            quality: quality.map(String::from),
            ..Default::default()
        };
        
        let response = self.generate_images(prompt, options).await?;
        
        response.images
            .first()
            .and_then(|img| img.url.clone())
            .ok_or_else(|| anyhow::anyhow!("No image URL in response"))
    }
}

// Request/Response structures

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImageGenerationOptions {
    pub n: Option<u8>,                      // 1-10 images
    pub size: Option<String>,               // "1024x1024", "1024x1792", "1792x1024", "512x512"
    pub quality: Option<String>,            // "standard" or "hd"
    pub style: Option<String>,               // "vivid" or "natural"
    pub response_format: Option<String>,    // "url" or "b64_json"
}

impl ImageGenerationOptions {
    pub fn hd() -> Self {
        Self {
            quality: Some("hd".to_string()),
            ..Default::default()
        }
    }
    
    pub fn natural() -> Self {
        Self {
            style: Some("natural".to_string()),
            ..Default::default()
        }
    }
    
    pub fn portrait() -> Self {
        Self {
            size: Some("1024x1792".to_string()),
            ..Default::default()
        }
    }
    
    pub fn landscape() -> Self {
        Self {
            size: Some("1792x1024".to_string()),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenerationResponse {
    pub images: Vec<GeneratedImage>,
    pub text_content: Option<String>,  // Any text the model included
    pub model: String,
    pub usage: Option<ImageUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedImage {
    pub url: Option<String>,           // Valid for 60 minutes
    pub b64_json: Option<String>,      // Base64 encoded image
    pub revised_prompt: Option<String>, // How the model interpreted the prompt
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

// Builder pattern for convenience

pub struct ImageRequestBuilder {
    prompt: String,
    options: ImageGenerationOptions,
}

impl ImageRequestBuilder {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            options: ImageGenerationOptions::default(),
        }
    }
    
    pub fn count(mut self, n: u8) -> Self {
        self.options.n = Some(n.min(10).max(1));
        self
    }
    
    pub fn size(mut self, size: &str) -> Self {
        self.options.size = Some(size.to_string());
        self
    }
    
    pub fn quality(mut self, quality: &str) -> Self {
        self.options.quality = Some(quality.to_string());
        self
    }
    
    pub fn style(mut self, style: &str) -> Self {
        self.options.style = Some(style.to_string());
        self
    }
    
    pub async fn generate(self, manager: &ImageGenerationManager) -> Result<ImageGenerationResponse> {
        manager.generate_images(&self.prompt, self.options).await
    }
}
