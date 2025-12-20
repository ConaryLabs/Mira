//! Context assembly and formatting
//!
//! Assembles context from multiple sources and formats it for the LLM prompt.

use super::types::AssembledContext;

/// Token usage breakdown by tier for debugging/monitoring
#[derive(Debug, Default)]
pub struct TokenUsage {
    pub corrections: usize,
    pub goals: usize,
    pub git_activity: usize,
    pub recent_messages: usize,
    pub summaries: usize,
    pub semantic_context: usize,
    pub memories: usize,
    pub constraints: usize,  // rejected approaches + decisions
    pub total: usize,
}

/// Budget limits for DeepSeek context assembly
/// Keeps context lean and prioritized to avoid bloat
pub struct DeepSeekBudget {
    /// Max recent messages to include
    pub max_recent_messages: usize,
    /// Max summaries to include
    pub max_summaries: usize,
    /// Max semantic hits to include
    pub max_semantic_hits: usize,
    /// Max memories to include
    pub max_memories: usize,
    /// Max goals to include
    pub max_goals: usize,
    /// Max rejected approaches + decisions to include
    pub max_constraints: usize,
    /// Rough token budget for entire context blob
    pub token_budget: usize,
}

impl Default for DeepSeekBudget {
    fn default() -> Self {
        Self {
            // DeepSeek V3.2 has 128K context with DSA sparse attention
            // Verified Dec 2025: 128K context, ~50% cost reduction for long-context
            max_recent_messages: 15,  // was 8 - full conversation continuity
            max_summaries: 8,         // was 5 - rich historical context
            max_semantic_hits: 10,    // was 5 - more relevant past discussion
            max_memories: 25,         // was 15 - room for preferences
            max_goals: 8,             // was 5 - show active goals
            max_constraints: 10,      // rejected approaches + past decisions
            token_budget: 90000,      // was 24k - utilize 128K context (leaving 38K for output)
        }
    }
}

impl AssembledContext {
    /// Format context for injection into system prompt
    ///
    /// IMPORTANT: Order is optimized for LLM caching (prefix matching).
    /// Static/stable content comes FIRST for maximum cache hits:
    ///   1. Mira context (corrections, goals, memories) - stable per session
    ///   2. Code compaction blob - stable between compactions
    ///   3. Summaries - stable between batch summarizations
    ///   4. Semantic context - changes per query
    ///   5. Code index hints - changes per query
    ///   6. Recent messages (raw) - changes every turn (LEAST cacheable)
    pub fn format_for_prompt(&self) -> String {
        let mut sections = Vec::new();

        // 1. Mira context (corrections, goals, memories) - MOST STABLE
        // These rarely change within a session
        let mira = self.mira_context.as_system_prompt();
        if !mira.is_empty() {
            sections.push(mira);
        }

        // 2. Code compaction blob - stable between compactions
        // This is an opaque encrypted blob from OpenAI that preserves code understanding
        if let Some(ref blob) = self.code_compaction {
            sections.push(format!(
                "## Code Context (Compacted)\n<compacted_context>{}</compacted_context>",
                blob
            ));
        }

        // 3. Summaries - stable between summarizations
        if !self.summaries.is_empty() {
            let mut summary_section = String::from("## Previous Context (Summarized)\n");
            for (i, s) in self.summaries.iter().enumerate() {
                summary_section.push_str(&format!("{}. {}\n", i + 1, s));
            }
            sections.push(summary_section);
        }

        // 4. Semantic context - QUERY-DEPENDENT (at end for cache friendliness)
        // Relevant past conversation snippets based on current query
        if !self.semantic_context.is_empty() {
            let mut semantic_section = String::from("## Relevant Past Context\n");
            for hit in &self.semantic_context {
                let preview = if hit.content.len() > 200 {
                    // Find valid char boundary near 200
                    let mut end = 200;
                    while !hit.content.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}...", &hit.content[..end])
                } else {
                    hit.content.clone()
                };
                semantic_section.push_str(&format!("- [{}] {}\n", hit.role, preview));
            }
            sections.push(semantic_section);
        }

        // 5. Code index hints - QUERY-DEPENDENT (at end for cache friendliness)
        // Relevant symbols from the codebase based on current query
        if !self.code_index_hints.is_empty() {
            let mut code_section = String::from("## Relevant Code Locations\n");
            for hint in &self.code_index_hints {
                code_section.push_str(&format!("**{}**\n", hint.file_path));
                for sym in &hint.symbols {
                    let sig = sym.signature.as_deref().unwrap_or("");
                    if sig.is_empty() {
                        code_section.push_str(&format!(
                            "  - {} `{}` (L{})\n",
                            sym.symbol_type, sym.name, sym.start_line
                        ));
                    } else {
                        code_section.push_str(&format!(
                            "  - {} `{}` (L{}): {}\n",
                            sym.symbol_type, sym.name, sym.start_line, sig
                        ));
                    }
                }
            }
            sections.push(code_section);
        }

        // 6. Recent messages (raw) - CHANGES EVERY TURN (at very end)
        // Full fidelity for the most recent conversation turns
        if !self.recent_messages.is_empty() {
            let mut recent_section = String::from("## Recent Conversation\n");
            for msg in &self.recent_messages {
                let role_label = if msg.role == "user" { "User" } else { "Assistant" };
                // Truncate long messages for context efficiency
                let content = if msg.content.len() > 500 {
                    let mut end = 500;
                    while !msg.content.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}...", &msg.content[..end])
                } else {
                    msg.content.clone()
                };
                recent_section.push_str(&format!("**{}**: {}\n\n", role_label, content));
            }
            sections.push(recent_section);
        }

        if sections.is_empty() {
            String::new()
        } else {
            sections.join("\n\n")
        }
    }

    /// Format context for DeepSeek with budget awareness
    ///
    /// Priority order (highest to lowest):
    /// 1. Corrections (always include - they're rules)
    /// 1.5. Anti-amnesia: rejected approaches + past decisions (prevent repeating mistakes)
    /// 2. Goals (capped)
    /// 3. Recent messages (capped, most recent first)
    /// 4. Summaries (capped)
    /// 5. Semantic context (capped, highest score first)
    /// 6. Memories (aggressively capped)
    ///
    /// Skips code_compaction (OpenAI-specific) and code_index_hints (usually verbose).
    pub fn format_for_deepseek(&self, budget: &super::context::DeepSeekBudget) -> String {
        let mut sections = Vec::new();
        let mut estimated_tokens = 0;

        // Helper to estimate tokens (rough: 1 token ≈ 4 chars)
        let estimate_tokens = |s: &str| s.len() / 4;

        // 1. Corrections - ALWAYS include (they're rules to follow)
        if !self.mira_context.corrections.is_empty() {
            let mut lines = vec!["## Corrections".to_string()];
            for c in &self.mira_context.corrections {
                lines.push(format!("- {}: \"{}\" → \"{}\"",
                    c.correction_type, c.what_was_wrong, c.what_is_right));
            }
            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            sections.push(section);
        }

        // 1.5. Anti-amnesia: rejected approaches and past decisions
        // These prevent repeating mistakes and ensure past decisions are respected
        let half_constraints = budget.max_constraints / 2;

        if !self.rejected_approaches.is_empty() {
            let mut lines = vec!["## Constraints (DO NOT repeat these approaches)".to_string()];
            for ra in self.rejected_approaches.iter().take(half_constraints) {
                // Format: [!] problem: approach → reason
                let problem_preview = if ra.problem_context.len() > 60 {
                    format!("{}...", &ra.problem_context[..60])
                } else {
                    ra.problem_context.clone()
                };
                let approach_preview = if ra.approach.len() > 80 {
                    format!("{}...", &ra.approach[..80])
                } else {
                    ra.approach.clone()
                };
                lines.push(format!("[!] {}: \"{}\" → rejected: {}",
                    problem_preview, approach_preview, ra.rejection_reason));
            }
            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            if estimated_tokens < budget.token_budget {
                sections.push(section);
            }
        }

        if !self.past_decisions.is_empty() {
            let mut lines = vec!["## Past Decisions".to_string()];
            for d in self.past_decisions.iter().take(half_constraints) {
                let context_str = d.context.as_deref().unwrap_or("");
                if context_str.is_empty() {
                    lines.push(format!("- {}: {}", d.key, d.decision));
                } else {
                    let context_preview = if context_str.len() > 50 {
                        format!("{}...", &context_str[..50])
                    } else {
                        context_str.to_string()
                    };
                    lines.push(format!("- {}: {} ({})", d.key, d.decision, context_preview));
                }
            }
            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            if estimated_tokens < budget.token_budget {
                sections.push(section);
            }
        }

        // 2. Goals - capped
        if !self.mira_context.goals.is_empty() {
            let mut lines = vec!["## Active Goals".to_string()];
            for g in self.mira_context.goals.iter().take(budget.max_goals) {
                let status_icon = match g.status.as_str() {
                    "in_progress" => "→",
                    "blocked" => "!",
                    _ => "○",
                };
                lines.push(format!("{} {} ({}%)", status_icon, g.title, g.progress));
            }
            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            if estimated_tokens < budget.token_budget {
                sections.push(section);
            }
        }

        // 2.5. Git activity - recent commits and changes
        // This gives the LLM awareness of "what just happened" in the codebase
        if let Some(ref activity) = self.repo_activity {
            if !activity.is_empty() {
                let mut lines = vec!["## Recent Project Activity".to_string()];

                // Recent commits
                if !activity.recent_commits.is_empty() {
                    lines.push("Commits:".to_string());
                    for commit in activity.recent_commits.iter().take(5) {
                        lines.push(format!("- {}: \"{}\" ({})",
                            commit.hash, commit.message, commit.relative_time));
                    }
                }

                // Changed files (show stat summary, not full diff)
                if !activity.changed_files.is_empty() {
                    lines.push(String::new());
                    lines.push("Changed files:".to_string());
                    for file in activity.changed_files.iter().take(15) {
                        let change = if file.is_new {
                            format!("{} (new file, +{} lines)", file.path, file.insertions)
                        } else {
                            format!("{} (+{}, -{})", file.path, file.insertions, file.deletions)
                        };
                        lines.push(format!("  {}", change));
                    }
                    if activity.changed_files.len() > 15 {
                        lines.push(format!("  ... and {} more files",
                            activity.changed_files.len() - 15));
                    }
                }

                // Note uncommitted changes
                if activity.has_uncommitted {
                    lines.push(String::new());
                    lines.push("[Uncommitted changes present]".to_string());
                }

                let section = lines.join("\n");
                estimated_tokens += estimate_tokens(&section);
                if estimated_tokens < budget.token_budget {
                    sections.push(section);
                }
            }
        }

        // 3. Recent messages - capped, for conversation continuity
        if !self.recent_messages.is_empty() {
            let mut lines = vec!["## Recent".to_string()];
            // Take most recent N messages
            let recent: Vec<_> = self.recent_messages.iter()
                .rev()
                .take(budget.max_recent_messages)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();

            for msg in recent {
                let role = if msg.role == "user" { "U" } else { "A" };
                // Truncate to 800 chars - enough for substantial responses
                let content = if msg.content.len() > 800 {
                    let mut end = 800;
                    while !msg.content.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}…", &msg.content[..end])
                } else {
                    msg.content.clone()
                };
                lines.push(format!("[{}] {}", role, content));
            }
            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            if estimated_tokens < budget.token_budget {
                sections.push(section);
            }
        }

        // 4. Summaries - capped
        if !self.summaries.is_empty() {
            let mut lines = vec!["## Context Summary".to_string()];
            for s in self.summaries.iter().take(budget.max_summaries) {
                // Truncate long summaries to 1000 chars
                let summary = if s.len() > 1000 {
                    let mut end = 1000;
                    while !s.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}…", &s[..end])
                } else {
                    s.clone()
                };
                lines.push(format!("- {}", summary));
            }
            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            if estimated_tokens < budget.token_budget {
                sections.push(section);
            }
        }

        // 5. Semantic context - capped, highest score first
        if !self.semantic_context.is_empty() {
            let mut lines = vec!["## Related Context".to_string()];
            // semantic_context should already be sorted by score
            for hit in self.semantic_context.iter().take(budget.max_semantic_hits) {
                let preview = if hit.content.len() > 400 {
                    let mut end = 400;
                    while !hit.content.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}…", &hit.content[..end])
                } else {
                    hit.content.clone()
                };
                lines.push(format!("- {}", preview));
            }
            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            if estimated_tokens < budget.token_budget {
                sections.push(section);
            }
        }

        // 6. Memories - capped but not aggressively
        if !self.mira_context.memories.is_empty() {
            let mut lines = vec!["## Preferences".to_string()];
            for m in self.mira_context.memories.iter().take(budget.max_memories) {
                // 200 chars is enough for most memory items
                let content = if m.content.len() > 200 {
                    let mut end = 200;
                    while !m.content.is_char_boundary(end) && end > 0 {
                        end -= 1;
                    }
                    format!("{}…", &m.content[..end])
                } else {
                    m.content.clone()
                };
                lines.push(format!("- {}", content));
            }
            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            if estimated_tokens < budget.token_budget {
                sections.push(section);
            }
        }

        // 7. Code Relationships - cochange patterns and call graph
        // Shows what files/functions are related to current focus
        if !self.related_files.is_empty() || !self.call_context.is_empty() {
            let mut lines = vec!["## Code Relationships".to_string()];

            // Related files from cochange patterns
            if !self.related_files.is_empty() {
                lines.push("Files that change together:".to_string());
                for rf in self.related_files.iter().take(5) {
                    // Shorten file path for display
                    let short_path = rf.file_path
                        .split('/')
                        .skip_while(|p| *p != "src" && *p != "lib" && *p != "studio")
                        .collect::<Vec<_>>()
                        .join("/");
                    let path = if short_path.is_empty() { &rf.file_path } else { &short_path };
                    lines.push(format!("  {} ({:.0}% confidence)", path, rf.confidence * 100.0));
                }
            }

            // Call graph context
            if !self.call_context.is_empty() {
                if !self.related_files.is_empty() {
                    lines.push(String::new());
                }
                lines.push("Call relationships:".to_string());

                // Group by direction
                let callers: Vec<_> = self.call_context.iter()
                    .filter(|c| c.direction == "caller")
                    .take(5)
                    .collect();
                let callees: Vec<_> = self.call_context.iter()
                    .filter(|c| c.direction == "callee")
                    .take(5)
                    .collect();

                if !callers.is_empty() {
                    lines.push("  Called by:".to_string());
                    for c in callers {
                        lines.push(format!("    - {}()", c.symbol_name));
                    }
                }
                if !callees.is_empty() {
                    lines.push("  Calls:".to_string());
                    for c in callees {
                        lines.push(format!("    - {}()", c.symbol_name));
                    }
                }
            }

            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            if estimated_tokens < budget.token_budget {
                sections.push(section);
            }
        }

        if sections.is_empty() {
            String::new()
        } else {
            sections.join("\n\n")
        }
    }

    /// Format recent messages as conversation history
    pub fn format_conversation_history(&self) -> String {
        if self.recent_messages.is_empty() {
            return String::new();
        }

        let mut history = String::from("## Recent Conversation\n");
        for msg in &self.recent_messages {
            let role_label = if msg.role == "user" {
                "User"
            } else {
                "Assistant"
            };
            // Use chars to avoid UTF-8 boundary panic
            let preview = if msg.content.chars().count() > 500 {
                format!("{}...", msg.content.chars().take(500).collect::<String>())
            } else {
                msg.content.clone()
            };
            history.push_str(&format!("**{}**: {}\n\n", role_label, preview));
        }
        history
    }
}
