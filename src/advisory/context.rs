//! Context Injection - Shared context building for advisory calls
//!
//! Provides project context injection for all advisory providers.
//! Fetches corrections, goals, decisions, memories, commits, etc.
//! and formats them for LLM consumption.

use sqlx::SqlitePool;
use std::sync::Arc;

use crate::core::SemanticSearch;
use crate::tools::proactive;
use crate::tools::git_intel;
use crate::tools::types::{GetProactiveContextRequest, GetRecentCommitsRequest};

/// Fetch and format project context for advisory calls
///
/// Returns a formatted string with:
/// - Corrections (learned mistakes to avoid)
/// - Active goals with progress
/// - Past decisions
/// - Relevant memories
/// - Rejected approaches
/// - Recent commits
/// - Related files and symbols
/// - Code style guidance
pub async fn get_project_context(
    db: &SqlitePool,
    semantic: &Arc<SemanticSearch>,
    project_id: Option<i64>,
    project_name: Option<&str>,
    project_type: Option<&str>,
) -> Option<String> {
    // Fetch proactive context with generous limits
    let ctx_result = proactive::get_proactive_context(
        db,
        semantic,
        GetProactiveContextRequest {
            files: None,
            topics: None,
            error: None,
            task: None,
            limit_per_category: Some(5),
        },
        project_id,
    ).await;

    let ctx = match ctx_result {
        Ok(c) => c,
        Err(_) => return None,
    };

    // Fetch recent commits
    let commits = git_intel::get_recent_commits(
        db,
        GetRecentCommitsRequest {
            limit: Some(5),
            file_path: None,
            author: None,
        },
    ).await.unwrap_or_default();

    Some(format_context_for_llm(&ctx, &commits, project_name, project_type))
}

/// Format context into a readable string for LLM consumption
pub fn format_context_for_llm(
    ctx: &serde_json::Value,
    commits: &[serde_json::Value],
    project_name: Option<&str>,
    project_type: Option<&str>,
) -> String {
    let mut parts = vec![];

    // Project header
    if let (Some(name), Some(ptype)) = (project_name, project_type) {
        parts.push(format!("# Project: {} ({})\n", name, ptype));
    }

    // Corrections - full detail
    if let Some(corrections) = ctx["corrections"].as_array() {
        if !corrections.is_empty() {
            parts.push("## Corrections (learned mistakes to avoid)".to_string());
            for c in corrections {
                let wrong = c["what_was_wrong"].as_str().unwrap_or("");
                let right = c["what_is_right"].as_str().unwrap_or("");
                let rationale = c["rationale"].as_str().unwrap_or("");
                parts.push(format!("- **{}** -> {}", wrong, right));
                if !rationale.is_empty() {
                    parts.push(format!("  Rationale: {}", rationale));
                }
            }
            parts.push(String::new());
        }
    }

    // Active goals with progress
    if let Some(goals) = ctx["active_goals"].as_array() {
        if !goals.is_empty() {
            parts.push("## Active Goals".to_string());
            for g in goals {
                let title = g["title"].as_str().unwrap_or("");
                let status = g["status"].as_str().unwrap_or("");
                let progress = g["progress_percent"].as_i64().unwrap_or(0);
                let desc = g["description"].as_str().unwrap_or("");
                parts.push(format!("- **{}** [{}] {}%", title, status, progress));
                if !desc.is_empty() {
                    parts.push(format!("  {}", desc));
                }
            }
            parts.push(String::new());
        }
    }

    // Related decisions
    if let Some(decisions) = ctx["related_decisions"].as_array() {
        if !decisions.is_empty() {
            parts.push("## Past Decisions".to_string());
            for d in decisions {
                let content = d["content"].as_str().unwrap_or("");
                if !content.is_empty() {
                    parts.push(format!("- {}", content));
                }
            }
            parts.push(String::new());
        }
    }

    // Relevant memories
    if let Some(memories) = ctx["relevant_memories"].as_array() {
        if !memories.is_empty() {
            parts.push("## Relevant Context".to_string());
            for m in memories {
                let content = m["content"].as_str().unwrap_or("");
                if !content.is_empty() {
                    parts.push(format!("- {}", content));
                }
            }
            parts.push(String::new());
        }
    }

    // Rejected approaches
    if let Some(rejected) = ctx["rejected_approaches"].as_array() {
        if !rejected.is_empty() {
            parts.push("## Rejected Approaches (don't suggest these)".to_string());
            for r in rejected {
                let approach = r["approach"].as_str().unwrap_or("");
                let reason = r["rejection_reason"].as_str().unwrap_or("");
                if !approach.is_empty() {
                    parts.push(format!("- **{}**: {}", approach, reason));
                }
            }
            parts.push(String::new());
        }
    }

    // Recent commits
    if !commits.is_empty() {
        parts.push("## Recent Commits".to_string());
        for c in commits {
            let msg = c["message"].as_str().unwrap_or("");
            let author = c["author_name"].as_str().unwrap_or("");
            // Truncate long commit messages
            let msg_short = if msg.len() > 80 {
                format!("{}...", &msg[..77])
            } else {
                msg.to_string()
            };
            parts.push(format!("- {} ({})", msg_short, author));
        }
        parts.push(String::new());
    }

    // Similar fixes (error patterns that have been solved before)
    if let Some(fixes) = ctx["similar_fixes"].as_array() {
        if !fixes.is_empty() {
            parts.push("## Similar Past Fixes".to_string());
            for f in fixes {
                let pattern = f["error_pattern"].as_str().unwrap_or("");
                let fix = f["fix_description"].as_str().unwrap_or("");
                if !pattern.is_empty() && !fix.is_empty() {
                    parts.push(format!("- **{}**: {}", pattern, fix));
                }
            }
            parts.push(String::new());
        }
    }

    // Code context - related files and key symbols
    if let Some(code_ctx) = ctx.get("code_context") {
        // Related files (cochange patterns and imports)
        if let Some(related) = code_ctx["related_files"].as_array() {
            if !related.is_empty() {
                parts.push("## Related Files".to_string());
                for r in related.iter().take(5) {
                    let file = r["file"].as_str().unwrap_or("");
                    let relation = r["relation"].as_str().unwrap_or("");
                    let related_to = r["related_to"].as_str().unwrap_or("");
                    if !file.is_empty() {
                        parts.push(format!("- {} ({} of {})", file, relation, related_to));
                    }
                }
                parts.push(String::new());
            }
        }

        // Key symbols in the codebase
        if let Some(symbols) = code_ctx["key_symbols"].as_array() {
            if !symbols.is_empty() {
                parts.push("## Key Symbols".to_string());
                for s in symbols.iter().take(8) {
                    let name = s["name"].as_str().unwrap_or("");
                    let stype = s["type"].as_str().unwrap_or("");
                    let file = s["file"].as_str().unwrap_or("");
                    if !name.is_empty() {
                        parts.push(format!("- `{}` ({}) in {}", name, stype, file));
                    }
                }
                parts.push(String::new());
            }
        }

        // Codebase style guidance
        if let Some(style) = code_ctx.get("codebase_style") {
            if let Some(guidance) = style["guidance"].as_str() {
                parts.push("## Code Style".to_string());
                parts.push(format!("- {}", guidance));
                parts.push(String::new());
            }
        }

        // Improvement suggestions
        if let Some(improvements) = code_ctx["improvement_suggestions"].as_array() {
            if !improvements.is_empty() {
                parts.push("## Suggested Improvements".to_string());
                for i in improvements.iter().take(3) {
                    let symbol = i["symbol_name"].as_str().unwrap_or("");
                    let suggestion = i["suggestion"].as_str().unwrap_or("");
                    if !symbol.is_empty() && !suggestion.is_empty() {
                        parts.push(format!("- `{}`: {}", symbol, suggestion));
                    }
                }
                parts.push(String::new());
            }
        }
    }

    // Call graph relationships
    if let Some(call_graph) = ctx["call_graph"].as_array() {
        if !call_graph.is_empty() {
            parts.push("## Call Relationships".to_string());
            for cg in call_graph.iter().take(8) {
                let caller = cg["caller"].as_str().unwrap_or("");
                let callee = cg["callee"].as_str().unwrap_or("");
                if !caller.is_empty() && !callee.is_empty() {
                    parts.push(format!("- `{}` → `{}`", caller, callee));
                }
            }
            parts.push(String::new());
        }
    }

    // Index status (alert if stale)
    if let Some(index) = ctx.get("index_status") {
        if let Some(stale) = index["stale_files"].as_array() {
            if !stale.is_empty() {
                parts.push("## Index Status".to_string());
                parts.push(format!("⚠️ {} files may be out of date in the index", stale.len()));
                parts.push(String::new());
            }
        }
    }

    parts.join("\n")
}
