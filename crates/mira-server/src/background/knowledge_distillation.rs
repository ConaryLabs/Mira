// crates/mira-server/src/background/knowledge_distillation.rs
// Knowledge Distillation for Team Intelligence (Phase 7)
//
// When a team session ends, extracts key findings/decisions from the team's work
// and stores them as team-scoped memories with category="distilled".
// Uses heuristic summarization (no LLM calls) for speed.

use crate::db::pool::DatabasePool;
use crate::db::{StoreMemoryParams, store_memory_sync};
use std::collections::HashMap;
use std::sync::Arc;

/// Maximum number of distilled memories to store per team per distillation run
const MAX_DISTILLED_MEMORIES: usize = 10;

/// Minimum number of memories to bother distilling (skip trivial sessions)
const MIN_MEMORIES_FOR_DISTILLATION: usize = 2;

/// A distilled finding from a team session
#[derive(Debug, Clone)]
pub struct DistilledFinding {
    pub category: String,
    pub content: String,
    pub source_count: usize,
}

/// Result of a distillation run
#[derive(Debug, Clone)]
pub struct DistillationResult {
    pub team_name: String,
    pub findings: Vec<DistilledFinding>,
    pub total_memories_processed: usize,
    pub total_files_touched: usize,
}

/// Run knowledge distillation for a team session.
///
/// Gathers team session data (memories, files modified), groups and deduplicates,
/// then stores distilled summaries as team-scoped memories.
///
/// Returns the distillation result, or None if insufficient data.
pub async fn distill_team_session(
    pool: &Arc<DatabasePool>,
    team_id: i64,
    project_id: Option<i64>,
) -> Result<Option<DistillationResult>, String> {
    pool.run(move |conn| distill_team_session_sync(conn, team_id, project_id))
        .await
}

/// Synchronous distillation logic for use within pool.interact().
pub fn distill_team_session_sync(
    conn: &rusqlite::Connection,
    team_id: i64,
    project_id: Option<i64>,
) -> Result<Option<DistillationResult>, anyhow::Error> {
    // Get team name
    let team_name: String = conn
        .query_row(
            "SELECT name FROM teams WHERE id = ?1",
            rusqlite::params![team_id],
            |row| row.get(0),
        )
        .map_err(|e| anyhow::anyhow!("Team not found: {}", e))?;

    // 1. Gather team memories created during this team's active sessions
    let team_memories = gather_team_memories(conn, team_id, project_id);

    // 2. Gather files modified by team members
    let team_files = gather_team_files(conn, team_id);
    let total_files = team_files.len();

    // Check minimum threshold
    if team_memories.len() < MIN_MEMORIES_FOR_DISTILLATION && team_files.is_empty() {
        return Ok(None);
    }

    // 3. Group memories by category and extract key findings
    let grouped = group_memories_by_category(&team_memories);

    // 4. Deduplicate and distill
    let mut findings = distill_findings(&grouped);

    // 5. Add a files summary if files were touched
    if !team_files.is_empty() {
        let file_summary = summarize_files(&team_files);
        findings.push(DistilledFinding {
            category: "files".to_string(),
            content: file_summary,
            source_count: team_files.len(),
        });
    }

    // 6. Truncate to max
    findings.truncate(MAX_DISTILLED_MEMORIES);

    let total_memories_processed = team_memories.len();

    // 7. Store distilled findings as team-scoped memories
    for finding in &findings {
        let key = format!(
            "distilled:{}:{}:{}",
            team_id,
            finding.category,
            hash_content(&finding.content)
        );
        if let Err(e) = store_memory_sync(
            conn,
            StoreMemoryParams {
                project_id,
                key: Some(&key),
                content: &finding.content,
                fact_type: "distilled",
                category: Some("distilled"),
                confidence: 0.7,
                session_id: None,
                user_id: None,
                scope: "team",
                branch: None,
                team_id: Some(team_id),
            },
        ) {
            tracing::warn!("Failed to store distilled finding: {}", e);
        }
    }

    Ok(Some(DistillationResult {
        team_name,
        findings,
        total_memories_processed,
        total_files_touched: total_files,
    }))
}

/// Raw memory data gathered from team sessions
struct RawMemory {
    content: String,
    fact_type: String,
    category: Option<String>,
}

/// Gather memories created by team members during their active sessions.
fn gather_team_memories(
    conn: &rusqlite::Connection,
    team_id: i64,
    project_id: Option<i64>,
) -> Vec<RawMemory> {
    let sql = r#"
        SELECT mf.content, mf.fact_type, mf.category
        FROM memory_facts mf
        WHERE (mf.project_id = ?1 OR mf.project_id IS NULL)
          AND (
            mf.team_id = ?2
            OR mf.last_session_id IN (
              SELECT session_id FROM team_sessions WHERE team_id = ?2
            )
          )
          AND mf.fact_type NOT IN ('health', 'persona', 'distilled')
        ORDER BY mf.updated_at DESC
        LIMIT 100
    "#;

    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map(rusqlite::params![project_id, team_id], |row| {
        Ok(RawMemory {
            content: row.get(0)?,
            fact_type: row.get(1)?,
            category: row.get(2)?,
        })
    })
    .map(|rows| rows.filter_map(crate::db::log_and_discard).collect())
    .unwrap_or_default()
}

/// Gather files modified by all team members.
fn gather_team_files(conn: &rusqlite::Connection, team_id: i64) -> Vec<(String, String)> {
    let sql = r#"
        SELECT DISTINCT tfo.file_path, tfo.member_name
        FROM team_file_ownership tfo
        WHERE tfo.team_id = ?1
        ORDER BY tfo.timestamp DESC
        LIMIT 50
    "#;

    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map(rusqlite::params![team_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })
    .map(|rows| rows.filter_map(crate::db::log_and_discard).collect())
    .unwrap_or_default()
}

/// Group memories by their effective category.
fn group_memories_by_category(memories: &[RawMemory]) -> HashMap<String, Vec<&RawMemory>> {
    let mut groups: HashMap<String, Vec<&RawMemory>> = HashMap::new();

    for mem in memories {
        let category = mem.category.as_deref().unwrap_or(&mem.fact_type);
        groups.entry(category.to_string()).or_default().push(mem);
    }

    groups
}

/// Distill grouped memories into findings, deduplicating similar content.
fn distill_findings(grouped: &HashMap<String, Vec<&RawMemory>>) -> Vec<DistilledFinding> {
    let mut findings = Vec::new();

    // Priority order for categories
    let priority_categories = [
        "decision",
        "preference",
        "pattern",
        "convention",
        "context",
        "general",
    ];

    // Process priority categories first
    for cat in &priority_categories {
        if let Some(memories) = grouped.get(*cat)
            && let Some(finding) = distill_category(cat, memories)
        {
            findings.push(finding);
        }
    }

    // Then any remaining categories
    for (cat, memories) in grouped {
        if priority_categories.contains(&cat.as_str()) {
            continue;
        }
        if let Some(finding) = distill_category(cat, memories) {
            findings.push(finding);
        }
    }

    findings
}

/// Distill a single category of memories into a finding.
/// Deduplicates by checking content similarity (simple substring overlap).
fn distill_category(category: &str, memories: &[&RawMemory]) -> Option<DistilledFinding> {
    if memories.is_empty() {
        return None;
    }

    // Deduplicate: keep only memories with sufficiently distinct content
    let mut unique_contents: Vec<&str> = Vec::new();
    for mem in memories {
        let dominated = unique_contents
            .iter()
            .any(|existing| content_similar(existing, &mem.content));
        if !dominated {
            unique_contents.push(&mem.content);
        }
    }

    if unique_contents.is_empty() {
        return None;
    }

    let source_count = memories.len();

    // Build summary
    let content = if unique_contents.len() == 1 {
        format!("[Team {}] {}", category, unique_contents[0])
    } else {
        let items: Vec<String> = unique_contents
            .iter()
            .take(5)
            .map(|c| format!("- {}", truncate(c, 200)))
            .collect();
        let extra = if unique_contents.len() > 5 {
            format!("\n(+{} more)", unique_contents.len() - 5)
        } else {
            String::new()
        };
        format!(
            "[Team {}] {} items:\n{}{}",
            category,
            unique_contents.len(),
            items.join("\n"),
            extra,
        )
    };

    Some(DistilledFinding {
        category: category.to_string(),
        content,
        source_count,
    })
}

/// Check if two content strings are similar enough to consider duplicates.
/// Uses simple heuristic: one contains the other, or significant word overlap.
fn content_similar(a: &str, b: &str) -> bool {
    // Containment check
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    if a_lower.contains(&b_lower) || b_lower.contains(&a_lower) {
        return true;
    }

    // Word overlap check (Jaccard similarity > 0.7)
    let a_words: std::collections::HashSet<&str> = a_lower.split_whitespace().collect();
    let b_words: std::collections::HashSet<&str> = b_lower.split_whitespace().collect();

    if a_words.is_empty() || b_words.is_empty() {
        return false;
    }

    let intersection = a_words.intersection(&b_words).count();
    let union = a_words.union(&b_words).count();

    if union == 0 {
        return false;
    }

    (intersection as f64 / union as f64) > 0.7
}

/// Summarize files touched by the team.
fn summarize_files(files: &[(String, String)]) -> String {
    // Group files by member
    let mut by_member: HashMap<&str, Vec<&str>> = HashMap::new();
    for (file, member) in files {
        by_member
            .entry(member.as_str())
            .or_default()
            .push(file.as_str());
    }

    let mut parts: Vec<String> = Vec::new();
    for (member, member_files) in &by_member {
        let file_list: Vec<&str> = member_files.iter().take(5).copied().collect();
        let extra = if member_files.len() > 5 {
            format!(" (+{} more)", member_files.len() - 5)
        } else {
            String::new()
        };
        parts.push(format!("{}: {}{}", member, file_list.join(", "), extra,));
    }

    format!("[Team files] {}", parts.join("; "))
}

/// Simple hash of content for dedup key generation.
fn hash_content(content: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

/// Truncate a string at a word boundary.
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        return s;
    }
    // Find last space before max_len
    match s[..max_len].rfind(' ') {
        Some(pos) => &s[..pos],
        None => &s[..max_len],
    }
}

/// Format a DistillationResult into a human-readable summary.
pub fn format_distillation_result(result: &DistillationResult) -> String {
    let mut parts = Vec::new();
    parts.push(format!(
        "Distilled {} finding(s) from team '{}'",
        result.findings.len(),
        result.team_name,
    ));

    if result.total_memories_processed > 0 {
        parts.push(format!(
            "Processed {} memories",
            result.total_memories_processed,
        ));
    }

    if result.total_files_touched > 0 {
        parts.push(format!("{} files touched", result.total_files_touched,));
    }

    let summary = parts.join(". ");

    let mut output = format!("{}.", summary);

    if !result.findings.is_empty() {
        output.push_str("\n\nFindings:");
        for finding in &result.findings {
            output.push_str(&format!(
                "\n[{}] ({} source(s)): {}",
                finding.category,
                finding.source_count,
                truncate(&finding.content, 300),
            ));
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::pool::DatabasePool;

    async fn setup_pool() -> Arc<DatabasePool> {
        Arc::new(DatabasePool::open_in_memory().await.unwrap())
    }

    async fn setup_pool_with_team(pool: &Arc<DatabasePool>) -> (i64, i64) {
        pool.interact(|conn| {
            let (pid, _) = crate::db::get_or_create_project_sync(conn, "/test", Some("test"))
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            let tid = crate::db::get_or_create_team_sync(conn, "test-team", Some(pid), "/config")
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            crate::db::register_team_session_sync(conn, tid, "s1", "alice", "lead", None)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            crate::db::register_team_session_sync(conn, tid, "s2", "bob", "teammate", None)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            Ok::<_, anyhow::Error>((pid, tid))
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn test_distill_empty_team() {
        let pool = setup_pool().await;
        let (_pid, tid) = setup_pool_with_team(&pool).await;

        let result = distill_team_session(&pool, tid, Some(1)).await.unwrap();
        // No memories or files → None
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_distill_with_memories() {
        let pool = setup_pool().await;
        let (pid, tid) = setup_pool_with_team(&pool).await;

        // Store some team memories
        pool.interact(move |conn| {
            store_memory_sync(
                conn,
                StoreMemoryParams {
                    project_id: Some(pid),
                    key: Some("test-decision-1"),
                    content: "Decided to use builder pattern for Config",
                    fact_type: "decision",
                    category: Some("decision"),
                    confidence: 0.8,
                    session_id: Some("s1"),
                    user_id: None,
                    scope: "team",
                    branch: None,
                    team_id: Some(tid),
                },
            )?;
            store_memory_sync(
                conn,
                StoreMemoryParams {
                    project_id: Some(pid),
                    key: Some("test-decision-2"),
                    content: "Chose async-first API for database layer",
                    fact_type: "decision",
                    category: Some("decision"),
                    confidence: 0.8,
                    session_id: Some("s2"),
                    user_id: None,
                    scope: "team",
                    branch: None,
                    team_id: Some(tid),
                },
            )?;
            store_memory_sync(
                conn,
                StoreMemoryParams {
                    project_id: Some(pid),
                    key: Some("test-context-1"),
                    content: "Project uses SQLite with connection pooling",
                    fact_type: "context",
                    category: Some("context"),
                    confidence: 0.7,
                    session_id: Some("s1"),
                    user_id: None,
                    scope: "team",
                    branch: None,
                    team_id: Some(tid),
                },
            )?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .unwrap();

        let result = distill_team_session(&pool, tid, Some(pid)).await.unwrap();
        assert!(result.is_some());

        let result = result.unwrap();
        assert_eq!(result.team_name, "test-team");
        assert_eq!(result.total_memories_processed, 3);
        assert!(!result.findings.is_empty());
    }

    #[tokio::test]
    async fn test_distill_with_files() {
        let pool = setup_pool().await;
        let (pid, tid) = setup_pool_with_team(&pool).await;

        // Record file ownership and add some memories
        pool.interact(move |conn| {
            crate::db::record_file_ownership_sync(conn, tid, "s1", "alice", "src/main.rs", "Edit")?;
            crate::db::record_file_ownership_sync(conn, tid, "s2", "bob", "src/lib.rs", "Write")?;
            // Need at least MIN_MEMORIES_FOR_DISTILLATION memories OR files to produce a result
            store_memory_sync(
                conn,
                StoreMemoryParams {
                    project_id: Some(pid),
                    key: Some("test-mem-1"),
                    content: "First finding",
                    fact_type: "context",
                    category: Some("context"),
                    confidence: 0.7,
                    session_id: Some("s1"),
                    user_id: None,
                    scope: "team",
                    branch: None,
                    team_id: Some(tid),
                },
            )?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .unwrap();

        let result = distill_team_session(&pool, tid, Some(pid)).await.unwrap();
        assert!(result.is_some());
        let result = result.unwrap();
        assert!(result.total_files_touched > 0);

        // Should have a files finding
        let has_files_finding = result.findings.iter().any(|f| f.category == "files");
        assert!(has_files_finding);
    }

    #[test]
    fn test_content_similar_containment() {
        assert!(content_similar("hello world", "hello world foo"));
        assert!(content_similar(
            "use builder pattern",
            "use builder pattern for config"
        ));
    }

    #[test]
    fn test_content_similar_distinct() {
        assert!(!content_similar(
            "use builder pattern",
            "async database layer"
        ));
        assert!(!content_similar("SQLite pooling", "React components"));
    }

    #[test]
    fn test_content_similar_word_overlap() {
        // High overlap
        assert!(content_similar(
            "decided to use builder pattern for config struct",
            "decided to use builder pattern for config object"
        ));
        // Low overlap
        assert!(!content_similar(
            "decided to use builder pattern",
            "implemented async database layer"
        ));
    }

    #[test]
    fn test_truncate() {
        // Fits entirely
        assert_eq!(truncate("hello world", 20), "hello world");
        // Truncates at last space before limit
        assert_eq!(truncate("hello world foo bar", 15), "hello world");
        assert_eq!(truncate("hello world foo bar", 11), "hello");
        // Exact word boundary (len=5 -> "hello" fits, space at 5 not included)
        assert_eq!(truncate("hello world foo bar", 5), "hello");
        // No spaces -> hard cut
        assert_eq!(truncate("nospaces", 4), "nosp");
    }

    #[test]
    fn test_format_distillation_result() {
        let result = DistillationResult {
            team_name: "test-team".to_string(),
            findings: vec![DistilledFinding {
                category: "decision".to_string(),
                content: "Use builder pattern".to_string(),
                source_count: 2,
            }],
            total_memories_processed: 5,
            total_files_touched: 3,
        };

        let formatted = format_distillation_result(&result);
        assert!(formatted.contains("test-team"));
        assert!(formatted.contains("1 finding"));
        assert!(formatted.contains("5 memories"));
        assert!(formatted.contains("3 files"));
    }
}
