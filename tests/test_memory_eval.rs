use std::sync::Arc;
use mira_backend::{
    services::{MemoryService, ContextService, ChatService},
    llm::{OpenAIClient, responses::{thread::ThreadManager, vector_store::VectorStoreManager}},
    persona::PersonaOverlay,
};

#[tokio::test]
async fn memory_eval_tags_basic_fact() -> anyhow::Result<()> {
    // Arrange
    let llm = Arc::new(OpenAIClient::new()?);
    let threads = Arc::new(ThreadManager::new());
    let memory = Arc::new(MemoryService::new(/* sqlite_store */ todo!(), /* qdrant_store */ todo!(), llm.clone()));
    let context = Arc::new(ContextService::new(/* sqlite_store */ todo!(), /* qdrant_store */ todo!()));
    let vectors = Arc::new(VectorStoreManager::new(llm.clone()));
    let chat = ChatService::new(llm, threads, memory.clone(), context, vectors, PersonaOverlay::default());

    // Act: send a message that should be tagged as salient
    let _reply = chat.process_message("test-session", "My flight to NYC is on Oct 12.", None, true).await?;

    // Assert: pull last saved message metadata and assert tags/salience exist
    let recent = memory.get_recent_messages("test-session", 1).await?;
    assert!(!recent.is_empty());
    assert!(recent[0].tags.as_deref().unwrap_or(&[]).contains(&"event".to_string()));

    Ok(())
}
