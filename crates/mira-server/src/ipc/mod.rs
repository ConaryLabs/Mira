// crates/mira-server/src/ipc/mod.rs
// Cross-platform IPC for hook-to-server communication

pub mod client;
pub mod handler;
pub mod ops;
pub mod protocol;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

/// Returns the path to the Mira IPC socket (~/.mira/mira.sock).
///
/// Fallback when HOME is unset: prefers `$XDG_RUNTIME_DIR` (per-user, 0700,
/// enforced by systemd) over `/tmp`. If `/tmp` is used, the path includes the
/// UID to prevent socket impersonation on shared systems.
#[cfg(unix)]
pub fn socket_path() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        return home.join(".mira").join("mira.sock");
    }

    tracing::warn!("HOME directory not set — using fallback for Mira IPC socket");

    // Prefer XDG_RUNTIME_DIR (per-user, typically /run/user/$UID, 0700)
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("mira").join("mira.sock");
    }

    // Last resort: UID-scoped /tmp to prevent impersonation
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/mira-{uid}")).join("mira.sock")
}

/// Returns the Named Pipe name for Mira IPC on Windows.
///
/// Format: `\\.\pipe\mira-{username}`. Includes the username to prevent
/// collisions and impersonation on multi-user systems.
#[cfg(windows)]
pub fn pipe_name() -> String {
    let username = std::env::var("USERNAME").unwrap_or_else(|_| "default".to_string());
    format!(r"\\.\pipe\mira-{username}")
}

/// Start the IPC listener using Unix domain sockets.
#[cfg(unix)]
pub async fn run_ipc_listener(server: crate::mcp::MiraServer) -> anyhow::Result<()> {
    use std::sync::Arc;
    use tracing::info;

    let path = socket_path();

    // Ensure parent directory exists (needed for fallback paths like
    // $XDG_RUNTIME_DIR/mira/ or /tmp/mira-<uid>/ when HOME is unset)
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Remove stale socket from previous run
    if path.exists() {
        std::fs::remove_file(&path)?;
    }

    // Set restrictive umask before bind so the socket is created with owner-only
    // permissions. This closes the TOCTOU race between bind and set_permissions.
    let old_umask = unsafe { libc::umask(0o177) };
    let bind_result = tokio::net::UnixListener::bind(&path);
    unsafe { libc::umask(old_umask) };
    let listener = bind_result?;

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

/// Start the IPC listener using Windows Named Pipes.
///
/// Named Pipes require a different pattern than Unix sockets: instead of
/// accept(), you create a pipe instance, wait for a client, then create
/// a new instance before handling the connected one. This ensures a pipe
/// is always available for incoming connections.
#[cfg(windows)]
pub async fn run_ipc_listener(server: crate::mcp::MiraServer) -> anyhow::Result<()> {
    use std::sync::Arc;
    use tokio::net::windows::named_pipe::ServerOptions;
    use tracing::info;

    let name = pipe_name();

    // Create the first pipe instance (first_pipe_instance ensures no duplicates)
    let mut pipe = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&name)?;

    info!("IPC listener started on {}", name);

    let semaphore = Arc::new(tokio::sync::Semaphore::new(16));

    loop {
        // Wait for a client to connect to this pipe instance
        if let Err(e) = pipe.connect().await {
            tracing::warn!("IPC pipe connect error: {}", e);
            continue;
        }

        // Create the next pipe instance BEFORE moving the connected one.
        // This ensures a pipe is always available for the next client.
        let next_pipe = match ServerOptions::new().create(&name) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("IPC: failed to create next pipe instance: {}", e);
                // pipe still has a connected client but we can't replace it.
                // Serve this client inline, then retry creating a new pipe.
                let srv = server.clone();
                handler::handle_connection(pipe, srv).await;
                pipe = ServerOptions::new().create(&name)?;
                continue;
            }
        };

        let connected_pipe = std::mem::replace(&mut pipe, next_pipe);

        // Wait up to 2s for a semaphore slot
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
                // Drop the connected pipe without handling it
                drop(connected_pipe);
                continue;
            }
        };

        let srv = server.clone();
        tokio::spawn(async move {
            handler::handle_connection(connected_pipe, srv).await;
            drop(permit);
        });
    }
}
