// crates/mira-server/src/mcp/tasks.rs
// Task eligibility and TTL configuration for async long-running operations.

/// Function names in Rhai scripts that indicate long-running operations.
const LONG_RUNNING_FUNCTIONS: &[&str] = &["index_project", "diff"];

/// Per-tool TTL in seconds. Returns None if tool+action is not task-eligible.
///
/// For the `run` tool, we scan the script code for known long-running function
/// calls (index_project, diff) and return the maximum TTL if any are found.
pub fn task_ttl(tool_name: &str, action: Option<&str>, code: Option<&str>) -> Option<u64> {
    match (tool_name, action) {
        ("index", Some("project")) => Some(600),
        ("diff", _) => Some(300),
        ("run", _) => run_script_ttl(code.unwrap_or("")),
        _ => None,
    }
}

/// Scan script code for long-running function calls and return appropriate TTL.
fn run_script_ttl(code: &str) -> Option<u64> {
    let mut max_ttl: Option<u64> = None;
    for name in LONG_RUNNING_FUNCTIONS {
        if code.contains(name) {
            let ttl = match *name {
                "index_project" => 600,
                "diff" => 300,
                _ => 300,
            };
            max_ttl = Some(max_ttl.map_or(ttl, |current: u64| current.max(ttl)));
        }
    }
    max_ttl
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eligible_tools() {
        assert_eq!(task_ttl("index", Some("project"), None), Some(600));
        assert_eq!(task_ttl("diff", None, None), Some(300));
    }

    #[test]
    fn test_ineligible_tools() {
        assert_eq!(task_ttl("index", Some("summarize"), None), None);
        assert_eq!(task_ttl("index", Some("health"), None), None);
        assert_eq!(task_ttl("index", Some("status"), None), None);
        assert_eq!(task_ttl("index", Some("compact"), None), None);
        assert_eq!(task_ttl("index", Some("file"), None), None);
        assert_eq!(task_ttl("index", None, None), None);
        assert_eq!(task_ttl("memory", Some("recall"), None), None);
        assert_eq!(task_ttl("code", Some("search"), None), None);
        assert_eq!(task_ttl("session", Some("recap"), None), None);
    }

    #[test]
    fn test_run_with_long_running_script() {
        assert_eq!(task_ttl("run", None, Some("index_project()")), Some(600));
        assert_eq!(
            task_ttl("run", None, Some("let r = diff(); format(r)")),
            Some(300)
        );
        assert_eq!(
            task_ttl(
                "run",
                None,
                Some("index_project(); let d = diff(); d")
            ),
            Some(600) // max of 600 and 300
        );
    }

    #[test]
    fn test_run_with_simple_script() {
        assert_eq!(task_ttl("run", None, Some("search(\"hello\")")), None);
        assert_eq!(task_ttl("run", None, Some("help()")), None);
        assert_eq!(task_ttl("run", None, Some("42")), None);
    }
}
