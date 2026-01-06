// src/mcp/tools/dev.rs
// Developer experience tools

use crate::mcp::MiraServer;
use chrono::{DateTime, Utc};

/// Get session recap formatted exactly as it appears in system prompts
pub async fn get_session_recap(server: &MiraServer) -> Result<String, String> {
    let mut recap_parts = Vec::new();

    // Get project info
    let project = server.project.read().await;
    let project_id = project.as_ref().map(|p| p.id);
    let project_name = project.as_ref().and_then(|p| p.name.clone());

    // Welcome header
    let welcome = if let Some(name) = project_name {
        format!("Welcome back to {} project!", name)
    } else {
        "Welcome back!".to_string()
    };
    recap_parts.push(format!("╔══════════════════════════════════════╗\n║   {}      ║\n╚══════════════════════════════════════╝", welcome));

    // Time since last chat
    match server.db.get_last_chat_time() {
        Ok(Some(last_chat_time)) => {
            if let Ok(parsed) = DateTime::parse_from_rfc3339(&last_chat_time) {
                let now = Utc::now();
                let duration = now.signed_duration_since(parsed);
                let hours = duration.num_hours();
                let minutes = duration.num_minutes() % 60;
                let time_ago = if hours > 0 {
                    format!("{} hours, {} minutes ago", hours, minutes)
                } else {
                    format!("{} minutes ago", minutes)
                };
                recap_parts.push(format!("Last chat: {}", time_ago));
            }
        }
        Ok(None) => {
            // No chat history yet
        }
        Err(e) => {
            // Log error but continue
            eprintln!("Error getting last chat time: {}", e);
        }
    }

    // Recent sessions (excluding current)
    if let Some(pid) = project_id {
        match server.db.get_recent_sessions(pid, 2) {
            Ok(sessions) => {
                let recent: Vec<_> = sessions.iter().filter(|s| s.status != "active").collect();
                if !recent.is_empty() {
                    let mut session_lines = Vec::new();
                    for sess in recent {
                        let short_id = &sess.id[..8];
                        let timestamp = &sess.last_activity[..16]; // YYYY-MM-DD HH:MM
                        if let Some(ref summary) = sess.summary {
                            session_lines.push(format!("• [{}] {} - {}", short_id, timestamp, summary));
                        } else {
                            session_lines.push(format!("• [{}] {}", short_id, timestamp));
                        }
                    }
                    recap_parts.push(format!("Recent sessions:\n{}", session_lines.join("\n")));
                }
            }
            Err(e) => {
                eprintln!("Error getting recent sessions: {}", e);
            }
        }
    }

    // Pending tasks
    match server.db.get_pending_tasks(project_id, 3) {
        Ok(tasks) => {
            if !tasks.is_empty() {
                let task_lines: Vec<String> = tasks.iter()
                    .map(|t| format!("• [ ] {} ({})", t.title, t.priority))
                    .collect();
                recap_parts.push(format!("Pending tasks:\n{}", task_lines.join("\n")));
            }
        }
        Err(e) => {
            eprintln!("Error getting pending tasks: {}", e);
        }
    }

    // Active goals
    match server.db.get_active_goals(project_id, 3) {
        Ok(goals) => {
            if !goals.is_empty() {
                let goal_lines: Vec<String> = goals.iter()
                    .map(|g| format!("• {} ({}%) - {}", g.title, g.progress_percent, g.status))
                    .collect();
                recap_parts.push(format!("Active goals:\n{}", goal_lines.join("\n")));
            }
        }
        Err(e) => {
            eprintln!("Error getting active goals: {}", e);
        }
    }

    // If we have any recap content, format it nicely
    if recap_parts.len() > 1 { // More than just welcome header
        Ok(recap_parts.join("\n\n"))
    } else {
        Ok(recap_parts.join("\n")) // Just welcome header
    }
}
