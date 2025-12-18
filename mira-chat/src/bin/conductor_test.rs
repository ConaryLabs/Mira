//! Quick test of the DeepSeek conductor
//!
//! Run with: cargo run --bin conductor_test

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    #[serde(default)]
    reasoning_tokens: u32,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load env
    let env_path = dirs::home_dir()
        .map(|h| h.join(".mira").join(".env"))
        .filter(|p| p.exists());
    if let Some(path) = env_path {
        let _ = dotenvy::from_path(&path);
    }

    let deepseek_key = std::env::var("DEEPSEEK_API_KEY")
        .expect("DEEPSEEK_API_KEY not set");

    let client = Client::new();
    let sep = "─".repeat(40);

    println!("═══ DeepSeek Conductor Test ═══\n");

    // Test 1: Simple DeepSeek Chat call
    println!("Test 1: DeepSeek Chat - Simple coding task");
    println!("{}", sep);

    let task = r#"Write a Rust function called `is_palindrome` that checks if a string is a palindrome.
It should ignore case and non-alphanumeric characters.
Just output the function, no explanation."#;

    println!("Task: {}\n", task);

    let request = ChatRequest {
        model: "deepseek-chat".into(),
        messages: vec![
            Message {
                role: "system".into(),
                content: "You are a helpful coding assistant. Be concise.".into(),
            },
            Message {
                role: "user".into(),
                content: task.into(),
            },
        ],
        max_tokens: Some(1000),
    };

    let start = std::time::Instant::now();
    let response: ChatResponse = client
        .post("https://api.deepseek.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", deepseek_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await?
        .json()
        .await?;
    let elapsed = start.elapsed();

    if let Some(choice) = response.choices.first() {
        println!("Response ({:.2}s):", elapsed.as_secs_f64());
        println!("```rust");
        println!("{}", choice.message.content);
        println!("```\n");
    }

    if let Some(usage) = &response.usage {
        let cost = (usage.prompt_tokens as f64 * 0.27 + usage.completion_tokens as f64 * 0.41) / 1_000_000.0;
        println!("Tokens: {}in / {}out", usage.prompt_tokens, usage.completion_tokens);
        println!("Cost: ${:.6}", cost);
    }

    // Test 2: DeepSeek Reasoner (planning)
    println!("\n═══════════════════════════════════════");
    println!("Test 2: DeepSeek Reasoner - Planning task");
    println!("{}", sep);

    let planning_task = r#"I need to add a caching layer to a Rust web service.
The service currently makes database calls on every request.
Create a brief execution plan (3-5 steps) for implementing Redis caching.
Output as a numbered list."#;

    println!("Task: {}\n", planning_task);

    let request = ChatRequest {
        model: "deepseek-reasoner".into(),
        messages: vec![
            Message {
                role: "system".into(),
                content: "You are a software architect. Be concise and practical.".into(),
            },
            Message {
                role: "user".into(),
                content: planning_task.into(),
            },
        ],
        max_tokens: Some(2000),
    };

    let start = std::time::Instant::now();
    let response: ChatResponse = client
        .post("https://api.deepseek.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", deepseek_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await?
        .json()
        .await?;
    let elapsed = start.elapsed();

    if let Some(choice) = response.choices.first() {
        println!("Response ({:.2}s):", elapsed.as_secs_f64());
        println!("{}\n", choice.message.content);
    }

    if let Some(usage) = &response.usage {
        let cost = (usage.prompt_tokens as f64 * 0.55 + usage.completion_tokens as f64 * 2.19) / 1_000_000.0;
        println!("Tokens: {}in / {}out / {}reasoning",
            usage.prompt_tokens, usage.completion_tokens, usage.reasoning_tokens);
        println!("Cost: ${:.6}", cost);
    }

    println!("\n═══ Test Complete ═══");

    Ok(())
}
