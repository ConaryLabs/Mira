use anyhow::Result;
use clap::{Parser, Subcommand};
use mira::background;
use mira::db::pool::DatabasePool;
use mira::db::Database;
use mira::embeddings::EmbeddingClient;
use mira::http::create_shared_client;
use mira::llm::DeepSeekClient;
use mira::hooks::session::read_claude_session_id;
use mira::mcp::{
    MiraServer,
    SessionStartRequest, SetProjectRequest, RememberRequest, RecallRequest,
    ForgetRequest, GetSymbolsRequest, SemanticCodeSearchRequest,
    FindCallersRequest, FindCalleesRequest, CheckCapabilityRequest,
    TaskRequest, GoalRequest, IndexRequest, SessionHistoryRequest,
    ConsultArchitectRequest, ConsultCodeReviewerRequest,
    ConsultPlanReviewerRequest, ConsultScopeAnalystRequest,
    ConsultSecurityRequest, ConsultExpertsRequest, ConfigureExpertRequest,
    ReplyToMiraRequest
};
use mira::tools::core::ToolContext;
use mira_types::ProjectContext;
use tokio::sync::watch;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

/// Get embeddings client if API key is available
/// Supports multiple providers via MIRA_EMBEDDING_PROVIDER env var (openai, google)
#[allow(dead_code)]
fn get_embeddings(http_client: reqwest::Client) -> Option<Arc<EmbeddingClient>> {
    get_embeddings_with_db(None, http_client)
}

/// Get embeddings client with database for usage tracking
///
/// Environment variables:
/// - MIRA_EMBEDDING_PROVIDER: "openai" (default) or "google"
/// - MIRA_EMBEDDING_MODEL: Model name (e.g., "text-embedding-3-small", "gemini-embedding-001")
/// - MIRA_EMBEDDING_DIMENSIONS: Output dimensions (Google only, default: 768)
/// - MIRA_EMBEDDING_TASK_TYPE: Task type for Google (RETRIEVAL_DOCUMENT, SEMANTIC_SIMILARITY, etc.)
/// - OPENAI_API_KEY: Required for OpenAI provider
/// - GOOGLE_API_KEY: Required for Google provider
fn get_embeddings_with_db(db: Option<Arc<Database>>, http_client: reqwest::Client) -> Option<Arc<EmbeddingClient>> {
    let client = EmbeddingClient::from_env_with_http_client(db.clone(), http_client)?;

    // Log the configured provider
    info!(
        "Embedding provider: {} (model: {}, {} dimensions)",
        client.provider(),
        client.model_name(),
        client.dimensions()
    );

    Some(Arc::new(client))
}

/// Get DeepSeek client if API key is available
fn get_deepseek(http_client: reqwest::Client) -> Option<Arc<DeepSeekClient>> {
    std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .map(|key| Arc::new(DeepSeekClient::with_http_client(key, "deepseek-reasoner".into(), http_client)))
}

#[derive(Parser)]
#[command(name = "mira")]
#[command(about = "Memory and Intelligence Layer for AI Agents")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run as MCP server (default)
    Serve,

    /// Execute a tool directly
    Tool {
        /// Tool name (e.g. search_code, remember)
        #[arg(index = 1)]
        name: String,

        /// JSON arguments (e.g. '{"query": "foo"}')
        #[arg(index = 2)]
        args: String,
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

    /// Client hook handlers
    Hook {
        #[command(subcommand)]
        action: HookAction,
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

    /// LLM proxy server management
    Proxy {
        #[command(subcommand)]
        action: ProxyAction,
    },

    /// Manage LLM backends
    Backend {
        #[command(subcommand)]
        action: BackendAction,
    },
}

#[derive(Subcommand)]
enum ProxyAction {
    /// Start the proxy server
    Start {
        /// Config file path (default: ~/.config/mira/proxy.toml)
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Host to bind to (overrides config)
        #[arg(long)]
        host: Option<String>,

        /// Port to listen on (overrides config)
        #[arg(short, long)]
        port: Option<u16>,

        /// Run in background (daemon mode)
        #[arg(short, long)]
        daemon: bool,
    },

    /// Stop the running proxy server
    Stop,

    /// Check proxy server status
    Status,
}

#[derive(Subcommand)]
enum HookAction {
    /// Handle PermissionRequest hooks
    Permission,
    /// Handle SessionStart hooks - captures Claude's session_id
    SessionStart,
    /// Handle PreCompact hooks - preserve context before summarization
    PreCompact,
    /// Handle UserPromptSubmit hooks - inject proactive context
    UserPrompt,
    /// Legacy PostToolUse hook (no-op for compatibility)
    Posttool,
    /// Legacy PreToolUse hook (no-op for compatibility)
    Pretool,
}

#[derive(Subcommand)]
enum BackendAction {
    /// List configured backends
    List,

    /// Set the default backend
    Use {
        /// Backend name to set as default
        name: String,
    },

    /// Test connectivity to a backend
    Test {
        /// Backend name to test
        name: String,
    },

    /// Print environment variables for a backend (shell export format)
    Env {
        /// Backend name (uses default if not specified)
        name: Option<String>,
    },

    /// Show usage statistics
    Usage {
        /// Filter by backend name
        #[arg(short, long)]
        backend: Option<String>,

        /// Number of days to show (default: 7)
        #[arg(short, long, default_value = "7")]
        days: u32,
    },
}

fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}

/// Setup server context with database, embeddings, and restored project/session state
async fn setup_server_context() -> Result<MiraServer> {
    // Create shared HTTP client for all network operations
    let http_client = create_shared_client();

    // Open database (both legacy sync and new async pool)
    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);
    let pool = Arc::new(DatabasePool::open(&db_path).await?);
    let embeddings = get_embeddings_with_db(Some(db.clone()), http_client.clone());

    // Create server context
    let server = MiraServer::new(db.clone(), pool, embeddings);

    // Restore context (Project & Session)
    if let Ok(Some(path)) = db.get_last_active_project() {
        if let Ok((id, name)) = db.get_or_create_project(&path, None) {
            let project = ProjectContext {
                id,
                path: path.clone(),
                name,
            };
            server.set_project(project).await;
        }
    } else {
        // Fallback: Check if CWD is a project
        if let Ok(cwd) = std::env::current_dir() {
            let path_str = cwd.to_string_lossy().to_string();
            // Simple heuristic: if we can get a name, it's likely a project
            if let Ok((id, name)) = db.get_or_create_project(&path_str, None) {
                 let project = ProjectContext {
                    id,
                    path: path_str,
                    name,
                };
                server.set_project(project).await;
            }
        }
    }

    if let Ok(Some(sid)) = db.get_server_state("active_session_id") {
        server.set_session_id(sid).await;
    }

    Ok(server)
}

async fn run_tool(name: String, args: String) -> Result<()> {
    // Setup server context with restored project/session state
    let server = setup_server_context().await?;

    // Execute tool
    let res = match name.as_str() {
        "session_start" => {
            let req: SessionStartRequest = serde_json::from_str(&args)?;
            // Use provided session ID, or fall back to Claude's hook-generated ID
            let session_id = req.session_id.or_else(read_claude_session_id);
            mira::tools::session_start(&server, req.project_path, req.name, session_id).await
        }
        "set_project" => {
            let req: SetProjectRequest = serde_json::from_str(&args)?;
            mira::tools::set_project(&server, req.project_path, req.name).await
        }
        "get_project" => {
             mira::tools::get_project(&server).await
        }
        "remember" => {
             let req: RememberRequest = serde_json::from_str(&args)?;
             mira::tools::remember(&server, req.content, req.key, req.fact_type, req.category, req.confidence, req.scope).await
        }
        "recall" => {
            let req: RecallRequest = serde_json::from_str(&args)?;
            mira::tools::recall(&server, req.query, req.limit, req.category, req.fact_type).await
        }
        "forget" => {
            let req: ForgetRequest = serde_json::from_str(&args)?;
            mira::tools::forget(&server, req.id).await
        }
        "get_symbols" => {
            let req: GetSymbolsRequest = serde_json::from_str(&args)?;
            mira::tools::get_symbols(req.file_path, req.symbol_type)
        }
        "search_code" => {
            let req: SemanticCodeSearchRequest = serde_json::from_str(&args)?;
            mira::tools::search_code(&server, req.query, req.language, req.limit).await
        }
        "find_callers" => {
            let req: FindCallersRequest = serde_json::from_str(&args)?;
            mira::tools::find_function_callers(&server, req.function_name, req.limit).await
        }
        "find_callees" => {
            let req: FindCalleesRequest = serde_json::from_str(&args)?;
            mira::tools::find_function_callees(&server, req.function_name, req.limit).await
        }
        "check_capability" => {
            let req: CheckCapabilityRequest = serde_json::from_str(&args)?;
            mira::tools::check_capability(&server, req.description).await
        }
        "task" => {
             let req: TaskRequest = serde_json::from_str(&args)?;
             mira::tools::task(&server, req.action, req.task_id, req.title, req.description, req.status, req.priority, req.include_completed, req.limit, req.tasks).await
        }
        "goal" => {
             let req: GoalRequest = serde_json::from_str(&args)?;
             mira::tools::goal(&server, req.action, req.goal_id, req.title, req.description, req.status, req.priority, req.progress_percent, req.include_finished, req.limit, req.goals).await
        }
        "index" => {
             let req: IndexRequest = serde_json::from_str(&args)?;
             mira::tools::index(&server, req.action, req.path, req.skip_embed.unwrap_or(false)).await
        }
        "summarize_codebase" => {
            mira::tools::summarize_codebase(&server).await
        }
        "get_session_recap" => {
            mira::tools::get_session_recap(&server).await
        }
        "session_history" => {
            let req: SessionHistoryRequest = serde_json::from_str(&args)?;
            mira::tools::session_history(&server, req.action, req.session_id, req.limit).await
        }
        "consult_architect" => {
            let req: ConsultArchitectRequest = serde_json::from_str(&args)?;
            mira::tools::consult_architect(&server, req.context, req.question).await
        }
        "consult_code_reviewer" => {
             let req: ConsultCodeReviewerRequest = serde_json::from_str(&args)?;
             mira::tools::consult_code_reviewer(&server, req.context, req.question).await
        }
        "consult_plan_reviewer" => {
             let req: ConsultPlanReviewerRequest = serde_json::from_str(&args)?;
             mira::tools::consult_plan_reviewer(&server, req.context, req.question).await
        }
        "consult_scope_analyst" => {
             let req: ConsultScopeAnalystRequest = serde_json::from_str(&args)?;
             mira::tools::consult_scope_analyst(&server, req.context, req.question).await
        }
        "consult_security" => {
             let req: ConsultSecurityRequest = serde_json::from_str(&args)?;
             mira::tools::consult_security(&server, req.context, req.question).await
        }
        "consult_experts" => {
             let req: ConsultExpertsRequest = serde_json::from_str(&args)?;
             mira::tools::consult_experts(&server, req.roles, req.context, req.question).await
        }
        "configure_expert" => {
             let req: ConfigureExpertRequest = serde_json::from_str(&args)?;
             mira::tools::configure_expert(&server, req.action, req.role, req.prompt, req.provider, req.model).await
        }
        "reply_to_mira" => {
             let req: ReplyToMiraRequest = serde_json::from_str(&args)?;
             // Just print locally since we don't have a collaborative frontend connected
             Ok(format!("(Reply not sent - no frontend connected) Content: {}", req.content))
        }
        _ => Err(format!("Unknown tool: {}", name).into()),
    };

    match res {
        Ok(output) => println!("{}", output),
        Err(e) => eprintln!("Error: {}", e),
    }
    Ok(())
}

async fn run_mcp_server() -> Result<()> {
    // Create shared HTTP client for all network operations
    let http_client = create_shared_client();

    // Open database (both legacy sync and new async pool)
    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);
    let pool = Arc::new(DatabasePool::open(&db_path).await?);

    // Initialize embeddings if API key available (with usage tracking)
    let embeddings = get_embeddings_with_db(Some(db.clone()), http_client.clone());

    if embeddings.is_some() {
        info!("Semantic search enabled (OpenAI embeddings)");
    } else {
        info!("Semantic search disabled (no OPENAI_API_KEY)");
    }

    // Initialize DeepSeek client if API key available
    let deepseek = get_deepseek(http_client.clone());

    if deepseek.is_some() {
        info!("DeepSeek enabled (for experts and module summaries)");
    } else {
        info!("DeepSeek disabled (no DEEPSEEK_API_KEY)");
    }

    // Spawn background worker for batch processing
    let bg_db = db.clone();
    let bg_embeddings = embeddings.clone();
    let bg_deepseek = deepseek.clone();
    let _shutdown_tx = background::spawn(bg_db, bg_embeddings, bg_deepseek);
    info!("Background worker started");

    // Spawn file watcher for incremental indexing
    let (_watcher_shutdown_tx, watcher_shutdown_rx) = watch::channel(false);
    let watcher_handle = background::watcher::spawn(db.clone(), watcher_shutdown_rx);
    info!("File watcher started");

    // Clone db for restoration before moving ownership to server
    let db_for_restore = db.clone();

    // Create MCP server with watcher
    let server = MiraServer::with_watcher(db, pool, embeddings, watcher_handle);

    // Restore context (Project & Session) - similar to run_tool()
    if let Ok(Some(path)) = db_for_restore.get_last_active_project() {
        if let Ok((id, name)) = db_for_restore.get_or_create_project(&path, None) {
            let project = ProjectContext {
                id,
                path: path.clone(),
                name,
            };
            info!("Restoring project: {} (id: {})", project.path, project.id);
            server.set_project(project).await;

            // Register with watcher if available
            if let Some(watcher) = server.watcher() {
                watcher.watch(id, std::path::PathBuf::from(path)).await;
            }
        }
    } else {
        // Fallback: Check if CWD is a project
        if let Ok(cwd) = std::env::current_dir() {
            let path_str = cwd.to_string_lossy().to_string();
            // Simple heuristic: if we can get a name, it's likely a project
            if let Ok((id, name)) = db_for_restore.get_or_create_project(&path_str, None) {
                let project = ProjectContext {
                    id,
                    path: path_str,
                    name,
                };
                info!("Restoring project from CWD: {} (id: {})", project.path, project.id);
                server.set_project(project).await;

                if let Some(watcher) = server.watcher() {
                    watcher.watch(id, cwd).await;
                }
            }
        }
    }

    if let Ok(Some(sid)) = db_for_restore.get_server_state("active_session_id") {
        info!("Restoring session: {}", sid);
        server.set_session_id(sid).await;
    }

    // Run with stdio transport
    let transport = rmcp::transport::io::stdio();
    let service = rmcp::serve_server(server, transport).await?;
    service.waiting().await?;

    Ok(())
}

async fn run_index(path: Option<PathBuf>, no_embed: bool) -> Result<()> {
    let path = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    info!("Indexing project at {}", path.display());

    // Create shared HTTP client
    let http_client = create_shared_client();

    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);

    let embeddings = if no_embed { None } else { get_embeddings_with_db(Some(db.clone()), http_client) };

    // Get or create project
    let (project_id, _project_name) = db.get_or_create_project(
        path.to_string_lossy().as_ref(),
        path.file_name().and_then(|n| n.to_str()),
    )?;

    // Set project ID for usage tracking
    if let Some(ref emb) = embeddings {
        emb.set_project_id(Some(project_id)).await;
    }

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
        if let Err(e) = dotenvy::from_path(home.join(".mira/.env")) {
            tracing::debug!("Failed to load global .env file: {}", e);
        }
    }
    if let Err(e) = dotenvy::dotenv() {
        tracing::debug!("Failed to load local .env file: {}", e);
    } // Load .env from current directory

    let cli = Cli::parse();

    // Set up logging based on command
    let log_level = match &cli.command {
        Some(Commands::Serve) | None => Level::WARN, // Quiet for MCP stdio
        Some(Commands::Tool { .. }) => Level::WARN,
        Some(Commands::Hook { .. }) => Level::WARN,
        Some(Commands::Index { .. }) => Level::INFO,
        Some(Commands::DebugCarto { .. }) => Level::DEBUG,
        Some(Commands::DebugSession { .. }) => Level::DEBUG,
        Some(Commands::Proxy { .. }) => Level::INFO,
        Some(Commands::Backend { .. }) => Level::INFO,
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
        Some(Commands::Tool { name, args }) => {
            run_tool(name, args).await?;
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
            HookAction::PreCompact => {
                mira::hooks::precompact::run().await?;
            }
            HookAction::UserPrompt => {
                mira::hooks::user_prompt::run().await?;
            }
            HookAction::Posttool | HookAction::Pretool => {
                // Legacy no-op hooks for compatibility
            }
        },
        Some(Commands::DebugCarto { path }) => {
            run_debug_carto(path)?;
        }
        Some(Commands::DebugSession { path }) => {
            run_debug_session(path).await?;
        }
        Some(Commands::Proxy { action }) => match action {
            ProxyAction::Start { config, host, port, daemon } => {
                run_proxy_start(config, host, port, daemon).await?;
            }
            ProxyAction::Stop => {
                run_proxy_stop()?;
            }
            ProxyAction::Status => {
                run_proxy_status()?;
            }
        }
        Some(Commands::Backend { action }) => match action {
            BackendAction::List => {
                run_backend_list()?;
            }
            BackendAction::Use { name } => {
                run_backend_use(&name).await?;
            }
            BackendAction::Test { name } => {
                run_backend_test(&name).await?;
            }
            BackendAction::Env { name } => {
                run_backend_env(name.as_deref())?;
            }
            BackendAction::Usage { backend, days } => {
                run_backend_usage(backend.as_deref(), days).await?;
            }
        }
    }

    Ok(())
}

/// Debug session_start output
async fn run_debug_session(path: Option<PathBuf>) -> Result<()> {
    let project_path = match path {
        Some(p) => p,
        None => std::env::current_dir()?,
    };
    println!("=== Debug Session Start ===\n");
    println!("Project: {:?}\n", project_path);

    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);
    let pool = Arc::new(DatabasePool::open(&db_path).await?);

    // Create a minimal MCP server context
    let server = mira::mcp::MiraServer::new(db.clone(), pool, None);

    // Call session_start
    let result = mira::tools::session_start(
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
    let project_path = match path {
        Some(p) => p,
        None => std::env::current_dir()?,
    };
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
        project_path.to_string_lossy().as_ref(),
        None,
    )?;
    println!("Project ID: {}, Name: {:?}", project_id, name);

    match mira::cartographer::get_or_generate_map(
        &db,
        project_id,
        project_path.to_string_lossy().as_ref(),
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

/// Get the PID file path
fn get_proxy_pid_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/proxy.pid")
}

/// Start the LLM proxy server
async fn run_proxy_start(
    config_path: Option<PathBuf>,
    host_override: Option<String>,
    port_override: Option<u16>,
    daemon: bool,
) -> Result<()> {
    use mira::proxy::{ProxyConfig, ProxyServer};

    // Handle daemon mode first (before consuming config_path/host_override)
    if daemon {
        use std::process::Command;

        // Re-exec ourselves without --daemon flag
        let exe = std::env::current_exe()?;
        let mut args = vec!["proxy".to_string(), "start".to_string()];

        if let Some(ref path) = config_path {
            args.push("-c".to_string());
            args.push(path.to_string_lossy().to_string());
        }
        if let Some(ref host) = host_override {
            args.push("--host".to_string());
            args.push(host.clone());
        }
        if let Some(port) = port_override {
            args.push("-p".to_string());
            args.push(port.to_string());
        }

        let child = Command::new(&exe)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        let pid = child.id();

        // Write PID file
        let pid_path = get_proxy_pid_path();
        if let Some(parent) = pid_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&pid_path, pid.to_string())?;

        // Load config just to show host/port
        let config = match config_path {
            Some(path) => ProxyConfig::load_from(&path)?,
            None => ProxyConfig::load()?,
        };
        let host = host_override.as_deref().unwrap_or(&config.host);
        let port = port_override.unwrap_or(config.port);

        println!("Mira proxy started in background (PID: {})", pid);
        println!("Listening on {}:{}", host, port);
        println!("Stop with: mira proxy stop");

        return Ok(());
    }

    // Foreground mode - load config and run
    let mut config = match config_path {
        Some(path) => ProxyConfig::load_from(&path)?,
        None => ProxyConfig::load()?,
    };

    // Apply CLI overrides
    if let Some(host) = host_override {
        config.host = host;
    }
    if let Some(port) = port_override {
        config.port = port;
    }

    // Check if we have any backends configured
    if config.backends.is_empty() {
        eprintln!("No backends configured. Create a config file at:");
        eprintln!("  {:?}", ProxyConfig::default_config_path()?);
        eprintln!("\nExample config:\n");
        eprintln!(r#"port = 8100
default_backend = "anthropic"

[backends.anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com"
api_key_env = "ANTHROPIC_API_KEY"
"#);
        return Ok(());
    }

    let usable = config.usable_backends();
    if usable.is_empty() {
        eprintln!("No usable backends (check API keys are set):");
        for (name, backend) in &config.backends {
            eprintln!("  {} - enabled: {}, has_key: {}",
                name,
                backend.enabled,
                backend.get_api_key().is_some()
            );
        }
        return Ok(());
    }

    // Foreground mode - write PID file for status checks
    let pid_path = get_proxy_pid_path();
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pid_path, std::process::id().to_string())?;

    info!("Starting Mira proxy on {}:{}", config.host, config.port);
    info!("Available backends: {:?}", usable.iter().map(|(n, _)| n).collect::<Vec<_>>());

    // Open database for usage tracking
    let db_path = get_db_path();
    let db = match Database::open(&db_path) {
        Ok(db) => {
            info!("Usage tracking enabled (database: {:?})", db_path);
            Some(Arc::new(db))
        }
        Err(e) => {
            tracing::warn!("Failed to open database for usage tracking: {}", e);
            None
        }
    };

    let server = ProxyServer::with_db(config, db);

    // Clean up PID file on exit
    let pid_path_clone = pid_path.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        let _ = std::fs::remove_file(&pid_path_clone);
        std::process::exit(0);
    });

    server.run().await?;

    // Clean up PID file
    let _ = std::fs::remove_file(&pid_path);

    Ok(())
}

/// Stop the running proxy server
fn run_proxy_stop() -> Result<()> {
    let pid_path = get_proxy_pid_path();

    if !pid_path.exists() {
        println!("No proxy PID file found. Is the proxy running?");
        return Ok(());
    }

    let pid_str = std::fs::read_to_string(&pid_path)?;
    let pid: i32 = pid_str.trim().parse()?;

    // Check if process exists
    unsafe {
        if libc::kill(pid, 0) != 0 {
            println!("Proxy process {} not found (stale PID file)", pid);
            std::fs::remove_file(&pid_path)?;
            return Ok(());
        }

        // Send SIGTERM
        if libc::kill(pid, libc::SIGTERM) == 0 {
            println!("Sent SIGTERM to proxy (PID: {})", pid);
            std::fs::remove_file(&pid_path)?;
        } else {
            eprintln!("Failed to stop proxy (PID: {})", pid);
        }
    }

    Ok(())
}

/// Check proxy server status
fn run_proxy_status() -> Result<()> {
    let pid_path = get_proxy_pid_path();

    if !pid_path.exists() {
        println!("Proxy is not running (no PID file)");
        return Ok(());
    }

    let pid_str = std::fs::read_to_string(&pid_path)?;
    let pid: i32 = pid_str.trim().parse()?;

    // Check if process exists
    unsafe {
        if libc::kill(pid, 0) == 0 {
            println!("Proxy is running (PID: {})", pid);
        } else {
            println!("Proxy is not running (stale PID file for PID: {})", pid);
            std::fs::remove_file(&pid_path)?;
        }
    }

    Ok(())
}

/// List configured backends
fn run_backend_list() -> Result<()> {
    use mira::proxy::ProxyConfig;

    let config = ProxyConfig::load()?;

    if config.backends.is_empty() {
        println!("No backends configured.");
        println!("\nCreate a config file at: {:?}", ProxyConfig::default_config_path()?);
        return Ok(());
    }

    println!("Configured backends:\n");

    let default = config.default_backend.as_deref();

    for (name, backend) in &config.backends {
        let has_key = backend.get_api_key().is_some();
        let status = if !backend.enabled {
            "disabled"
        } else if !has_key {
            "no API key"
        } else {
            "ready"
        };

        let is_default = default == Some(name.as_str());
        let marker = if is_default { " (default)" } else { "" };

        println!(
            "  {} [{}]{}",
            name,
            status,
            marker
        );
        println!("    URL: {}", backend.base_url);
        if let Some(env_var) = &backend.api_key_env {
            println!("    Key: ${}", env_var);
        }
        if !backend.env.is_empty() {
            println!("    Model: {}", backend.env.get("ANTHROPIC_MODEL").unwrap_or(&"-".to_string()));
        }
        println!();
    }

    Ok(())
}

/// Set the default backend
async fn run_backend_use(name: &str) -> Result<()> {
    use mira::proxy::ProxyConfig;

    let mut config = ProxyConfig::load()?;

    // Verify backend exists
    if !config.backends.contains_key(name) {
        eprintln!("Backend '{}' not found.", name);
        eprintln!("\nAvailable backends:");
        for backend_name in config.backends.keys() {
            eprintln!("  {}", backend_name);
        }
        return Ok(());
    }

    // Check if it's usable
    let backend = config.backends.get(name).unwrap();
    if !backend.enabled {
        eprintln!("Backend '{}' is disabled in config.", name);
        return Ok(());
    }
    if backend.get_api_key().is_none() {
        eprintln!("Warning: Backend '{}' has no API key configured.", name);
    }

    // Update config
    config.default_backend = Some(name.to_string());

    // Write back to config file
    let config_path = ProxyConfig::default_config_path()?;
    let toml_str = toml::to_string_pretty(&config)?;
    std::fs::write(&config_path, &toml_str)?;

    println!("Default backend set to '{}'", name);
    println!("Config updated: {:?}", config_path);

    // If proxy is running, notify user to restart
    let pid_path = get_proxy_pid_path();
    if pid_path.exists() {
        println!("\nNote: Restart the proxy for changes to take effect:");
        println!("  mira proxy stop && mira proxy start -d");
    }

    Ok(())
}

/// Test connectivity to a backend
async fn run_backend_test(name: &str) -> Result<()> {
    use mira::proxy::ProxyConfig;

    let config = ProxyConfig::load()?;

    // Get backend config
    let backend = match config.backends.get(name) {
        Some(b) => b,
        None => {
            eprintln!("Backend '{}' not found.", name);
            eprintln!("\nAvailable backends:");
            for backend_name in config.backends.keys() {
                eprintln!("  {}", backend_name);
            }
            return Ok(());
        }
    };

    // Check prerequisites
    if !backend.enabled {
        eprintln!("Backend '{}' is disabled.", name);
        return Ok(());
    }

    let api_key = match backend.get_api_key() {
        Some(k) => k,
        None => {
            eprintln!("Backend '{}' has no API key configured.", name);
            if let Some(env_var) = &backend.api_key_env {
                eprintln!("Set the {} environment variable.", env_var);
            }
            return Ok(());
        }
    };

    println!("Testing backend '{}'...", name);
    println!("  URL: {}", backend.base_url);

    // Send a minimal test request
    let client = create_shared_client();
    let test_url = format!("{}/v1/messages", backend.base_url);

    let test_body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1,
        "messages": [{"role": "user", "content": "Hi"}]
    });

    let start = std::time::Instant::now();
    let response = client
        .post(&test_url)
        .header("content-type", "application/json")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&test_body)
        .send()
        .await;

    let elapsed = start.elapsed();

    match response {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                println!("\n✓ Connection successful!");
                println!("  Status: {}", status);
                println!("  Latency: {:?}", elapsed);
            } else {
                let body = resp.text().await.unwrap_or_default();
                eprintln!("\n✗ Request failed");
                eprintln!("  Status: {}", status);
                if !body.is_empty() {
                    // Try to extract error message
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                        if let Some(msg) = json.get("error").and_then(|e| e.get("message")) {
                            eprintln!("  Error: {}", msg);
                        } else {
                            eprintln!("  Body: {}", body);
                        }
                    } else {
                        eprintln!("  Body: {}", body);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("\n✗ Connection failed");
            eprintln!("  Error: {}", e);
        }
    }

    Ok(())
}

/// Print environment variables for a backend in shell export format
fn run_backend_env(name: Option<&str>) -> Result<()> {
    use mira::proxy::ProxyConfig;

    let config = ProxyConfig::load()?;

    // Get backend name (use default if not specified)
    let backend_name = match name {
        Some(n) => n.to_string(),
        None => match &config.default_backend {
            Some(d) => d.clone(),
            None => {
                eprintln!("No backend specified and no default set.");
                eprintln!("Usage: mira backend env <name>");
                return Ok(());
            }
        }
    };

    // Get backend config
    let backend = match config.backends.get(&backend_name) {
        Some(b) => b,
        None => {
            eprintln!("Backend '{}' not found.", backend_name);
            eprintln!("\nAvailable backends:");
            for name in config.backends.keys() {
                eprintln!("  {}", name);
            }
            return Ok(());
        }
    };

    // Print base URL and auth token
    println!("export ANTHROPIC_BASE_URL=\"{}\"", backend.base_url);

    // Print API key (from env var or inline)
    if let Some(env_var) = &backend.api_key_env {
        // Reference the env var
        println!("export ANTHROPIC_AUTH_TOKEN=\"${}\"", env_var);
    } else if let Some(key) = &backend.api_key {
        println!("export ANTHROPIC_AUTH_TOKEN=\"{}\"", key);
    }

    // Print all env overrides
    for (key, value) in &backend.env {
        println!("export {}=\"{}\"", key, value);
    }

    eprintln!("\n# Usage: eval \"$(mira backend env {})\"", backend_name);

    Ok(())
}

/// Show usage statistics from the database
async fn run_backend_usage(backend: Option<&str>, days: u32) -> Result<()> {
    use mira::db::Database;

    let db_path = get_db_path();
    let db = Database::open(&db_path)?;

    // Calculate date range
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
    let cutoff_str = cutoff.format("%Y-%m-%d").to_string();

    // Query usage from database
    let conn = db.conn();
    let sql = if let Some(backend_name) = backend {
        format!(
            "SELECT backend_name, model,
                    SUM(input_tokens) as total_input,
                    SUM(output_tokens) as total_output,
                    SUM(cache_creation_tokens) as total_cache_create,
                    SUM(cache_read_tokens) as total_cache_read,
                    SUM(cost_estimate) as total_cost,
                    COUNT(*) as request_count
             FROM proxy_usage
             WHERE backend_name = '{}' AND created_at >= '{}'
             GROUP BY backend_name, model
             ORDER BY total_cost DESC",
            backend_name, cutoff_str
        )
    } else {
        format!(
            "SELECT backend_name, model,
                    SUM(input_tokens) as total_input,
                    SUM(output_tokens) as total_output,
                    SUM(cache_creation_tokens) as total_cache_create,
                    SUM(cache_read_tokens) as total_cache_read,
                    SUM(cost_estimate) as total_cost,
                    COUNT(*) as request_count
             FROM proxy_usage
             WHERE created_at >= '{}'
             GROUP BY backend_name, model
             ORDER BY total_cost DESC",
            cutoff_str
        )
    };

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => {
            println!("No usage data available yet.");
            println!("\nUsage tracking starts when requests go through the proxy.");
            println!("Start the proxy with: mira proxy start -d");
            return Ok(());
        }
    };

    let rows: Vec<(String, Option<String>, i64, i64, i64, i64, f64, i64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get::<_, f64>(6).unwrap_or(0.0),
                row.get(7)?,
            ))
        })?
        .filter_map(Result::ok)
        .collect();

    if rows.is_empty() {
        println!("No usage data in the last {} days.", days);
        return Ok(());
    }

    println!("Usage Statistics (last {} days)\n", days);
    println!("{:<12} {:<25} {:>10} {:>10} {:>10} {:>8}",
        "Backend", "Model", "Input", "Output", "Requests", "Cost");
    println!("{}", "-".repeat(80));

    let mut total_cost = 0.0;
    let mut total_requests = 0i64;

    for (backend_name, model, input, output, _cache_create, _cache_read, cost, requests) in &rows {
        let model_str = model.as_deref().unwrap_or("-");
        let model_display = if model_str.len() > 24 {
            format!("{}...", &model_str[..21])
        } else {
            model_str.to_string()
        };

        println!("{:<12} {:<25} {:>10} {:>10} {:>10} ${:>7.4}",
            backend_name,
            model_display,
            format_tokens(*input),
            format_tokens(*output),
            requests,
            cost
        );

        total_cost += cost;
        total_requests += requests;
    }

    println!("{}", "-".repeat(80));
    println!("{:<12} {:<25} {:>10} {:>10} {:>10} ${:>7.4}",
        "TOTAL", "", "", "", total_requests, total_cost);

    // Also show embedding usage
    drop(stmt);
    let embed_sql = format!(
        "SELECT provider, model,
                SUM(tokens) as total_tokens,
                SUM(text_count) as total_texts,
                SUM(cost_estimate) as total_cost,
                COUNT(*) as request_count
         FROM embeddings_usage
         WHERE created_at >= '{}'
         GROUP BY provider, model
         ORDER BY total_cost DESC",
        cutoff_str
    );

    if let Ok(mut embed_stmt) = conn.prepare(&embed_sql) {
        let embed_rows: Vec<(String, String, i64, i64, f64, i64)> = embed_stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get::<_, f64>(4).unwrap_or(0.0),
                    row.get(5)?,
                ))
            })?
            .filter_map(Result::ok)
            .collect();

        if !embed_rows.is_empty() {
            println!("\n\nEmbedding Usage\n");
            println!("{:<12} {:<25} {:>12} {:>10} {:>10} {:>8}",
                "Provider", "Model", "Tokens", "Texts", "Requests", "Cost");
            println!("{}", "-".repeat(80));

            let mut embed_total_cost = 0.0;
            let mut embed_total_requests = 0i64;

            for (provider, model, tokens, texts, cost, requests) in &embed_rows {
                let model_display = if model.len() > 24 {
                    format!("{}...", &model[..21])
                } else {
                    model.clone()
                };

                println!("{:<12} {:<25} {:>12} {:>10} {:>10} ${:>7.4}",
                    provider,
                    model_display,
                    format_tokens(*tokens),
                    texts,
                    requests,
                    cost
                );

                embed_total_cost += cost;
                embed_total_requests += requests;
            }

            println!("{}", "-".repeat(80));
            println!("{:<12} {:<25} {:>12} {:>10} {:>10} ${:>7.4}",
                "TOTAL", "", "", "", embed_total_requests, embed_total_cost);
        }
    }

    Ok(())
}

/// Format token count with K/M suffix
fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}
