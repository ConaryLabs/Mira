// crates/mira-server/src/mux/mod.rs
// Session agent (mux) for real-time hook data

mod local;
mod state;
mod upstream;

pub use state::SessionState;

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, watch};

/// Run the session agent mux process.
pub async fn run(session_id: String) -> anyhow::Result<()> {
    let state = Arc::new(RwLock::new(SessionState::default()));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // 1. Connect upstream and subscribe
    let (upstream_writer, pending_requests) =
        upstream::connect_and_subscribe(&session_id, state.clone(), shutdown_tx.clone()).await?;

    // 2. Bind local mux.sock
    let mux_sock = mux_socket_path(&session_id);

    let upstream = Arc::new(tokio::sync::Mutex::new(upstream_writer));

    // 3. Write PID file
    write_pid_file(&session_id)?;

    eprintln!("[mira/mux] Session agent started for {session_id}");

    // 4. Serve local connections (blocks until shutdown or inactivity)
    let result = local::serve(mux_sock.clone(), state, upstream, pending_requests, shutdown_tx, shutdown_rx).await;

    // 5. Cleanup
    let _ = std::fs::remove_file(&mux_sock);
    let _ = std::fs::remove_file(pid_file_path(&session_id));
    eprintln!("[mira/mux] Session agent stopped for {session_id}");

    result
}

/// Path to the mux socket for a given session.
pub fn mux_socket_path(session_id: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".mira")
        .join("sessions")
        .join(session_id)
        .join("mux.sock")
}

fn pid_file_path(session_id: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join(".mira")
        .join("sessions")
        .join(session_id)
        .join("mux.pid")
}

fn write_pid_file(session_id: &str) -> anyhow::Result<()> {
    let pid_path = pid_file_path(session_id);
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(pid_path, std::process::id().to_string())?;
    Ok(())
}
