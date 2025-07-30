// src/llm/client.rs

use reqwest::{Client, Method, RequestBuilder};
use std::env;

#[derive(Clone)]
pub struct OpenAIClient {
    pub client: Client,
    pub api_key: String,
    pub api_base: String,
}

impl OpenAIClient {
    pub fn new() -> Self {
        let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
        let api_base = env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        Self {
            client: Client::new(),
            api_key,
            api_base,
        }
    }

    pub fn auth_header(&self) -> (&'static str, String) {
        ("Authorization", format!("Bearer {}", self.api_key))
    }

    /// Universal request builder for all OpenAI JSON endpoints
    pub fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.client
            .request(
                method,
                format!("{}/{}", self.api_base.trim_end_matches('/'), path.trim_start_matches('/')),
            )
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
    }

    /// Multipart request builder for file uploads (Content-Type set by reqwest)
    pub fn request_multipart(&self, path: &str) -> RequestBuilder {
        self.client
            .post(format!("{}/{}", self.api_base.trim_end_matches('/'), path.trim_start_matches('/')))
            .header("Authorization", format!("Bearer {}", self.api_key))
        // Don't set Content-Type: multipart is handled by reqwest
    }
}
