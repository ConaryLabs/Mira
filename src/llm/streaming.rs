// src/llm/streaming.rs

use crate::api::ws::message::WsServerMessage;
use futures::stream::Stream;
use std::pin::Pin;

// Minimal streaming implementation
// The actual chat logic is handled by ChatService (using Claude)
// These methods are kept for any legacy code that might still reference them

impl crate::llm::client::OpenAIClient {
    /// Legacy streaming method - not used with Claude orchestration
    pub async fn stream_gpt4_ws(
        &self,
        _prompt: String,
        _system_prompt: String,
        _model: Option<&str>,
    ) -> Pin<Box<dyn Stream<Item = WsServerMessage> + Send + 'static>> {
        // Return empty stream since we use ChatService with Claude
        let stream = async_stream::stream! {
            yield WsServerMessage::Done;
        };
        Box::pin(stream)
    }
}

// Note: Real chat processing happens through:
// 1. ChatService.process_message() which uses Claude for orchestration
// 2. WebSocket handler in src/api/ws/chat.rs streams the response
