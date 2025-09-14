// src/llm/streaming/mod.rs

mod request;
mod processor;

pub use processor::StreamEvent;

use anyhow::Result;
use futures::Stream;
use std::pin::Pin;
use tracing::info;
use crate::llm::client::OpenAIClient;

pub type StreamResult = Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>;

pub async fn start_response_stream(
    client: &OpenAIClient,
    user_text: &str,
    system_prompt: Option<&str>,
    structured_json: bool,
) -> Result<StreamResult> {
    info!("Starting response stream - structured_json: {}", structured_json);
    
    let body = request::build_request_body(
        client,
        user_text,
        system_prompt,
        structured_json
    )?;
    
    let sse_stream = client.post_response_stream(body).await?;
    
    let event_stream = processor::process_stream(sse_stream, structured_json);
    
    Ok(Box::pin(event_stream))
}

pub async fn stream_response(
    client: &OpenAIClient,
    user_text: &str,
    system_prompt: Option<&str>,
    structured_json: bool,
) -> Result<StreamResult> {
    start_response_stream(client, user_text, system_prompt, structured_json).await
}

pub struct StreamingHandler {
    client: std::sync::Arc<OpenAIClient>,
}

impl StreamingHandler {
    pub fn new(client: std::sync::Arc<OpenAIClient>) -> Self {
        Self { client }
    }
    
    pub async fn generate_response(
        &self,
        user_text: &str,
        context: &crate::memory::recall::RecallContext,
    ) -> Result<String> {
        // TODO: Build system prompt from context
        let system_prompt = Some("You are a helpful assistant.");
        
        let mut stream = start_response_stream(
            &self.client,
            user_text,
            system_prompt,
            false
        ).await?;
        
        use futures::StreamExt;
        let mut full_response = String::new();
        
        while let Some(event) = stream.next().await {
            match event? {
                StreamEvent::Delta(text) | StreamEvent::Text(text) => {
                    full_response.push_str(&text);
                }
                StreamEvent::Done { full_text, .. } => {
                    return Ok(full_text);
                }
                StreamEvent::Error(e) => {
                    return Err(anyhow::anyhow!("Streaming error: {}", e));
                }
            }
        }
        
        Ok(full_response)
    }
}
