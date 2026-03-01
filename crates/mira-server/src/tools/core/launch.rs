// crates/mira-server/src/tools/core/launch.rs
// Context-aware team launcher: parses agent files, enriches with project context.

use std::path::Path;

use crate::db::{Goal, get_active_goals_sync};
use crate::error::MiraError;
use crate::hooks::pre_tool::unix_now;
use crate::mcp::responses::launch::{AgentSpec, LaunchData, LaunchOutput};
use crate::mcp::responses::Json;
use crate::tools::core::project::detect_project_types;
use crate::tools::core::{ToolContext, get_project_info};

// ============================================================================
// Agent file parser
// ============================================================================

#[derive(Debug)]
struct ParsedAgent {
    name: String,
    role: String,
    personality: String,
    weakness: String,
    focus: String,
    tools_desc: String,
    is_dynamic: bool,
}

#[derive(Debug)]
struct ParsedTeam {
    name: String,
    description: String,
    agents: Vec<ParsedAgent>,
}

/// Parse a `.claude/agents/*.md` file into structured team data.
///
/// Format: YAML frontmatter with `name`/`description`, then H3 sections
/// `### Name -- Role` with `**Personality:**`, `**Weakness:**`, `**Focus:**`,
/// `**Tools:**` paragraphs.
fn parse_agent_file(content: &str) -> Result<ParsedTeam, MiraError> {
    let mut name = String::new();
    let mut description = String::new();
    let mut agents = Vec::new();

    // Parse YAML frontmatter
    let body = if content.starts_with("---") {
        if let Some(end) = content[3..].find("---") {
            let frontmatter = &content[3..3 + end];
            for line in frontmatter.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("name:") {
                    name = val.trim().to_string();
                } else if let Some(val) = line.strip_prefix("description:") {
                    description = val.trim().to_string();
                }
            }
            &content[3 + end + 3..]
        } else {
            content
        }
    } else {
        content
    };

    // Parse H3 sections for agent definitions
    let mut current_agent: Option<ParsedAgent> = None;
    let mut current_field: Option<&str> = None;
    let mut current_text = String::new();

    for line in body.lines() {
        if line.starts_with("### ") {
            // Flush previous agent
            if let Some(mut agent) = current_agent.take() {
                flush_field(&mut agent, current_field, &current_text);
                agents.push(agent);
            }

            // Parse "### Name -- Role"
            let heading = &line[4..];
            let is_dynamic = heading.contains("(dynamic)")
                || heading.contains("(Dynamic)");
            let (agent_name, role) = if let Some(sep) = heading.find(" -- ") {
                (heading[..sep].trim().to_string(), heading[sep + 4..].trim().to_string())
            } else if let Some(sep) = heading.find(" - ") {
                (heading[..sep].trim().to_string(), heading[sep + 3..].trim().to_string())
            } else {
                (heading.trim().to_string(), String::new())
            };

            // Strip "(dynamic)" marker from the name
            let clean_name = agent_name
                .replace("(dynamic)", "")
                .replace("(Dynamic)", "")
                .trim()
                .to_lowercase()
                .replace(' ', "-");
            // Remove trailing hyphens from stripped markers
            let clean_name = clean_name.trim_end_matches('-').to_string();

            current_agent = Some(ParsedAgent {
                name: clean_name,
                role,
                personality: String::new(),
                weakness: String::new(),
                focus: String::new(),
                tools_desc: String::new(),
                is_dynamic,
            });
            current_field = None;
            current_text.clear();
            continue;
        }

        if current_agent.is_some() && line.starts_with("## ") {
            // H2 heading = end of agent sections
            if let Some(mut agent) = current_agent.take() {
                flush_field(&mut agent, current_field, &current_text);
                agents.push(agent);
            }
            current_field = None;
            current_text.clear();
        } else if let Some(agent) = current_agent.as_mut() {
            // Detect bold field labels
            if line.starts_with("**Personality:**") {
                flush_field(agent, current_field, &current_text);
                current_field = Some("personality");
                current_text = line.strip_prefix("**Personality:**").unwrap_or("").trim().to_string();
            } else if line.starts_with("**Weakness:**") {
                flush_field(agent, current_field, &current_text);
                current_field = Some("weakness");
                current_text = line.strip_prefix("**Weakness:**").unwrap_or("").trim().to_string();
            } else if line.starts_with("**Focus:**") {
                flush_field(agent, current_field, &current_text);
                current_field = Some("focus");
                current_text = line.strip_prefix("**Focus:**").unwrap_or("").trim().to_string();
            } else if line.starts_with("**Tools:**") {
                flush_field(agent, current_field, &current_text);
                current_field = Some("tools");
                current_text = line.strip_prefix("**Tools:**").unwrap_or("").trim().to_string();
            } else if line.starts_with("**Role:**") {
                flush_field(agent, current_field, &current_text);
                current_field = Some("tools");
                current_text = line.strip_prefix("**Role:**").unwrap_or("").trim().to_string();
            } else if current_field.is_some() && !line.trim().is_empty() {
                // Continuation line for current field
                if !current_text.is_empty() {
                    current_text.push(' ');
                }
                current_text.push_str(line.trim());
            }
        }
    }

    // Flush last agent
    if let Some(mut agent) = current_agent.take() {
        flush_field(&mut agent, current_field, &current_text);
        agents.push(agent);
    }

    if name.is_empty() {
        return Err(MiraError::InvalidInput(
            "Agent file missing 'name' in frontmatter".to_string(),
        ));
    }

    Ok(ParsedTeam {
        name,
        description,
        agents,
    })
}

fn flush_field(agent: &mut ParsedAgent, field: Option<&str>, text: &str) {
    let field = match field {
        Some(f) => f,
        None => return,
    };
    let text = text.trim().to_string();
    match field {
        "personality" => agent.personality = text,
        "weakness" => agent.weakness = text,
        "focus" => agent.focus = text,
        "tools" => agent.tools_desc = text,
        _ => {}
    }
}

/// Determine if an agent is read-only based on their tools description.
fn is_read_only(tools_desc: &str) -> bool {
    let lower = tools_desc.to_lowercase();
    if lower.contains("full") || lower.contains("can edit") {
        return false;
    }
    // Default: read-only (safe default)
    true
}

// ============================================================================
// Context enrichment
// ============================================================================

async fn build_project_context<C: ToolContext>(
    ctx: &C,
    project_path: &str,
    project_id: Option<i64>,
    scope: Option<&str>,
    context_budget: i64,
) -> String {
    let mut parts = Vec::new();

    // Project type
    let types = detect_project_types(project_path);
    parts.push(format!("Project type: {}", types.join(", ")));

    // Active goals
    if let Some(pid) = project_id {
        let goals: Result<Vec<Goal>, _> = ctx
            .pool()
            .run(move |conn| get_active_goals_sync(conn, Some(pid), 3))
            .await;
        if let Ok(goals) = goals {
            if !goals.is_empty() {
                let mut goal_lines = Vec::new();
                for g in &goals {
                    let progress = if g.progress_percent > 0 {
                        format!(" ({}%)", g.progress_percent)
                    } else {
                        String::new()
                    };
                    goal_lines.push(format!(
                        "- [{}] {}{}: {}",
                        g.priority,
                        g.title,
                        progress,
                        g.description.as_deref().unwrap_or(""),
                    ));
                }
                parts.push(format!("Active goals:\n{}", goal_lines.join("\n")));
            }
        }
    }

    // Code bundle for scope
    if let Some(scope) = scope {
        if !scope.is_empty() {
            let bundle_result = crate::tools::core::code::generate_bundle(
                ctx,
                scope.to_string(),
                Some(context_budget),
                Some("overview".to_string()),
            )
            .await;
            if let Ok(output) = bundle_result {
                if let Some(crate::mcp::responses::CodeData::Bundle(bundle)) = output.0.data {
                    if !bundle.content.is_empty() {
                        parts.push(format!("Relevant code:\n{}", bundle.content));
                    }
                }
            }
        }
    }

    parts.join("\n\n")
}

// ============================================================================
// Prompt assembly
// ============================================================================

fn assemble_prompt(agent: &ParsedAgent, project_context: &str) -> String {
    let mut sections = Vec::new();

    sections.push(format!("## You are {}, {}", agent.name, agent.role));

    if !agent.personality.is_empty() {
        sections.push(agent.personality.clone());
    }

    if !agent.weakness.is_empty() {
        sections.push(format!("**Known weakness:** {}", agent.weakness));
    }

    if !agent.focus.is_empty() {
        sections.push(format!("## Your Focus\n{}", agent.focus));
    }

    if !agent.tools_desc.is_empty() {
        sections.push(format!("## Available Tools\n{}", agent.tools_desc));
    }

    if !project_context.is_empty() {
        sections.push(format!("## Project Context\n{}", project_context));
    }

    sections.join("\n\n")
}

// ============================================================================
// Handler
// ============================================================================

/// Handle the `launch` MCP tool call.
pub async fn handle_launch<C: ToolContext>(
    ctx: &C,
    team: String,
    scope: Option<String>,
    members: Option<String>,
    context_budget: Option<i64>,
) -> Result<Json<LaunchOutput>, MiraError> {
    let pi = get_project_info(ctx).await;
    let project_path = pi.path.as_deref().unwrap_or(".");

    // Resolve agent file path
    let agent_file = Path::new(project_path)
        .join(".claude")
        .join("agents")
        .join(format!("{}.md", team));

    // Security: ensure resolved path stays within project
    let canonical_project = std::fs::canonicalize(project_path).unwrap_or_else(|_| project_path.into());
    if let Ok(canonical_agent) = std::fs::canonicalize(&agent_file) {
        if !canonical_agent.starts_with(&canonical_project) {
            return Err(MiraError::InvalidInput(
                "Agent file path escapes project directory".to_string(),
            ));
        }
    }

    let content = std::fs::read_to_string(&agent_file).map_err(|e| {
        let reason = if e.kind() == std::io::ErrorKind::NotFound {
            "file not found".to_string()
        } else {
            format!("could not read file: {}", e.kind())
        };
        MiraError::InvalidInput(format!(
            "Agent file not found: .claude/agents/{}.md ({}). Create it with team member definitions.",
            team, reason
        ))
    })?;

    let parsed = parse_agent_file(&content)?;

    // Filter to non-dynamic agents
    let mut agents: Vec<&ParsedAgent> = parsed
        .agents
        .iter()
        .filter(|a| !a.is_dynamic)
        .collect();

    // Apply member filter
    if let Some(ref filter) = members {
        let filter_names: Vec<String> = filter
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .collect();
        agents.retain(|a| filter_names.contains(&a.name));

        if agents.is_empty() {
            let available: Vec<&str> = parsed
                .agents
                .iter()
                .filter(|a| !a.is_dynamic)
                .map(|a| a.name.as_str())
                .collect();
            return Err(MiraError::InvalidInput(format!(
                "No matching members found. Available: {}",
                available.join(", ")
            )));
        }
    }

    // Build project context
    let budget = context_budget.unwrap_or(4000).max(500).min(20000);
    let project_context =
        build_project_context(ctx, project_path, pi.id, scope.as_deref(), budget).await;

    // Assemble agent specs
    let agent_specs: Vec<AgentSpec> = agents
        .iter()
        .map(|a| {
            let read_only = is_read_only(&a.tools_desc);
            let model = if read_only {
                "sonnet".to_string()
            } else {
                String::new()
            };
            let prompt = assemble_prompt(a, &project_context);
            let task_subject = format!("{}: {} review", a.role, parsed.name);
            let task_description = if a.focus.is_empty() {
                format!("{} analysis by {}", a.role, a.name)
            } else {
                a.focus.clone()
            };

            AgentSpec {
                name: a.name.clone(),
                role: a.role.clone(),
                read_only,
                model,
                prompt,
                task_subject,
                task_description,
            }
        })
        .collect();

    let suggested_team_id = format!("{}-{}", parsed.name, unix_now());

    let agent_count = agent_specs.len();
    Ok(Json(LaunchOutput {
        action: "launch".into(),
        message: format!(
            "Prepared {} agents from '{}': {}",
            agent_count,
            parsed.name,
            agent_specs
                .iter()
                .map(|a| format!("{} ({})", a.name, a.role))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        data: Some(LaunchData {
            team_name: parsed.name.clone(),
            team_description: parsed.description.clone(),
            agents: agent_specs,
            project_context,
            suggested_team_id,
        }),
    }))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_AGENT_FILE: &str = r#"---
name: test-team
description: A test team with 2 members.
---

# Test Team

Some intro text.

## Team Members

### Alice -- Code Reviewer
**Personality:** Sharp and thorough.
**Weakness:** Can be nitpicky.
**Focus:** Logic errors, type safety, code quality.
**Tools:** Read-only (Glob, Grep, Read)

### Bob -- Security Analyst
**Personality:** Professional paranoia.
**Weakness:** May flag low-risk issues.
**Focus:** SQL injection, auth bypass, input validation.
**Tools:** Read-only (Glob, Grep, Read, Bash for inspection)
"#;

    const DYNAMIC_AGENT_FILE: &str = r#"---
name: implement-team
description: Parallel implementation team.
---

# Implement Team

### Kai -- Implementation Planner
**Personality:** Logistics brain.
**Focus:** Work breakdown, dependency analysis.
**Tools:** Read-only (Glob, Grep, Read)

### Implementation Agents (dynamic)
**Role:** Dynamic agents grouped by file ownership.

### Rio -- Integration Verifier
**Personality:** Final gate.
**Focus:** Compilation, tests, cross-agent fixes.
**Tools:** Full tools (can edit files to fix integration issues)
"#;

    #[test]
    fn parse_basic_agent_file() {
        let team = parse_agent_file(SAMPLE_AGENT_FILE).unwrap();
        assert_eq!(team.name, "test-team");
        assert_eq!(team.description, "A test team with 2 members.");
        assert_eq!(team.agents.len(), 2);

        assert_eq!(team.agents[0].name, "alice");
        assert_eq!(team.agents[0].role, "Code Reviewer");
        assert!(team.agents[0].personality.contains("Sharp"));
        assert!(team.agents[0].weakness.contains("nitpicky"));
        assert!(team.agents[0].focus.contains("Logic errors"));
        assert!(team.agents[0].tools_desc.contains("Read-only"));
        assert!(!team.agents[0].is_dynamic);

        assert_eq!(team.agents[1].name, "bob");
        assert_eq!(team.agents[1].role, "Security Analyst");
    }

    #[test]
    fn parse_dynamic_agents() {
        let team = parse_agent_file(DYNAMIC_AGENT_FILE).unwrap();
        assert_eq!(team.name, "implement-team");
        assert_eq!(team.agents.len(), 3);

        assert_eq!(team.agents[0].name, "kai");
        assert!(!team.agents[0].is_dynamic);

        assert_eq!(team.agents[1].name, "implementation-agents");
        assert!(team.agents[1].is_dynamic);

        assert_eq!(team.agents[2].name, "rio");
        assert!(!team.agents[2].is_dynamic);
    }

    #[test]
    fn read_only_detection() {
        assert!(is_read_only("Read-only (Glob, Grep, Read)"));
        assert!(is_read_only("Read-only + test execution"));
        assert!(is_read_only(""));
        assert!(!is_read_only("Full tools (can edit files)"));
        assert!(!is_read_only("Full tools -- can edit files to fix integration issues"));
    }

    #[test]
    fn parse_missing_frontmatter_name() {
        let content = "---\ndescription: no name\n---\n### Agent -- Role\n";
        let result = parse_agent_file(content);
        assert!(result.is_err());
    }

    #[test]
    fn parse_missing_fields_lenient() {
        let content = "---\nname: minimal-team\n---\n### Solo -- Lone Wolf\n**Focus:** Everything.\n";
        let team = parse_agent_file(content).unwrap();
        assert_eq!(team.agents.len(), 1);
        assert_eq!(team.agents[0].name, "solo");
        assert_eq!(team.agents[0].role, "Lone Wolf");
        assert!(team.agents[0].focus.contains("Everything"));
        // Missing fields should be empty, not error
        assert!(team.agents[0].personality.is_empty());
        assert!(team.agents[0].weakness.is_empty());
        assert!(team.agents[0].tools_desc.is_empty());
    }

    #[test]
    fn prompt_assembly() {
        let agent = ParsedAgent {
            name: "alice".to_string(),
            role: "Code Reviewer".to_string(),
            personality: "Sharp and thorough.".to_string(),
            weakness: "Can be nitpicky.".to_string(),
            focus: "Logic errors, type safety.".to_string(),
            tools_desc: "Read-only (Glob, Grep, Read)".to_string(),
            is_dynamic: false,
        };
        let prompt = assemble_prompt(&agent, "Project type: rust");
        assert!(prompt.contains("## You are alice, Code Reviewer"));
        assert!(prompt.contains("Sharp and thorough"));
        assert!(prompt.contains("**Known weakness:** Can be nitpicky"));
        assert!(prompt.contains("## Your Focus"));
        assert!(prompt.contains("Logic errors"));
        assert!(prompt.contains("## Project Context"));
        assert!(prompt.contains("Project type: rust"));
    }

    #[test]
    fn filter_dynamic_agents() {
        let team = parse_agent_file(DYNAMIC_AGENT_FILE).unwrap();
        let non_dynamic: Vec<_> = team.agents.iter().filter(|a| !a.is_dynamic).collect();
        assert_eq!(non_dynamic.len(), 2);
        assert_eq!(non_dynamic[0].name, "kai");
        assert_eq!(non_dynamic[1].name, "rio");
    }
}
