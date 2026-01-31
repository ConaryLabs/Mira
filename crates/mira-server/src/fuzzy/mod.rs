// crates/mira-server/src/fuzzy/mod.rs
// Nucleo-based fuzzy fallback search for code chunks and memories

use crate::db::pool::DatabasePool;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};
use rusqlite;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Default refresh interval for code chunk cache
const CODE_CACHE_TTL: Duration = Duration::from_secs(60);
/// Default refresh interval for memory cache
const MEMORY_CACHE_TTL: Duration = Duration::from_secs(30);
/// Hard cap on code chunks held in fuzzy cache
const MAX_CODE_ITEMS: usize = 200_000;
/// Hard cap on memory facts held in fuzzy cache
const MAX_MEMORY_ITEMS: usize = 50_000;

#[derive(Clone)]
struct FuzzyCodeItem {
    file_path: String,
    content: String,
    start_line: u32,
    search_text: String,
}

#[derive(Clone)]
struct FuzzyMemoryItem {
    id: i64,
    project_id: Option<i64>,
    content: String,
    fact_type: String,
    category: Option<String>,
    scope: Option<String>,
    user_id: Option<String>,
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

/// Fuzzy cache for code chunks and memories.
/// Rebuilt on-demand and refreshed with a TTL.
pub struct FuzzyCache {
    code_index: RwLock<FuzzyIndex<FuzzyCodeItem>>,
    memory_index: RwLock<FuzzyIndex<FuzzyMemoryItem>>,
}

impl FuzzyCache {
    pub fn new() -> Self {
        Self {
            code_index: RwLock::new(FuzzyIndex::default()),
            memory_index: RwLock::new(FuzzyIndex::default()),
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
        let stale = {
            let idx = self.code_index.read().await;
            idx.is_stale(project_id, CODE_CACHE_TTL)
        };
        if !stale {
            return Ok(());
        }

        let project_id_for_query = project_id.ok_or("No active project")?;
        let items: Vec<FuzzyCodeItem> = code_pool
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT file_path, chunk_content, start_line
                     FROM code_chunks WHERE project_id = ?
                     LIMIT ?",
                )?;
                let rows = stmt.query_map(
                    rusqlite::params![project_id_for_query, MAX_CODE_ITEMS as i64],
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
            .map_err(|e| e.to_string())?;

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
        let stale = {
            let idx = self.memory_index.read().await;
            idx.is_stale(project_id, MEMORY_CACHE_TTL)
        };
        if !stale {
            return Ok(());
        }

        let project_id_for_query = project_id;
        let items: Vec<FuzzyMemoryItem> = pool
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, content, fact_type, category, scope, user_id
                     FROM memory_facts
                     WHERE project_id = ? OR project_id IS NULL
                     LIMIT ?",
                )?;
                let rows = stmt.query_map(
                    rusqlite::params![project_id_for_query, MAX_MEMORY_ITEMS as i64],
                    |row| {
                        let content: String = row.get(2)?;
                        Ok(FuzzyMemoryItem {
                            id: row.get(0)?,
                            project_id: row.get(1)?,
                            content: content.clone(),
                            fact_type: row.get(3)?,
                            category: row.get(4)?,
                            scope: row.get(5)?,
                            user_id: row.get(6)?,
                            search_text: content,
                        })
                    },
                )?;
                rows.collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(|e| e.to_string())?;

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

        let mut candidates: Vec<CandidateKey<'_>> = Vec::with_capacity(idx.items.len());
        for (i, item) in idx.items.iter().enumerate() {
            candidates.push(CandidateKey {
                idx: i,
                text: &item.search_text,
            });
        }

        let matches = pattern.match_list(candidates, &mut matcher);
        let mut results = Vec::new();
        for (candidate, score) in matches.into_iter().take(limit) {
            let item = &idx.items[candidate.idx];
            results.push(FuzzyCodeResult {
                file_path: item.file_path.clone(),
                content: item.content.clone(),
                start_line: item.start_line,
                score: score as f32 / 1000.0,
            });
        }

        Ok(results)
    }

    pub async fn search_memories(
        &self,
        pool: &Arc<DatabasePool>,
        project_id: Option<i64>,
        user_id: Option<&str>,
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

        let mut candidates: Vec<CandidateKey<'_>> = Vec::new();
        for (i, item) in idx.items.iter().enumerate() {
            if !memory_visible(item, project_id, user_id) {
                continue;
            }
            candidates.push(CandidateKey {
                idx: i,
                text: &item.search_text,
            });
        }

        let matches = pattern.match_list(candidates, &mut matcher);
        let mut results = Vec::new();
        for (candidate, score) in matches.into_iter().take(limit) {
            let item = &idx.items[candidate.idx];
            results.push(FuzzyMemoryResult {
                id: item.id,
                content: item.content.clone(),
                fact_type: item.fact_type.clone(),
                category: item.category.clone(),
                score: score as f32 / 1000.0,
            });
        }

        Ok(results)
    }
}

#[derive(Clone)]
struct CandidateKey<'a> {
    idx: usize,
    text: &'a str,
}

impl<'a> AsRef<str> for CandidateKey<'a> {
    fn as_ref(&self) -> &str {
        self.text
    }
}

fn memory_visible(item: &FuzzyMemoryItem, project_id: Option<i64>, user_id: Option<&str>) -> bool {
    // Match the SQL filtering in search_memories_sync
    if item.project_id.is_some() && item.project_id != project_id {
        return false;
    }

    match item.scope.as_deref() {
        Some("personal") => item.user_id.as_deref() == user_id,
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
