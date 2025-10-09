// src/llm/responses/image.rs
// Image generation using gpt-image-1 via unified /v1/responses API

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn};

/// Manages image generation through the unified Responses API
#[derive(Clone)]
pub struct ImageGenerationManager {
    client: Client,
    api_key: String,
}

impl ImageGenerationManager {
    pub fn new(api_key: String) -> Self {
        Self { 
            client: Client::new(),
            api_key,
        }
    }

    /// Generate images using GPT-Image-1 via the unified /v1/responses endpoint
    pub async fn generate_images(
        &self,
        prompt: &str,
        options: ImageOptions,
    ) -> Result<ImageGenerationResponse> {
        info!("Generating image with GPT-Image-1 via /v1/responses");
        info!("Prompt: {}", prompt);
        
        // Build the unified request body
        let mut body = json!({
            "model": "gpt-image-1",
            "input": [{
                "role": "user",
                "content": [{ 
                    "type": "input_text", 
                    "text": prompt 
                }]
            }],
            "max_output_tokens": 1,
        });

        // Add image generation parameters at top level with proper prefix
        if let Some(n) = options.n {
            body["image_n"] = json!(n);
            info!("Number of images: {}", n);
        }
        if let Some(ref size) = options.size {
            body["image_size"] = json!(size);
            info!("Size: {}", size);
        }
        if let Some(ref quality) = options.quality {
            body["image_quality"] = json!(quality);
            info!("Quality: {}", quality);
        }
        if let Some(ref style) = options.style {
            body["image_style"] = json!(style);
            info!("Style: {}", style);
        }

        // Make the API call
        let response = self.client
            .post("https://api.openai.com/v1/responses")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to call GPT-Image-1 via /v1/responses")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("Image generation API error {}: {}", status, error_text));
        }

        let response_json: Value = response.json().await?;

        // Parse the response to extract image URLs
        let images = self.parse_images_from_response(&response_json)?;

        if images.is_empty() {
            return Err(anyhow::anyhow!("No images returned from GPT-Image-1"));
        }

        info!("Successfully generated {} image(s)", images.len());
        
        Ok(ImageGenerationResponse {
            images,
            model: "gpt-image-1".to_string(),
        })
    }

    /// Parse image URLs from the unified response format
    fn parse_images_from_response(&self, response: &Value) -> Result<Vec<ImageData>> {
        let mut images = Vec::new();

        // Try the unified output format first
        if let Some(output) = response.get("output").and_then(|o| o.as_array()) {
            for item in output {
                if let Some(url) = item.get("url").and_then(|u| u.as_str()) {
                    images.push(ImageData {
                        url: url.to_string(),
                        revised_prompt: item.get("revised_prompt")
                            .and_then(|p| p.as_str())
                            .map(|s| s.to_string()),
                    });
                }
            }
        }

        // Fallback: try data array format (OpenAI DALL-E format)
        if images.is_empty() {
            if let Some(data) = response.get("data").and_then(|d| d.as_array()) {
                for item in data {
                    if let Some(url) = item.get("url").and_then(|u| u.as_str()) {
                        images.push(ImageData {
                            url: url.to_string(),
                            revised_prompt: item.get("revised_prompt")
                                .and_then(|p| p.as_str())
                                .map(|s| s.to_string()),
                        });
                    }
                }
            }
        }

        if images.is_empty() {
            warn!("Could not find image URLs in response: {:?}", response);
        }

        Ok(images)
    }
}

/// Options for image generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageOptions {
    pub n: Option<u8>,           // Number of images (1-10)
    pub size: Option<String>,    // Size: "1024x1024", "1792x1024", "1024x1792"
    pub quality: Option<String>, // Quality: "standard" or "hd"
    pub style: Option<String>,   // Style: "vivid" or "natural"
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            n: Some(1),
            size: Some("1024x1024".to_string()),
            quality: Some("standard".to_string()),
            style: Some("vivid".to_string()),
        }
    }
}

impl ImageOptions {
    /// Validate the options
    pub fn validate(&self) -> Result<()> {
        if let Some(n) = self.n {
            if n == 0 || n > 10 {
                return Err(anyhow::anyhow!("Number of images must be between 1 and 10"));
            }
        }
        
        if let Some(ref size) = self.size {
            let valid_sizes = ["1024x1024", "1792x1024", "1024x1792"];
            if !valid_sizes.contains(&size.as_str()) {
                return Err(anyhow::anyhow!("Invalid image size: {}", size));
            }
        }
        
        if let Some(ref quality) = self.quality {
            if quality != "standard" && quality != "hd" {
                return Err(anyhow::anyhow!("Quality must be 'standard' or 'hd'"));
            }
        }
        
        if let Some(ref style) = self.style {
            if style != "vivid" && style != "natural" {
                return Err(anyhow::anyhow!("Style must be 'vivid' or 'natural'"));
            }
        }
        
        Ok(())
    }
}

/// Response from image generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenerationResponse {
    pub images: Vec<ImageData>,
    pub model: String,
}

impl ImageGenerationResponse {
    /// Get URLs from the response
    pub fn urls(&self) -> Vec<&str> {
        self.images.iter().map(|img| img.url.as_str()).collect()
    }
}

/// Individual image data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    pub url: String,
    pub revised_prompt: Option<String>,
}
