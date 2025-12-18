//! Test context injection for DeepSeek conductor
//!
//! Demonstrates how Mira corrections are injected into the system prompt.

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePoolOptions;

// Minimal types for the test
#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

// Minimal correction struct
struct Correction {
    what_was_wrong: String,
    what_is_right: String,
}

/// Build conductor prompt with corrections (same logic as session.rs)
fn build_conductor_prompt(corrections: &[Correction]) -> String {
    let mut sections = Vec::new();

    sections.push("You are a skilled software engineer executing code changes.".to_string());

    if !corrections.is_empty() {
        let mut lines = vec!["\n## Code Quality Rules (follow strictly)".to_string()];
        for c in corrections {
            lines.push(format!("- ❌ {} → ✓ {}", c.what_was_wrong, c.what_is_right));
        }
        sections.push(lines.join("\n"));
    }

    sections.push(r#"
## Execution Rules
- Use diff format for edits (old_string/new_string)
- Be precise and minimal in changes
- Output ONLY code when asked for implementation
- Use .expect("reason") not .unwrap()"#.to_string());

    sections.join("\n")
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
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite:///home/peter/Mira/data/mira.db".to_string());

    let sep = "─".repeat(60);

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║          CONTEXT INJECTION TEST - Mira → DeepSeek            ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // === Load corrections from Mira ===
    println!("📚 Loading corrections from Mira database...");
    println!("{}", sep);

    let corrections = match SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
    {
        Ok(pool) => {
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT what_was_wrong, what_is_right FROM corrections WHERE status = 'active' LIMIT 5"
            )
            .fetch_all(&pool)
            .await
            .unwrap_or_default();

            rows.into_iter()
                .map(|(wrong, right)| Correction {
                    what_was_wrong: wrong,
                    what_is_right: right,
                })
                .collect::<Vec<_>>()
        }
        Err(e) => {
            println!("⚠️  Database unavailable: {}", e);
            vec![]
        }
    };

    println!("Found {} active corrections\n", corrections.len());
    for c in &corrections {
        println!("  ❌ {}", c.what_was_wrong);
        println!("  ✓ {}", c.what_is_right);
        println!();
    }

    // === Build injected system prompt ===
    println!("{}", sep);
    println!("🔧 Building conductor system prompt with injections...\n");

    let system_prompt = build_conductor_prompt(&corrections);
    println!("{}\n", system_prompt);

    // === Test with DeepSeek ===
    println!("{}", sep);
    println!("🧪 Testing DeepSeek Chat with injected corrections...\n");

    let task = r#"Write a Rust function that reads a file and returns its contents.
Use error handling that follows the code quality rules.
Output ONLY the function."#;

    println!("Task: {}\n", task);

    let request = ChatRequest {
        model: "deepseek-chat".into(),
        messages: vec![
            Message { role: "system".into(), content: system_prompt },
            Message { role: "user".into(), content: task.into() },
        ],
        max_tokens: Some(500),
    };

    let client = Client::new();
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
        println!("```");
    }

    println!("\n{}", sep);

    // Check if the correction was followed
    if let Some(choice) = response.choices.first() {
        let content = &choice.message.content;
        if content.contains(".expect(") {
            println!("✅ SUCCESS: DeepSeek followed the correction (uses .expect())");
        } else if content.contains(".unwrap()") {
            println!("❌ MISS: DeepSeek used .unwrap() despite correction");
        } else if content.contains("?") {
            println!("✅ SUCCESS: DeepSeek used ? operator (acceptable)");
        } else {
            println!("⚠️  UNCLEAR: Check the output manually");
        }
    }

    println!();
    Ok(())
}
