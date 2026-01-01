// src/main.rs
// Mira - Memory and Intelligence Layer for Claude Code

use anyhow::Result;
use clap::{Parser, Subcommand};
use mira::{db::Database, embeddings::Embeddings, mcp::MiraServer, web};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser)]
#[command(name = "mira")]
#[command(about = "Memory and Intelligence Layer for Claude Code")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run as MCP server (default, for Claude Code)
    Serve,

    /// Run web UI server (Mira Studio)
    Web {
        /// Port to listen on
        #[arg(short, long, default_value = "3000")]
        port: u16,
    },

    /// Index a project
    Index {
        /// Project path (default: current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Claude Code hook handlers
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },

    /// Test chat endpoint (for debugging without UI)
    TestChat {
        /// Message to send to DeepSeek
        message: String,

        /// Enable verbose output (shows reasoning, tool calls, etc.)
        #[arg(short, long)]
        verbose: bool,

        /// Project path to set context
        #[arg(short, long)]
        project: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Handle PermissionRequest hooks
    Permission,
    /// Handle SessionStart hooks - captures Claude's session_id
    SessionStart,
    /// Legacy PostToolUse hook (no-op for compatibility)
    Posttool,
    /// Legacy PreToolUse hook (no-op for compatibility)
    Pretool,
}

fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}

async fn run_mcp_server() -> Result<()> {
    // Open database
    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);

    // Initialize embeddings if API key available
    let embeddings = std::env::var("GEMINI_API_KEY")
        .ok()
        .or_else(|| std::env::var("GOOGLE_API_KEY").ok())
        .map(|key| Arc::new(Embeddings::new(key)));

    if embeddings.is_some() {
        info!("Semantic search enabled (Gemini API key found)");
    } else {
        info!("Semantic search disabled (no GEMINI_API_KEY)");
    }

    // Create shared broadcast channel for MCP <-> Web communication
    let (ws_tx, _) = tokio::sync::broadcast::channel::<mira_types::WsEvent>(256);

    // Shared session ID between MCP server and web server
    let session_id: Arc<tokio::sync::RwLock<Option<String>>> = Arc::new(tokio::sync::RwLock::new(None));

    // Spawn embedded web server in background
    let web_port: u16 = std::env::var("MIRA_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);

    let web_db = db.clone();
    let web_embeddings = embeddings.clone();
    let web_ws_tx = ws_tx.clone();
    let web_session_id = session_id.clone();

    tokio::spawn(async move {
        let state = web::state::AppState::with_broadcaster(web_db, web_embeddings, web_ws_tx, web_session_id);
        let app = web::create_router(state);
        let addr = format!("0.0.0.0:{}", web_port);

        if let Ok(listener) = tokio::net::TcpListener::bind(&addr).await {
            eprintln!("Mira Studio running on http://localhost:{}", web_port);
            let _ = axum::serve(listener, app).await;
        }
    });

    // Create MCP server with broadcaster and shared session ID
    let server = MiraServer::with_broadcaster(db, embeddings, ws_tx, session_id);

    // Run with stdio transport
    let transport = rmcp::transport::io::stdio();
    let service = rmcp::serve_server(server, transport).await?;
    service.waiting().await?;

    Ok(())
}

async fn run_web_server(port: u16) -> Result<()> {
    // Open database
    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);

    // Initialize embeddings if API key available
    let embeddings = std::env::var("GEMINI_API_KEY")
        .ok()
        .or_else(|| std::env::var("GOOGLE_API_KEY").ok())
        .map(|key| Arc::new(Embeddings::new(key)));

    if embeddings.is_some() {
        info!("Semantic search enabled (Gemini API key found)");
    } else {
        info!("Semantic search disabled (no GEMINI_API_KEY)");
    }

    // Create app state
    let state = web::state::AppState::new(db, embeddings);

    // Create router
    let app = web::create_router(state);

    // Start server
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("Mira Studio running on http://localhost:{}", port);
    println!("Mira Studio running on http://localhost:{}", port);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn run_index(path: Option<PathBuf>) -> Result<()> {
    let path = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    info!("Indexing project at {}", path.display());

    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);

    let embeddings = std::env::var("GEMINI_API_KEY")
        .ok()
        .or_else(|| std::env::var("GOOGLE_API_KEY").ok())
        .map(|key| Arc::new(Embeddings::new(key)));

    // Get or create project
    let (project_id, _project_name) = db.get_or_create_project(
        path.to_string_lossy().as_ref(),
        path.file_name().and_then(|n| n.to_str()),
    )?;

    let stats = mira::indexer::index_project(&path, db, embeddings, Some(project_id)).await?;

    println!(
        "Indexed {} files, {} symbols, {} code chunks",
        stats.files, stats.symbols, stats.chunks
    );

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env files (global first, then project - project overrides)
    if let Some(home) = dirs::home_dir() {
        let _ = dotenvy::from_path(home.join(".mira/.env"));
    }
    let _ = dotenvy::dotenv(); // Load .env from current directory

    let cli = Cli::parse();

    // Set up logging based on command
    let log_level = match &cli.command {
        Some(Commands::Serve) | None => Level::WARN, // Quiet for MCP stdio
        Some(Commands::Hook { .. }) => Level::WARN,
        Some(Commands::Web { .. }) => Level::INFO,   // Verbose for web server
        Some(Commands::Index { .. }) => Level::INFO,
        Some(Commands::TestChat { verbose: true, .. }) => Level::DEBUG, // Full debug for verbose test
        Some(Commands::TestChat { .. }) => Level::INFO,
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    match cli.command {
        None | Some(Commands::Serve) => {
            run_mcp_server().await?;
        }
        Some(Commands::Web { port }) => {
            run_web_server(port).await?;
        }
        Some(Commands::Index { path }) => {
            run_index(path).await?;
        }
        Some(Commands::Hook { action }) => match action {
            HookAction::Permission => {
                mira::hooks::permission::run().await?;
            }
            HookAction::SessionStart => {
                mira::hooks::session::run()?;
            }
            HookAction::Posttool | HookAction::Pretool => {
                // Legacy no-op hooks for compatibility
            }
        },
        Some(Commands::TestChat { message, verbose, project }) => {
            run_test_chat(message, verbose, project).await?;
        }
    }

    Ok(())
}

/// Test chat endpoint without UI - useful for debugging
async fn run_test_chat(message: String, verbose: bool, project: Option<PathBuf>) -> Result<()> {
    use tokio::sync::broadcast;
    use mira_types::WsEvent;

    println!("=== Mira Chat Test ===\n");

    // Open database
    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);

    // Initialize embeddings
    let embeddings = std::env::var("GEMINI_API_KEY")
        .ok()
        .or_else(|| std::env::var("GOOGLE_API_KEY").ok())
        .map(|key| Arc::new(Embeddings::new(key)));

    // Create broadcast channel (we'll listen to it for events)
    let (ws_tx, mut ws_rx) = broadcast::channel::<WsEvent>(256);

    // Get DeepSeek API key
    let api_key = match std::env::var("DEEPSEEK_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            eprintln!("ERROR: DEEPSEEK_API_KEY not set");
            std::process::exit(1);
        }
    };

    // Create DeepSeek client
    let deepseek = Arc::new(web::deepseek::DeepSeekClient::new(api_key, ws_tx.clone()));

    // Create Claude manager
    let claude_manager = Arc::new(web::claude::ClaudeManager::new(ws_tx.clone()));

    // Set up project if specified
    if let Some(ref path) = project {
        let path_str = path.to_string_lossy();
        let name = path.file_name().and_then(|n| n.to_str());
        if let Ok((id, detected_name)) = db.get_or_create_project(&path_str, name) {
            let display_name = detected_name.unwrap_or_else(|| "Unknown".to_string());
            println!("Project: {} (id={}) @ {}", display_name, id, path_str);
        }
    }

    println!("Message: {}\n", message);
    println!("---");

    // Spawn event listener in background
    let verbose_clone = verbose;
    let event_task = tokio::spawn(async move {
        while let Ok(event) = ws_rx.recv().await {
            match event {
                WsEvent::Thinking { content, phase } => {
                    if verbose_clone {
                        eprintln!("[THINKING:{:?}] {}", phase, content);
                    }
                }
                WsEvent::TerminalOutput { content, is_stderr } => {
                    if is_stderr {
                        eprint!("{}", content);
                    } else {
                        print!("{}", content);
                    }
                }
                WsEvent::ToolStart { tool_name, arguments, call_id } => {
                    println!("\n[TOOL:{}] id={}", tool_name, call_id);
                    if verbose_clone {
                        println!("  args: {}", arguments);
                    }
                }
                WsEvent::ToolResult { tool_name, result, success, call_id, duration_ms } => {
                    let status = if success { "OK" } else { "ERR" };
                    println!("[TOOL:{}:{}] {} ({}ms)", tool_name, status, call_id, duration_ms);
                    if verbose_clone {
                        println!("  result: {}", result);
                    }
                }
                WsEvent::ChatComplete { content, model, usage } => {
                    println!("\n=== Response ({}) ===", model);
                    println!("{}", content);
                    if let Some(u) = usage {
                        println!("\n[Usage: {} prompt, {} completion, cache hit: {:?}]",
                            u.prompt_tokens, u.completion_tokens, u.cache_hit_tokens);
                    }
                }
                WsEvent::ChatError { message } => {
                    eprintln!("\n[ERROR] {}", message);
                }
                WsEvent::ClaudeSpawned { instance_id, working_dir } => {
                    println!("\n[CLAUDE:{}] spawned in {}", instance_id, working_dir);
                }
                WsEvent::ClaudeStopped { instance_id } => {
                    println!("\n[CLAUDE:{}] stopped", instance_id);
                }
                _ => {
                    if verbose_clone {
                        eprintln!("[EVENT] {:?}", event);
                    }
                }
            }
        }
    });

    // Build messages
    let mut messages = vec![web::deepseek::Message::system(
        "You are an AI assistant integrated with Mira Studio.\n\
         You have tools for searching memory, code, and spawning Claude Code.\n\
         Use tools when helpful. Be concise."
    )];
    messages.push(web::deepseek::Message::user(&message));

    // Get tools
    let tools = web::deepseek::mira_tools();

    // Call DeepSeek
    match deepseek.chat(messages, Some(tools)).await {
        Ok(result) => {
            // Execute any tool calls
            if let Some(ref tool_calls) = result.tool_calls {
                println!("\n=== Executing {} tool calls ===", tool_calls.len());

                for tc in tool_calls {
                    let args: serde_json::Value =
                        serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);

                    let tool_result = match tc.function.name.as_str() {
                        "recall_memories" => {
                            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                            execute_recall_test(&db, embeddings.as_ref(), query, 5).await
                        }
                        "search_code" => {
                            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                            execute_code_search_test(&db, embeddings.as_ref(), query, 10).await
                        }
                        "spawn_claude" => {
                            let prompt = args.get("initial_prompt").and_then(|v| v.as_str()).unwrap_or("");
                            let working_dir = args.get("working_directory")
                                .and_then(|v| v.as_str())
                                .map(String::from)
                                .or_else(|| project.as_ref().map(|p| p.to_string_lossy().to_string()))
                                .unwrap_or_else(|| ".".to_string());

                            match claude_manager.spawn(working_dir, Some(prompt.to_string())).await {
                                Ok(id) => format!("Claude spawned with ID: {}", id),
                                Err(e) => format!("Error: {}", e),
                            }
                        }
                        "send_to_claude" => {
                            let id = args.get("instance_id").and_then(|v| v.as_str()).unwrap_or("");
                            let msg = args.get("message").and_then(|v| v.as_str()).unwrap_or("");

                            match claude_manager.send_input(id, msg).await {
                                Ok(_) => "Message sent".to_string(),
                                Err(e) => format!("Error: {}", e),
                            }
                        }
                        _ => format!("Unknown tool: {}", tc.function.name),
                    };

                    let _ = ws_tx.send(WsEvent::ToolResult {
                        tool_name: tc.function.name.clone(),
                        result: tool_result,
                        success: true,
                        call_id: tc.id.clone(),
                        duration_ms: 0,
                    });
                }
            }

            // Print reasoning if verbose
            if verbose {
                if let Some(reasoning) = &result.reasoning_content {
                    println!("\n=== Reasoning ===");
                    println!("{}", reasoning);
                }
            }

            // Print final response
            if let Some(content) = &result.content {
                println!("\n=== Final Response ===");
                println!("{}", content);
            }

            // Print usage
            if let Some(usage) = &result.usage {
                println!("\n=== Usage ===");
                println!("Prompt tokens: {}", usage.prompt_tokens);
                println!("Completion tokens: {}", usage.completion_tokens);
                if let Some(hit) = usage.prompt_cache_hit_tokens {
                    println!("Cache hit: {} tokens", hit);
                }
                if let Some(miss) = usage.prompt_cache_miss_tokens {
                    println!("Cache miss: {} tokens", miss);
                }
            }
        }
        Err(e) => {
            eprintln!("\n=== Error ===");
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }

    // Give events time to flush
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    event_task.abort();

    println!("\n=== Test Complete ===");
    Ok(())
}

/// Execute recall for test CLI
async fn execute_recall_test(
    db: &Arc<Database>,
    embeddings: Option<&Arc<Embeddings>>,
    query: &str,
    limit: i64,
) -> String {
    if let Some(embeddings) = embeddings {
        if let Ok(query_embedding) = embeddings.embed(query).await {
            let conn = db.conn();

            let embedding_bytes: Vec<u8> = query_embedding
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();

            let result: Result<Vec<String>, _> = (|| {
                let mut stmt = conn.prepare(
                    "SELECT content FROM memory_facts f
                     JOIN vec_memory v ON f.id = v.fact_id
                     ORDER BY vec_distance_cosine(v.embedding, ?1)
                     LIMIT ?2"
                )?;

                let rows = stmt.query_map(
                    rusqlite::params![embedding_bytes, limit],
                    |row| row.get(0),
                )?;

                rows.collect::<Result<Vec<_>, _>>()
            })();

            if let Ok(memories) = result {
                if !memories.is_empty() {
                    return format!("Found {} memories:\n{}", memories.len(), memories.join("\n---\n"));
                }
            }
        }
    }
    "No memories found".to_string()
}

/// Execute code search for test CLI
async fn execute_code_search_test(
    db: &Arc<Database>,
    embeddings: Option<&Arc<Embeddings>>,
    query: &str,
    limit: i64,
) -> String {
    if let Some(embeddings) = embeddings {
        if let Ok(query_embedding) = embeddings.embed(query).await {
            let conn = db.conn();

            let embedding_bytes: Vec<u8> = query_embedding
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();

            let result: Result<Vec<String>, _> = (|| {
                let mut stmt = conn.prepare(
                    "SELECT file_path, chunk_content FROM vec_code
                     ORDER BY vec_distance_cosine(embedding, ?1)
                     LIMIT ?2"
                )?;

                let rows = stmt.query_map(
                    rusqlite::params![embedding_bytes, limit],
                    |row| {
                        let path: String = row.get(0)?;
                        let content: String = row.get(1)?;
                        Ok(format!("## {}\n```\n{}\n```", path, content))
                    },
                )?;

                rows.collect::<Result<Vec<_>, _>>()
            })();

            if let Ok(results) = result {
                if !results.is_empty() {
                    return format!("Found {} matches:\n{}", results.len(), results.join("\n\n"));
                }
            }
        }
    }
    "No code found".to_string()
}
