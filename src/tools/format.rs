// src/tools/format.rs
// Human-readable formatters for MCP tool responses
// Makes output clean and concise like native Claude Code tools
//
// Organization:
//   - Memory: remember, recall_results, forgotten
//   - Permissions: permission_saved, permission_list, permission_deleted
//   - Sessions: session_stored, session_results, session_start, session_context
//   - Tasks: task_list, task_action
//   - Goals: goal_list, goal_action
//   - Corrections: correction_recorded, correction_list
//   - Code Intelligence: index_status, code_search_results, symbols_list, related_files, call_graph
//   - Git: commit_list
//   - Admin: project_set, table_list, query_results
//   - Build: build_errors
//   - Proactive: proactive_context
//   - Guidelines: guidelines

use serde_json::Value;

// === Memory Formatters ===

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

/// Format proactive context - shows actionable info for the current work
pub fn proactive_context(ctx: &Value) -> String {
    let mut out = String::new();

    // Corrections are most important - these are mistakes to avoid
    if let Some(corrections) = ctx.get("corrections").and_then(|v| v.as_array()) {
        if !corrections.is_empty() {
            out.push_str("Corrections to follow:\n");
            for c in corrections.iter().take(5) {
                let wrong = c.get("what_was_wrong").and_then(|v| v.as_str()).unwrap_or("?");
                let right = c.get("what_is_right").and_then(|v| v.as_str()).unwrap_or("?");
                let wrong_short = if wrong.len() > 25 { format!("{}...", &wrong[..22]) } else { wrong.to_string() };
                let right_short = if right.len() > 40 { format!("{}...", &right[..37]) } else { right.to_string() };
                out.push_str(&format!("  {} → {}\n", wrong_short, right_short));
            }
        }
    }

    // Rejected approaches - don't repeat these
    if let Some(rejected) = ctx.get("rejected_approaches").and_then(|v| v.as_array()) {
        if !rejected.is_empty() {
            if !out.is_empty() { out.push('\n'); }
            out.push_str("Rejected approaches:\n");
            for r in rejected.iter().take(3) {
                let approach = r.get("approach").and_then(|v| v.as_str()).unwrap_or("?");
                let reason = r.get("rejection_reason").and_then(|v| v.as_str()).unwrap_or("?");
                let approach_short = if approach.len() > 30 { format!("{}...", &approach[..27]) } else { approach.to_string() };
                let reason_short = if reason.len() > 30 { format!("{}...", &reason[..27]) } else { reason.to_string() };
                out.push_str(&format!("  {} ({})\n", approach_short, reason_short));
            }
        }
    }

    // Relevant decisions
    if let Some(decisions) = ctx.get("decisions").and_then(|v| v.as_array()) {
        if !decisions.is_empty() {
            if !out.is_empty() { out.push('\n'); }
            out.push_str("Relevant decisions:\n");
            for d in decisions.iter().take(3) {
                let decision = d.get("decision").and_then(|v| v.as_str())
                    .or_else(|| d.get("value").and_then(|v| v.as_str()))
                    .unwrap_or("?");
                let display = if decision.len() > 60 { format!("{}...", &decision[..57]) } else { decision.to_string() };
                out.push_str(&format!("  {}\n", display));
            }
        }
    }

    // Active goals (brief)
    if let Some(goals) = ctx.get("goals").and_then(|v| v.as_array()) {
        if !goals.is_empty() {
            if !out.is_empty() { out.push('\n'); }
            out.push_str("Active goals:\n");
            for g in goals.iter().take(3) {
                let title = g.get("title").and_then(|v| v.as_str()).unwrap_or("?");
                let progress = g.get("progress_percent").and_then(|v| v.as_i64()).unwrap_or(0);
                out.push_str(&format!("  {} ({}%)\n", title, progress));
            }
        }
    }

    if out.is_empty() {
        "No relevant context.".to_string()
    } else {
        out.trim_end().to_string()
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

/// Format session_start result - concise startup summary
pub fn session_start(result: &super::sessions::SessionStartResult) -> String {
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
            out.push_str("⚠ RESUME - Plan mode was in progress\n");
            out.push_str("Use EnterPlanMode to continue planning.\n");
        } else if plan.status == "ready" {
            out.push_str("📋 RESUME - Active plan from previous session:\n");
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
            out.push_str("⚠ RESUME - Active todos from previous session:\n");
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
        out.push_str("📄 Working documents:\n");
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

// === Work Context Formatter (for session resume) ===

/// Format work context entries for session resume detection
/// Outputs context_type keywords that the SessionStart hook looks for
pub fn work_context(results: &[Value]) -> String {
    if results.is_empty() {
        return "No work context found.".to_string();
    }

    let mut types: Vec<&str> = Vec::new();

    for r in results {
        if let Some(ct) = r.get("context_type").and_then(|v| v.as_str()) {
            if !types.contains(&ct) {
                types.push(ct);
            }
        }
    }

    // Build output with keywords the SessionStart hook can detect
    let mut out = String::new();

    for ct in &types {
        match *ct {
            "active_todos" => out.push_str("active_todos: Found in-progress tasks\n"),
            "active_plan" => out.push_str("active_plan: Found plan in progress\n"),
            t if t.starts_with("working_doc") => out.push_str("working_doc: Found working documents\n"),
            _ => out.push_str(&format!("{}: Found\n", ct)),
        }
    }

    out.push_str(&format!("\n{} work context entries total", results.len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // === Memory Formatter Tests ===

    #[test]
    fn test_remember_with_category() {
        let result = remember("test fact", "preference", Some("coding"));
        assert!(result.contains("test fact"));
        assert!(result.contains("preference"));
        assert!(result.contains("coding"));
    }

    #[test]
    fn test_remember_without_category() {
        let result = remember("test fact", "decision", None);
        assert!(result.contains("test fact"));
        assert!(result.contains("decision"));
    }

    #[test]
    fn test_recall_results_empty() {
        let results: Vec<Value> = vec![];
        let output = recall_results(&results);
        assert_eq!(output, "No memories found.");
    }

    #[test]
    fn test_recall_results_with_data() {
        let results = vec![
            json!({
                "fact_type": "preference",
                "value": "Use tabs for indentation",
                "times_used": 5
            }),
            json!({
                "fact_type": "decision",
                "value": "Chose SQLite over PostgreSQL",
                "times_used": 2,
                "score": 0.95
            }),
        ];
        let output = recall_results(&results);
        assert!(output.contains("Found 2 memories"));
        assert!(output.contains("preference"));
        assert!(output.contains("decision"));
        assert!(output.contains("(5x)"));
        assert!(output.contains("[95%]"));
    }

    #[test]
    fn test_forgotten() {
        assert!(forgotten("abc123", true).contains("Forgotten"));
        assert!(forgotten("abc123", false).contains("Not found"));
    }

    // === Task/Goal Formatter Tests ===

    #[test]
    fn test_task_list_empty() {
        let tasks: Vec<Value> = vec![];
        assert_eq!(task_list(&tasks), "No tasks.");
    }

    #[test]
    fn test_task_list_with_items() {
        let tasks = vec![
            json!({"status": "completed", "title": "Fix bug", "priority": "high"}),
            json!({"status": "in_progress", "title": "Add feature"}),
            json!({"status": "pending", "title": "Write tests"}),
        ];
        let output = task_list(&tasks);
        assert!(output.contains("✓ Fix bug [high]"));
        assert!(output.contains("→ Add feature"));
        assert!(output.contains("○ Write tests"));
    }

    #[test]
    fn test_goal_list_with_progress() {
        let goals = vec![
            json!({"status": "in_progress", "title": "Release v1.0", "progress_percent": 75}),
            json!({"status": "completed", "title": "Setup CI", "progress_percent": 100}),
        ];
        let output = goal_list(&goals);
        assert!(output.contains("→ Release v1.0 (75%)"));
        assert!(output.contains("✓ Setup CI (100%)"));
    }

    // === Code Intelligence Formatter Tests ===

    #[test]
    fn test_symbols_list_empty() {
        let symbols: Vec<Value> = vec![];
        assert_eq!(symbols_list(&symbols), "No symbols.");
    }

    #[test]
    fn test_symbols_list_with_data() {
        let symbols = vec![
            json!({"name": "MyStruct", "kind": "struct", "line": 10}),
            json!({"name": "process", "kind": "function", "line": 25}),
        ];
        let output = symbols_list(&symbols);
        assert!(output.contains("MyStruct (struct):10"));
        assert!(output.contains("process (function):25"));
    }

    #[test]
    fn test_commit_list() {
        let commits = vec![
            json!({"hash": "abc123def456", "message": "Fix critical bug", "author": "alice"}),
            json!({"hash": "xyz789", "message": "Add new feature with a very long description that should be truncated at some point to keep the output clean"}),
        ];
        let output = commit_list(&commits);
        assert!(output.contains("abc123d Fix critical bug (alice)"));
        assert!(output.contains("xyz789"));
        assert!(output.contains("...")); // Long message truncated
    }

    // === Admin Formatter Tests ===

    #[test]
    fn test_table_list() {
        let tables = vec![
            ("memories".to_string(), 150i64),
            ("sessions".to_string(), 25i64),
        ];
        let output = table_list(&tables);
        assert!(output.contains("2 tables"));
        assert!(output.contains("memories (150)"));
        assert!(output.contains("sessions (25)"));
    }

    #[test]
    fn test_query_results_empty() {
        let columns: Vec<String> = vec![];
        let rows: Vec<Vec<Value>> = vec![];
        assert_eq!(query_results(&columns, &rows), "No results.");
    }

    #[test]
    fn test_query_results_with_data() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![
            vec![json!(1), json!("Alice")],
            vec![json!(2), json!("Bob")],
        ];
        let output = query_results(&columns, &rows);
        assert!(output.contains("2 rows"));
        assert!(output.contains("id"));
        assert!(output.contains("name"));
        assert!(output.contains("Alice"));
        assert!(output.contains("Bob"));
    }

    // === Correction Formatter Tests ===

    #[test]
    fn test_correction_list_empty() {
        let corrections: Vec<Value> = vec![];
        assert_eq!(correction_list(&corrections), "No corrections.");
    }

    #[test]
    fn test_correction_list_with_data() {
        let corrections = vec![
            json!({
                "correction_type": "code_style",
                "what_was_wrong": "Using var",
                "what_is_right": "Use const/let"
            }),
        ];
        let output = correction_list(&corrections);
        assert!(output.contains("1 correction"));
        assert!(output.contains("code_style"));
        assert!(output.contains("Using var"));
        assert!(output.contains("Use const/let"));
    }

    // === Proactive Context Formatter Tests ===

    #[test]
    fn test_proactive_context_empty() {
        let ctx = json!({});
        assert_eq!(proactive_context(&ctx), "No relevant context.");
    }

    #[test]
    fn test_proactive_context_with_corrections() {
        let ctx = json!({
            "corrections": [
                {"what_was_wrong": "Old approach", "what_is_right": "New approach"}
            ],
            "goals": [
                {"title": "Complete refactor", "progress_percent": 50}
            ]
        });
        let output = proactive_context(&ctx);
        assert!(output.contains("Corrections to follow"));
        assert!(output.contains("Old approach"));
        assert!(output.contains("Active goals"));
        assert!(output.contains("Complete refactor"));
    }

    // === Permission Formatter Tests ===

    #[test]
    fn test_permission_saved() {
        let output = permission_saved("Bash", Some("cargo "), "prefix", "project");
        assert!(output.contains("Bash"));
        assert!(output.contains("cargo"));
        assert!(output.contains("prefix"));
    }

    #[test]
    fn test_permission_list_grouped() {
        let rules = vec![
            json!({"tool_name": "Bash", "input_pattern": "cargo ", "match_type": "prefix", "scope": "project"}),
            json!({"tool_name": "Bash", "input_pattern": "git ", "match_type": "prefix", "scope": "global"}),
            json!({"tool_name": "Read", "input_pattern": "*", "match_type": "any", "scope": "global"}),
        ];
        let output = permission_list(&rules);
        assert!(output.contains("3 permission rules"));
        assert!(output.contains("Bash:"));
        assert!(output.contains("Read:"));
    }
}
