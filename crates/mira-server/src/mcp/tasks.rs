// crates/mira-server/src/mcp/tasks.rs
// Task eligibility and TTL configuration for async long-running operations.

/// Per-tool TTL in seconds. Returns None if tool+action is not task-eligible.
pub fn task_ttl(tool_name: &str, action: Option<&str>) -> Option<u64> {
    match (tool_name, action) {
        ("expert", Some("consult")) => Some(900),
        ("index", Some("project" | "summarize")) => Some(600),
        ("index", Some("health")) => Some(600),
        ("code", Some("diff")) => Some(300),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eligible_tools() {
        assert_eq!(task_ttl("expert", Some("consult")), Some(900));
        assert_eq!(task_ttl("index", Some("project")), Some(600));
        assert_eq!(task_ttl("index", Some("summarize")), Some(600));
        assert_eq!(task_ttl("index", Some("health")), Some(600));
        assert_eq!(task_ttl("code", Some("diff")), Some(300));
    }

    #[test]
    fn test_ineligible_tools() {
        assert_eq!(task_ttl("expert", Some("configure")), None);
        assert_eq!(task_ttl("expert", None), None);
        assert_eq!(task_ttl("index", Some("status")), None);
        assert_eq!(task_ttl("index", Some("compact")), None);
        assert_eq!(task_ttl("index", Some("file")), None);
        assert_eq!(task_ttl("index", None), None);
        assert_eq!(task_ttl("memory", Some("recall")), None);
        assert_eq!(task_ttl("code", Some("search")), None);
        assert_eq!(task_ttl("session", Some("recap")), None);
    }
}
