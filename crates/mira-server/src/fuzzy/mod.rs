// crates/mira-server/src/fuzzy/mod.rs
// Nucleo-based fuzzy search for code chunks

use crate::db::pool::DatabasePool;
use crate::error::MiraError;
use crate::utils::ResultExt;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};
use rusqlite::params;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

/// Default refresh interval for code chunk cache
const CODE_CACHE_TTL: Duration = Duration::from_secs(60);
/// Hard cap on code chunks held in fuzzy cache
const MAX_CODE_ITEMS: usize = 200_000;

struct FuzzyCodeItem {
    file_path: String,
    content: String,
    start_line: u32,
    /// Pre-computed "{file_path} {content}" to avoid repeated allocation during fuzzy matching.
    search_text: String,
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

/// Normalize raw nucleo scores to 0.0-1.0 relative to the max score in the set.
fn normalize_scores(matches: &[(usize, u32)]) -> Vec<(usize, f32)> {
    let max_score = matches.iter().map(|(_, s)| *s).max().unwrap_or(1).max(1);
    matches
        .iter()
        .map(|(idx, score)| (*idx, *score as f32 / max_score as f32))
        .collect()
}

/// Fuzzy cache for code chunks.
/// Rebuilt on-demand and refreshed with a TTL.
/// Memory search removed (Phase 4 of memory system removal).
pub struct FuzzyCache {
    code_index: RwLock<FuzzyIndex<FuzzyCodeItem>>,
    /// Guards against concurrent refresh of the code index
    code_refresh: Mutex<()>,
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
            code_refresh: Mutex::new(()),
        }
    }

    pub async fn invalidate_code(&self, project_id: Option<i64>) {
        let mut idx = self.code_index.write().await;
        if project_id.is_none() || idx.project_id == project_id {
            idx.loaded_at = None;
        }
    }

    /// No-op: memory index removed (Phase 4).
    pub async fn invalidate_memory(&self, _project_id: Option<i64>) {}

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

        // Re-check after acquiring the refresh lock -- another task may have refreshed
        {
            let idx = self.code_index.read().await;
            if !idx.is_stale(project_id, CODE_CACHE_TTL) {
                return Ok(());
            }
        }

        let project_id_for_query = project_id.ok_or(MiraError::ProjectNotSet)?;
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

    /// Memory search removed (Phase 4 of memory system removal). Returns empty results.
    pub async fn search_memories(
        &self,
        _pool: &Arc<DatabasePool>,
        _project_id: Option<i64>,
        _user_id: Option<&str>,
        _team_id: Option<i64>,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<FuzzyMemoryResult>, String> {
        Ok(Vec::new())
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

/// Result type for fuzzy code search
pub struct FuzzyCodeResult {
    pub file_path: String,
    pub content: String,
    pub start_line: u32,
    pub score: f32,
}

/// Result type for fuzzy memory search (kept for API compatibility, always empty)
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
