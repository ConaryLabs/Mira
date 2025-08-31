// src/llm/responses/image.rs
// Phase 5: Image generation using gpt-image-1 via unified /v1/responses API

use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::llm::client::OpenAIClient;

/// Manages image generation through the unified Responses API
#[derive(Clone)]
pub struct ImageGenerationManager {
    client: Arc<OpenAIClient>,
}

impl ImageGenerationManager {
    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self { client }
    }

    /// Generate images using GPT-Image-1 via the unified /v1/responses endpoint
    pub async fn generate_images(
        &self,
        prompt: &str,
        options: ImageOptions,
    ) -> Result<ImageGenerationResponse> {
        info!("🎨 Generating image with GPT-Image-1 via /v1/responses");
        info!("   Prompt: {}", prompt);
        
        // Build parameters for image generation
        let mut image_params = json!({});
        
        if let Some(n) = options.n {
            image_params["n"] = json!(n);
            info!("   Number of images: {}", n);
        }
        
        if let Some(ref size) = options.size {
            image_params["size"] = json!(size);
            info!("   Size: {}", size);
        }
        
        if let Some(ref quality) = options.quality {
            image_params["quality"] = json!(quality);
            info!("   Quality: {}", quality);
        }
        
        if let Some(ref style) = options.style {
            image_params["style"] = json!(style);
            info!("   Style: {}", style);
        }

        // Build the unified request body
        let body = json!({
            "model": "gpt-image-1",
            "input": [{
                "role": "user",
                "content": [{ 
                    "type": "input_text", 
                    "text": prompt 
                }]
            }],
            "parameters": {
                "image_generation": image_params,
                "max_output_tokens": 1,  // Minimal tokens for image response
            }
        });

        // Make the API call
        let response = self.client.post_response(body).await
            .context("Failed to call GPT-Image-1 via /v1/responses")?;

        // Parse the response to extract image URLs
        let images = self.parse_images_from_response(&response)?;

        if images.is_empty() {
            return Err(anyhow::anyhow!("No images returned from GPT-Image-1"));
        }

        info!("✅ Successfully generated {} image(s)", images.len());
        info!("⚠️  Note: Image URLs are temporary and typically valid for ~60 minutes");

        Ok(ImageGenerationResponse { images })
    }

    /// Parse image URLs from the unified response format
    fn parse_images_from_response(&self, v: &Value) -> Result<Vec<GeneratedImage>> {
        let mut images = Vec::new();

        // Primary path: Check the unified output array
        if let Some(output_array) = v.get("output").and_then(|o| o.as_array()) {
            for item in output_array {
                if let Some(item_type) = item.get("type").and_then(|t| t.as_str()) {
                    match item_type {
                        "image_url" => {
                            // Direct image URL in output
                            if let Some(url) = self.extract_image_url(item) {
                                images.push(GeneratedImage {
                                    url: Some(url),
                                    b64_json: None,
                                    revised_prompt: self.extract_revised_prompt(item),
                                });
                            }
                        }
                        "image" => {
                            // Alternative format with nested structure
                            if let Some(url) = item.get("url").and_then(|u| u.as_str()) {
                                images.push(GeneratedImage {
                                    url: Some(url.to_string()),
                                    b64_json: None,
                                    revised_prompt: self.extract_revised_prompt(item),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Fallback path: Check choices[0].message.content for compatibility
        if images.is_empty() {
            if let Some(content_array) = v
                .pointer("/choices/0/message/content")
                .and_then(|c| c.as_array())
            {
                for part in content_array {
                    if part.get("type").and_then(|t| t.as_str()) == Some("image_url") {
                        if let Some(url) = self.extract_image_url(part) {
                            images.push(GeneratedImage {
                                url: Some(url),
                                b64_json: None,
                                revised_prompt: self.extract_revised_prompt(part),
                            });
                        }
                    }
                }
            }
        }

        // Legacy fallback: Check for data array (old DALL-E format)
        if images.is_empty() {
            if let Some(data_array) = v.get("data").and_then(|d| d.as_array()) {
                warn!("Using legacy data array format - consider updating API");
                for item in data_array {
                    if let Some(url) = item.get("url").and_then(|u| u.as_str()) {
                        images.push(GeneratedImage {
                            url: Some(url.to_string()),
                            b64_json: item.get("b64_json").and_then(|b| b.as_str()).map(String::from),
                            revised_prompt: item.get("revised_prompt").and_then(|r| r.as_str()).map(String::from),
                        });
                    }
                }
            }
        }

        Ok(images)
    }

    /// Extract image URL from various nested structures
    fn extract_image_url(&self, item: &Value) -> Option<String> {
        // Try nested image_url.url structure
        if let Some(url) = item
            .get("image_url")
            .and_then(|img| img.get("url"))
            .and_then(|u| u.as_str())
        {
            return Some(url.to_string());
        }

        // Try direct url field
        if let Some(url) = item.get("url").and_then(|u| u.as_str()) {
            return Some(url.to_string());
        }

        None
    }

    /// Extract revised prompt if present
    fn extract_revised_prompt(&self, item: &Value) -> Option<String> {
        item.get("revised_prompt")
            .and_then(|r| r.as_str())
            .map(String::from)
    }
}

/// Options for image generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageOptions {
    /// Number of images to generate (1-10)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    
    /// Size of the generated images
    /// Valid options: "256x256", "512x512", "1024x1024", "1792x1024", "1024x1792"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    
    /// Quality of the image
    /// Valid options: "standard", "hd"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    
    /// Style of the generated images
    /// Valid options: "vivid", "natural"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
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
    /// Create options for a single standard image
    pub fn single() -> Self {
        Self::default()
    }

    /// Create options for HD quality images
    pub fn hd() -> Self {
        Self {
            quality: Some("hd".to_string()),
            ..Self::default()
        }
    }

    /// Create options for multiple images
    pub fn multiple(count: u32) -> Self {
        Self {
            n: Some(count.min(10).max(1)),
            ..Self::default()
        }
    }

    /// Validate options against API constraints
    pub fn validate(&self) -> Result<()> {
        if let Some(n) = self.n {
            if n < 1 || n > 10 {
                return Err(anyhow::anyhow!("Number of images must be between 1 and 10"));
            }
        }

        if let Some(ref size) = self.size {
            let valid_sizes = vec![
                "256x256", "512x512", "1024x1024", 
                "1792x1024", "1024x1792"
            ];
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
    pub images: Vec<GeneratedImage>,
}

impl ImageGenerationResponse {
    /// Get the first image URL if available
    pub fn first_url(&self) -> Option<&str> {
        self.images.first()
            .and_then(|img| img.url.as_deref())
    }

    /// Get all image URLs
    pub fn urls(&self) -> Vec<&str> {
        self.images.iter()
            .filter_map(|img| img.url.as_deref())
            .collect()
    }
}

/// A single generated image
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedImage {
    /// URL of the generated image (temporary, ~60 minutes)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    
    /// Base64 encoded image data (if requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b64_json: Option<String>,
    
    /// Revised prompt used by the model (if different from input)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revised_prompt: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_options_validation() {
        // Valid options
        assert!(ImageOptions::default().validate().is_ok());
        assert!(ImageOptions::hd().validate().is_ok());
        assert!(ImageOptions::multiple(5).validate().is_ok());

        // Invalid number of images
        let invalid_count = ImageOptions {
            n: Some(15),
            ..Default::default()
        };
        assert!(invalid_count.validate().is_err());

        // Invalid size
        let invalid_size = ImageOptions {
            size: Some("2048x2048".to_string()),
            ..Default::default()
        };
        assert!(invalid_size.validate().is_err());

        // Invalid quality
        let invalid_quality = ImageOptions {
            quality: Some("ultra".to_string()),
            ..Default::default()
        };
        assert!(invalid_quality.validate().is_err());
    }

    #[test]
    fn test_response_parsing() {
        // Test unified output format
        let response = json!({
            "output": [{
                "type": "image_url",
                "image_url": {
                    "url": "https://example.com/image1.png"
                }
            }]
        });

        let manager = ImageGenerationManager {
            client: OpenAIClient::new().unwrap(),
        };

        let images = manager.parse_images_from_response(&response).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].url.as_deref(), Some("https://example.com/image1.png"));
    }
}
