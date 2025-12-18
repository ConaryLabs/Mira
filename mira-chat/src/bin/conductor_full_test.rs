//! Full conductor pipeline test
//!
//! Tests: Reasoner (planning) -> Chat (execution) -> Comparison
//!
//! Now with Mira context injection for corrections!

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
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
    #[serde(default)]
    reasoning_tokens: u32,
}

struct CostTracker {
    reasoner_input: u32,
    reasoner_output: u32,
    chat_input: u32,
    chat_output: u32,
    requests: u32,
}

impl CostTracker {
    fn new() -> Self {
        Self {
            reasoner_input: 0,
            reasoner_output: 0,
            chat_input: 0,
            chat_output: 0,
            requests: 0,
        }
    }

    fn add_reasoner(&mut self, input: u32, output: u32) {
        self.reasoner_input += input;
        self.reasoner_output += output;
        self.requests += 1;
    }

    fn add_chat(&mut self, input: u32, output: u32) {
        self.chat_input += input;
        self.chat_output += output;
        self.requests += 1;
    }

    fn deepseek_cost(&self) -> f64 {
        // Reasoner: $0.55/M in, $2.19/M out
        // Chat: $0.27/M in, $0.41/M out
        let reasoner = (self.reasoner_input as f64 * 0.55 + self.reasoner_output as f64 * 2.19) / 1_000_000.0;
        let chat = (self.chat_input as f64 * 0.27 + self.chat_output as f64 * 0.41) / 1_000_000.0;
        reasoner + chat
    }

    fn equivalent_gpt_cost(&self) -> f64 {
        // GPT-5.2: $2.50/M in, $10.00/M out
        let total_input = self.reasoner_input + self.chat_input;
        let total_output = self.reasoner_output + self.chat_output;
        (total_input as f64 * 2.50 + total_output as f64 * 10.00) / 1_000_000.0
    }

    fn savings_pct(&self) -> f64 {
        let actual = self.deepseek_cost();
        let equivalent = self.equivalent_gpt_cost();
        if equivalent > 0.0 {
            (1.0 - actual / equivalent) * 100.0
        } else {
            0.0
        }
    }

    fn summary(&self) -> String {
        format!(
            "DeepSeek: ${:.6} | GPT-5.2 equivalent: ${:.6} | Savings: {:.1}%",
            self.deepseek_cost(),
            self.equivalent_gpt_cost(),
            self.savings_pct()
        )
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let env_path = dirs::home_dir()
        .map(|h| h.join(".mira").join(".env"))
        .filter(|p| p.exists());
    if let Some(path) = env_path {
        let _ = dotenvy::from_path(&path);
    }

    let deepseek_key = std::env::var("DEEPSEEK_API_KEY")
        .expect("DEEPSEEK_API_KEY not set");

    let client = Client::new();
    let mut costs = CostTracker::new();
    let total_start = Instant::now();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║          DEEPSEEK CONDUCTOR - FULL PIPELINE TEST             ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // === THE TASK ===
    let task = r#"Implement a rate limiter module in Rust with these requirements:
1. Token bucket algorithm
2. Configurable rate (tokens per second) and burst size
3. Thread-safe (can be shared across async tasks)
4. Methods: new(), try_acquire() -> bool, acquire() -> impl Future
5. Include unit tests

Create complete, production-ready code."#;

    let sep = "─".repeat(60);

    println!("📋 TASK:");
    println!("{}", sep);
    println!("{}", task);
    println!("{}", sep);
    println!();

    // === PHASE 1: PLANNING WITH REASONER ===
    println!("🧠 PHASE 1: Planning with DeepSeek Reasoner");
    println!("{}", sep);

    let planning_prompt = format!(r#"You are a Rust expert. Analyze this task and create an implementation plan.

TASK: {}

Create a structured plan with:
1. Key design decisions
2. Data structures needed
3. Implementation steps (numbered)
4. Test cases to include

Be concise but thorough."#, task);

    let request = ChatRequest {
        model: "deepseek-reasoner".into(),
        messages: vec![
            Message { role: "system".into(), content: "You are a senior Rust developer.".into() },
            Message { role: "user".into(), content: planning_prompt },
        ],
        max_tokens: Some(4000),
    };

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
    let planning_time = start.elapsed();

    let plan = response.choices.first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    if let Some(usage) = &response.usage {
        costs.add_reasoner(usage.prompt_tokens, usage.completion_tokens);
        println!("⏱️  Time: {:.2}s | Tokens: {}in/{}out",
            planning_time.as_secs_f64(), usage.prompt_tokens, usage.completion_tokens);
    }

    println!("\n📝 PLAN:");
    println!("{}\n", plan);

    // === PHASE 2: EXECUTION WITH CHAT ===
    println!("⚡ PHASE 2: Implementation with DeepSeek Chat");
    println!("{}", sep);

    let implementation_prompt = format!(r#"Based on this plan, implement the complete rate limiter module.

PLAN:
{}

REQUIREMENTS:
- Complete, compilable Rust code
- Include all imports
- Include unit tests with #[cfg(test)]
- Use standard library + tokio for async
- Output ONLY the code, no explanations

```rust"#, plan);

    let request = ChatRequest {
        model: "deepseek-chat".into(),
        messages: vec![
            Message { role: "system".into(), content: "You are a Rust developer. Output only code.".into() },
            Message { role: "user".into(), content: implementation_prompt },
        ],
        max_tokens: Some(4000),
    };

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
    let impl_time = start.elapsed();

    let implementation = response.choices.first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    if let Some(usage) = &response.usage {
        costs.add_chat(usage.prompt_tokens, usage.completion_tokens);
        println!("⏱️  Time: {:.2}s | Tokens: {}in/{}out",
            impl_time.as_secs_f64(), usage.prompt_tokens, usage.completion_tokens);
    }

    println!("\n💻 IMPLEMENTATION:");
    println!("```rust");
    println!("{}", implementation);
    println!("```\n");

    // === SUMMARY ===
    let total_time = total_start.elapsed();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                      EXECUTION SUMMARY                        ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!("⏱️  Total time: {:.2}s (planning: {:.2}s, implementation: {:.2}s)",
        total_time.as_secs_f64(), planning_time.as_secs_f64(), impl_time.as_secs_f64());
    println!("📊 API calls: {}", costs.requests);
    println!("🪙 Tokens: Reasoner {}in/{}out | Chat {}in/{}out",
        costs.reasoner_input, costs.reasoner_output,
        costs.chat_input, costs.chat_output);
    println!();
    println!("💰 {}", costs.summary());
    println!();

    Ok(())
}
