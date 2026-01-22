use anyhow::Result;
use clap::{Parser, Subcommand};
use mira::background;
use mira::db::pool::DatabasePool;
use mira::db::Database;
use mira::embeddings::Embeddings;
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

/// Get embeddings client if API key is available (filters empty keys)
fn get_embeddings() -> Option<Arc<Embeddings>> {
    std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .map(|key| Arc::new(Embeddings::new(key)))
}

/// Get DeepSeek client if API key is available
fn get_deepseek() -> Option<Arc<DeepSeekClient>> {
    std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .map(|key| Arc::new(DeepSeekClient::new(key)))
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
}

#[derive(Subcommand)]
enum HookAction {
    /// Handle PermissionRequest hooks
    Permission,
    /// Handle SessionStart hooks - captures Claude's session_id
    SessionStart,
    /// Handle PreCompact hooks - preserve context before summarization
    PreCompact,
    /// Legacy PostToolUse hook (no-op for compatibility)
    Posttool,
    /// Legacy PreToolUse hook (no-op for compatibility)
    Pretool,
}

fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}

/// Setup server context with database, embeddings, and restored project/session state
async fn setup_server_context() -> Result<MiraServer> {
    // Open database (both legacy sync and new async pool)
    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);
    let pool = Arc::new(DatabasePool::open(&db_path).await?);
    let embeddings = get_embeddings();

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
    // Open database (both legacy sync and new async pool)
    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);
    let pool = Arc::new(DatabasePool::open(&db_path).await?);

    // Initialize embeddings if API key available
    let embeddings = get_embeddings();

    if embeddings.is_some() {
        info!("Semantic search enabled (OpenAI embeddings)");
    } else {
        info!("Semantic search disabled (no OPENAI_API_KEY)");
    }

    // Initialize DeepSeek client if API key available
    let deepseek = get_deepseek();

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
