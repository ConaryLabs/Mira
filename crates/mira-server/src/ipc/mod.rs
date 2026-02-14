// crates/mira-server/src/ipc/mod.rs
// Unix socket IPC for hook-to-server communication

pub mod client;
pub mod handler;
pub mod ops;
pub mod protocol;

use crate::mcp::MiraServer;
use std::path::PathBuf;
use tracing::info;

/// Returns the path to the Mira IPC socket (~/.mira/mira.sock).
pub fn socket_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".mira")
        .join("mira.sock")
}

/// Start the Unix socket listener, accepting one-shot IPC connections from hooks.
pub async fn run_socket_listener(server: MiraServer) -> anyhow::Result<()> {
    let path = socket_path();

    // Remove stale socket from previous run
    if path.exists() {
        std::fs::remove_file(&path)?;
    }

    let listener = tokio::net::UnixListener::bind(&path)?;

    // Restrict socket to owner only
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }

    info!("IPC listener started on {}", path.display());

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let srv = server.clone();
                tokio::spawn(async move {
                    handler::handle_connection(stream, srv).await;
                });
            }
            Err(e) => {
                tracing::warn!("IPC accept error: {}", e);
            }
        }
    }
}
