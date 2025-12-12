// src/tools/format.rs
// Human-readable formatters for MCP tool responses
// Makes output clean and concise like native Claude Code tools

use serde_json::Value;

/// Format a "remembered" response
pub fn remember(key: &str, fact_type: &str, category: Option<&str>) -> String {
    match category {
        Some(cat) => format!("Remembered: \"{}\" ({}, {})", key, fact_type, cat),
        None => format!("Remembered: \"{}\" ({})", key, fact_type),
    }
}

/// Format recall results
pub fn recall_results(results: &[Value]) -> String {
    if results.is_empty() {
        return "No memories found.".to_string();
    }

    let mut out = format!("Found {} memor{}:\n",
        results.len(),
        if results.len() == 1 { "y" } else { "ies" }
    );

    for r in results {
        let fact_type = r.get("fact_type").and_then(|v| v.as_str()).unwrap_or("general");
        let value = r.get("value").and_then(|v| v.as_str()).unwrap_or("");
        let times_used = r.get("times_used").and_then(|v| v.as_i64()).unwrap_or(0);
        let score = r.get("score").and_then(|v| v.as_f64());

        // Truncate long values
        let display_value = if value.len() > 100 {
            format!("{}...", &value[..97])
        } else {
            value.to_string()
        };

        let usage = if times_used > 0 {
            format!(" ({}x)", times_used)
        } else {
            String::new()
        };

        let relevance = score.map(|s| format!(" [{:.0}%]", s * 100.0)).unwrap_or_default();

        out.push_str(&format!("  [{}] {}{}{}\n", fact_type, display_value, usage, relevance));
    }

    out.trim_end().to_string()
}

/// Format forgotten response
pub fn forgotten(id: &str, found: bool) -> String {
    if found {
        format!("Forgotten: {}", id)
    } else {
        format!("Not found: {}", id)
    }
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
            "abandoned" => "✗",
            _ => "○",
        };

        out.push_str(&format!("{} {} ({}%)\n", icon, title, progress));
    }

    out.trim_end().to_string()
}

/// Format goal action
pub fn goal_action(action: &str, goal_id: &str, title: &str) -> String {
    format!("Goal {}: {} ({})", action, title, &goal_id[..8])
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

/// Format index status
pub fn index_status(action: &str, path: &str, stats: Option<&Value>) -> String {
    match stats {
        Some(s) => {
            let files = s.get("files_indexed").and_then(|v| v.as_i64()).unwrap_or(0);
            let symbols = s.get("symbols_indexed").and_then(|v| v.as_i64()).unwrap_or(0);
            let commits = s.get("commits_indexed").and_then(|v| v.as_i64()).unwrap_or(0);

            if commits > 0 {
                format!("Indexed {}: {} files, {} symbols, {} commits", path, files, symbols, commits)
            } else if symbols > 0 {
                format!("Indexed {}: {} files, {} symbols", path, files, symbols)
            } else {
                format!("Indexed {}: {} files", path, files)
            }
        }
        None => format!("{} complete: {}", action, path),
    }
}

/// Format code search results
pub fn code_search_results(results: &[Value]) -> String {
    if results.is_empty() {
        return "No matches.".to_string();
    }

    let mut out = format!("Found {} match{}:\n",
        results.len(),
        if results.len() == 1 { "" } else { "es" }
    );

    for r in results {
        let file = r.get("file_path").and_then(|v| v.as_str()).unwrap_or("?");
        let symbol = r.get("symbol_name").and_then(|v| v.as_str());
        let score = r.get("score").and_then(|v| v.as_f64());

        let rel = score.map(|s| format!(" [{:.0}%]", s * 100.0)).unwrap_or_default();

        match symbol {
            Some(sym) => out.push_str(&format!("  {}:{}{}\n", file, sym, rel)),
            None => out.push_str(&format!("  {}{}\n", file, rel)),
        }
    }

    out.trim_end().to_string()
}

/// Format symbols list
pub fn symbols_list(results: &[Value]) -> String {
    if results.is_empty() {
        return "No symbols.".to_string();
    }

    let mut out = String::new();
    for r in results {
        let name = r.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let kind = r.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
        let line = r.get("line").and_then(|v| v.as_i64());

        let loc = line.map(|l| format!(":{}", l)).unwrap_or_default();
        out.push_str(&format!("  {} ({}){}\n", name, kind, loc));
    }

    out.trim_end().to_string()
}

/// Format commit list
pub fn commit_list(results: &[Value]) -> String {
    if results.is_empty() {
        return "No commits.".to_string();
    }

    let mut out = String::new();
    for r in results {
        let hash = r.get("hash").and_then(|v| v.as_str()).unwrap_or("?");
        let message = r.get("message").and_then(|v| v.as_str()).unwrap_or("?");
        let author = r.get("author").and_then(|v| v.as_str());

        let short_hash = if hash.len() > 7 { &hash[..7] } else { hash };
        let short_msg = if message.len() > 60 { format!("{}...", &message[..57]) } else { message.to_string() };

        match author {
            Some(a) => out.push_str(&format!("  {} {} ({})\n", short_hash, short_msg, a)),
            None => out.push_str(&format!("  {} {}\n", short_hash, short_msg)),
        }
    }

    out.trim_end().to_string()
}

/// Format project set response
pub fn project_set(name: &str, path: &str) -> String {
    format!("Project: {} ({})", name, path)
}

/// Format simple status response
pub fn status(action: &str, success: bool) -> String {
    if success {
        format!("{}: OK", action)
    } else {
        format!("{}: failed", action)
    }
}

/// Format table list
pub fn table_list(tables: &[(String, i64)]) -> String {
    if tables.is_empty() {
        return "No tables.".to_string();
    }

    let mut out = format!("{} tables:\n", tables.len());
    for (name, count) in tables {
        out.push_str(&format!("  {} ({})\n", name, count));
    }

    out.trim_end().to_string()
}

/// Format query results
pub fn query_results(columns: &[String], rows: &[Vec<Value>]) -> String {
    if rows.is_empty() {
        return "No results.".to_string();
    }

    let mut out = format!("{} row{}:\n", rows.len(), if rows.len() == 1 { "" } else { "s" });

    // Calculate column widths
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
    for row in rows {
        for (i, val) in row.iter().enumerate() {
            let len = format_value(val).len();
            if i < widths.len() && len > widths[i] {
                widths[i] = len.min(30); // Cap at 30
            }
        }
    }

    // Header
    out.push_str("  ");
    for (i, col) in columns.iter().enumerate() {
        let w = widths.get(i).copied().unwrap_or(10);
        out.push_str(&format!("{:width$} ", col, width = w));
    }
    out.push('\n');

    // Rows (limit to 20)
    for row in rows.iter().take(20) {
        out.push_str("  ");
        for (i, val) in row.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(10);
            let formatted = format_value(val);
            let display = if formatted.len() > 30 {
                format!("{}...", &formatted[..27])
            } else {
                formatted
            };
            out.push_str(&format!("{:width$} ", display, width = w));
        }
        out.push('\n');
    }

    if rows.len() > 20 {
        out.push_str(&format!("  ... and {} more\n", rows.len() - 20));
    }

    out.trim_end().to_string()
}

fn format_value(val: &Value) -> String {
    match val {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(a) => format!("[{}]", a.len()),
        Value::Object(o) => format!("{{{}}}", o.len()),
    }
}

/// Format related files
pub fn related_files(results: &[Value]) -> String {
    if results.is_empty() {
        return "No related files.".to_string();
    }

    let mut out = format!("{} related file{}:\n",
        results.len(),
        if results.len() == 1 { "" } else { "s" }
    );

    for r in results {
        let path = r.get("file_path").and_then(|v| v.as_str()).unwrap_or("?");
        let rel_type = r.get("relation_type").and_then(|v| v.as_str()).unwrap_or("?");
        let score = r.get("score").and_then(|v| v.as_f64());

        let rel = score.map(|s| format!(" ({:.0}%)", s * 100.0)).unwrap_or_default();
        out.push_str(&format!("  {} [{}]{}\n", path, rel_type, rel));
    }

    out.trim_end().to_string()
}

/// Format call graph
pub fn call_graph(results: &[Value]) -> String {
    if results.is_empty() {
        return "No call graph data.".to_string();
    }

    let mut out = String::new();
    for r in results {
        let caller = r.get("caller").and_then(|v| v.as_str()).unwrap_or("?");
        let callee = r.get("callee").and_then(|v| v.as_str()).unwrap_or("?");
        out.push_str(&format!("  {} → {}\n", caller, callee));
    }

    out.trim_end().to_string()
}

/// Format build errors
pub fn build_errors(results: &[Value]) -> String {
    if results.is_empty() {
        return "No build errors.".to_string();
    }

    let mut out = format!("{} error{}:\n",
        results.len(),
        if results.len() == 1 { "" } else { "s" }
    );

    for r in results {
        let file = r.get("file_path").and_then(|v| v.as_str()).unwrap_or("?");
        let line = r.get("line_number").and_then(|v| v.as_i64());
        let message = r.get("message").and_then(|v| v.as_str()).unwrap_or("?");

        let loc = line.map(|l| format!(":{}", l)).unwrap_or_default();
        let msg_short = if message.len() > 60 { format!("{}...", &message[..57]) } else { message.to_string() };

        out.push_str(&format!("  {}{} {}\n", file, loc, msg_short));
    }

    out.trim_end().to_string()
}

/// Format proactive context
pub fn proactive_context(ctx: &Value) -> String {
    let mut sections = Vec::new();

    if let Some(corrections) = ctx.get("corrections").and_then(|v| v.as_array()) {
        if !corrections.is_empty() {
            sections.push(format!("{} correction{}", corrections.len(), if corrections.len() == 1 { "" } else { "s" }));
        }
    }

    if let Some(decisions) = ctx.get("decisions").and_then(|v| v.as_array()) {
        if !decisions.is_empty() {
            sections.push(format!("{} decision{}", decisions.len(), if decisions.len() == 1 { "" } else { "s" }));
        }
    }

    if let Some(goals) = ctx.get("goals").and_then(|v| v.as_array()) {
        if !goals.is_empty() {
            sections.push(format!("{} goal{}", goals.len(), if goals.len() == 1 { "" } else { "s" }));
        }
    }

    if let Some(tasks) = ctx.get("tasks").and_then(|v| v.as_array()) {
        if !tasks.is_empty() {
            sections.push(format!("{} task{}", tasks.len(), if tasks.len() == 1 { "" } else { "s" }));
        }
    }

    if sections.is_empty() {
        "No relevant context.".to_string()
    } else {
        format!("Context: {}", sections.join(", "))
    }
}

/// Format guidelines
pub fn guidelines(results: &[Value]) -> String {
    if results.is_empty() {
        return "No guidelines.".to_string();
    }

    let mut out = String::new();
    let mut current_category = String::new();

    for r in results {
        let category = r.get("category").and_then(|v| v.as_str()).unwrap_or("general");
        let content = r.get("content").and_then(|v| v.as_str()).unwrap_or("?");

        if category != current_category {
            if !current_category.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("[{}]\n", category));
            current_category = category.to_string();
        }

        out.push_str(&format!("  {}\n", content));
    }

    out.trim_end().to_string()
}

/// Format session context summary
pub fn session_context(ctx: &Value) -> String {
    let mut parts = Vec::new();

    if let Some(sessions) = ctx.get("recent_sessions").and_then(|v| v.as_array()) {
        if !sessions.is_empty() {
            parts.push(format!("{} recent session{}", sessions.len(), if sessions.len() == 1 { "" } else { "s" }));
        }
    }

    if let Some(tasks) = ctx.get("pending_tasks").and_then(|v| v.as_array()) {
        if !tasks.is_empty() {
            parts.push(format!("{} pending task{}", tasks.len(), if tasks.len() == 1 { "" } else { "s" }));
        }
    }

    if let Some(goals) = ctx.get("active_goals").and_then(|v| v.as_array()) {
        if !goals.is_empty() {
            parts.push(format!("{} active goal{}", goals.len(), if goals.len() == 1 { "" } else { "s" }));
        }
    }

    if let Some(corrections) = ctx.get("recent_corrections").and_then(|v| v.as_array()) {
        if !corrections.is_empty() {
            parts.push(format!("{} correction{}", corrections.len(), if corrections.len() == 1 { "" } else { "s" }));
        }
    }

    if parts.is_empty() {
        "Fresh session - no prior context.".to_string()
    } else {
        format!("Session context: {}", parts.join(", "))
    }
}
