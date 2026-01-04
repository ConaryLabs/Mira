// src/main.rs
// Mira - Memory and Intelligence Layer for Claude Code

use anyhow::Result;
use clap::{Parser, Subcommand};
use mira::{background, db::Database, embeddings::Embeddings, mcp::MiraServer, web};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

/// Get embeddings client if API key is available (filters empty keys)
fn get_embeddings() -> Option<Arc<Embeddings>> {
    std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .map(|key| Arc::new(Embeddings::new(key)))
}

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

        /// Skip embeddings (faster, no semantic search)
        #[arg(long)]
        no_embed: bool,
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

    /// Debug cartographer module detection
    DebugCarto {
        /// Project path to analyze
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Debug session_start output
    DebugSession {
        /// Project path
        #[arg(short, long)]
        path: Option<PathBuf>,
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
    let embeddings = get_embeddings();

    if embeddings.is_some() {
        info!("Semantic search enabled (OpenAI embeddings)");
    } else {
        info!("Semantic search disabled (no OPENAI_API_KEY)");
    }

    // Create shared broadcast channel for MCP <-> Web communication
    let (ws_tx, _) = tokio::sync::broadcast::channel::<mira_types::WsEvent>(256);

    // Initialize DeepSeek client if API key available
    let deepseek = std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .map(|key| Arc::new(web::deepseek::DeepSeekClient::new(key)));

    if deepseek.is_some() {
        info!("DeepSeek enabled (for chat and module summaries)");
    } else {
        info!("DeepSeek disabled (no DEEPSEEK_API_KEY)");
    }

    // Shared state between MCP server and web server
    let session_id: Arc<tokio::sync::RwLock<Option<String>>> = Arc::new(tokio::sync::RwLock::new(None));
    let project: Arc<tokio::sync::RwLock<Option<mira_types::ProjectContext>>> = Arc::new(tokio::sync::RwLock::new(None));
    let pending_responses: Arc<tokio::sync::RwLock<std::collections::HashMap<String, tokio::sync::oneshot::Sender<String>>>> =
        Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));

    // Spawn embedded web server in background
    let web_port: u16 = std::env::var("MIRA_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);

    let web_db = db.clone();
    let web_embeddings = embeddings.clone();
    let web_ws_tx = ws_tx.clone();
    let web_session_id = session_id.clone();
    let web_project = project.clone();
    let web_pending = pending_responses.clone();

    tokio::spawn(async move {
        let state = web::state::AppState::with_broadcaster(web_db, web_embeddings, web_ws_tx, web_session_id, web_project, web_pending);
        let app = web::create_router(state);
        let addr = format!("0.0.0.0:{}", web_port);

        if let Ok(listener) = tokio::net::TcpListener::bind(&addr).await {
            eprintln!("Mira Studio running on http://localhost:{}", web_port);
            let _ = axum::serve(listener, app).await;
        }
    });

    // Spawn background worker for batch processing
    let bg_db = db.clone();
    let bg_embeddings = embeddings.clone();
    let bg_deepseek = deepseek.clone();
    let _shutdown_tx = background::spawn(bg_db, bg_embeddings, bg_deepseek);
    info!("Background worker started");

    // Create MCP server with broadcaster and shared state
    // Note: In combined mode, file watcher is not available (use `mira web` for full functionality)
    let server = MiraServer::with_broadcaster(db, embeddings, deepseek, ws_tx, session_id, project, pending_responses, None);

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
    let embeddings = get_embeddings();

    if embeddings.is_some() {
        info!("Semantic search enabled (OpenAI embeddings)");
    } else {
        info!("Semantic search disabled (no OPENAI_API_KEY)");
    }

    // Initialize DeepSeek for background summaries
    let deepseek = std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .map(|key| Arc::new(web::deepseek::DeepSeekClient::new(key)));

    // Spawn background worker for batch processing
    let _shutdown_tx = background::spawn(db.clone(), embeddings.clone(), deepseek);
    info!("Background worker started");

    // Spawn file watcher for automatic incremental indexing
    let (watcher_shutdown_tx, watcher_shutdown_rx) = tokio::sync::watch::channel(false);
    let watcher_handle = background::watcher::spawn(
        db.clone(),
        watcher_shutdown_rx,
    );
    info!("File watcher started");

    // Create app state with watcher handle
    let state = web::state::AppState::with_watcher(db, embeddings, watcher_handle);

    // Create router
    let app = web::create_router(state);

    // Start server
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("Mira Studio running on http://localhost:{}", port);
    println!("Mira Studio running on http://localhost:{}", port);

    axum::serve(listener, app).await?;

    // Shutdown watcher on exit
    let _ = watcher_shutdown_tx.send(true);

    Ok(())
}

async fn run_index(path: Option<PathBuf>, no_embed: bool) -> Result<()> {
    let path = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    info!("Indexing project at {}", path.display());

    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);

    let embeddings = if no_embed { None } else { get_embeddings() };

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
        Some(Commands::DebugCarto { .. }) => Level::DEBUG,
        Some(Commands::DebugSession { .. }) => Level::DEBUG,
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
        Some(Commands::Index { path, no_embed }) => {
            run_index(path, no_embed).await?;
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
        Some(Commands::DebugCarto { path }) => {
            run_debug_carto(path)?;
        }
        Some(Commands::DebugSession { path }) => {
            run_debug_session(path).await?;
        }
    }

    Ok(())
}

/// Debug session_start output
async fn run_debug_session(path: Option<PathBuf>) -> Result<()> {
    use std::sync::Arc;

    let project_path = path.unwrap_or_else(|| std::env::current_dir().unwrap());
    println!("=== Debug Session Start ===\n");
    println!("Project: {:?}\n", project_path);

    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);

    // Create a minimal MCP server context
    let server = mira::mcp::MiraServer::new(db.clone(), None);

    // Call session_start
    let result = mira::mcp::tools::project::session_start(
        &server,
        project_path.to_string_lossy().to_string(),
        None,
        None,
    ).await;

    match result {
        Ok(output) => {
            println!("--- Session Start Output ({} chars) ---\n", output.len());
            println!("{}", output);
        }
        Err(e) => {
            println!("ERROR: {}", e);
        }
    }

    Ok(())
}

/// Debug cartographer module detection
fn run_debug_carto(path: Option<PathBuf>) -> Result<()> {
    let project_path = path.unwrap_or_else(|| std::env::current_dir().unwrap());
    println!("=== Cartographer Debug ===\n");
    println!("Project path: {:?}\n", project_path);

    // Test module detection
    let modules = mira::cartographer::detect_rust_modules(&project_path);
    println!("Detected {} modules:\n", modules.len());

    for m in &modules {
        println!("  {} ({})", m.id, m.path);
        if let Some(ref purpose) = m.purpose {
            println!("    Purpose: {}", purpose);
        }
    }

    // Try full map generation with database
    println!("\n--- Database Integration ---\n");
    let db_path = get_db_path();
    let db = Database::open(&db_path)?;
    let (project_id, name) = db.get_or_create_project(
        project_path.to_str().unwrap(),
        None,
    )?;
    println!("Project ID: {}, Name: {:?}", project_id, name);

    match mira::cartographer::get_or_generate_map(
        &db,
        project_id,
        project_path.to_str().unwrap(),
        name.as_deref().unwrap_or("unknown"),
        "rust",
    ) {
        Ok(map) => {
            println!("\nCodebase map generated with {} modules", map.modules.len());
            println!("\n{}", mira::cartographer::format_compact(&map));
        }
        Err(e) => {
            println!("\nError generating map: {}", e);
        }
    }

    Ok(())
}

/// Test chat endpoint without UI - useful for debugging
/// Uses HTTP to call the web server so messages are stored and background tasks run
async fn run_test_chat(message: String, verbose: bool, project: Option<PathBuf>) -> Result<()> {
    use mira_types::{ApiResponse, ChatRequest};

    println!("=== Mira Chat Test ===\n");

    let client = reqwest::Client::new();

    // Set project if specified
    if let Some(ref path) = project {
        println!("Project: {}", path.display());
        let project_request = serde_json::json!({
            "path": path.to_string_lossy(),
            "name": path.file_name().and_then(|n| n.to_str()).unwrap_or("project")
        });
        let _ = client
            .post("http://localhost:3000/api/project/set")
            .json(&project_request)
            .send()
            .await;
    }
    println!("Message: {}\n", message);
    println!("---");

    // Build request
    let request = ChatRequest {
        message: message.clone(),
        history: vec![], // Let server load from DB
    };

    // Make HTTP request to local server
    let response = client
        .post("http://localhost:3000/api/chat/test")
        .json(&request)
        .send()
        .await;

    match response {
        Ok(resp) => {
            if !resp.status().is_success() {
                eprintln!("\n=== Error ===");
                eprintln!("HTTP {}: {}", resp.status(), resp.text().await.unwrap_or_default());
                std::process::exit(1);
            }

            let body: ApiResponse<serde_json::Value> = resp.json().await?;

            if !body.success {
                eprintln!("\n=== Error ===");
                eprintln!("{}", body.error.unwrap_or_else(|| "Unknown error".to_string()));
                std::process::exit(1);
            }

            if let Some(data) = body.data {
                // Print reasoning if verbose
                if verbose {
                    if let Some(reasoning) = data.get("reasoning_content").and_then(|v| v.as_str()) {
                        if !reasoning.is_empty() {
                            println!("\n=== Reasoning ===");
                            println!("{}", reasoning);
                        }
                    }
                }

                // Print tool calls if any
                if let Some(tool_calls) = data.get("tool_calls").and_then(|v| v.as_array()) {
                    if !tool_calls.is_empty() {
                        println!("\n=== Tool Calls ===");
                        for tc in tool_calls {
                            let name = tc.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()).unwrap_or("?");
                            let id = tc.get("id").and_then(|i| i.as_str()).unwrap_or("?");
                            println!("[{}] id={}", name, id);
                            if verbose {
                                if let Some(args) = tc.get("function").and_then(|f| f.get("arguments")).and_then(|a| a.as_str()) {
                                    println!("  args: {}", args);
                                }
                            }
                        }
                    }
                }

                // Print tool results if any
                if let Some(tool_results) = data.get("tool_results").and_then(|v| v.as_array()) {
                    if !tool_results.is_empty() {
                        println!("\n=== Tool Results ===");
                        for tr in tool_results {
                            let call_id = tr.get("call_id").and_then(|i| i.as_str()).unwrap_or("?");
                            let result = tr.get("result").and_then(|r| r.as_str()).unwrap_or("?");
                            println!("[{}] {}", call_id, if result.len() > 500 { &result[..500] } else { result });
                        }
                    }
                }

                // Print final response
                if let Some(content) = data.get("content").and_then(|v| v.as_str()) {
                    println!("\n=== Response ===");
                    println!("{}", content);
                } else if let Some(reasoning) = data.get("reasoning_content").and_then(|v| v.as_str()) {
                    // DeepSeek reasoner sometimes puts response in reasoning_content
                    if !verbose {
                        println!("\n=== Response ===");
                        println!("{}", reasoning);
                    }
                }

                // Print usage
                if let Some(usage) = data.get("usage") {
                    println!("\n=== Usage ===");
                    if let Some(prompt) = usage.get("prompt_tokens").and_then(|v| v.as_u64()) {
                        println!("Prompt tokens: {}", prompt);
                    }
                    if let Some(completion) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
                        println!("Completion tokens: {}", completion);
                    }
                    if let Some(hit) = usage.get("cache_hit_tokens").and_then(|v| v.as_u64()) {
                        println!("Cache hit: {} tokens", hit);
                    }
                    if let Some(miss) = usage.get("cache_miss_tokens").and_then(|v| v.as_u64()) {
                        println!("Cache miss: {} tokens", miss);
                    }
                }

                // Print timing
                if let Some(duration) = data.get("duration_ms").and_then(|v| v.as_u64()) {
                    println!("\n[Total: {}ms]", duration);
                }
            }
        }
        Err(e) => {
            eprintln!("\n=== Error ===");
            eprintln!("Failed to connect to Mira server at localhost:3000");
            eprintln!("Make sure 'mira web' is running");
            eprintln!("\nDetails: {}", e);
            std::process::exit(1);
        }
    }

    println!("\n=== Test Complete ===");
    Ok(())
}

