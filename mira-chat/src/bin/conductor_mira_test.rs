//! End-to-end test of DeepSeek Conductor with Mira integration
//!
//! Tests: Smart excerpts, error fix lookup, rejected approaches, cochange patterns

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePoolOptions;
use std::time::Instant;

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
}

// ============================================================================
// Mira Intelligence (simplified inline version for testing)
// ============================================================================

struct MiraIntel {
    db: sqlx::SqlitePool,
}

#[derive(Debug)]
struct FixSuggestion {
    error_pattern: String,
    fix_description: String,
}

#[derive(Debug)]
struct RejectedApproach {
    approach: String,
    rejection_reason: String,
}

#[derive(Debug)]
struct CochangePattern {
    related_file: String,
    cochange_count: i32,
}

impl MiraIntel {
    async fn find_similar_fixes(&self, error: &str) -> Vec<FixSuggestion> {
        let snippet = if error.len() > 50 { &error[..50] } else { error };
        let fixes: Vec<(String, String)> = sqlx::query_as(
            "SELECT error_pattern, fix_description FROM error_fixes
             WHERE error_pattern LIKE '%' || $1 || '%' LIMIT 3"
        )
        .bind(snippet)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        fixes.into_iter()
            .map(|(p, f)| FixSuggestion { error_pattern: p, fix_description: f })
            .collect()
    }

    async fn get_rejected_approaches(&self, task: &str) -> Vec<RejectedApproach> {
        let approaches: Vec<(String, String)> = sqlx::query_as(
            "SELECT approach, rejection_reason FROM rejected_approaches LIMIT 5"
        )
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        let task_lower = task.to_lowercase();
        approaches.into_iter()
            .filter(|(a, _)| task_lower.split_whitespace().any(|w| w.len() > 3 && a.to_lowercase().contains(w)))
            .map(|(a, r)| RejectedApproach { approach: a, rejection_reason: r })
            .collect()
    }

    async fn get_cochange_patterns(&self, file: &str) -> Vec<CochangePattern> {
        let patterns: Vec<(String, i32)> = sqlx::query_as(
            "SELECT CASE WHEN file_a = $1 THEN file_b ELSE file_a END, cochange_count
             FROM cochange_patterns WHERE file_a = $1 OR file_b = $1
             ORDER BY cochange_count DESC LIMIT 5"
        )
        .bind(file)
        .fetch_all(&self.db)
        .await
        .unwrap_or_default();

        patterns.into_iter()
            .map(|(f, c)| CochangePattern { related_file: f, cochange_count: c })
            .collect()
    }
}

// ============================================================================
// Smart Excerpts (from mira-core)
// ============================================================================

fn smart_excerpt(tool: &str, output: &str) -> String {
    if output.len() <= 2048 {
        return output.to_string();
    }

    let lines: Vec<&str> = output.lines().collect();
    let preview: String = match tool {
        "grep" => {
            let take = lines.iter().take(20).cloned().collect::<Vec<_>>().join("\n");
            format!("{}\n\n…[{} more lines truncated]", take, lines.len() - 20)
        }
        _ => {
            let head: String = output.chars().take(1200).collect();
            let tail: String = output.chars().rev().take(800).collect::<String>().chars().rev().collect();
            format!("{}…\n\n[{} chars truncated]\n\n…{}", head, output.len() - 2000, tail)
        }
    };
    preview
}

// ============================================================================
// Main Test
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Load env
    let env_path = dirs::home_dir()
        .map(|h| h.join(".mira").join(".env"))
        .filter(|p| p.exists());
    if let Some(path) = env_path {
        let _ = dotenvy::from_path(&path);
    }

    let deepseek_key = std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY not set");
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite:///home/peter/Mira/data/mira.db".to_string());

    let sep = "─".repeat(60);

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║     CONDUCTOR + MIRA INTEGRATION TEST (End-to-End)          ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // Connect to Mira database
    println!("📚 Connecting to Mira database...");
    let db = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;
    let mira = MiraIntel { db };
    println!("✓ Connected\n");

    // === Test 1: Rejected Approaches ===
    println!("🧪 Test 1: Rejected Approaches Lookup");
    println!("{}", sep);
    let task = "implement rate limiting for the API";
    let rejected = mira.get_rejected_approaches(task).await;
    println!("Task: {}", task);
    println!("Found {} rejected approaches:", rejected.len());
    for r in &rejected {
        println!("  ❌ {} (reason: {})", r.approach, r.rejection_reason);
    }
    println!();

    // === Test 2: Error Fix Lookup ===
    println!("🧪 Test 2: Error Fix Lookup");
    println!("{}", sep);
    let error = "cannot find type `RateLimiter` in this scope";
    let fixes = mira.find_similar_fixes(error).await;
    println!("Error: {}", error);
    println!("Found {} similar fixes:", fixes.len());
    for f in &fixes {
        println!("  💡 {} → {}", f.error_pattern, f.fix_description);
    }
    println!();

    // === Test 3: Cochange Patterns ===
    println!("🧪 Test 3: Cochange Patterns");
    println!("{}", sep);
    let file = "mira-chat/src/conductor/executor.rs";
    let cochange = mira.get_cochange_patterns(file).await;
    println!("File: {}", file);
    println!("Found {} cochange patterns:", cochange.len());
    for c in &cochange {
        println!("  📎 {} (changed together {} times)", c.related_file, c.cochange_count);
    }
    println!();

    // === Test 4: Smart Excerpts ===
    println!("🧪 Test 4: Smart Excerpts");
    println!("{}", sep);
    let large_grep = (1..=100).map(|i| format!("file.rs:{}:match line {}", i, i)).collect::<Vec<_>>().join("\n");
    let excerpted = smart_excerpt("grep", &large_grep);
    println!("Original: {} lines, {} bytes", 100, large_grep.len());
    println!("Excerpted: {} bytes", excerpted.len());
    println!("Preview:\n{}\n", &excerpted[..excerpted.len().min(200)]);

    // === Test 5: Full DeepSeek Call with Mira Context ===
    println!("🧪 Test 5: DeepSeek with Mira Context Injection");
    println!("{}", sep);

    // Build system prompt with Mira context
    let mut system_parts = vec!["You are a Rust developer. Output only code.".to_string()];

    // Add rejected approaches if any
    if !rejected.is_empty() {
        let mut warning = String::from("\n## Approaches to AVOID:\n");
        for r in &rejected {
            warning.push_str(&format!("- ❌ {} ({})\n", r.approach, r.rejection_reason));
        }
        system_parts.push(warning);
    }

    // Add corrections (hardcoded for test since we know these exist)
    system_parts.push("\n## Code Quality Rules:\n- ❌ .unwrap() → ✓ .expect(\"reason\")\n".to_string());

    let system_prompt = system_parts.join("");
    println!("System prompt ({} chars):", system_prompt.len());
    println!("{}\n", &system_prompt[..system_prompt.len().min(300)]);

    let user_prompt = "Write a function that reads a config file and returns a HashMap<String, String>. Handle errors properly.";

    println!("User prompt: {}\n", user_prompt);

    let request = ChatRequest {
        model: "deepseek-chat".into(),
        messages: vec![
            Message { role: "system".into(), content: system_prompt },
            Message { role: "user".into(), content: user_prompt.into() },
        ],
        max_tokens: Some(800),
    };

    let client = Client::new();
    let start = Instant::now();
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

        // Verify corrections were followed
        let content = &choice.message.content;
        if content.contains(".expect(") || content.contains("?") {
            println!("✅ Correction followed: Uses .expect() or ? operator");
        } else if content.contains(".unwrap()") {
            println!("❌ Correction NOT followed: Still uses .unwrap()");
        }
    }

    if let Some(usage) = &response.usage {
        let cost = (usage.prompt_tokens as f64 * 0.27 + usage.completion_tokens as f64 * 0.41) / 1_000_000.0;
        println!("Tokens: {}in / {}out | Cost: ${:.6}", usage.prompt_tokens, usage.completion_tokens, cost);
    }

    println!("\n{}", sep);
    println!("✓ All integration tests complete!");

    Ok(())
}
