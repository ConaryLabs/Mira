// backend/src/tools/mira_import/schema.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::{DateTime, Utc};

#[derive(Debug, Deserialize)]
pub struct ChatExport(pub Vec<ChatThread>);

#[derive(Debug, Deserialize)]
pub struct ChatThread {
    pub title: Option<String>,
    pub create_time: Option<f64>,
    pub update_time: Option<f64>,
    pub mapping: HashMap<String, MessageNode>,
    pub conversation_id: Option<String>,
    pub default_model_slug: Option<String>,
    pub is_archived: Option<bool>,
    pub is_starred: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct MessageNode {
    pub id: String,
    pub message: Option<ChatMessage>,
    pub parent: Option<String>,
    pub children: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub author: AuthorInfo,
    pub create_time: Option<f64>,
    pub update_time: Option<f64>,
    pub content: MessageContent,
    pub status: Option<String>,
    pub end_turn: Option<bool>,
    pub weight: Option<f32>,
    pub metadata: Option<MessageMetadata>,
    pub recipient: Option<String>,
    pub channel: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AuthorInfo {
    pub role: String,
    pub name: Option<String>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
pub struct MessageContent {
    pub content_type: String,
    pub parts: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct MessageMetadata {
    pub attachments: Option<Vec<Attachment>>,
    pub model_slug: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub mime_type: Option<String>,
    pub file_size_tokens: Option<usize>,
}

#[derive(Debug, Serialize, Clone)]
pub struct MiraMessage {
    pub thread_title: Option<String>,
    pub thread_id: Option<String>,
    pub message_id: String,
    pub parent_id: Option<String>,
    pub role: String,
    pub content: String,
    pub create_time: Option<DateTime<Utc>>,
    pub update_time: Option<DateTime<Utc>>,
    pub attachments: Vec<Attachment>,
    pub model_slug: Option<String>,
    pub starred: bool,
    pub archived: bool,
}

impl ChatThread {
    pub fn flatten(&self) -> Vec<MiraMessage> {
        let mut messages = vec![];
        for node in self.mapping.values() {
            if let Some(ref msg) = node.message {
                let content = msg
                    .content
                    .parts
                    .iter()
                    .map(|p| p.as_str().unwrap_or(""))
                    .collect::<Vec<_>>()
                    .join("\n");
                let create_time = msg.create_time.map(epoch_to_utc);
                let update_time = msg.update_time.map(epoch_to_utc);
                let attachments = msg
                    .metadata
                    .as_ref()
                    .and_then(|m| m.attachments.clone())
                    .unwrap_or_default();
                let model_slug = msg
                    .metadata
                    .as_ref()
                    .and_then(|m| m.model_slug.clone())
                    .or_else(|| self.default_model_slug.clone());
                let starred = self.is_starred.unwrap_or(false);
                let archived = self.is_archived.unwrap_or(false);
                messages.push(MiraMessage {
                    thread_title: self.title.clone(),
                    thread_id: self.conversation_id.clone(),
                    message_id: msg.id.clone(),
                    parent_id: node.parent.clone(),
                    role: msg.author.role.clone(),
                    content,
                    create_time,
                    update_time,
                    attachments,
                    model_slug,
                    starred,
                    archived,
                });
            }
        }
        messages.sort_by(|a, b| {
            a.update_time
                .unwrap_or_else(|| a.create_time.unwrap_or(Utc::now()))
                .cmp(&b.update_time.unwrap_or_else(|| b.create_time.unwrap_or(Utc::now())))
        });
        messages
    }
}

pub fn epoch_to_utc(secs: f64) -> DateTime<Utc> {
    use chrono::TimeZone;
    let nanos = (secs.fract() * 1e9) as u32;
    Utc.timestamp_opt(secs as i64, nanos).unwrap()
}
