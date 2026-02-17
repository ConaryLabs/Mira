// crates/mira-server/src/ipc/mod.rs
// Unix socket IPC for hook-to-server communication

pub mod client;
#[cfg(unix)]
pub mod handler;
pub mod ops;
pub mod protocol;

#[cfg(all(test, unix))]
mod tests;

use std::path::PathBuf;

/// Returns the path to the Mira IPC socket (~/.mira/mira.sock).
pub fn socket_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| {
            tracing::warn!(
                "HOME directory not set — using /tmp for Mira IPC socket. Consider setting $HOME."
            );
            PathBuf::from("/tmp")
        })
        .join(".mira")
        .join("mira.sock")
}

/// Start the Unix socket listener, accepting one-shot IPC connections from hooks.
#[cfg(unix)]
pub async fn run_socket_listener(server: crate::mcp::MiraServer) -> anyhow::Result<()> {
    use std::sync::Arc;
    use tracing::info;

    let path = socket_path();

    // Remove stale socket from previous run
    if path.exists() {
        std::fs::remove_file(&path)?;
    }

    let listener = tokio::net::UnixListener::bind(&path)?;

    // Restrict socket to owner only
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }

    info!("IPC listener started on {}", path.display());

    let semaphore = Arc::new(tokio::sync::Semaphore::new(16));

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                // Wait up to 2s for a slot instead of instant reject —
                // prevents silent hook drops under burst load
                let permit = match tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    semaphore.clone().acquire_owned(),
                )
                .await
                {
                    Ok(Ok(p)) => p,
                    Ok(Err(_)) => unreachable!("semaphore closed"),
                    Err(_) => {
                        tracing::warn!("IPC: connection limit reached after 2s, rejecting");
                        // Write error before closing so client gets a response, not EOF
                        let _ = stream.try_write(
                            b"{\"id\":\"\",\"ok\":false,\"error\":\"server overloaded\"}\n",
                        );
                        drop(stream);
                        continue;
                    }
                };
                let srv = server.clone();
                tokio::spawn(async move {
                    handler::handle_connection(stream, srv).await;
                    drop(permit);
                });
            }
            Err(e) => {
                tracing::warn!("IPC accept error: {}", e);
            }
        }
    }
}
