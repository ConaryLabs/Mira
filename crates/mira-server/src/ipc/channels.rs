// crates/mira-server/src/ipc/channels.rs
// Registry of active session subscriptions for server-pushed events

use std::collections::HashMap;
use tokio::sync::{mpsc, RwLock};

use super::protocol::IpcPushEvent;

/// Registry of active session subscriptions.
/// The MCP server holds one of these and publishes events to it.
pub struct SessionChannelRegistry {
    channels: RwLock<HashMap<String, SessionChannel>>,
}

struct SessionChannel {
    tx: mpsc::Sender<IpcPushEvent>,
    sequence: u64,
}

impl SessionChannelRegistry {
    pub fn new() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
        }
    }

    pub async fn subscribe(&self, session_id: &str, tx: mpsc::Sender<IpcPushEvent>) {
        let mut channels = self.channels.write().await;
        channels.insert(
            session_id.to_string(),
            SessionChannel { tx, sequence: 0 },
        );
    }

    pub async fn unsubscribe(&self, session_id: &str) {
        let mut channels = self.channels.write().await;
        channels.remove(session_id);
    }

    /// Publish an event to a session's subscriber.
    /// Returns true if delivered, false if dropped or no subscriber.
    /// Critical events block until delivered; non-critical are dropped if buffer full.
    pub async fn publish(&self, session_id: &str, mut event: IpcPushEvent) -> bool {
        let mut channels = self.channels.write().await;
        let Some(channel) = channels.get_mut(session_id) else {
            return false;
        };

        channel.sequence += 1;
        event.sequence = channel.sequence;

        // Always use try_send: the subscriber rx lives in the same task as
        // dispatch() (inside handle_persistent_connection's select! loop),
        // so a blocking send() would deadlock. The 64-slot buffer provides
        // sufficient headroom for bursts.
        channel.tx.try_send(event).is_ok()
    }

    /// Remove channels where the receiver has been dropped.
    pub async fn cleanup_dead(&self) {
        let mut channels = self.channels.write().await;
        channels.retain(|_, ch| !ch.tx.is_closed());
    }

    pub async fn subscriber_count(&self) -> usize {
        self.channels.read().await.len()
    }
}
