// src/api/ws/chat/heartbeat.rs
// Heartbeat manager that automatically stops when the WS closes.
// Fixes: "WebSocket protocol error: Sending after closing is not allowed" by cancelling the task on close.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval};

/// Trait abstraction for sending status/heartbeat messages.
pub trait StatusSender: Send + Sync + 'static {
    fn send_status(&self, message: &str);
}

/// Simple wrapper so you can pass your existing connection sender.
impl<F> StatusSender for F
where
    F: Fn(&str) + Send + Sync + 'static,
{
    fn send_status(&self, message: &str) {
        (self)(message)
    }
}

pub struct HeartbeatManager<S: StatusSender> {
    sender: Arc<S>,
    stop_tx: watch::Sender<bool>,
    stop_rx: watch::Receiver<bool>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl<S: StatusSender> HeartbeatManager<S> {
    pub fn new(sender: Arc<S>) -> Self {
        let (stop_tx, stop_rx) = watch::channel(false);
        Self {
            sender,
            stop_tx,
            stop_rx,
            handle: Mutex::new(None),
        }
    }

    /// Starts a heartbeat loop that emits every `period`.
    /// Safe to call once; subsequent calls replace the previous task.
    pub fn start(&self, period: Duration) {
        // Stop any existing task
        self.stop();

        let mut rx = self.stop_rx.clone();
        let sender = self.sender.clone();
        let handle = tokio::spawn(async move {
            let mut ticker = interval(period);
            // If ticks are missed (e.g. GC/long ops), fire as soon as we wake
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        // Build and send heartbeat payload
                        let ts = chrono::Utc::now().timestamp();
                        let msg = format!("{{\"message\":\"ping\",\"timestamp\":{},\"type\":\"heartbeat\"}}", ts);
                        sender.send_status(&msg);
                    }
                    changed = rx.changed() => {
                        if changed.is_ok() && *rx.borrow() {
                            // Stop requested
                            break;
                        }
                    }
                }
            }
        });

        *self.handle.lock() = Some(handle);
    }

    /// Signals the heartbeat task to stop and waits for it to finish.
    pub fn stop(&self) {
        // Signal stop
        let _ = self.stop_tx.send(true);
        // Take the handle and await it in background (non-blocking here)
        if let Some(handle) = self.handle.lock().take() {
            tokio::spawn(async move {
                let _ = handle.await;
            });
        }
    }
}

impl<S: StatusSender> Drop for HeartbeatManager<S> {
    fn drop(&mut self) {
        // Best-effort stop if not already
        let _ = self.stop_tx.send(true);
    }
}
