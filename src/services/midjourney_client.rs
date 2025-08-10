// src/services/midjourney_client.rs

use anyhow::{Result, Context};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use tokio::time::{sleep, Duration};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

pub struct MidjourneyClient {
    client: Client,
    api_key: String,
    api_url: String,
    webhook_url: Option<String>,
}

// Core generation request with ALL v6.5 parameters
#[derive(Debug, Serialize, Default)]
pub struct ImagineRequest {
    pub prompt: String,
    
    // Basic parameters
    pub aspect_ratio: Option<String>,     // "1:1", "16:9", "9:16", etc.
    pub chaos: Option<u8>,                // 0-100
    pub quality: Option<f32>,             // 0.25, 0.5, 1, 2
    pub stylize: Option<u32>,             // 0-1000
    pub weird: Option<u32>,               // 0-3000 (v6.5 max)
    pub version: Option<String>,          // "6.5", "niji 6"
    
    // Style parameters
    pub style: Option<String>,            // "raw", "cute", "expressive", "scenic"
    pub sref: Option<Vec<String>>,        // Style references
    pub sw: Option<u32>,                  // Style weight 0-1000
    pub cref: Option<Vec<String>>,        // Character references
    pub cw: Option<u32>,                  // Character weight 0-100
    
    // Advanced features
    pub tile: Option<bool>,               // Seamless patterns
    pub turbo: Option<bool>,              // Fast mode
    pub subtle: Option<bool>,             // Subtle variations
    pub video: Option<bool>,              // Generate video
    pub repeat: Option<u8>,               // Repeat generations 1-40
    
    // Exclusions
    pub no: Option<Vec<String>>,          // Things to exclude
    
    // Seeds and personalization
    pub seed: Option<u64>,
    pub personalize: Option<String>,      // Personal style code
}

// Describe (image to prompt)
#[derive(Debug, Serialize)]
pub struct DescribeRequest {
    pub image: String,                    // Base64 encoded
    pub detail_level: Option<String>,     // "brief", "detailed", "verbose"
}

#[derive(Debug, Deserialize)]
pub struct DescribeResponse {
    pub descriptions: Vec<String>,
    pub tags: Vec<String>,
    pub style: String,
    pub mood: String,
}

// Blend multiple images
#[derive(Debug, Serialize)]
pub struct BlendRequest {
    pub images: Vec<String>,              // 2-5 base64 images
    pub dimensions: Option<String>,
    pub blend_mode: Option<String>,       // "default", "soft", "hard"
}

// FaceSwap
#[derive(Debug, Serialize)]
pub struct FaceSwapRequest {
    pub source_image: String,
    pub target_image: String,
    pub preserve_target: Option<bool>,
}

// Zoom/Pan for expansion
#[derive(Debug, Serialize)]
pub struct ZoomRequest {
    pub job_id: String,
    pub direction: String,                // "out", "custom"
    pub custom_zoom: Option<f32>,         // 1.0 to 2.0
}

#[derive(Debug, Serialize)]
pub struct PanRequest {
    pub job_id: String,
    pub direction: String,                // "left", "right", "up", "down"
}

// Inpaint for regional editing
#[derive(Debug, Serialize)]
pub struct InpaintRequest {
    pub job_id: String,
    pub prompt: String,
    pub mask: String,                     // Base64 mask image
}

// Response structure
#[derive(Debug, Deserialize)]
pub struct ImagineResponse {
    pub job_id: String,
    pub status: String,
    pub progress: Option<u8>,
    pub image_urls: Option<Vec<String>>,
    pub video_url: Option<String>,
    pub seed: Option<u64>,
}

impl MidjourneyClient {
    pub fn new() -> Result<Self> {
        let api_key = env::var("MIDJOURNEY_API_KEY")
            .context("MIDJOURNEY_API_KEY must be set")?;
        
        let api_url = env::var("MIDJOURNEY_API_URL")
            .unwrap_or_else(|_| "https://api.midjourney.com/v2".to_string());
        
        let webhook_url = env::var("MIDJOURNEY_WEBHOOK_URL").ok();
        
        eprintln!("🎨 Midjourney v6.5 client initialized");
        eprintln!("   API URL: {}", api_url);
        eprintln!("   Webhook: {:?}", webhook_url);
        eprintln!("   Features: All v6.5 capabilities enabled");
        
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()?,
            api_key,
            api_url,
            webhook_url,
        })
    }

    /// Generate image(s) with full v6.5 capabilities
    pub async fn imagine(&self, request: ImagineRequest) -> Result<ImagineResponse> {
        // Build the Midjourney prompt with parameters
        let mut prompt_parts = vec![request.prompt.clone()];
        
        // Add parameters to prompt
        if let Some(ar) = &request.aspect_ratio {
            prompt_parts.push(format!("--ar {}", ar));
        }
        if let Some(c) = request.chaos {
            prompt_parts.push(format!("--chaos {}", c));
        }
        if let Some(q) = request.quality {
            prompt_parts.push(format!("--quality {}", q));
        }
        if let Some(s) = request.stylize {
            prompt_parts.push(format!("--stylize {}", s));
        }
        if let Some(w) = request.weird {
            prompt_parts.push(format!("--weird {}", w));
        }
        if let Some(v) = &request.version {
            prompt_parts.push(format!("--v {}", v));
        }
        if let Some(style) = &request.style {
            prompt_parts.push(format!("--style {}", style));
        }
        if request.tile == Some(true) {
            prompt_parts.push("--tile".to_string());
        }
        if request.turbo == Some(true) {
            prompt_parts.push("--turbo".to_string());
        }
        if request.video == Some(true) {
            prompt_parts.push("--video".to_string());
        }
        if let Some(seed) = request.seed {
            prompt_parts.push(format!("--seed {}", seed));
        }
        if let Some(no) = &request.no {
            prompt_parts.push(format!("--no {}", no.join(",")));
        }

        let full_prompt = prompt_parts.join(" ");
        
        let mut body = json!({
            "prompt": full_prompt,
            "type": "imagine",
        });
        
        if let Some(webhook) = &self.webhook_url {
            body["webhook_url"] = json!(webhook);
        }
        
        let response = self.client
            .post(format!("{}/imagine", self.api_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_body = response.text().await?;
            return Err(anyhow::anyhow!("Midjourney API error: {}", error_body));
        }
        
        response.json::<ImagineResponse>().await
            .context("Failed to parse Midjourney response")
    }

    /// Wait for a job to complete
    pub async fn wait_for_completion(&self, job_id: &str, max_wait_secs: u64) -> Result<Vec<String>> {
        let start = std::time::Instant::now();
        let max_duration = Duration::from_secs(max_wait_secs);
        
        loop {
            let response = self.client
                .get(format!("{}/job/{}", self.api_url, job_id))
                .header("Authorization", format!("Bearer {}", self.api_key))
                .send()
                .await?;
            
            let job: ImagineResponse = response.json().await?;
            
            match job.status.as_str() {
                "completed" => {
                    return job.image_urls
                        .ok_or_else(|| anyhow::anyhow!("Job completed but no images returned"));
                },
                "failed" | "cancelled" => {
                    return Err(anyhow::anyhow!("Job {} with status: {}", job_id, job.status));
                },
                _ => {
                    if start.elapsed() > max_duration {
                        return Err(anyhow::anyhow!("Job timeout after {} seconds", max_wait_secs));
                    }
                    
                    if let Some(progress) = job.progress {
                        eprintln!("🎨 Generation progress: {}%", progress);
                    }
                    
                    sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    /// Describe an image (reverse engineer prompt)
    pub async fn describe(&self, image_data: &[u8]) -> Result<DescribeResponse> {
        let base64_image = BASE64.encode(image_data);
        
        let request = DescribeRequest {
            image: base64_image,
            detail_level: Some("verbose".to_string()),
        };
        
        let response = self.client
            .post(format!("{}/describe", self.api_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_body = response.text().await?;
            return Err(anyhow::anyhow!("Describe API error: {}", error_body));
        }
        
        response.json::<DescribeResponse>().await
            .context("Failed to parse describe response")
    }

    /// Blend multiple images
    pub async fn blend(&self, images: Vec<Vec<u8>>) -> Result<ImagineResponse> {
        if images.len() < 2 || images.len() > 5 {
            return Err(anyhow::anyhow!("Blend requires 2-5 images"));
        }
        
        let base64_images: Vec<String> = images.iter()
            .map(|img| BASE64.encode(img))
            .collect();
        
        let request = BlendRequest {
            images: base64_images,
            dimensions: Some("1024x1024".to_string()),
            blend_mode: Some("default".to_string()),
        };
        
        let response = self.client
            .post(format!("{}/blend", self.api_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_body = response.text().await?;
            return Err(anyhow::anyhow!("Blend API error: {}", error_body));
        }
        
        response.json::<ImagineResponse>().await
            .context("Failed to parse blend response")
    }

    /// Upscale a specific image from generation
    pub async fn upscale(&self, job_id: &str, index: u8) -> Result<String> {
        let response = self.client
            .post(format!("{}/upscale", self.api_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&json!({
                "job_id": job_id,
                "index": index,
            }))
            .send()
            .await?;
        
        let job: ImagineResponse = response.json().await?;
        let urls = self.wait_for_completion(&job.job_id, 30).await?;
        Ok(urls.first().unwrap().clone())
    }

    /// Create variations of an image
    pub async fn vary(&self, job_id: &str, index: u8, strong: bool) -> Result<Vec<String>> {
        let variation_type = if strong { "strong" } else { "subtle" };
        
        let response = self.client
            .post(format!("{}/vary", self.api_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&json!({
                "job_id": job_id,
                "index": index,
                "type": variation_type,
            }))
            .send()
            .await?;
        
        let job: ImagineResponse = response.json().await?;
        self.wait_for_completion(&job.job_id, 60).await
    }

    /// Zoom out from an image
    pub async fn zoom_out(&self, job_id: &str, zoom_level: f32) -> Result<Vec<String>> {
        let request = ZoomRequest {
            job_id: job_id.to_string(),
            direction: "custom".to_string(),
            custom_zoom: Some(zoom_level.clamp(1.0, 2.0)),
        };
        
        let response = self.client
            .post(format!("{}/zoom", self.api_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?;
        
        let job: ImagineResponse = response.json().await?;
        self.wait_for_completion(&job.job_id, 60).await
    }

    /// Pan image in a direction
    pub async fn pan(&self, job_id: &str, direction: &str) -> Result<Vec<String>> {
        let request = PanRequest {
            job_id: job_id.to_string(),
            direction: direction.to_string(),
        };
        
        let response = self.client
            .post(format!("{}/pan", self.api_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?;
        
        let job: ImagineResponse = response.json().await?;
        self.wait_for_completion(&job.job_id, 60).await
    }

    /// Inpaint (regional editing)
    pub async fn inpaint(&self, job_id: &str, prompt: &str, mask_data: &[u8]) -> Result<Vec<String>> {
        let request = InpaintRequest {
            job_id: job_id.to_string(),
            prompt: prompt.to_string(),
            mask: BASE64.encode(mask_data),
        };
        
        let response = self.client
            .post(format!("{}/inpaint", self.api_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?;
        
        let job: ImagineResponse = response.json().await?;
        self.wait_for_completion(&job.job_id, 90).await
    }
}
