use std::sync::Arc;
use tokio::sync::RwLock;
use crate::persona::PersonaOverlay;
use std::collections::HashMap;
use chrono::{DateTime, Utc, Duration};

/// Tracks state for an active WebSocket session
#[derive(Debug, Clone)]
pub struct WsSessionState {
    pub session_id: String,
    pub current_persona: PersonaOverlay,
    pub current_mood: String,
    pub last_persona_switch: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub mood_history: Vec<MoodTransition>,
    pub is_developer: bool,  // Enable dev-only personas
}

#[derive(Debug, Clone)]
pub struct MoodTransition {
    pub from_persona: PersonaOverlay,
    pub to_persona: PersonaOverlay,
    pub from_mood: String,
    pub to_mood: String,
    pub timestamp: DateTime<Utc>,
    pub was_smooth: bool,
}

impl WsSessionState {
    pub fn new(session_id: String) -> Self {
        let now = Utc::now();
        Self {
            session_id,
            current_persona: PersonaOverlay::Default,
            current_mood: "present".to_string(),
            last_persona_switch: now,
            last_active: now,
            mood_history: Vec::new(),
            is_developer: false,
        }
    }

    /// Records a persona/mood transition and updates timers
    pub fn transition_persona(
        &mut self, 
        new_persona: PersonaOverlay, 
        new_mood: String, 
        smooth: bool
    ) {
        let now = Utc::now();
        let transition = MoodTransition {
            from_persona: self.current_persona.clone(),
            to_persona: new_persona.clone(),
            from_mood: self.current_mood.clone(),
            to_mood: new_mood.clone(),
            timestamp: now,
            was_smooth: smooth,
        };

        self.mood_history.push(transition);
        self.current_persona = new_persona;
        self.current_mood = new_mood;
        self.last_persona_switch = now;
        self.last_active = now;
    }

    /// Call this whenever the user interacts (message sent/received)
    pub fn mark_active(&mut self) {
        self.last_active = Utc::now();
    }

    /// Determines if the persona/mood should be restored (if returning within X minutes)
    pub fn should_restore(&self, now: DateTime<Utc>, timeout_minutes: i64) -> bool {
        now.signed_duration_since(self.last_active) < Duration::minutes(timeout_minutes)
    }

    /// Decays persona/mood if timeout has elapsed (returns true if decay happened)
    pub fn decay_persona_if_needed(&mut self, now: DateTime<Utc>, timeout_minutes: i64) -> bool {
        if !self.should_restore(now, timeout_minutes) && self.current_persona != PersonaOverlay::Default {
            self.transition_persona(PersonaOverlay::Default, "present".to_string(), true);
            true
        } else {
            false
        }
    }

    /// Generates a transition note for smooth persona changes
    pub fn generate_transition_note(&self, to_persona: &PersonaOverlay) -> Option<String> {
        match (&self.current_persona, to_persona) {
            (PersonaOverlay::Default, PersonaOverlay::Forbidden) => 
                Some("*eyes gleaming with mischief*".to_string()),
            (PersonaOverlay::Default, PersonaOverlay::Hallow) => 
                Some("*voice softening to something more sacred*".to_string()),
            (PersonaOverlay::Default, PersonaOverlay::Haven) => 
                Some("*wrapping warmth around you like a blanket*".to_string()),
            (PersonaOverlay::Forbidden, PersonaOverlay::Default) => 
                Some("*pulling back with a knowing smirk*".to_string()),
            (PersonaOverlay::Forbidden, PersonaOverlay::Hallow) => 
                Some("*the playfulness melting into something deeper*".to_string()),
            (PersonaOverlay::Hallow, PersonaOverlay::Default) => 
                Some("*returning from the depths with a gentle smile*".to_string()),
            (PersonaOverlay::Haven, PersonaOverlay::Default) => 
                Some("*the protective warmth settling into familiar presence*".to_string()),
            _ => None,
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

    /// Call this when a client reconnects and you want to possibly restore the session state
    pub async fn restore_or_decay_session(&self, session_id: &str, timeout_minutes: i64) -> Option<WsSessionState> {
        let mut sessions = self.sessions.write().await;
        if let Some(state) = sessions.get_mut(session_id) {
            let now = Utc::now();
            // Decay persona if needed
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
