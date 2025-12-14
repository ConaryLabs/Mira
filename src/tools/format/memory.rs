// src/tools/format/memory.rs
// Formatters for memory operations (remember, recall, forget)

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
