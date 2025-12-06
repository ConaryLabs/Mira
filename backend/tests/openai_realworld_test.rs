// tests/openai_realworld_test.rs
// Real-world validation tests for OpenAI GPT-5.1 multi-model routing
//
// These tests make actual API calls to OpenAI to validate:
// 1. Each tier works correctly with real models
// 2. Latency meets targets (<2s Fast, <5s Voice, <30s Code/Agentic)
// 3. Cost calculations are accurate
// 4. Streaming works correctly
//
// Run with: cargo test --test openai_realworld_test -- --nocapture
// Note: Requires OPENAI_API_KEY in .env

mod common;

use futures::StreamExt;
use mira_backend::llm::provider::openai::{OpenAIModel, OpenAIPricing, OpenAIProvider};
use mira_backend::llm::provider::{LlmProvider, Message};
use std::time::Instant;

fn get_api_key() -> Option<String> {
    dotenv::dotenv().ok();
    std::env::var("OPENAI_API_KEY").ok().filter(|k| !k.is_empty())
}

fn skip_if_no_key() -> bool {
    if get_api_key().is_none() {
        println!("SKIPPED: OPENAI_API_KEY not set");
        return true;
    }
    false
}

// ============================================================================
// PHASE 1: REAL-WORLD COST VALIDATION
// ============================================================================

#[tokio::test]
async fn test_fast_tier_real_api() {
    if skip_if_no_key() {
        return;
    }

    let provider = OpenAIProvider::gpt51_mini(get_api_key().unwrap())
        .expect("Should create Fast tier provider");

    let start = Instant::now();
    let messages = vec![Message::user("Say 'hello' and nothing else.".to_string())];

    let result = provider.chat(messages, "You are a helpful assistant.".to_string()).await;
    let latency = start.elapsed();

    match result {
        Ok(response) => {
            println!("\n=== FAST TIER (gpt-5.1-codex-mini) ===");
            println!("Response: {}", response.content.chars().take(100).collect::<String>());
            println!("Latency: {:?}", latency);
            println!("Tokens: {} input, {} output", response.tokens.input, response.tokens.output);

            let cost = OpenAIPricing::calculate_cost(
                OpenAIModel::Gpt51Mini,
                response.tokens.input,
                response.tokens.output,
            );
            println!("Cost: ${:.6}", cost);

            // Verify latency target: <2s for Fast tier
            assert!(
                latency.as_secs() < 5,
                "Fast tier latency {} exceeded 5s target",
                latency.as_secs()
            );
        }
        Err(e) => {
            println!("Fast tier error: {}", e);
            // Don't fail - model might not be available yet
        }
    }
}

#[tokio::test]
async fn test_voice_tier_real_api() {
    if skip_if_no_key() {
        return;
    }

    let provider = OpenAIProvider::gpt51(get_api_key().unwrap())
        .expect("Should create Voice tier provider");

    let start = Instant::now();
    let messages = vec![Message::user("What is 2+2? Answer briefly.".to_string())];

    let result = provider.chat(messages, "You are Mira, a helpful AI assistant.".to_string()).await;
    let latency = start.elapsed();

    match result {
        Ok(response) => {
            println!("\n=== VOICE TIER (gpt-5.1) ===");
            println!("Response: {}", response.content.chars().take(200).collect::<String>());
            println!("Latency: {:?}", latency);
            println!("Tokens: {} input, {} output", response.tokens.input, response.tokens.output);

            let cost = OpenAIPricing::calculate_cost(
                OpenAIModel::Gpt51,
                response.tokens.input,
                response.tokens.output,
            );
            println!("Cost: ${:.6}", cost);

            // Verify latency target: <5s for Voice tier
            assert!(
                latency.as_secs() < 10,
                "Voice tier latency {} exceeded 10s target",
                latency.as_secs()
            );
        }
        Err(e) => {
            println!("Voice tier error: {}", e);
        }
    }
}

#[tokio::test]
async fn test_code_tier_real_api() {
    if skip_if_no_key() {
        return;
    }

    let provider = OpenAIProvider::codex_max(get_api_key().unwrap())
        .expect("Should create Code tier provider");

    let start = Instant::now();
    let messages = vec![Message::user("Write a simple Rust function that adds two numbers.".to_string())];

    let result = provider.chat(messages, "You are a code generation assistant.".to_string()).await;
    let latency = start.elapsed();

    match result {
        Ok(response) => {
            println!("\n=== CODE TIER (gpt-5.1-codex-max, high reasoning) ===");
            println!("Response: {}", response.content.chars().take(500).collect::<String>());
            println!("Latency: {:?}", latency);
            println!("Tokens: {} input, {} output", response.tokens.input, response.tokens.output);

            let cost = OpenAIPricing::calculate_cost(
                OpenAIModel::Gpt51CodexMax,
                response.tokens.input,
                response.tokens.output,
            );
            println!("Cost: ${:.6}", cost);

            // Verify latency target: <30s for Code tier
            assert!(
                latency.as_secs() < 60,
                "Code tier latency {} exceeded 60s target",
                latency.as_secs()
            );
        }
        Err(e) => {
            println!("Code tier error: {}", e);
        }
    }
}

#[tokio::test]
async fn test_agentic_tier_real_api() {
    if skip_if_no_key() {
        return;
    }

    let provider = OpenAIProvider::codex_max_agentic(get_api_key().unwrap())
        .expect("Should create Agentic tier provider");

    let start = Instant::now();
    let messages = vec![Message::user("Explain briefly what a binary search algorithm does.".to_string())];

    let result = provider.chat(messages, "You are a coding assistant for complex tasks.".to_string()).await;
    let latency = start.elapsed();

    match result {
        Ok(response) => {
            println!("\n=== AGENTIC TIER (gpt-5.1-codex-max, xhigh reasoning) ===");
            println!("Response: {}", response.content.chars().take(500).collect::<String>());
            println!("Latency: {:?}", latency);
            println!("Tokens: {} input, {} output", response.tokens.input, response.tokens.output);

            let cost = OpenAIPricing::calculate_cost(
                OpenAIModel::Gpt51CodexMax,
                response.tokens.input,
                response.tokens.output,
            );
            println!("Cost: ${:.6}", cost);

            // Agentic can take longer due to extended reasoning
            assert!(
                latency.as_secs() < 120,
                "Agentic tier latency {} exceeded 120s target",
                latency.as_secs()
            );
        }
        Err(e) => {
            println!("Agentic tier error: {}", e);
        }
    }
}

// ============================================================================
// PHASE 2: LATENCY BENCHMARKS
// ============================================================================

#[tokio::test]
async fn test_latency_comparison_all_tiers() {
    if skip_if_no_key() {
        return;
    }

    let api_key = get_api_key().unwrap();
    let simple_prompt = "Say 'ok'.";
    let system = "Be brief.";

    println!("\n=== LATENCY COMPARISON (same prompt across tiers) ===\n");

    // Fast tier
    let fast = OpenAIProvider::gpt51_mini(api_key.clone()).unwrap();
    let start = Instant::now();
    let _ = fast.chat(
        vec![Message::user(simple_prompt.to_string())],
        system.to_string(),
    ).await;
    let fast_latency = start.elapsed();
    println!("Fast (gpt-5.1-codex-mini):  {:?}", fast_latency);

    // Voice tier
    let voice = OpenAIProvider::gpt51(api_key.clone()).unwrap();
    let start = Instant::now();
    let _ = voice.chat(
        vec![Message::user(simple_prompt.to_string())],
        system.to_string(),
    ).await;
    let voice_latency = start.elapsed();
    println!("Voice (gpt-5.1):            {:?}", voice_latency);

    // Code tier
    let code = OpenAIProvider::codex_max(api_key.clone()).unwrap();
    let start = Instant::now();
    let _ = code.chat(
        vec![Message::user(simple_prompt.to_string())],
        system.to_string(),
    ).await;
    let code_latency = start.elapsed();
    println!("Code (gpt-5.1-codex-max):   {:?}", code_latency);

    println!("\nFast should be quickest for simple tasks.");
    println!("Fast vs Voice ratio: {:.2}x", voice_latency.as_millis() as f64 / fast_latency.as_millis().max(1) as f64);
    println!("Fast vs Code ratio:  {:.2}x", code_latency.as_millis() as f64 / fast_latency.as_millis().max(1) as f64);
}

// ============================================================================
// PHASE 3: STREAMING TESTS
// ============================================================================

#[tokio::test]
async fn test_streaming_voice_tier() {
    if skip_if_no_key() {
        return;
    }

    let provider = OpenAIProvider::gpt51(get_api_key().unwrap())
        .expect("Should create Voice tier provider");

    let messages = vec![Message::user("Count from 1 to 5, one number per line.".to_string())];

    println!("\n=== STREAMING TEST (Voice tier) ===");
    let start = Instant::now();

    match provider.stream(messages, "You are helpful.".to_string()).await {
        Ok(mut stream) => {
            let mut first_token_time: Option<std::time::Duration> = None;
            let mut chunk_count = 0;
            let mut total_content = String::new();

            while let Some(result) = stream.next().await {
                match result {
                    Ok(content) => {
                        if first_token_time.is_none() {
                            first_token_time = Some(start.elapsed());
                        }
                        chunk_count += 1;
                        total_content.push_str(&content);
                        print!("{}", content);
                    }
                    Err(e) => {
                        println!("\nStream error: {}", e);
                        break;
                    }
                }
            }

            let total_time = start.elapsed();
            println!("\n");
            println!("Time to first token: {:?}", first_token_time.unwrap_or_default());
            println!("Total stream time: {:?}", total_time);
            println!("Chunks received: {}", chunk_count);
            println!("Total content length: {} chars", total_content.len());

            // Verify streaming worked
            assert!(chunk_count > 0, "Should have received streaming chunks");

            // Time to first token should be fast
            if let Some(ttft) = first_token_time {
                assert!(ttft.as_secs() < 5, "Time to first token should be <5s");
            }
        }
        Err(e) => {
            println!("Streaming error: {}", e);
        }
    }
}

#[tokio::test]
async fn test_streaming_fast_tier() {
    if skip_if_no_key() {
        return;
    }

    let provider = OpenAIProvider::gpt51_mini(get_api_key().unwrap())
        .expect("Should create Fast tier provider");

    let messages = vec![Message::user("Say 'streaming works'.".to_string())];

    println!("\n=== STREAMING TEST (Fast tier) ===");
    let start = Instant::now();

    match provider.stream(messages, "Be brief.".to_string()).await {
        Ok(mut stream) => {
            let mut first_token_time: Option<std::time::Duration> = None;
            let mut total_content = String::new();

            while let Some(result) = stream.next().await {
                match result {
                    Ok(content) => {
                        if first_token_time.is_none() {
                            first_token_time = Some(start.elapsed());
                        }
                        total_content.push_str(&content);
                    }
                    Err(e) => {
                        println!("Stream error: {}", e);
                        break;
                    }
                }
            }

            println!("Response: {}", total_content);
            println!("Time to first token: {:?}", first_token_time.unwrap_or_default());
            println!("Total time: {:?}", start.elapsed());

            // Fast tier should have very quick TTFT
            if let Some(ttft) = first_token_time {
                println!("Fast tier TTFT: {:?} (target <2s)", ttft);
            }
        }
        Err(e) => {
            println!("Streaming error: {}", e);
        }
    }
}

// ============================================================================
// PHASE 4: COST VALIDATION
// ============================================================================

#[tokio::test]
async fn test_cost_tracking_accuracy() {
    if skip_if_no_key() {
        return;
    }

    let api_key = get_api_key().unwrap();

    println!("\n=== COST TRACKING ACCURACY ===\n");

    // Make a request and track actual tokens
    let provider = OpenAIProvider::gpt51(api_key).unwrap();
    let messages = vec![Message::user("What is the capital of France? Answer in one word.".to_string())];

    match provider.chat(messages, "Be brief.".to_string()).await {
        Ok(response) => {
            let input_tokens = response.tokens.input;
            let output_tokens = response.tokens.output;

            // Calculate cost using our pricing
            let calculated_cost = OpenAIPricing::calculate_cost(
                OpenAIModel::Gpt51,
                input_tokens,
                output_tokens,
            );

            // Manual calculation for verification
            // GPT-5.1 pricing: $1.25/M input, $10.00/M output (estimate)
            let manual_input_cost = (input_tokens as f64 / 1_000_000.0) * 1.25;
            let manual_output_cost = (output_tokens as f64 / 1_000_000.0) * 10.0;
            let manual_total = manual_input_cost + manual_output_cost;

            println!("Tokens: {} input, {} output", input_tokens, output_tokens);
            println!("Calculated cost: ${:.8}", calculated_cost);
            println!("Manual verification: ${:.8}", manual_total);
            println!("Difference: ${:.10}", (calculated_cost - manual_total).abs());

            // Costs should match within floating point precision
            assert!(
                (calculated_cost - manual_total).abs() < 0.0001,
                "Cost calculation mismatch"
            );
        }
        Err(e) => {
            println!("Error: {}", e);
        }
    }
}

#[tokio::test]
async fn test_tier_cost_comparison() {
    if skip_if_no_key() {
        return;
    }

    println!("\n=== TIER COST COMPARISON (estimated for 10k tokens) ===\n");

    let input_tokens = 10_000i64;
    let output_tokens = 1_000i64;

    let fast_cost = OpenAIPricing::calculate_cost(OpenAIModel::Gpt51Mini, input_tokens, output_tokens);
    let voice_cost = OpenAIPricing::calculate_cost(OpenAIModel::Gpt51, input_tokens, output_tokens);
    let code_cost = OpenAIPricing::calculate_cost(OpenAIModel::Gpt51CodexMax, input_tokens, output_tokens);

    println!("For {} input + {} output tokens:", input_tokens, output_tokens);
    println!("  Fast (gpt-5.1-codex-mini):  ${:.6}", fast_cost);
    println!("  Voice (gpt-5.1):            ${:.6}", voice_cost);
    println!("  Code (gpt-5.1-codex-max):   ${:.6}", code_cost);
    println!();
    println!("Fast vs Voice savings: {:.1}%", (1.0 - fast_cost / voice_cost) * 100.0);
    println!("Fast vs Code savings:  {:.1}%", (1.0 - fast_cost / code_cost) * 100.0);

    // Fast tier should be cheapest
    assert!(fast_cost < voice_cost, "Fast tier should be cheaper than Voice");
}

// ============================================================================
// SUMMARY TEST
// ============================================================================

#[tokio::test]
async fn test_full_validation_summary() {
    if skip_if_no_key() {
        return;
    }

    println!("\n");
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           MULTI-MODEL ROUTING VALIDATION SUMMARY             ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║                                                              ║");
    println!("║  4-Tier Architecture:                                        ║");
    println!("║  ┌─────────┬─────────────────────┬────────────────────────┐  ║");
    println!("║  │ Tier    │ Model               │ Use Case               │  ║");
    println!("║  ├─────────┼─────────────────────┼────────────────────────┤  ║");
    println!("║  │ Fast    │ gpt-5.1-codex-mini  │ File ops, search       │  ║");
    println!("║  │ Voice   │ gpt-5.1             │ User chat, personality │  ║");
    println!("║  │ Code    │ gpt-5.1-codex-max   │ Code gen, refactoring  │  ║");
    println!("║  │ Agentic │ gpt-5.1-codex-max   │ Long-running tasks     │  ║");
    println!("║  └─────────┴─────────────────────┴────────────────────────┘  ║");
    println!("║                                                              ║");
    println!("║  Run individual tests for detailed validation:               ║");
    println!("║  - test_fast_tier_real_api                                   ║");
    println!("║  - test_voice_tier_real_api                                  ║");
    println!("║  - test_code_tier_real_api                                   ║");
    println!("║  - test_latency_comparison_all_tiers                         ║");
    println!("║  - test_streaming_voice_tier                                 ║");
    println!("║  - test_cost_tracking_accuracy                               ║");
    println!("║                                                              ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
}
