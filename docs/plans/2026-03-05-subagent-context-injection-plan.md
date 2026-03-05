# Subagent Context Injection Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Give narrow subagents (Explore, code-reviewer, etc.) useful project orientation and search hints instead of zero context.

**Architecture:** Two new IPC ops (`get_project_map`, `search_for_subagent`) backed by existing code index and search infrastructure. The SubagentStart hook calls both for narrow subagents and formats the results into a compact context block.

**Tech Stack:** Rust, SQLite (code_symbols table), hybrid_search (embeddings + keyword fallback), IPC NDJSON protocol.

---

### Task 1: Add `get_project_map` IPC server op

**Files:**
- Modify: `crates/mira-server/src/ipc/ops.rs` (add new function)
- Modify: `crates/mira-server/src/ipc/handler.rs` (register op)

**Step 1: Add the op function to ops.rs**

Add after `generate_bundle`:

```rust
/// Get a compact project map showing top-level directories and file counts.
/// Used by SubagentStart to give narrow subagents project orientation.
pub async fn get_project_map(server: &MiraServer, params: Value) -> Result<Value> {
    let project_id = params
        .get("project_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: project_id"))?;
    let budget = params
        .get("budget")
        .and_then(|v| v.as_i64())
        .unwrap_or(500) as usize;

    // Get project name from main DB
    let project_name: String = server
        .pool
        .interact(move |conn| {
            conn.query_row(
                "SELECT COALESCE(name, '') FROM projects WHERE id = ?1",
                [project_id],
                |r| r.get(0),
            )
            .unwrap_or_default()
        })
        .await;

    // Get top-level directory counts from code index
    let dirs: Vec<(String, i64)> = server
        .code_pool
        .run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT
                    CASE
                        WHEN INSTR(SUBSTR(file_path, 1), '/') > 0
                        THEN SUBSTR(file_path, 1, INSTR(file_path, '/'))
                        ELSE file_path
                    END AS top_dir,
                    COUNT(DISTINCT file_path) AS file_count
                 FROM code_symbols
                 WHERE project_id = ?1
                 GROUP BY top_dir
                 ORDER BY file_count DESC
                 LIMIT 15"
            ).map_err(|e| anyhow::anyhow!("{e}"))?;
            let rows = stmt.query_map([project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            }).map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok::<_, anyhow::Error>(rows.filter_map(|r| r.ok()).collect::<Vec<_>>())
        })
        .await
        .unwrap_or_default();

    if dirs.is_empty() {
        return Ok(json!({"content": "", "empty": true}));
    }

    // Format: "Project: Name (dir1/ (N), dir2/ (M), ...)"
    let mut parts: Vec<String> = Vec::new();
    let mut total_len = 0;
    for (dir, count) in &dirs {
        let part = format!("{} ({})", dir, count);
        total_len += part.len() + 2; // ", " separator
        if total_len > budget {
            parts.push("...".to_string());
            break;
        }
        parts.push(part);
    }

    let label = if project_name.is_empty() {
        "[Mira/context] Project".to_string()
    } else {
        format!("[Mira/context] Project: {}", project_name)
    };
    let content = format!("{} ({})", label, parts.join(", "));

    Ok(json!({"content": content, "empty": false}))
}
```

**Step 2: Register the op in handler.rs**

In `dispatch_op`, add before the `_ => anyhow::bail!` line:

```rust
        "get_project_map" => super::ops::get_project_map(server, params).await,
```

Also add a timeout entry:

```rust
        "get_project_map" => Duration::from_secs(2),
```

**Step 3: Build and verify**

Run: `cargo check`
Expected: compiles clean

**Step 4: Commit**

```bash
git add crates/mira-server/src/ipc/ops.rs crates/mira-server/src/ipc/handler.rs
git commit -m "feat: add get_project_map IPC op for subagent orientation"
```

---

### Task 2: Add `search_for_subagent` IPC server op

**Files:**
- Modify: `crates/mira-server/src/ipc/ops.rs` (add new function)
- Modify: `crates/mira-server/src/ipc/handler.rs` (register op)

**Step 1: Add the op function to ops.rs**

Add after `get_project_map`:

```rust
/// Search code index using task description, returning compact file:symbol hints.
/// Used by SubagentStart to give narrow subagents relevant starting points.
pub async fn search_for_subagent(server: &MiraServer, params: Value) -> Result<Value> {
    let project_id = params
        .get("project_id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("missing required param: project_id"))?;
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing required param: query"))?
        .to_string();
    let limit = params
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(5) as usize;
    let budget = params
        .get("budget")
        .and_then(|v| v.as_i64())
        .unwrap_or(1000) as usize;

    // Get project path for hybrid_search
    let project_path: Option<String> = server
        .pool
        .interact(move |conn| {
            conn.query_row(
                "SELECT path FROM projects WHERE id = ?1",
                [project_id],
                |r| r.get(0),
            )
            .ok()
        })
        .await;

    use crate::search::semantic::hybrid_search;

    let result = hybrid_search(
        server.code_pool.inner(),
        server.embeddings.as_ref(),
        Some(&server.fuzzy_cache),
        &query,
        Some(project_id),
        project_path.as_deref(),
        limit,
    )
    .await;

    let hits = match result {
        Ok(r) => r.results,
        Err(e) => {
            tracing::debug!("search_for_subagent failed: {e}");
            return Ok(json!({"content": "", "empty": true}));
        }
    };

    if hits.is_empty() {
        return Ok(json!({"content": "", "empty": true}));
    }

    // Format as compact "- file_path: symbol1, symbol2" lines
    let mut lines: Vec<String> = Vec::new();
    let mut total_len = 0;
    for hit in &hits {
        let line = if hit.symbol_name.is_empty() {
            format!("- {}", hit.file_path)
        } else {
            format!("- {}: {}", hit.file_path, hit.symbol_name)
        };
        total_len += line.len() + 1;
        if total_len > budget {
            break;
        }
        lines.push(line);
    }

    let content = format!("Relevant code for this task:\n{}", lines.join("\n"));

    Ok(json!({
        "content": content,
        "empty": false,
        "hits": hits.len(),
    }))
}
```

**Step 2: Register the op in handler.rs**

In `dispatch_op`, add:

```rust
        "search_for_subagent" => super::ops::search_for_subagent(server, params).await,
```

Timeout entry:

```rust
        "search_for_subagent" => Duration::from_secs(3),
```

**Step 3: Build and verify**

Run: `cargo check`
Expected: compiles clean

**Step 4: Commit**

```bash
git add crates/mira-server/src/ipc/ops.rs crates/mira-server/src/ipc/handler.rs
git commit -m "feat: add search_for_subagent IPC op for task-based code hints"
```

---

### Task 3: Add IPC client methods

**Files:**
- Modify: `crates/mira-server/src/ipc/client/state_ops.rs` (add two methods)

**Step 1: Add `get_project_map` client method**

Add after `generate_bundle`:

```rust
    /// Get a compact project map for subagent orientation.
    /// Returns None if not in IPC mode or project has no indexed files.
    pub async fn get_project_map(
        &mut self,
        project_id: i64,
        budget: usize,
    ) -> Option<String> {
        if !self.is_ipc() {
            return None;
        }
        let params = json!({
            "project_id": project_id,
            "budget": budget,
        });
        let result = self.call("get_project_map", params).await.ok()?;
        if result.get("empty").and_then(|v| v.as_bool()).unwrap_or(true) {
            return None;
        }
        result.get("content").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from)
    }

    /// Search code index using a task description, returning compact hints.
    /// Returns None if not in IPC mode or search returns no results.
    pub async fn search_for_subagent(
        &mut self,
        project_id: i64,
        query: &str,
        limit: usize,
        budget: usize,
    ) -> Option<String> {
        if !self.is_ipc() {
            return None;
        }
        let params = json!({
            "project_id": project_id,
            "query": query,
            "limit": limit,
            "budget": budget,
        });
        let result = self.call("search_for_subagent", params).await.ok()?;
        if result.get("empty").and_then(|v| v.as_bool()).unwrap_or(true) {
            return None;
        }
        result.get("content").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from)
    }
```

**Step 2: Build and verify**

Run: `cargo check`
Expected: compiles clean

**Step 3: Commit**

```bash
git add crates/mira-server/src/ipc/client/state_ops.rs
git commit -m "feat: add get_project_map and search_for_subagent client methods"
```

---

### Task 4: Update SubagentStart hook for narrow subagents

**Files:**
- Modify: `crates/mira-server/src/hooks/subagent.rs`

**Step 1: Add narrow subagent context injection**

In `run_start()`, replace the section after `let narrow = is_narrow_subagent(...)` that currently skips goals for narrow types. Add a new branch for narrow subagents:

```rust
    if narrow {
        // Narrow subagents get project map + search hints instead of goals + bundle

        // 1. Always try project map
        if let Some(map) = client.get_project_map(project_id, 500).await {
            context_parts.push(map);
        }

        // 2. Try search hints from task description
        if let Some(ref task_desc) = start_input.task_description {
            if let Some(hints) = client
                .search_for_subagent(project_id, task_desc, 5, 1000)
                .await
            {
                context_parts.push(hints);
            }
        }
    } else {
        // Full subagents get goals + bundle (existing logic)
        // ... existing code for goals_already_shown, get_active_goals, etc.
    }
```

Keep the existing bundle logic inside the `else` branch for full subagents.

Update `sources_kept` tracking to include "project_map" and "search_hints" categories.

**Step 2: Build and verify**

Run: `cargo check`
Expected: compiles clean

**Step 3: Test manually**

Run Mira, launch an Explore subagent in a project with indexed code. Check:
- `context_injections` table has a SubagentStart record with chars > 0
- The injected content has the project map and/or search hints

**Step 4: Commit**

```bash
git add crates/mira-server/src/hooks/subagent.rs
git commit -m "feat: inject project map and search hints for narrow subagents"
```

---

### Task 5: Add IPC tests

**Files:**
- Modify: `crates/mira-server/src/ipc/tests.rs`

**Step 1: Write test for `get_project_map`**

Follow the pattern of the existing `generate_bundle` tests. Seed `code_symbols` with a few entries across different directories, then call the `get_project_map` op and verify:
- Returns `empty: false` when code is indexed
- Content contains directory names and counts
- Returns `empty: true` when project has no indexed files

**Step 2: Write test for `search_for_subagent`**

Seed `code_symbols` with known entries, then call `search_for_subagent` with a query that should match:
- Returns `empty: false` with matching results
- Content contains file paths and symbol names
- Returns `empty: true` for queries with no matches

**Step 3: Run tests**

Run: `cargo test ipc::tests`
Expected: all pass

**Step 4: Commit**

```bash
git add crates/mira-server/src/ipc/tests.rs
git commit -m "test: add IPC tests for get_project_map and search_for_subagent"
```

---

### Task 6: Add subagent hook unit tests

**Files:**
- Modify: `crates/mira-server/src/hooks/subagent.rs` (tests module)

**Step 1: Test narrow subagent gets project_map source**

Verify that when `is_narrow_subagent` returns true, the hook attempts to call `get_project_map` (test the logic flow, not the IPC).

**Step 2: Test full subagent is unchanged**

Verify that `is_narrow_subagent("Plan")` returns false and the existing goal+bundle path is taken.

**Step 3: Run tests**

Run: `cargo test hooks::subagent::tests`
Expected: all pass

**Step 4: Commit**

```bash
git add crates/mira-server/src/hooks/subagent.rs
git commit -m "test: add narrow subagent context injection tests"
```
