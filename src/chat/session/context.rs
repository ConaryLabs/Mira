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
    /// IMPORTANT: Order is optimized for LLM KV caching (prefix matching).
    /// DeepSeek caches prompt prefixes automatically - content that matches
    /// previous requests costs 90% less. We order from MOST STABLE to LEAST:
    ///
    /// STABLE (rarely change within session - high cache hit rate):
    ///   1. Corrections (rules - almost never change)
    ///   2. Constraints (rejected approaches + decisions)
    ///   3. Goals (change occasionally)
    ///   4. Memories (change occasionally)
    ///   5. Summaries (stable between summarizations)
    ///
    /// DYNAMIC (change frequently - low cache hit rate, put LAST):
    ///   6. Git activity (changes with commits)
    ///   7. Code relationships (changes per query)
    ///   8. Similar fixes (changes per query)
    ///   9. Semantic context (changes per query)
    ///   10. Recent messages (changes EVERY turn - MUST BE LAST)
    ///
    /// Skips code_compaction (OpenAI-specific) and code_index_hints (verbose).
    pub fn format_for_deepseek(&self, budget: &super::context::DeepSeekBudget) -> String {
        let mut sections = Vec::new();
        let mut estimated_tokens = 0;

        // Helper to estimate tokens (rough: 1 token ≈ 4 chars)
        let estimate_tokens = |s: &str| s.len() / 4;

        // ========================================================================
        // STABLE SECTION - rarely changes, maximizes cache hits
        // ========================================================================

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

        // 2. Anti-amnesia: rejected approaches and past decisions
        let half_constraints = budget.max_constraints / 2;

        if !self.rejected_approaches.is_empty() {
            let mut lines = vec!["## Constraints (DO NOT repeat these approaches)".to_string()];
            for ra in self.rejected_approaches.iter().take(half_constraints) {
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

        // 3. Goals - change occasionally
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

        // 4. Memories/Preferences - change occasionally
        if !self.mira_context.memories.is_empty() {
            let mut lines = vec!["## Preferences".to_string()];
            for m in self.mira_context.memories.iter().take(budget.max_memories) {
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

        // 5. Summaries - stable between batch summarizations
        if !self.summaries.is_empty() {
            let mut lines = vec!["## Context Summary".to_string()];
            for s in self.summaries.iter().take(budget.max_summaries) {
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

        // 6. Index Status - rarely changes (only when files become stale)
        if let Some(ref status) = self.index_status {
            if !status.stale_files.is_empty() {
                let mut lines = vec!["## Index Status".to_string()];
                lines.push(format!("[!] {} files may have outdated symbol info:", status.stale_files.len()));

                for file in status.stale_files.iter().take(5) {
                    let short = file
                        .split('/')
                        .skip_while(|p| *p != "src" && *p != "lib" && *p != "studio")
                        .collect::<Vec<_>>()
                        .join("/");
                    let display = if short.is_empty() { file.as_str() } else { &short };
                    lines.push(format!("  - {}", display));
                }

                if status.stale_files.len() > 5 {
                    lines.push(format!("  ... and {} more", status.stale_files.len() - 5));
                }

                let section = lines.join("\n");
                estimated_tokens += estimate_tokens(&section);
                if estimated_tokens < budget.token_budget {
                    sections.push(section);
                }
            }
        }

        // ========================================================================
        // DYNAMIC SECTION - changes frequently, put LAST for cache efficiency
        // ========================================================================

        // 7. Git activity - changes with commits
        if let Some(ref activity) = self.repo_activity {
            if !activity.is_empty() {
                let mut lines = vec!["## Recent Project Activity".to_string()];

                if !activity.recent_commits.is_empty() {
                    lines.push("Commits:".to_string());
                    for commit in activity.recent_commits.iter().take(5) {
                        lines.push(format!("- {}: \"{}\" ({})",
                            commit.hash, commit.message, commit.relative_time));
                    }
                }

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

        // 8. Code Relationships - changes per query focus
        if !self.related_files.is_empty() || !self.call_context.is_empty() {
            let mut lines = vec!["## Code Relationships".to_string()];

            if !self.related_files.is_empty() {
                lines.push("Files that change together:".to_string());
                for rf in self.related_files.iter().take(5) {
                    let short_path = rf.file_path
                        .split('/')
                        .skip_while(|p| *p != "src" && *p != "lib" && *p != "studio")
                        .collect::<Vec<_>>()
                        .join("/");
                    let path = if short_path.is_empty() { &rf.file_path } else { &short_path };
                    lines.push(format!("  {} ({:.0}% confidence)", path, rf.confidence * 100.0));
                }
            }

            if !self.call_context.is_empty() {
                if !self.related_files.is_empty() {
                    lines.push(String::new());
                }
                lines.push("Call relationships:".to_string());

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

        // 9. Similar Fixes - changes based on detected errors
        if !self.similar_fixes.is_empty() {
            let mut lines = vec!["## Relevant Past Fixes".to_string()];
            lines.push("Similar errors have been fixed before:".to_string());

            for fix in self.similar_fixes.iter().take(3) {
                let error_preview = if fix.error_pattern.len() > 60 {
                    format!("{}...", &fix.error_pattern[..60])
                } else {
                    fix.error_pattern.clone()
                };
                let fix_preview = if fix.fix_description.len() > 150 {
                    format!("{}...", &fix.fix_description[..150])
                } else {
                    fix.fix_description.clone()
                };
                lines.push(format!("  • \"{}\"", error_preview));
                lines.push(format!("    Fix: {}", fix_preview));
            }

            let section = lines.join("\n");
            estimated_tokens += estimate_tokens(&section);
            if estimated_tokens < budget.token_budget {
                sections.push(section);
            }
        }

        // 10. Semantic context - changes per query
        if !self.semantic_context.is_empty() {
            let mut lines = vec!["## Related Context".to_string()];
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

        // NOTE: Recent messages are NOT included in the system prompt.
        // They are passed separately as conversation history to avoid duplication.
        // The model sees recent messages in the actual message array, not in the system prompt.

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
