//! Quick comparison of MCP vs Chat tool outputs for indexer/code.rs

use anyhow::Result;
use serde_json::json;
use sqlx::SqlitePool;
use std::path::Path;
use std::sync::Arc;

use mira::chat::tools::{code_intel::CodeIntelTools, git_intel::GitIntelTools};
use mira::core::SemanticSearch;

#[tokio::main]
async fn main() -> Result<()> {
    let db = SqlitePool::connect("sqlite:/home/peter/.mira/mira.db").await?;
    let cwd = Path::new("/home/peter/Mira");
    let db_opt = Some(db);
    let semantic: Option<Arc<SemanticSearch>> = None;

    let code_tools = CodeIntelTools { cwd, db: &db_opt, semantic: &semantic };
    let git_tools = GitIntelTools { cwd, db: &db_opt, semantic: &semantic };

    // 1. Get symbols from indexer
    println!("\n=== SYMBOLS: indexer/code.rs ===");
    let result = code_tools.get_symbols(&json!({
        "file_path": "/home/peter/Mira/src/indexer/code.rs"
    })).await?;
    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    println!("Count: {}", parsed["count"]);
    if let Some(symbols) = parsed["symbols"].as_array() {
        for s in symbols.iter().take(5) {
            println!("  {} ({}) lines {}-{}", 
                s["name"].as_str().unwrap_or("?"),
                s["type"].as_str().unwrap_or("?"),
                s["start_line"], s["end_line"]);
        }
        if symbols.len() > 5 {
            println!("  ... and {} more", symbols.len() - 5);
        }
    }

    // 2. Recent commits mentioning "index"
    println!("\n=== COMMITS: 'index' ===");
    let result = git_tools.search_commits(&json!({
        "query": "index",
        "limit": 5
    })).await?;
    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    println!("Count: {}", parsed["count"]);
    if let Some(commits) = parsed["commits"].as_array() {
        for c in commits.iter().take(3) {
            let hash = c["commit_hash"].as_str().unwrap_or("?");
            let msg = c["message"].as_str().unwrap_or("?");
            let first_line = msg.lines().next().unwrap_or("?");
            println!("  {} {}", &hash[..7], first_line);
        }
    }

    // 3. Call graph for index_file function
    println!("\n=== CALL GRAPH: index_file ===");
    let result = code_tools.get_call_graph(&json!({
        "symbol": "index_file",
        "depth": 1
    })).await?;
    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    println!("Symbol: {}", parsed["symbol"]);
    println!("Called by: {} functions", parsed["called_by"].as_array().map(|a| a.len()).unwrap_or(0));
    println!("Calls: {} functions", parsed["calls"].as_array().map(|a| a.len()).unwrap_or(0));

    // 4. Files that change with code.rs
    println!("\n=== COCHANGE: indexer/code.rs ===");
    let result = git_tools.find_cochange_patterns(&json!({
        "file_path": "/home/peter/Mira/src/indexer/code.rs",
        "limit": 5
    })).await?;
    let parsed: serde_json::Value = serde_json::from_str(&result)?;
    if let Some(patterns) = parsed["patterns"].as_array() {
        for p in patterns.iter().take(5) {
            println!("  {} ({}x, {:.0}% confidence)", 
                p["file"].as_str().unwrap_or("?"),
                p["cochange_count"],
                p["confidence"].as_f64().unwrap_or(0.0) * 100.0);
        }
    }

    Ok(())
}
