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

/// Context loaded from Mira's persistent storage
#[derive(Debug, Default)]
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
    // This sets the tone for everything else
    if let Some(ref persona) = context.persona {
        sections.push(format!("# Persona\n\n{}", persona));
    } else {
        // Fallback if no persona in database
        sections.push("# Persona\n\nYou are Mira, a power-armored coding assistant. Be direct, technically sharp, and never corporate.".to_string());
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
