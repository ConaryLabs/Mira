// src/llm/streaming.rs
// Phase 9: Streaming support for GPT-5 responses

use futures::stream::{Stream, StreamExt};
use std::pin::Pin;
use std::task::{Context, Poll};

// The actual chat logic is handled by ChatService using GPT-5
pub struct EmptyStream;

impl EmptyStream {
    /// Legacy streaming method - replaced by GPT-5 streaming in ChatService
    pub fn new() -> Self {
        Self
    }
}

impl Stream for EmptyStream {
    type Item = Result<String, anyhow::Error>;
    
    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Return empty stream since we use ChatService with GPT-5
        // Real streaming is handled in WebSocket handler
        Poll::Ready(None)
    }
}

// Note: Actual streaming implementation is in:
// 1. ChatService.process_message() which uses GPT-5 for processing
// 2. WebSocket handler which streams responses to clients
