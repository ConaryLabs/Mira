// crates/mira-server/src/mux/state.rs
// Cached session state maintained by the mux from server push events

use crate::ipc::protocol::{
    FileConflictSnapshot, GoalSnapshot, InjectionStatsSnapshot, IpcPushEvent,
    SessionStateSnapshot,
};

/// Cached session state maintained by the mux from server push events.
#[derive(Debug, Default, Clone)]
pub struct SessionState {
    pub sequence: u64,
    pub goals: Vec<GoalSnapshot>,
    pub injection_stats: InjectionStatsSnapshot,
    pub modified_files: Vec<String>,
    pub team_conflicts: Vec<FileConflictSnapshot>,
}

impl SessionState {
    /// Initialize from server snapshot.
    pub fn from_snapshot(snapshot: SessionStateSnapshot) -> Self {
        Self {
            sequence: snapshot.sequence,
            goals: snapshot.goals,
            injection_stats: snapshot.injection_stats,
            modified_files: snapshot.modified_files,
            team_conflicts: snapshot.team_conflicts,
        }
    }

    /// Apply an incremental push event.
    pub fn apply_event(&mut self, event: &IpcPushEvent) {
        self.sequence = event.sequence;
        match event.event_type.as_str() {
            "goal_updated" => {
                if let Ok(goal) = serde_json::from_value::<GoalSnapshot>(event.data.clone()) {
                    if let Some(existing) = self.goals.iter_mut().find(|g| g.id == goal.id) {
                        *existing = goal;
                    } else {
                        self.goals.push(goal);
                    }
                }
            }
            "file_modified" => {
                if let Some(path) = event.data["file_path"].as_str() {
                    let path = path.to_string();
                    if !self.modified_files.contains(&path) {
                        self.modified_files.push(path);
                    }
                }
            }
            "file_conflict" => {
                if let Ok(conflict) =
                    serde_json::from_value::<FileConflictSnapshot>(event.data.clone())
                {
                    self.team_conflicts.push(conflict);
                }
            }
            "injection_stats" => {
                if let Some(total) = event.data["total_injections"].as_u64() {
                    self.injection_stats.total_injections = total;
                }
                if let Some(chars) = event.data["total_chars"].as_u64() {
                    self.injection_stats.total_chars = chars;
                }
            }
            _ => {} // Unknown events ignored
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_apply_file_modified() {
        let mut state = SessionState::default();
        let event = IpcPushEvent {
            event_type: "file_modified".to_string(),
            sequence: 1,
            data: json!({ "file_path": "src/main.rs" }),
        };
        state.apply_event(&event);
        assert_eq!(state.modified_files, vec!["src/main.rs"]);
        assert_eq!(state.sequence, 1);

        // Duplicate should not be added
        state.apply_event(&event);
        assert_eq!(state.modified_files.len(), 1);
    }

    #[test]
    fn test_apply_goal_updated() {
        let mut state = SessionState::default();
        let event = IpcPushEvent {
            event_type: "goal_updated".to_string(),
            sequence: 1,
            data: json!({ "id": 1, "title": "Test", "status": "in_progress", "progress_percent": 50 }),
        };
        state.apply_event(&event);
        assert_eq!(state.goals.len(), 1);
        assert_eq!(state.goals[0].progress_percent, 50);

        // Update existing goal
        let update = IpcPushEvent {
            event_type: "goal_updated".to_string(),
            sequence: 2,
            data: json!({ "id": 1, "title": "Test", "status": "in_progress", "progress_percent": 80 }),
        };
        state.apply_event(&update);
        assert_eq!(state.goals.len(), 1);
        assert_eq!(state.goals[0].progress_percent, 80);
    }

    #[test]
    fn test_from_snapshot() {
        let snapshot = SessionStateSnapshot {
            sequence: 5,
            goals: vec![GoalSnapshot {
                id: 1,
                title: "Goal".to_string(),
                status: "active".to_string(),
                progress_percent: 25,
            }],
            injection_stats: InjectionStatsSnapshot {
                total_injections: 10,
                total_chars: 5000,
            },
            modified_files: vec!["a.rs".to_string()],
            team_conflicts: vec![],
        };
        let state = SessionState::from_snapshot(snapshot);
        assert_eq!(state.sequence, 5);
        assert_eq!(state.goals.len(), 1);
        assert_eq!(state.injection_stats.total_injections, 10);
    }
}
