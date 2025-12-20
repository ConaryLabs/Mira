// src/tools/format/sessions.rs
// Formatters for session operations (session_start, session_context, session_results)

use serde_json::Value;

/// Format session stored
pub fn session_stored(session_id: &str) -> String {
    format!("Session stored: {}", &session_id[..8])
}

/// Format session search results
pub fn session_results(results: &[Value]) -> String {
    if results.is_empty() {
        return "No sessions found.".to_string();
    }

    let mut out = format!("Found {} session{}:\n",
        results.len(),
        if results.len() == 1 { "" } else { "s" }
    );

    for r in results {
        let summary = r.get("summary").and_then(|v| v.as_str()).unwrap_or("");
        let created = r.get("created_at").and_then(|v| v.as_str()).unwrap_or("?");

        let display = if summary.len() > 80 {
            format!("{}...", &summary[..77])
        } else {
            summary.to_string()
        };

        out.push_str(&format!("  [{}] {}\n", created, display));
    }

    out.trim_end().to_string()
}

/// Format session_start result - concise startup summary
pub fn session_start(result: &crate::tools::sessions::SessionStartResult) -> String {
    let mut out = String::new();

    // Project header
    let type_suffix = result.project_type.as_ref()
        .map(|t| format!(" ({})", t))
        .unwrap_or_default();
    out.push_str(&format!("Project: {}{}\n", result.project_name, type_suffix));

    // Persona (if set)
    if let Some(ref persona) = result.persona_summary {
        out.push('\n');
        out.push_str(&format!("Persona: {}\n", persona));
    }

    // Corrections (important - mistakes to avoid)
    if !result.corrections.is_empty() {
        out.push('\n');
        out.push_str(&format!("{} correction{}:\n",
            result.corrections.len(),
            if result.corrections.len() == 1 { "" } else { "s" }
        ));
        for c in &result.corrections {
            let wrong = if c.what_was_wrong.len() > 30 {
                format!("{}...", &c.what_was_wrong[..27])
            } else {
                c.what_was_wrong.clone()
            };
            let right = if c.what_is_right.len() > 45 {
                format!("{}...", &c.what_is_right[..42])
            } else {
                c.what_is_right.clone()
            };
            out.push_str(&format!("  {} → {}\n", wrong, right));
        }
    }

    // Goals (active work)
    if !result.goals.is_empty() {
        out.push('\n');
        out.push_str(&format!("{} active goal{}:\n",
            result.goals.len(),
            if result.goals.len() == 1 { "" } else { "s" }
        ));
        for g in &result.goals {
            let icon = match g.status.as_str() {
                "completed" => "✓",
                "in_progress" => "→",
                "blocked" => "✗",
                _ => "○",
            };
            out.push_str(&format!("  {} {} ({}%)\n", icon, g.title, g.progress_percent));
        }
    }

    // Tasks (pending work)
    if !result.tasks.is_empty() {
        out.push('\n');
        out.push_str(&format!("{} pending task{}:\n",
            result.tasks.len(),
            if result.tasks.len() == 1 { "" } else { "s" }
        ));
        for t in &result.tasks {
            let icon = match t.status.as_str() {
                "in_progress" => "→",
                "blocked" => "✗",
                _ => "○",
            };
            out.push_str(&format!("  {} {}\n", icon, t.title));
        }
    }

    // Active plan from previous session (for seamless resume)
    if let Some(ref plan) = result.active_plan {
        out.push('\n');
        if plan.status == "planning" {
            out.push_str("! RESUME - Plan mode was in progress\n");
            out.push_str("Use EnterPlanMode to continue planning.\n");
        } else if plan.status == "ready" {
            out.push_str("# RESUME - Active plan from previous session:\n");
            if let Some(ref content) = plan.content {
                // Show first 500 chars of plan, or first 10 lines
                let preview: String = content
                    .lines()
                    .take(10)
                    .collect::<Vec<_>>()
                    .join("\n");
                let preview = if preview.len() > 500 {
                    format!("{}...", &preview[..497])
                } else if content.lines().count() > 10 {
                    format!("{}...", preview)
                } else {
                    preview
                };
                for line in preview.lines() {
                    out.push_str(&format!("  {}\n", line));
                }
            }
        }
    }

    // Active todos from previous session (for seamless resume)
    if let Some(ref todos) = result.active_todos {
        if !todos.is_empty() {
            out.push('\n');
            out.push_str("! RESUME - Active todos from previous session:\n");
            for t in todos {
                let icon = match t.status.as_str() {
                    "in_progress" => "→",
                    "completed" => "✓",
                    _ => "○",
                };
                out.push_str(&format!("  {} [{}] {}\n", icon, t.status, t.content));
            }
            out.push_str("\nUse TodoWrite to restore these or start fresh.\n");
        }
    }

    // Working documents from previous session
    if !result.working_docs.is_empty() {
        out.push('\n');
        out.push_str("# Working documents:\n");
        for doc in &result.working_docs {
            out.push_str(&format!("  {} - {}\n", doc.filename, doc.preview));
        }
    }

    // Recent session context
    if !result.recent_session_topics.is_empty() {
        out.push('\n');
        out.push_str("Recent:\n");
        for topic in &result.recent_session_topics {
            out.push_str(&format!("  {}\n", topic));
        }
    }

    // Index status
    if !result.index_fresh {
        out.push('\n');
        if result.stale_file_count > 0 {
            out.push_str(&format!("⚠ {} files have changed since last index. Run `index(action: \"project\")` to refresh.\n",
                result.stale_file_count));
        } else {
            out.push_str("⚠ No code index found. Run `index(action: \"project\")` to enable code intelligence.\n");
        }
    }

    // Footer with guidelines count
    out.push('\n');
    out.push_str(&format!("{} usage guidelines loaded. Ready.", result.usage_guidelines_loaded));

    out.trim_end().to_string()
}

/// Format session context summary - shows actual content, not just counts
pub fn session_context(ctx: &Value) -> String {
    let mut out = String::new();

    // Active goals with progress
    if let Some(goals) = ctx.get("active_goals").and_then(|v| v.as_array()) {
        if !goals.is_empty() {
            out.push_str("Goals:\n");
            for g in goals {
                let title = g.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                let progress = g.get("progress_percent").and_then(|v| v.as_i64()).unwrap_or(0);
                let status = g.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                let icon = match status {
                    "completed" => "✓",
                    "in_progress" => "→",
                    "blocked" => "✗",
                    _ => "○",
                };
                out.push_str(&format!("  {} {} ({}%)\n", icon, title, progress));
            }
        }
    }

    // Active corrections (important to follow)
    if let Some(corrections) = ctx.get("active_corrections").and_then(|v| v.as_array()) {
        if !corrections.is_empty() {
            if !out.is_empty() { out.push('\n'); }
            out.push_str("Corrections:\n");
            for c in corrections.iter().take(3) {
                let wrong = c.get("what_was_wrong").and_then(|v| v.as_str()).unwrap_or("?");
                let right = c.get("what_is_right").and_then(|v| v.as_str()).unwrap_or("?");
                let wrong_short = if wrong.len() > 30 { format!("{}...", &wrong[..27]) } else { wrong.to_string() };
                let right_short = if right.len() > 50 { format!("{}...", &right[..47]) } else { right.to_string() };
                out.push_str(&format!("  {} → {}\n", wrong_short, right_short));
            }
            if corrections.len() > 3 {
                out.push_str(&format!("  ...and {} more\n", corrections.len() - 3));
            }
        }
    }

    // Pending tasks
    if let Some(tasks) = ctx.get("pending_tasks").and_then(|v| v.as_array()) {
        if !tasks.is_empty() {
            if !out.is_empty() { out.push('\n'); }
            out.push_str("Pending tasks:\n");
            for t in tasks.iter().take(5) {
                let title = t.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                let status = t.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
                let icon = match status {
                    "in_progress" => "→",
                    "blocked" => "✗",
                    _ => "○",
                };
                out.push_str(&format!("  {} {}\n", icon, title));
            }
            if tasks.len() > 5 {
                out.push_str(&format!("  ...and {} more\n", tasks.len() - 5));
            }
        }
    }

    // Recent sessions (just summaries, truncated)
    if let Some(sessions) = ctx.get("recent_sessions").and_then(|v| v.as_array()) {
        if !sessions.is_empty() {
            if !out.is_empty() { out.push('\n'); }
            out.push_str("Recent sessions:\n");
            for s in sessions.iter().take(3) {
                let summary = s.get("summary").and_then(|v| v.as_str()).unwrap_or("?");
                // Get first line or truncate
                let first_line = summary.lines().next().unwrap_or(summary);
                let display = if first_line.len() > 70 {
                    format!("{}...", &first_line[..67])
                } else {
                    first_line.to_string()
                };
                out.push_str(&format!("  {}\n", display));
            }
        }
    }

    // Recent memories (just a count - these are context, not actionable)
    if let Some(memories) = ctx.get("recent_memories").and_then(|v| v.as_array()) {
        if !memories.is_empty() {
            if !out.is_empty() { out.push('\n'); }
            out.push_str(&format!("{} recent memories loaded\n", memories.len()));
        }
    }

    if out.is_empty() {
        "Fresh session - no prior context.".to_string()
    } else {
        out.trim_end().to_string()
    }
}
