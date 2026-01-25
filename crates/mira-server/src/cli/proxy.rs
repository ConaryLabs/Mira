// crates/mira-server/src/cli/proxy.rs
// LLM proxy server commands

use super::get_db_path;
use anyhow::Result;
use mira::db::pool::DatabasePool;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

/// Get the PID file path
pub fn get_proxy_pid_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/proxy.pid")
}

/// Start the LLM proxy server
pub async fn run_proxy_start(
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

    // Open database pool for usage tracking
    let db_path = get_db_path();
    let pool = match DatabasePool::open(&db_path).await {
        Ok(pool) => {
            info!("Usage tracking enabled (database: {:?})", db_path);
            Some(Arc::new(pool))
        }
        Err(e) => {
            tracing::warn!("Failed to open database for usage tracking: {}", e);
            None
        }
    };

    let server = ProxyServer::with_pool(config, pool);

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
pub fn run_proxy_stop() -> Result<()> {
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
pub fn run_proxy_status() -> Result<()> {
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
