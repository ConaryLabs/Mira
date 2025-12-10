// backend/src/cli/commands/builtin.rs
// Built-in CLI commands for session management, code review, and search

use serde::{Deserialize, Serialize};

/// Built-in CLI commands
#[derive(Debug, Clone)]
pub enum BuiltinCommand {
    /// Resume a session by name, ID, or show picker
    Resume {
        /// Target session (name or ID), None shows picker
        target: Option<String>,
        /// Resume the most recent session
        last: bool,
    },
    /// Review code changes
    Review {
        /// What to review
        target: ReviewTarget,
    },
    /// Rename current session
    Rename {
        /// New name for the session
        name: String,
    },
    /// List and manage background agents (Codex sessions)
    Agents {
        /// Subcommand (list, cancel, etc.)
        action: AgentAction,
    },
    /// Search the web
    Search {
        /// Search query
        query: String,
        /// Search type (general, docs, stackoverflow, github)
        search_type: Option<String>,
        /// Number of results
        num_results: Option<usize>,
    },
    /// Show status of current session
    Status,
}

/// What to review in code review
#[derive(Debug, Clone, Default)]
pub enum ReviewTarget {
    /// Review uncommitted changes (git diff HEAD)
    #[default]
    Uncommitted,
    /// Review changes against a base branch
    Branch {
        base: String,
    },
    /// Review a specific commit
    Commit {
        hash: String,
    },
    /// Review staged changes only
    Staged,
}

/// Agent management actions
#[derive(Debug, Clone, Default)]
pub enum AgentAction {
    /// List active background agents
    #[default]
    List,
    /// Cancel a running agent
    Cancel {
        agent_id: String,
    },
    /// Show details of an agent
    Show {
        agent_id: String,
    },
}

/// Result of a web search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Information about a running Codex session (background agent)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub task: String,
    pub status: String,
    pub started_at: i64,
    pub tokens_used: i64,
    pub cost_usd: f64,
}

impl BuiltinCommand {
    /// Parse a command string into a BuiltinCommand
    /// Returns None if not a builtin command
    pub fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if !input.starts_with('/') {
            return None;
        }

        let parts: Vec<&str> = input[1..].splitn(2, ' ').collect();
        let cmd = parts[0].to_lowercase();
        let args = parts.get(1).map(|s| s.trim()).unwrap_or("");

        match cmd.as_str() {
            "resume" | "r" => Some(Self::parse_resume(args)),
            "review" => Some(Self::parse_review(args)),
            "rename" => Self::parse_rename(args),
            "agents" | "agent" | "bg" => Some(Self::parse_agents(args)),
            "search" | "web" => Self::parse_search(args),
            "status" | "stat" => Some(Self::Status),
            _ => None,
        }
    }

    fn parse_resume(args: &str) -> Self {
        if args.is_empty() {
            return Self::Resume {
                target: None,
                last: false,
            };
        }

        if args == "--last" || args == "-l" {
            return Self::Resume {
                target: None,
                last: true,
            };
        }

        // Check for --last flag anywhere in args
        let parts: Vec<&str> = args.split_whitespace().collect();
        let mut last = false;
        let mut target = None;

        for part in parts {
            if part == "--last" || part == "-l" {
                last = true;
            } else if target.is_none() {
                target = Some(part.to_string());
            }
        }

        Self::Resume { target, last }
    }

    fn parse_review(args: &str) -> Self {
        if args.is_empty() {
            return Self::Review {
                target: ReviewTarget::Uncommitted,
            };
        }

        let parts: Vec<&str> = args.split_whitespace().collect();

        // Check for flags
        let mut i = 0;
        while i < parts.len() {
            match parts[i] {
                "--branch" | "-b" => {
                    let base = parts.get(i + 1).unwrap_or(&"main").to_string();
                    return Self::Review {
                        target: ReviewTarget::Branch { base },
                    };
                }
                "--commit" | "-c" => {
                    let hash = parts.get(i + 1).unwrap_or(&"HEAD").to_string();
                    return Self::Review {
                        target: ReviewTarget::Commit { hash },
                    };
                }
                "--staged" | "-s" => {
                    return Self::Review {
                        target: ReviewTarget::Staged,
                    };
                }
                _ => {}
            }
            i += 1;
        }

        // If no flags, treat first arg as branch name
        Self::Review {
            target: ReviewTarget::Branch {
                base: parts[0].to_string(),
            },
        }
    }

    fn parse_rename(args: &str) -> Option<Self> {
        if args.is_empty() {
            return None; // Name required
        }
        Some(Self::Rename {
            name: args.to_string(),
        })
    }

    fn parse_agents(args: &str) -> Self {
        if args.is_empty() {
            return Self::Agents {
                action: AgentAction::List,
            };
        }

        let parts: Vec<&str> = args.split_whitespace().collect();
        let subcmd = parts[0].to_lowercase();

        match subcmd.as_str() {
            "list" | "ls" => Self::Agents {
                action: AgentAction::List,
            },
            "cancel" | "kill" | "stop" => {
                let agent_id = parts.get(1).unwrap_or(&"").to_string();
                Self::Agents {
                    action: AgentAction::Cancel { agent_id },
                }
            }
            "show" | "info" => {
                let agent_id = parts.get(1).unwrap_or(&"").to_string();
                Self::Agents {
                    action: AgentAction::Show { agent_id },
                }
            }
            // Default: treat as agent ID to show
            _ => Self::Agents {
                action: AgentAction::Show {
                    agent_id: subcmd.to_string(),
                },
            },
        }
    }

    fn parse_search(args: &str) -> Option<Self> {
        if args.is_empty() {
            return None; // Query required
        }

        let mut query_parts = Vec::new();
        let mut search_type = None;
        let mut num_results = None;

        let parts: Vec<&str> = args.split_whitespace().collect();
        let mut i = 0;

        while i < parts.len() {
            match parts[i] {
                "--type" | "-t" => {
                    if let Some(t) = parts.get(i + 1) {
                        search_type = Some(t.to_string());
                        i += 1;
                    }
                }
                "--num" | "-n" => {
                    if let Some(n) = parts.get(i + 1) {
                        num_results = n.parse().ok();
                        i += 1;
                    }
                }
                _ => {
                    // Not a flag, add to query
                    query_parts.push(parts[i]);
                }
            }
            i += 1;
        }

        if query_parts.is_empty() {
            return None;
        }

        Some(Self::Search {
            query: query_parts.join(" "),
            search_type,
            num_results,
        })
    }

    /// Get help text for builtin commands
    pub fn help() -> &'static str {
        r#"
  Session Commands:
    /resume [name|id]     - Resume a session (shows picker if no arg)
    /resume --last        - Resume most recent session
    /rename <name>        - Rename current session
    /status               - Show current session status

  Code Review:
    /review               - Review uncommitted changes
    /review --branch main - Review against a base branch
    /review --commit abc  - Review a specific commit
    /review --staged      - Review staged changes only

  Background Agents:
    /agents               - List running background agents
    /agents cancel <id>   - Cancel a running agent
    /agents show <id>     - Show agent details

  Web Search:
    /search <query>       - Search the web
    /search -t docs <q>   - Search documentation
    /search -t github <q> - Search GitHub
    /search -n 10 <q>     - Get 10 results
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_resume() {
        // No args
        let cmd = BuiltinCommand::parse("/resume").unwrap();
        assert!(matches!(cmd, BuiltinCommand::Resume { target: None, last: false }));

        // With --last
        let cmd = BuiltinCommand::parse("/resume --last").unwrap();
        assert!(matches!(cmd, BuiltinCommand::Resume { target: None, last: true }));

        // With session ID
        let cmd = BuiltinCommand::parse("/resume abc123").unwrap();
        assert!(matches!(cmd, BuiltinCommand::Resume { target: Some(t), last: false } if t == "abc123"));

        // Short form
        let cmd = BuiltinCommand::parse("/r -l").unwrap();
        assert!(matches!(cmd, BuiltinCommand::Resume { target: None, last: true }));
    }

    #[test]
    fn test_parse_review() {
        // Default (uncommitted)
        let cmd = BuiltinCommand::parse("/review").unwrap();
        assert!(matches!(cmd, BuiltinCommand::Review { target: ReviewTarget::Uncommitted }));

        // Branch
        let cmd = BuiltinCommand::parse("/review --branch develop").unwrap();
        assert!(matches!(cmd, BuiltinCommand::Review { target: ReviewTarget::Branch { base } } if base == "develop"));

        // Staged
        let cmd = BuiltinCommand::parse("/review --staged").unwrap();
        assert!(matches!(cmd, BuiltinCommand::Review { target: ReviewTarget::Staged }));
    }

    #[test]
    fn test_parse_rename() {
        // With name
        let cmd = BuiltinCommand::parse("/rename My Session").unwrap();
        assert!(matches!(cmd, BuiltinCommand::Rename { name } if name == "My Session"));

        // No name (invalid)
        let cmd = BuiltinCommand::parse("/rename");
        assert!(cmd.is_none());
    }

    #[test]
    fn test_parse_search() {
        // Basic search
        let cmd = BuiltinCommand::parse("/search rust async").unwrap();
        assert!(matches!(cmd, BuiltinCommand::Search { query, search_type: None, .. } if query == "rust async"));

        // With type
        let cmd = BuiltinCommand::parse("/search -t docs tokio runtime").unwrap();
        assert!(matches!(cmd, BuiltinCommand::Search { query, search_type: Some(t), .. } if query == "tokio runtime" && t == "docs"));

        // No query (invalid)
        let cmd = BuiltinCommand::parse("/search");
        assert!(cmd.is_none());
    }

    #[test]
    fn test_parse_agents() {
        // List
        let cmd = BuiltinCommand::parse("/agents").unwrap();
        assert!(matches!(cmd, BuiltinCommand::Agents { action: AgentAction::List }));

        // Cancel
        let cmd = BuiltinCommand::parse("/agents cancel abc123").unwrap();
        assert!(matches!(cmd, BuiltinCommand::Agents { action: AgentAction::Cancel { agent_id } } if agent_id == "abc123"));
    }
}
