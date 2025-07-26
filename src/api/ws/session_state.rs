// src/api/ws/session_state.rs

use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use chrono::{DateTime, Utc, Duration};
use crate::persona::PersonaOverlay;

/// Represents the state of a single WebSocket session
#[derive(Debug, Clone)]
pub struct WsSessionState {
    pub session_id: String,
    pub current_persona: PersonaOverlay,  // Still tracks internally
    pub current_mood: String,
    pub last_active: DateTime<Utc>,
    pub persona_activated_at: DateTime<Utc>,
    pub active_project_id: Option<String>,
}

impl WsSessionState {
    pub fn new(session_id: String) -> Self {
        let now = Utc::now();
        Self {
            session_id,
            current_persona: PersonaOverlay::Default,
            current_mood: "present".to_string(),
            last_active: now,
            persona_activated_at: now,
            active_project_id: None,
        }
    }

    // Internal persona management - no external notifications
    pub fn set_persona(&mut self, persona: PersonaOverlay, mood: String) {
        self.current_persona = persona;
        self.current_mood = mood;
        self.persona_activated_at = Utc::now();
        self.mark_active();
    }

    pub fn set_mood(&mut self, mood: String) {
        self.current_mood = mood;
        self.mark_active();
    }

    pub fn set_project(&mut self, project_id: Option<String>) {
        self.active_project_id = project_id;
        self.mark_active();
    }

    pub fn mark_active(&mut self) {
        self.last_active = Utc::now();
    }

    pub fn is_stale(&self, timeout_minutes: i64) -> bool {
        let elapsed = Utc::now() - self.last_active;
        elapsed > Duration::minutes(timeout_minutes)
    }

    /// Personas can decay internally based on time
    pub fn decay_persona_if_needed(&mut self, now: DateTime<Utc>, timeout_minutes: i64) {
        let time_since_activation = now - self.persona_activated_at;
        
        // If non-default persona has been active too long, decay back
        if self.current_persona != PersonaOverlay::Default &&
           time_since_activation > Duration::minutes(timeout_minutes) {
            self.current_persona = PersonaOverlay::Default;
            self.current_mood = "present".to_string();
            self.persona_activated_at = now;
        }
    }
}

/// Manages all active WebSocket sessions
pub struct WsSessionManager {
    sessions: Arc<RwLock<HashMap<String, WsSessionState>>>,
}

impl WsSessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_session(&self, session_id: String) -> WsSessionState {
        let state = WsSessionState::new(session_id.clone());
        self.sessions.write().await.insert(session_id, state.clone());
        state
    }

    pub async fn get_session(&self, session_id: &str) -> Option<WsSessionState> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Call this when a client reconnects
    pub async fn restore_or_decay_session(&self, session_id: &str, timeout_minutes: i64) -> Option<WsSessionState> {
        let mut sessions = self.sessions.write().await;
        if let Some(state) = sessions.get_mut(session_id) {
            let now = Utc::now();
            // Decay persona if needed (internally)
            state.decay_persona_if_needed(now, timeout_minutes);
            state.mark_active();
            Some(state.clone())
        } else {
            None
        }
    }

    pub async fn update_session<F>(&self, session_id: &str, updater: F) -> Option<WsSessionState>
    where
        F: FnOnce(&mut WsSessionState),
    {
        let mut sessions = self.sessions.write().await;
        if let Some(state) = sessions.get_mut(session_id) {
            updater(state);
            Some(state.clone())
        } else {
            None
        }
    }

    pub async fn remove_session(&self, session_id: &str) {
        self.sessions.write().await.remove(session_id);
    }

    pub async fn get_all_sessions(&self) -> Vec<WsSessionState> {
        self.sessions.read().await.values().cloned().collect()
    }
}
