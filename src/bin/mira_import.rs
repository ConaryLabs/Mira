// src/bin/mira_import.rs

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::collections::HashMap;
use anyhow::Result;
use clap::Parser;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::SqlitePool;

use mira_backend::memory::storage::sqlite::store::SqliteMemoryStore;
use mira_backend::memory::core::types::MemoryEntry;
use mira_backend::memory::core::traits::MemoryStore;

#[derive(Parser)]
#[command(name = "mira-import")]
#[command(about = "Import ChatGPT export into Mira's memory system", long_about = None)]
struct Cli {
    /// Path to conversations.json
    #[arg(short, long)]
    input: PathBuf,
    
    /// SQLite DB path
    #[arg(long, default_value = "mira.db")]
    sqlite: String,
    
    /// Enable debug logging
    #[arg(short, long, default_value_t = false)]
    debug: bool,
}

#[derive(Debug, Deserialize)]
struct ChatExport(Vec<ChatThread>);

#[derive(Debug, Deserialize)]
struct ChatThread {
    title: Option<String>,
    mapping: HashMap<String, MessageNode>,
    conversation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessageNode {
    message: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    author: Author,
    content: Content,
    create_time: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct Author {
    role: String,
}

#[derive(Debug, Deserialize)]
struct Content {
    parts: Vec<serde_json::Value>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    
    if cli.debug {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    } else {
        tracing_subscriber::fmt().init();
    }
    
    let file = File::open(&cli.input)?;
    let reader = BufReader::new(file);
    let export: ChatExport = serde_json::from_reader(reader)?;
    
    let pool = SqlitePool::connect(&cli.sqlite).await?;
    let store = SqliteMemoryStore::new(pool);
    
    for thread in export.0 {
        let session_id = thread.conversation_id
            .or(thread.title)
            .unwrap_or_else(|| "imported".to_string());
        
        println!("Importing conversation: {}", session_id);
        
        let mut messages: Vec<_> = thread.mapping.values()
            .filter_map(|node| node.message.as_ref())
            .filter(|msg| msg.author.role != "system")
            .collect();
        
        messages.sort_by_key(|m| m.create_time.unwrap_or(0.0) as i64);
        
        for message in messages {
            let content = message.content.parts.iter()
                .filter_map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            
            if content.is_empty() {
                continue;
            }
            
            let timestamp = message.create_time.map(|secs| {
                DateTime::from_timestamp(secs as i64, 0).unwrap_or(Utc::now())
            }).unwrap_or(Utc::now());
            
            let entry = MemoryEntry {
                id: None,
                session_id: session_id.clone(),
                response_id: None,
                parent_id: None,
                role: message.author.role.clone(),
                content,
                timestamp,
                tags: Some(vec!["imported".to_string(), "chatgpt".to_string()]),
                mood: None,
                intensity: None,
                salience: Some(5.0),
                original_salience: None,
                intent: None,
                topics: None,
                summary: None,
                relationship_impact: None,
                contains_code: None,
                language: None,
                programming_lang: None,
                analyzed_at: None,
                analysis_version: None,
                routed_to_heads: None,
                last_recalled: Some(Utc::now()),
                recall_count: None,
                model_version: None,
                prompt_tokens: None,
                completion_tokens: None,
                reasoning_tokens: None,
                total_tokens: None,
                latency_ms: None,
                generation_time_ms: None,
                finish_reason: None,
                tool_calls: None,
                temperature: None,
                max_tokens: None,
                embedding: None,
                embedding_heads: None,
                qdrant_point_ids: None,
            };
            
            store.save(&entry).await?;
        }
    }
    
    println!("Import complete!");
    Ok(())
}
