// src/llm/assistant/types.rs

use serde::{Serialize, Deserialize};

/// Message role in a thread
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
}

/// Request to create a message in a thread
#[derive(Serialize, Debug)]
pub struct CreateMessageRequest {
    pub role: String,
    pub content: String,
}

/// Response from creating a message
#[derive(Deserialize, Debug)]
pub struct MessageResponse {
    pub id: String,
    pub role: String,
    pub content: Vec<MessageContent>,
}

/// Content within a message
#[derive(Deserialize, Debug)]
pub struct MessageContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: Option<TextContent>,
}

/// Text content details
#[derive(Deserialize, Debug)]
pub struct TextContent {
    pub value: String,
    pub annotations: Vec<serde_json::Value>,
}

/// Request to create a run
#[derive(Serialize, Debug)]
pub struct CreateRunRequest {
    pub assistant_id: String,
}

/// Response from creating a run
#[derive(Deserialize, Debug)]
pub struct RunResponse {
    pub id: String,
    pub status: String,
    pub thread_id: String,
}

/// Run status values
#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    InProgress,
    RequiresAction,
    Cancelling,
    Cancelled,
    Failed,
    Completed,
    Expired,
}

/// List messages response
#[derive(Deserialize, Debug)]
pub struct ListMessagesResponse {
    pub data: Vec<MessageResponse>,
    pub has_more: bool,
}

/// Tool resources for threads
#[derive(Serialize, Debug)]
pub struct ToolResources {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_search: Option<FileSearchResource>,
}

/// File search resource configuration
#[derive(Serialize, Debug)]
pub struct FileSearchResource {
    pub vector_store_ids: Vec<String>,
}
