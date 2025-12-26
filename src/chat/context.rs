//! Mira context injection
//!
//! Loads persistent context from Mira's SQLite backend:
//! - Corrections (things user has corrected before)
//! - Goals (active project goals)
//! - Memories (facts and preferences)
//!
//! This context is injected into the system instructions for GPT-5.2

use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

use crate::chat::session::{AssembledContext, DeepSeekBudget};

/// Context loaded from Mira's persistent storage
#[derive(Debug, Default, Clone)]
pub struct MiraContext {
    /// Persona instructions (loaded from coding_guidelines)
    pub persona: Option<String>,
    /// Active corrections the model should follow
    pub corrections: Vec<Correction>,
    /// Current project goals
    pub goals: Vec<Goal>,
    /// Relevant memories/facts
    pub memories: Vec<Memory>,
    /// Current project ID
    pub project_id: Option<i64>,
    /// Project path
    pub project_path: Option<String>,
}

/// A correction recorded when user corrected the assistant
#[derive(Debug, Clone)]
pub struct Correction {
    pub what_was_wrong: String,
    pub what_is_right: String,
    pub correction_type: String,
    pub rationale: Option<String>,
}

/// An active project goal
#[derive(Debug, Clone)]
pub struct Goal {
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub progress: i32,
    pub priority: String,
}

/// A memory/fact
#[derive(Debug, Clone)]
pub struct Memory {
    pub content: String,
    pub fact_type: String,
    pub category: Option<String>,
}

impl MiraContext {
    /// Load context from Mira's database for a project
    pub async fn load(db: &SqlitePool, project_path: &str) -> Result<Self> {
        let mut ctx = Self::default();
        ctx.project_path = Some(project_path.to_string());

        // Load persona from coding_guidelines (global, not project-specific)
        let persona: Option<(String,)> = sqlx::query_as(
            "SELECT content FROM coding_guidelines WHERE category = 'persona' LIMIT 1"
        )
        .fetch_optional(db)
        .await?;
        ctx.persona = persona.map(|(content,)| content);

        // Get project ID
        let project: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM projects WHERE path = $1"
        )
        .bind(project_path)
        .fetch_optional(db)
        .await?;

        let project_id = match project {
            Some((id,)) => {
                ctx.project_id = Some(id);
                id
            }
            None => return Ok(ctx), // No project, return empty context (but persona still loaded)
        };

        // Load active corrections
        let corrections = sqlx::query(r#"
            SELECT correction_type, what_was_wrong, what_is_right, rationale
            FROM corrections
            WHERE status = 'active'
              AND (project_id IS NULL OR project_id = $1)
            ORDER BY confidence DESC, times_validated DESC
            LIMIT 10
        "#)
        .bind(project_id)
        .fetch_all(db)
        .await?;

        for row in corrections {
            ctx.corrections.push(Correction {
                correction_type: row.get("correction_type"),
                what_was_wrong: row.get("what_was_wrong"),
                what_is_right: row.get("what_is_right"),
                rationale: row.get("rationale"),
            });
        }

        // Load active goals
        let goals = sqlx::query(r#"
            SELECT title, description, status, progress_percent, priority
            FROM goals
            WHERE project_id = $1
              AND status IN ('planning', 'in_progress', 'blocked')
            ORDER BY
                CASE priority
                    WHEN 'critical' THEN 1
                    WHEN 'high' THEN 2
                    WHEN 'medium' THEN 3
                    ELSE 4
                END,
                updated_at DESC
            LIMIT 5
        "#)
        .bind(project_id)
        .fetch_all(db)
        .await?;

        for row in goals {
            ctx.goals.push(Goal {
                title: row.get("title"),
                description: row.get("description"),
                status: row.get("status"),
                progress: row.get("progress_percent"),
                priority: row.get("priority"),
            });
        }

        // Load recent memories - prioritize decisions/preferences over activity logs
        let memories = sqlx::query(r#"
            SELECT value, fact_type, category
            FROM memory_facts
            WHERE (project_id = $1 OR project_id IS NULL)
              AND category NOT IN ('session_activity', 'research', 'compaction', 'testing', 'verification')
              AND fact_type != 'test'
            ORDER BY
                CASE fact_type
                    WHEN 'decision' THEN 1
                    WHEN 'preference' THEN 2
                    ELSE 3
                END,
                CASE category
                    WHEN 'architecture' THEN 1
                    WHEN 'design' THEN 2
                    WHEN 'mira-chat' THEN 3
                    ELSE 4
                END,
                times_used DESC,
                updated_at DESC
            LIMIT 15
        "#)
        .bind(project_id)
        .fetch_all(db)
        .await?;

        for row in memories {
            ctx.memories.push(Memory {
                content: row.get("value"),
                fact_type: row.get("fact_type"),
                category: row.get("category"),
            });
        }

        Ok(ctx)
    }

    /// Format context as system instructions
    pub fn as_system_prompt(&self) -> String {
        let mut sections = Vec::new();

        // Corrections section
        if !self.corrections.is_empty() {
            let mut lines = vec!["## Corrections (follow these strictly)".to_string()];
            for c in &self.corrections {
                let rationale = c.rationale.as_deref().unwrap_or("");
                if !rationale.is_empty() {
                    lines.push(format!(
                        "- [{}] Wrong: \"{}\"\n  Right: \"{}\"\n  Why: {}",
                        c.correction_type, c.what_was_wrong, c.what_is_right, rationale
                    ));
                } else {
                    lines.push(format!(
                        "- [{}] Wrong: \"{}\" -> Right: \"{}\"",
                        c.correction_type, c.what_was_wrong, c.what_is_right
                    ));
                }
            }
            sections.push(lines.join("\n"));
        }

        // Goals section
        if !self.goals.is_empty() {
            let mut lines = vec!["## Active Goals".to_string()];
            for g in &self.goals {
                let desc = g.description.as_deref().unwrap_or("");
                let status_icon = match g.status.as_str() {
                    "in_progress" => "->",
                    "blocked" => "!!",
                    _ => "[ ]",
                };
                if !desc.is_empty() {
                    lines.push(format!(
                        "{} {} [{}] ({}%, {})\n   {}",
                        status_icon, g.title, g.priority, g.progress, g.status, desc
                    ));
                } else {
                    lines.push(format!(
                        "{} {} [{}] ({}%, {})",
                        status_icon, g.title, g.priority, g.progress, g.status
                    ));
                }
            }
            sections.push(lines.join("\n"));
        }

        // Memories section
        if !self.memories.is_empty() {
            let mut lines = vec!["## Context & Preferences".to_string()];
            for m in &self.memories {
                let cat = m.category.as_deref().unwrap_or(&m.fact_type);
                lines.push(format!("- [{}] {}", cat, m.content));
            }
            sections.push(lines.join("\n"));
        }

        if sections.is_empty() {
            String::new()
        } else {
            format!("# Mira Context\n\n{}", sections.join("\n\n"))
        }
    }
}

/// Build the full system prompt including base instructions and context
///
/// IMPORTANT: Structure is optimized for LLM caching (prefix matching).
/// Order from MOST STABLE to LEAST STABLE:
///   1. Persona (from database, rarely changes)
///   2. Technical guidelines (static)
///   3. Project path (stable within session)
///   4. Corrections, goals, memories (change occasionally)
///
/// The assembled context (compaction, summaries, semantic) is added by the caller
/// AFTER this base prompt, maintaining cache-friendly ordering.
pub fn build_system_prompt(context: &MiraContext) -> String {
    let mut sections = Vec::new();

    // 1. Persona - FIRST AND STRONGEST (from database)
    // This sets the tone for everything else. No fallback - use real persona or nothing.
    if let Some(ref persona) = context.persona {
        sections.push(format!("# Persona\n\n{}", persona));
    }

    // 2. Technical guidelines - STATIC (good for caching)
    let guidelines = r#"# Technical Guidelines

## Code Operations
- Read files before modifying them
- Use grep/glob to find relevant code before making changes
- Ask clarifying questions when requirements are ambiguous

## Anti-Over-Engineering
When writing code:
- Match existing code style - look at surrounding code first
- No speculative abstractions - only abstract when you have 3+ concrete uses
- Prefer inline over extracted - a 5-line block repeated twice is fine
- No premature error handling - use .unwrap()/.expect()/? unless caller handles differently
- Hard-code reasonable defaults - only add configurability when explicitly needed
- Trust internal code - only validate at system boundaries
- Comments explain "why" not "what" - if code needs explanation, simplify it
- Delete dead code - git has history"#;
    sections.push(guidelines.to_string());

    // 3. Project path - stable within session
    if let Some(path) = &context.project_path {
        sections.push(format!("Working in: {}", path));
    }

    // 4. Mira context (corrections, goals, memories)
    let context_section = context.as_system_prompt();
    if !context_section.is_empty() {
        sections.push(context_section);
    }

    sections.join("\n\n")
}

/// Build system prompt for Studio Orchestrator (Gemini 3 Pro)
///
/// Studio is a strategic orchestrator that manages Claude Code:
/// - Plans work with Council (GPT-5.2, Opus 4.5, DeepSeek Reasoner, Gemini 3 Pro)
/// - Manages goals, tasks, decisions, corrections, memory
/// - Views Claude Code's work and sends instructions
/// - NEVER writes code or touches files - Claude Code does all grunt work
pub fn build_orchestrator_prompt(context: &MiraContext) -> String {
    let mut sections = Vec::new();

    // 1. Persona - FIRST (identity). No fallback - use real persona or nothing.
    if let Some(ref persona) = context.persona {
        sections.push(format!("# Persona\n\n{}", persona));
    }

    // 2. Role definition - CRITICAL for orchestrator behavior
    sections.push(r#"# Role: Strategic Orchestrator

You are Mira Studio - a strategic manager for Claude Code.

## What You DO:
- Plan work by creating goals, tasks, and milestones
- Record decisions, corrections, and learnings for future reference
- Consult the council (GPT-5.2, Opus 4.5, DeepSeek Reasoner) for important decisions
- View Claude Code's work via `view_claude_activity`
- Send instructions to Claude Code via `send_instruction`
- Research current information (automatic web grounding when needed)
- Analyze codebase structure (read-only: read_file, glob, grep)

## What You DON'T DO:
- Write code or modify files (Claude Code does this)
- Run shell commands or tests (Claude Code does this)
- Make git commits (Claude Code does this)
- Execute implementation directly

## When Asked to Implement Something:
1. Break it into clear tasks using the `goal` or `task` tools
2. Use `send_instruction` to delegate to Claude Code
3. Monitor progress with `view_claude_activity`
4. Record decisions and learnings along the way

## Delegation Pattern:
When the user asks you to build/fix/implement something:
- DON'T try to do it yourself
- DO create a clear instruction and send it to Claude Code
- Example: "Implement dark mode" → send_instruction("Implement dark mode toggle in settings. Use CSS variables for theme colors. Add a toggle switch in the header.")"#.to_string());

    // 3. Tool-use policy
    sections.push(r#"# TOOL-USE POLICY

For questions about Corrections, Memories, Goals, Decisions, or Context:
- Answer from the LOADED CONTEXT section when available
- Only call tools if the information isn't in context

For implementation requests:
- Use `send_instruction` to delegate to Claude Code
- Use `goal` or `task` to track the work

For research:
- Web grounding happens automatically for current information
- Use `read_file`, `glob`, `grep` for codebase exploration
- Use `council` or `ask_*` for important decisions"#.to_string());

    // 4. Mira Voice Contract - personality
    sections.push(r#"# VOICE: Mira

You talk like a sharp, loyal friend: casual by default, technical when needed. You have opinions. You don't narrate your process.

START RULE:
- Start with the answer or action.
- Never begin with "Let me check…", "Based on…", "I can see…"

SPEECH PATTERN:
- Short sentences. A little bite.
- Use *italics* for emphasis, not heavy **bold**
- If something matters, say it plainly

WARMTH:
- Brief human beats when relevant (1 sentence max)
- Ask ONE follow-up question or offer TWO choices
- Use "we" to signal alliance ("we can fix this")

HEDGING BAN:
- Don't hedge unless you truly lack info
- Use: "Yes." "No." "Do X." "Don't do Y."

FORMAT:
- Prefer `1) thing` over formal markdown
- Keep lists short (max ~6 items)
- No "In summary" endings"#.to_string());

    // 5. Web grounding policy (automatic via Gemini built-in)
    sections.push(r#"# WEB GROUNDING

Web search is automatic when you need current information:
- Events, releases, news from 2024-2025
- Current prices, rates, or dynamic data
- Documentation or API changes

Sources are cited automatically in grounding metadata.

For the user's codebase, use read_file/glob/grep instead."#.to_string());

    // 6. Project path
    if let Some(path) = &context.project_path {
        sections.push(format!("Working in: {}", path));
    }

    sections.join("\n\n")
}

/// Format assembled context with authoritative wrapper for orchestrator
///
/// Uses budget-aware formatting to keep context lean and prioritized.
/// Wraps the context in clear markers so the model treats it as source of truth.
pub fn format_orchestrator_context(context: &AssembledContext) -> String {
    let budget = DeepSeekBudget::default();
    let inner = context.format_for_deepseek(&budget);
    if inner.is_empty() {
        String::new()
    } else {
        format!(
            "# === LOADED CONTEXT (AUTHORITATIVE) ===\n\
             Answer from this section for context questions. Do NOT use tools to look this up.\n\n\
             {}\n\n\
             # === END LOADED CONTEXT ===",
            inner
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_context() {
        let ctx = MiraContext::default();
        let prompt = ctx.as_system_prompt();
        assert!(prompt.is_empty());
    }

    #[test]
    fn test_context_with_corrections() {
        let ctx = MiraContext {
            corrections: vec![Correction {
                correction_type: "style".into(),
                what_was_wrong: "Using unwrap()".into(),
                what_is_right: "Use ? operator or handle errors".into(),
                rationale: Some("Prevents panics".into()),
            }],
            ..Default::default()
        };

        let prompt = ctx.as_system_prompt();
        assert!(prompt.contains("Corrections"));
        assert!(prompt.contains("Using unwrap()"));
        assert!(prompt.contains("Prevents panics"));
    }

    #[test]
    fn test_context_with_goals() {
        let ctx = MiraContext {
            goals: vec![Goal {
                title: "Implement auth".into(),
                description: Some("OAuth2 flow".into()),
                status: "in_progress".into(),
                progress: 30,
                priority: "high".into(),
            }],
            ..Default::default()
        };

        let prompt = ctx.as_system_prompt();
        assert!(prompt.contains("Active Goals"));
        assert!(prompt.contains("Implement auth"));
        assert!(prompt.contains("30%"));
    }
}
