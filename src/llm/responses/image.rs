// src/llm/responses/image.rs
//! Image generation using gpt-image-1 via the unified /v1/responses API.

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::llm::client::OpenAIClient;

#[derive(Clone)]
pub struct ImageGenerationManager {
    client: Arc<OpenAIClient>,
}

impl ImageGenerationManager {
    pub fn new(client: Arc<OpenAIClient>) -> Self {
        Self { client }
    }

    /// Generate images from a text prompt.
    /// - `options.n` controls count (default 1)
    /// - `options.size` like "1024x1024"
    /// - `options.quality` "standard" | "hd"
    /// - `options.style` "vivid" | "natural"
    pub async fn generate_images(
        &self,
        prompt: &str,
        options: ImageOptions,
    ) -> Result<ImageGenerationResponse> {
        let mut parameters = json!({});
        if let Some(n) = options.n { parameters["n"] = json!(n); }
        if let Some(sz) = &options.size { parameters["size"] = json!(sz); }
        if let Some(q) = &options.quality { parameters["quality"] = json!(q); }
        if let Some(st) = &options.style { parameters["style"] = json!(st); }

        let body = json!({
            "model": "gpt-image-1",
            "input": [{
                "role": "user",
                "content": [{ "type": "input_text", "text": prompt }]
            }],
            "parameters": parameters
        });

        let v = self.client.post_response(body).await?;
        let images = parse_images_from_response(&v);

        if images.is_empty() {
            return Err(anyhow::anyhow!("No images returned from gpt-image-1"));
        }

        Ok(ImageGenerationResponse { images })
    }
}

fn parse_images_from_response(v: &Value) -> Vec<GeneratedImage> {
    // Preferred: unified `output` array with image_url parts
    if let Some(arr) = v.get("output").and_then(|o| o.as_array()) {
        let mut out = Vec::new();
        for item in arr {
            if item.get("type").and_then(|t| t.as_str()) == Some("image_url") {
                if let Some(url) = item
                    .get("image_url")
                    .and_then(|u| u.get("url"))
                    .and_then(|s| s.as_str())
                {
                    out.push(GeneratedImage {
                        url: Some(url.to_string()),
                        b64_json: None,
                        revised_prompt: None,
                    });
                }
            }
        }
        if !out.is_empty() {
            return out;
        }
    }

    // Fallback: legacy choices[0].message.content parts
    if let Some(parts) = v
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_array())
    {
        let mut out = Vec::new();
        for part in parts {
            if part.get("type").and_then(|t| t.as_str()) == Some("image_url") {
                if let Some(url) = part
                    .get("image_url")
                    .and_then(|u| u.get("url"))
                    .and_then(|s| s.as_str())
                {
                    out.push(GeneratedImage {
                        url: Some(url.to_string()),
                        b64_json: None,
                        revised_prompt: None,
                    });
                }
            }
        }
        return out;
    }

    Vec::new()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageOptions {
    pub n: Option<u32>,
    pub size: Option<String>,    // e.g., "1024x1024"
    pub quality: Option<String>, // "standard" | "hd"
    pub style: Option<String>,   // "vivid" | "natural"
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self {
            n: Some(1),
            size: Some("1024x1024".into()),
            quality: None,
            style: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenerationResponse {
    pub images: Vec<GeneratedImage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedImage {
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b64_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revised_prompt: Option<String>,
}
