// src/tools/format/proactive.rs
// Formatters for proactive context and work state

use serde_json::Value;

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

    // Relevant decisions (key is "related_decisions" from proactive context)
    if let Some(decisions) = ctx.get("related_decisions").and_then(|v| v.as_array())
        .or_else(|| ctx.get("decisions").and_then(|v| v.as_array())) {
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

    // Relevant memories (from proactive context)
    if let Some(memories) = ctx.get("relevant_memories").and_then(|v| v.as_array()) {
        if !memories.is_empty() {
            if !out.is_empty() { out.push('\n'); }
            out.push_str("Relevant context:\n");
            for m in memories.iter().take(3) {
                let content = m.get("content").and_then(|v| v.as_str())
                    .or_else(|| m.get("value").and_then(|v| v.as_str()))
                    .unwrap_or("?");
                let display = if content.len() > 60 { format!("{}...", &content[..57]) } else { content.to_string() };
                out.push_str(&format!("  {}\n", display));
            }
        }
    }

    // Active goals (key is "active_goals" from proactive context)
    if let Some(goals) = ctx.get("active_goals").and_then(|v| v.as_array())
        .or_else(|| ctx.get("goals").and_then(|v| v.as_array())) {
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

    // Code context - related files, symbols, and improvements
    if let Some(code_ctx) = ctx.get("code_context") {
        // Improvement suggestions - show high severity first
        if let Some(improvements) = code_ctx.get("improvement_suggestions").and_then(|v| v.as_array()) {
            let high: Vec<_> = improvements.iter()
                .filter(|i| i.get("severity").and_then(|s| s.as_str()) == Some("high"))
                .collect();
            if !high.is_empty() {
                if !out.is_empty() { out.push('\n'); }
                out.push_str("Code improvements needed:\n");
                for imp in high.iter().take(3) {
                    let symbol = imp.get("symbol_name").and_then(|v| v.as_str()).unwrap_or("?");
                    let imp_type = imp.get("improvement_type").and_then(|v| v.as_str()).unwrap_or("?");
                    let current = imp.get("current_value").and_then(|v| v.as_i64()).unwrap_or(0);
                    let threshold = imp.get("threshold").and_then(|v| v.as_i64()).unwrap_or(0);
                    out.push_str(&format!(
                        "  [HIGH] {}: {} ({} lines, max: {})\n",
                        imp_type.replace('_', " "),
                        symbol,
                        current,
                        threshold
                    ));
                }
            }
        }

        // Related files
        if let Some(related) = code_ctx.get("related_files").and_then(|v| v.as_array()) {
            if !related.is_empty() {
                if !out.is_empty() { out.push('\n'); }
                out.push_str("Related files:\n");
                for r in related.iter().take(5) {
                    let file = r.get("file").and_then(|v| v.as_str()).unwrap_or("?");
                    let relation = r.get("relation").and_then(|v| v.as_str()).unwrap_or("related");
                    // Extract just the filename for display
                    let filename = std::path::Path::new(file)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(file);
                    out.push_str(&format!("  {} ({})\n", filename, relation));
                }
            }
        }

        // Key symbols
        if let Some(symbols) = code_ctx.get("key_symbols").and_then(|v| v.as_array()) {
            if !symbols.is_empty() {
                if !out.is_empty() { out.push('\n'); }
                out.push_str("Key symbols:\n");
                for s in symbols.iter().take(8) {
                    let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let stype = s.get("type").and_then(|v| v.as_str()).unwrap_or("?");
                    out.push_str(&format!("  {} ({})\n", name, stype));
                }
            }
        }
    }

    if out.is_empty() {
        "No relevant context.".to_string()
    } else {
        out.trim_end().to_string()
    }
}

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
