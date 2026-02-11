// crates/mira-server/src/fuzzy/mod.rs
// Nucleo-based fuzzy fallback search for code chunks and memories

use crate::db::pool::DatabasePool;
use crate::tools::core::NO_ACTIVE_PROJECT_ERROR;
use crate::utils::ResultExt;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};
use rusqlite::params;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

/// Default refresh interval for code chunk cache
const CODE_CACHE_TTL: Duration = Duration::from_secs(60);
/// Default refresh interval for memory cache
const MEMORY_CACHE_TTL: Duration = Duration::from_secs(30);
/// Hard cap on code chunks held in fuzzy cache
const MAX_CODE_ITEMS: usize = 200_000;
/// Hard cap on memory facts held in fuzzy cache
const MAX_MEMORY_ITEMS: usize = 50_000;

struct FuzzyCodeItem {
    file_path: String,
    content: String,
    start_line: u32,
    /// Pre-computed "{file_path} {content}" to avoid repeated allocation during fuzzy matching.
    search_text: String,
}

struct FuzzyMemoryItem {
    id: i64,
    project_id: Option<i64>,
    content: String,
    fact_type: String,
    category: Option<String>,
    scope: Option<String>,
    user_id: Option<String>,
    team_id: Option<i64>,
}

struct FuzzyIndex<T> {
    project_id: Option<i64>,
    loaded_at: Option<Instant>,
    items: Vec<T>,
}

impl<T> Default for FuzzyIndex<T> {
    fn default() -> Self {
        Self {
            project_id: None,
            loaded_at: None,
            items: Vec::new(),
        }
    }
}

impl<T> FuzzyIndex<T> {
    fn is_stale(&self, project_id: Option<i64>, ttl: Duration) -> bool {
        if self.items.is_empty() {
            return true;
        }
        if self.project_id != project_id {
            return true;
        }
        match self.loaded_at {
            Some(t) => t.elapsed() > ttl,
            None => true,
        }
    }
}

/// Normalize raw nucleo scores to 0.0–1.0 relative to the max score in the set.
fn normalize_scores(matches: &[(usize, u32)]) -> Vec<(usize, f32)> {
    let max_score = matches.iter().map(|(_, s)| *s).max().unwrap_or(1).max(1);
    matches
        .iter()
        .map(|(idx, score)| (*idx, *score as f32 / max_score as f32))
        .collect()
}

/// Fuzzy cache for code chunks and memories.
/// Rebuilt on-demand and refreshed with a TTL.
pub struct FuzzyCache {
    code_index: RwLock<FuzzyIndex<FuzzyCodeItem>>,
    memory_index: RwLock<FuzzyIndex<FuzzyMemoryItem>>,
    /// Guards against concurrent refresh of the code index
    code_refresh: Mutex<()>,
    /// Guards against concurrent refresh of the memory index
    memory_refresh: Mutex<()>,
}

impl Default for FuzzyCache {
    fn default() -> Self {
        Self::new()
    }
}

impl FuzzyCache {
    pub fn new() -> Self {
        Self {
            code_index: RwLock::new(FuzzyIndex::default()),
            memory_index: RwLock::new(FuzzyIndex::default()),
            code_refresh: Mutex::new(()),
            memory_refresh: Mutex::new(()),
        }
    }

    pub async fn invalidate_code(&self, project_id: Option<i64>) {
        let mut idx = self.code_index.write().await;
        if project_id.is_none() || idx.project_id == project_id {
            idx.loaded_at = None;
        }
    }

    pub async fn invalidate_memory(&self, project_id: Option<i64>) {
        let mut idx = self.memory_index.write().await;
        if project_id.is_none() || idx.project_id == project_id {
            idx.loaded_at = None;
        }
    }

    async fn ensure_code_index(
        &self,
        code_pool: &Arc<DatabasePool>,
        project_id: Option<i64>,
    ) -> Result<(), String> {
        // Fast path: check without locking the refresh mutex
        {
            let idx = self.code_index.read().await;
            if !idx.is_stale(project_id, CODE_CACHE_TTL) {
                return Ok(());
            }
        }

        // Serialize refreshes so only one task loads from DB
        let _guard = self.code_refresh.lock().await;

        // Re-check after acquiring the refresh lock — another task may have refreshed
        {
            let idx = self.code_index.read().await;
            if !idx.is_stale(project_id, CODE_CACHE_TTL) {
                return Ok(());
            }
        }

        let project_id_for_query = project_id.ok_or(NO_ACTIVE_PROJECT_ERROR)?;
        let items: Vec<FuzzyCodeItem> = code_pool
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT file_path, chunk_content, start_line
                     FROM code_chunks WHERE project_id = ?
                     LIMIT ?",
                )?;
                let rows = stmt.query_map(
                    params![project_id_for_query, MAX_CODE_ITEMS as i64],
                    |row| {
                        let file_path: String = row.get(0)?;
                        let content: String = row.get(1)?;
                        let start_line: i64 = row.get(2)?;
                        let search_text = format!("{} {}", file_path, content);
                        Ok(FuzzyCodeItem {
                            file_path,
                            content,
                            start_line: start_line as u32,
                            search_text,
                        })
                    },
                )?;
                rows.collect::<Result<Vec<_>, _>>()
            })
            .await
            .str_err()?;

        let mut idx = self.code_index.write().await;
        idx.project_id = project_id;
        idx.loaded_at = Some(Instant::now());
        idx.items = items;
        Ok(())
    }

    async fn ensure_memory_index(
        &self,
        pool: &Arc<DatabasePool>,
        project_id: Option<i64>,
    ) -> Result<(), String> {
        // Fast path
        {
            let idx = self.memory_index.read().await;
            if !idx.is_stale(project_id, MEMORY_CACHE_TTL) {
                return Ok(());
            }
        }

        // Serialize refreshes
        let _guard = self.memory_refresh.lock().await;

        // Re-check after acquiring lock
        {
            let idx = self.memory_index.read().await;
            if !idx.is_stale(project_id, MEMORY_CACHE_TTL) {
                return Ok(());
            }
        }

        let project_id_for_query = project_id;
        let items: Vec<FuzzyMemoryItem> = pool
            .run(move |conn| {
                let cols = "id, project_id, content, fact_type, category, scope, user_id, team_id";
                let parse_row = |row: &rusqlite::Row| {
                    Ok(FuzzyMemoryItem {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        content: row.get(2)?,
                        fact_type: row.get(3)?,
                        category: row.get(4)?,
                        scope: row.get(5)?,
                        user_id: row.get(6)?,
                        team_id: row.get(7)?,
                    })
                };
                let lim = MAX_MEMORY_ITEMS as i64;
                match project_id_for_query {
                    Some(pid) => {
                        let sql = format!(
                            "SELECT {cols} FROM memory_facts WHERE project_id = ?1 \
                             UNION ALL \
                             SELECT {cols} FROM memory_facts WHERE project_id IS NULL \
                             LIMIT ?2"
                        );
                        let mut stmt = conn.prepare(&sql)?;
                        stmt.query_map(params![pid, lim], parse_row)?
                            .collect::<Result<Vec<_>, _>>()
                    }
                    None => {
                        let sql = format!(
                            "SELECT {cols} FROM memory_facts WHERE project_id IS NULL LIMIT ?1"
                        );
                        let mut stmt = conn.prepare(&sql)?;
                        stmt.query_map(params![lim], parse_row)?
                            .collect::<Result<Vec<_>, _>>()
                    }
                }
            })
            .await
            .str_err()?;

        let mut idx = self.memory_index.write().await;
        idx.project_id = project_id;
        idx.loaded_at = Some(Instant::now());
        idx.items = items;
        Ok(())
    }

    pub async fn search_code(
        &self,
        code_pool: &Arc<DatabasePool>,
        project_id: Option<i64>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<FuzzyCodeResult>, String> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_code_index(code_pool, project_id).await?;
        let idx = self.code_index.read().await;
        if idx.items.is_empty() {
            return Ok(Vec::new());
        }

        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);

        let candidates: Vec<CandidateKey<'_>> = idx
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| CandidateKey {
                idx: i,
                text: &item.search_text,
            })
            .collect();

        let raw_matches: Vec<(usize, u32)> = pattern
            .match_list(candidates, &mut matcher)
            .into_iter()
            .take(limit)
            .map(|(c, score)| (c.idx, score))
            .collect();

        let normalized = normalize_scores(&raw_matches);

        let results = normalized
            .into_iter()
            .map(|(item_idx, score)| {
                let item = &idx.items[item_idx];
                FuzzyCodeResult {
                    file_path: item.file_path.clone(),
                    content: item.content.clone(),
                    start_line: item.start_line,
                    score,
                }
            })
            .collect();

        Ok(results)
    }

    pub async fn search_memories(
        &self,
        pool: &Arc<DatabasePool>,
        project_id: Option<i64>,
        user_id: Option<&str>,
        team_id: Option<i64>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<FuzzyMemoryResult>, String> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_memory_index(pool, project_id).await?;
        let idx = self.memory_index.read().await;
        if idx.items.is_empty() {
            return Ok(Vec::new());
        }

        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);

        let candidates: Vec<CandidateKey<'_>> = idx
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| memory_visible(item, project_id, user_id, team_id))
            .map(|(i, item)| CandidateKey {
                idx: i,
                text: &item.content,
            })
            .collect();

        let raw_matches: Vec<(usize, u32)> = pattern
            .match_list(candidates, &mut matcher)
            .into_iter()
            .take(limit)
            .map(|(c, score)| (c.idx, score))
            .collect();

        let normalized = normalize_scores(&raw_matches);

        let results = normalized
            .into_iter()
            .map(|(item_idx, score)| {
                let item = &idx.items[item_idx];
                FuzzyMemoryResult {
                    id: item.id,
                    content: item.content.clone(),
                    fact_type: item.fact_type.clone(),
                    category: item.category.clone(),
                    score,
                }
            })
            .collect();

        Ok(results)
    }
}

struct CandidateKey<'a> {
    idx: usize,
    text: &'a str,
}

impl<'a> AsRef<str> for CandidateKey<'a> {
    fn as_ref(&self) -> &str {
        self.text
    }
}

fn memory_visible(
    item: &FuzzyMemoryItem,
    project_id: Option<i64>,
    user_id: Option<&str>,
    team_id: Option<i64>,
) -> bool {
    // Match the SQL filtering in scope_filter_sql
    if item.project_id.is_some() && item.project_id != project_id {
        return false;
    }

    match item.scope.as_deref() {
        Some("personal") => user_id.is_some() && item.user_id.as_deref() == user_id,
        Some("team") => team_id.is_some() && item.team_id == team_id,
        Some("project") | None => true,
        Some(_) => true,
    }
}

/// Result type for fuzzy code search
pub struct FuzzyCodeResult {
    pub file_path: String,
    pub content: String,
    pub start_line: u32,
    pub score: f32,
}

/// Result type for fuzzy memory search
pub struct FuzzyMemoryResult {
    pub id: i64,
    pub content: String,
    pub fact_type: String,
    pub category: Option<String>,
    pub score: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_scores_single() {
        let matches = vec![(0, 500)];
        let normalized = normalize_scores(&matches);
        assert_eq!(normalized.len(), 1);
        assert!((normalized[0].1 - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_normalize_scores_multiple() {
        let matches = vec![(0, 1000), (1, 500), (2, 250)];
        let normalized = normalize_scores(&matches);
        assert_eq!(normalized.len(), 3);
        assert!((normalized[0].1 - 1.0).abs() < f32::EPSILON);
        assert!((normalized[1].1 - 0.5).abs() < f32::EPSILON);
        assert!((normalized[2].1 - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn test_normalize_scores_empty() {
        let matches: Vec<(usize, u32)> = vec![];
        let normalized = normalize_scores(&matches);
        assert!(normalized.is_empty());
    }

    #[test]
    fn test_normalize_scores_zero() {
        // All zero scores should not panic (max clamped to 1)
        let matches = vec![(0, 0), (1, 0)];
        let normalized = normalize_scores(&matches);
        assert_eq!(normalized.len(), 2);
        assert!((normalized[0].1 - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_memory_visible_project_scope() {
        let item = FuzzyMemoryItem {
            id: 1,
            project_id: Some(10),
            content: "test".into(),
            fact_type: "general".into(),
            category: None,
            scope: Some("project".into()),
            user_id: None,
            team_id: None,
        };
        assert!(memory_visible(&item, Some(10), None, None));
        assert!(!memory_visible(&item, Some(99), None, None));
        assert!(!memory_visible(&item, None, None, None));
    }

    #[test]
    fn test_memory_visible_personal_scope() {
        let item = FuzzyMemoryItem {
            id: 1,
            project_id: Some(10),
            content: "test".into(),
            fact_type: "preference".into(),
            category: None,
            scope: Some("personal".into()),
            user_id: Some("alice".into()),
            team_id: None,
        };
        assert!(memory_visible(&item, Some(10), Some("alice"), None));
        assert!(!memory_visible(&item, Some(10), Some("bob"), None));
        assert!(!memory_visible(&item, Some(10), None, None));
    }

    #[test]
    fn test_memory_visible_global_memory() {
        let item = FuzzyMemoryItem {
            id: 1,
            project_id: None,
            content: "global fact".into(),
            fact_type: "general".into(),
            category: None,
            scope: None,
            user_id: None,
            team_id: None,
        };
        // Global memories (project_id=None) are visible to any project
        assert!(memory_visible(&item, Some(10), None, None));
        assert!(memory_visible(&item, None, None, None));
    }

    #[test]
    fn test_memory_visible_team_scope() {
        let item = FuzzyMemoryItem {
            id: 1,
            project_id: Some(10),
            content: "team knowledge".into(),
            fact_type: "general".into(),
            category: None,
            scope: Some("team".into()),
            user_id: None,
            team_id: Some(42),
        };
        // Same team: visible
        assert!(memory_visible(&item, Some(10), None, Some(42)));
        // Different team: not visible
        assert!(!memory_visible(&item, Some(10), None, Some(99)));
        // No team: not visible
        assert!(!memory_visible(&item, Some(10), None, None));
    }

    #[test]
    fn test_candidate_key_as_ref() {
        let key = CandidateKey {
            idx: 0,
            text: "hello world",
        };
        assert_eq!(key.as_ref(), "hello world");
    }

    #[test]
    fn test_fuzzy_index_stale_empty() {
        let idx: FuzzyIndex<FuzzyCodeItem> = FuzzyIndex::default();
        assert!(idx.is_stale(None, CODE_CACHE_TTL));
    }

    #[test]
    fn test_fuzzy_index_stale_project_mismatch() {
        let idx: FuzzyIndex<FuzzyCodeItem> = FuzzyIndex {
            project_id: Some(1),
            loaded_at: Some(Instant::now()),
            items: vec![FuzzyCodeItem {
                file_path: "test.rs".into(),
                content: "fn main() {}".into(),
                start_line: 1,
                search_text: "test.rs fn main() {}".into(),
            }],
        };
        assert!(idx.is_stale(Some(2), CODE_CACHE_TTL));
        assert!(!idx.is_stale(Some(1), CODE_CACHE_TTL));
    }

    #[test]
    fn test_fuzzy_code_item_search_text() {
        let item = FuzzyCodeItem {
            file_path: "src/main.rs".into(),
            content: "fn main() {}".into(),
            start_line: 1,
            search_text: "src/main.rs fn main() {}".into(),
        };
        assert_eq!(item.search_text, "src/main.rs fn main() {}");
    }
}
