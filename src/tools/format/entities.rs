// src/tools/format/entities.rs
// Formatters for entity operations (tasks, goals, corrections, permissions)

use serde_json::Value;

/// Format task list
pub fn task_list(results: &[Value]) -> String {
    if results.is_empty() {
        return "No tasks.".to_string();
    }

    let mut out = String::new();
    for r in results {
        let status = r.get("status").and_then(|v| v.as_str()).unwrap_or("?");
        let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("?");
        let priority = r.get("priority").and_then(|v| v.as_str());

        let icon = match status {
            "completed" => "✓",
            "in_progress" => "→",
            "blocked" => "✗",
            _ => "○",
        };

        let pri = priority.map(|p| format!(" [{}]", p)).unwrap_or_default();
        out.push_str(&format!("{} {}{}\n", icon, title, pri));
    }

    out.trim_end().to_string()
}

/// Format task created/updated
pub fn task_action(action: &str, task_id: &str, title: &str) -> String {
    format!("Task {}: {} ({})", action, title, &task_id[..8])
}

/// Format goal list
pub fn goal_list(results: &[Value]) -> String {
    if results.is_empty() {
        return "No goals.".to_string();
    }

    let mut out = String::new();
    for r in results {
        let status = r.get("status").and_then(|v| v.as_str()).unwrap_or("?");
        let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("?");
        let progress = r.get("progress_percent").and_then(|v| v.as_i64()).unwrap_or(0);

        let icon = match status {
            "completed" => "✓",
            "in_progress" => "→",
            "blocked" => "✗",
            "abandoned" => "⊘",
            _ => "○",
        };

        let suffix = match status {
            "abandoned" => " (abandoned)".to_string(),
            "completed" | "in_progress" | "blocked" => format!(" ({}%)", progress),
            _ => format!(" ({})", status),
        };

        out.push_str(&format!("{} {}{}\n", icon, title, suffix));
    }

    out.trim_end().to_string()
}

/// Format goal action
pub fn goal_action(action: &str, goal_id: &str, title: &str) -> String {
    format!("Goal {}: {} ({})", action, title, &goal_id[..8])
}

/// Format milestone added
pub fn milestone_added(milestone_id: &str, title: &str) -> String {
    let id_short = if milestone_id.len() > 8 { &milestone_id[..8] } else { milestone_id };
    format!("Milestone added: {} ({})", title, id_short)
}

/// Format goal detail (from get action)
pub fn goal_detail(v: &Value) -> String {
    let obj = match v.as_object() {
        Some(o) => o,
        None => return "Invalid goal".to_string(),
    };

    let title = obj.get("title").and_then(|v| v.as_str()).unwrap_or("?");
    let status = obj.get("status").and_then(|v| v.as_str()).unwrap_or("?");
    let priority = obj.get("priority").and_then(|v| v.as_str()).unwrap_or("medium");
    let progress = obj.get("progress_percent").and_then(|v| v.as_i64()).unwrap_or(0);
    let completed = obj.get("milestones_completed").and_then(|v| v.as_i64()).unwrap_or(0);
    let total = obj.get("milestones_total").and_then(|v| v.as_i64()).unwrap_or(0);
    let description = obj.get("description").and_then(|v| v.as_str());

    let icon = match status {
        "completed" => "✓",
        "in_progress" => "→",
        "blocked" => "✗",
        "abandoned" => "⊘",
        _ => "○",
    };

    let mut out = format!("{} {} [{}] ({}%)\n", icon, title, priority, progress);

    if let Some(desc) = description {
        if !desc.is_empty() {
            out.push_str(&format!("  {}\n", desc));
        }
    }

    if total > 0 {
        out.push_str(&format!("  Milestones: {}/{}\n", completed, total));
    }

    out.trim_end().to_string()
}

/// Format correction recorded
pub fn correction_recorded(correction_type: &str, scope: &str) -> String {
    format!("Correction recorded ({}, {})", correction_type, scope)
}

/// Format correction list
pub fn correction_list(results: &[Value]) -> String {
    if results.is_empty() {
        return "No corrections.".to_string();
    }

    let mut out = format!("{} correction{}:\n",
        results.len(),
        if results.len() == 1 { "" } else { "s" }
    );

    for r in results {
        let ctype = r.get("correction_type").and_then(|v| v.as_str()).unwrap_or("?");
        let wrong = r.get("what_was_wrong").and_then(|v| v.as_str()).unwrap_or("?");
        let right = r.get("what_is_right").and_then(|v| v.as_str()).unwrap_or("?");

        let wrong_short = if wrong.len() > 40 { format!("{}...", &wrong[..37]) } else { wrong.to_string() };
        let right_short = if right.len() > 40 { format!("{}...", &right[..37]) } else { right.to_string() };

        out.push_str(&format!("  [{}] {} → {}\n", ctype, wrong_short, right_short));
    }

    out.trim_end().to_string()
}

/// Format permission save response
pub fn permission_saved(tool: &str, pattern: Option<&str>, match_type: &str, scope: &str) -> String {
    match pattern {
        Some(p) => format!("Permission saved: {} {} ({}, {})", tool, p, match_type, scope),
        None => format!("Permission saved: {} ({}, {})", tool, match_type, scope),
    }
}

/// Format permission list
pub fn permission_list(results: &[Value]) -> String {
    if results.is_empty() {
        return "No permission rules.".to_string();
    }

    let mut out = format!("{} permission rule{}:\n",
        results.len(),
        if results.len() == 1 { "" } else { "s" }
    );

    // Group by tool
    let mut by_tool: std::collections::BTreeMap<String, Vec<&Value>> = std::collections::BTreeMap::new();
    for r in results {
        let tool = r.get("tool_name").and_then(|v| v.as_str()).unwrap_or("?");
        by_tool.entry(tool.to_string()).or_default().push(r);
    }

    for (tool, rules) in by_tool {
        out.push_str(&format!("\n{}:\n", tool));
        for r in rules {
            let pattern = r.get("input_pattern").and_then(|v| v.as_str()).unwrap_or("*");
            let match_type = r.get("match_type").and_then(|v| v.as_str()).unwrap_or("?");
            let scope = r.get("scope").and_then(|v| v.as_str()).unwrap_or("?");
            out.push_str(&format!("  {} ({}, {})\n", pattern, match_type, scope));
        }
    }

    out.trim_end().to_string()
}

/// Format permission deleted
pub fn permission_deleted(rule_id: &str, found: bool) -> String {
    if found {
        format!("Permission deleted: {}", &rule_id[..8])
    } else {
        format!("Permission not found: {}", &rule_id[..8.min(rule_id.len())])
    }
}
