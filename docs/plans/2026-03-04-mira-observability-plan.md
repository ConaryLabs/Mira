# Mira Observability Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make Mira's hook activity visible to users through stderr feedback, content logging, and an enhanced status dashboard with value heuristics.

**Architecture:** Extend the existing `context_injections` table with content/categories columns. Add stderr + context tag output to the 4 injection hooks. Build a dashboard action that queries injection stats and correlates with tool_history for value signals.

**Tech Stack:** Rust, SQLite (rusqlite), existing Mira hook/IPC infrastructure.

**Design doc:** `docs/plans/2026-03-04-mira-observability-design.md`

---

### Task 1: Schema migration -- add content and categories columns

**Files:**
- Modify: `crates/mira-server/src/db/schema/injection.rs`
- Modify: `crates/mira-server/src/db/injection.rs`

**Step 1: Write tests for new InjectionRecord fields**

Add to `crates/mira-server/src/db/injection.rs` tests:

```rust
#[test]
fn test_insert_with_content_and_categories() {
    let conn = setup_db();
    let mut record = make_record("UserPromptSubmit", Some("session-1"));
    record.content = Some("goals context here".to_string());
    record.categories = vec!["goals".to_string(), "file_hints".to_string()];
    let id = insert_injection_sync(&conn, &record).unwrap();
    assert!(id > 0);

    // Verify content was stored
    let stored: String = conn
        .query_row(
            "SELECT content FROM context_injections WHERE id = ?",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, "goals context here");

    // Verify categories
    let cats: String = conn
        .query_row(
            "SELECT categories FROM context_injections WHERE id = ?",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(cats, "goals,file_hints");
}

#[test]
fn test_content_truncated_at_limit() {
    let conn = setup_db();
    let mut record = make_record("SessionStart", Some("session-1"));
    record.content = Some("x".repeat(3000)); // Over 2000 char limit
    let id = insert_injection_sync(&conn, &record).unwrap();

    let stored: String = conn
        .query_row(
            "SELECT content FROM context_injections WHERE id = ?",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(stored.len() <= 2000);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib injection -- test_insert_with_content test_content_truncated`
Expected: compilation error (fields don't exist yet)

**Step 3: Add columns to schema migration**

In `crates/mira-server/src/db/schema/injection.rs`, the table is created with `CREATE TABLE IF NOT EXISTS`. Since the table already exists in production DBs, add ALTER TABLE migrations after the create:

```rust
pub fn migrate_context_injections_table(conn: &Connection) -> Result<()> {
    create_table_if_missing(
        conn,
        "context_injections",
        r#"
        CREATE TABLE IF NOT EXISTS context_injections (
            id INTEGER PRIMARY KEY,
            hook_name TEXT NOT NULL,
            session_id TEXT,
            project_id INTEGER REFERENCES projects(id),
            chars_injected INTEGER NOT NULL DEFAULT 0,
            sources_kept TEXT,
            sources_dropped TEXT,
            latency_ms INTEGER,
            was_deduped INTEGER NOT NULL DEFAULT 0,
            was_cached INTEGER NOT NULL DEFAULT 0,
            content TEXT,
            categories TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_ctx_inj_session ON context_injections(session_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_ctx_inj_hook ON context_injections(hook_name, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_ctx_inj_project ON context_injections(project_id, created_at DESC);
    "#,
    )?;
    // Add columns for existing databases (silently ignored if already present)
    let _ = conn.execute_batch(
        "ALTER TABLE context_injections ADD COLUMN content TEXT;
         ALTER TABLE context_injections ADD COLUMN categories TEXT;",
    );
    Ok(())
}
```

**Step 4: Add fields to InjectionRecord and update insert**

In `crates/mira-server/src/db/injection.rs`:

Add fields to `InjectionRecord`:
```rust
pub struct InjectionRecord {
    // ... existing fields ...
    pub content: Option<String>,
    pub categories: Vec<String>,
}
```

Update `insert_injection_sync` to include content (truncated to 2000 chars) and categories:
```rust
pub fn insert_injection_sync(conn: &Connection, record: &InjectionRecord) -> Result<i64> {
    // ... existing sources_kept/sources_dropped logic ...
    let categories = if record.categories.is_empty() {
        None
    } else {
        Some(record.categories.join(","))
    };
    let content = record.content.as_deref().map(|c| {
        if c.len() > 2000 { &c[..2000] } else { c }
    });

    conn.execute(
        "INSERT INTO context_injections (
            hook_name, session_id, project_id, chars_injected,
            sources_kept, sources_dropped, latency_ms, was_deduped, was_cached,
            content, categories
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            // ... existing params ...
            content,
            categories,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}
```

Update `make_record` in tests to include the new fields:
```rust
fn make_record(hook: &str, session_id: Option<&str>) -> InjectionRecord {
    InjectionRecord {
        // ... existing fields ...
        content: None,
        categories: vec![],
    }
}
```

**Step 5: Fix all compilation errors across the codebase**

Every callsite that constructs `InjectionRecord` needs the new fields. Search for `InjectionRecord {` and add `content: None, categories: vec![]` to each. Key files:
- `crates/mira-server/src/hooks/user_prompt.rs` (3 callsites)
- `crates/mira-server/src/hooks/pre_tool.rs` (2 callsites)
- `crates/mira-server/src/hooks/session/mod.rs` (1 callsite)
- `crates/mira-server/src/hooks/subagent.rs` (1 callsite)

**Step 6: Run tests to verify they pass**

Run: `cargo test --lib injection`
Expected: all tests pass including new ones

**Step 7: Commit**

```bash
git add -p
git commit -m "feat: add content and categories columns to context_injections"
```

---

### Task 2: Stderr feedback for all injection hooks

**Files:**
- Modify: `crates/mira-server/src/hooks/session/mod.rs` (SessionStart)
- Modify: `crates/mira-server/src/hooks/user_prompt.rs` (UserPromptSubmit)
- Modify: `crates/mira-server/src/hooks/pre_tool.rs` (PreToolUse)
- Modify: `crates/mira-server/src/hooks/subagent.rs` (SubagentStart)

**Step 1: Add a shared stderr helper**

In `crates/mira-server/src/hooks/mod.rs`, add a helper that writes to stderr:

```rust
/// Emit a one-line activity summary to stderr for user visibility.
/// Format: `[Mira] {hook}: {summary}`
/// Stderr is shown briefly in the Claude Code terminal.
pub fn emit_activity(hook: &str, summary: &str) {
    eprintln!("[Mira] {}: {}", hook, summary);
}
```

**Step 2: Add stderr output to SessionStart**

In `crates/mira-server/src/hooks/session/mod.rs`, after the context is built (around line 367, before writing hook output), add:

```rust
if let Some(ref ctx) = context {
    let item_count = ctx.matches("[Mira/").count();
    crate::hooks::emit_activity(
        "SessionStart",
        &format!("injected {} items ({} chars)", item_count, ctx.len()),
    );
}
```

**Step 3: Add stderr output to UserPromptSubmit**

In `crates/mira-server/src/hooks/user_prompt.rs`, in `assemble_output_from_ipc` after the dedup check passes and before writing output (around line 431), add:

```rust
crate::hooks::emit_activity(
    "UserPromptSubmit",
    &format!(
        "{} items ({} chars) -- {}",
        budget_result.kept_sources.len(),
        final_context.len(),
        budget_result.kept_sources.join(", ")
    ),
);
```

Same pattern in `run_direct` (around line 594).

**Step 4: Add stderr output to PreToolUse**

In `crates/mira-server/src/hooks/pre_tool.rs`, when hints are produced (around line 327), add:

```rust
let hint_types: Vec<&str> = hints.iter().map(|h| {
    if h.contains("[Mira/efficiency]") { "reread advisory" }
    else if h.contains("[Mira/symbols]") { "symbol hints" }
    else if h.contains("[Mira/patterns]") { "change patterns" }
    else { "hint" }
}).collect();
crate::hooks::emit_activity(
    "PreToolUse",
    &format!("{}", hint_types.join(", ")),
);
```

Also in `handle_edit_write_patterns` (around line 497):
```rust
crate::hooks::emit_activity(
    "PreToolUse",
    &format!("change pattern warning for {}", filename),
);
```

**Step 5: Add stderr output to SubagentStart**

In `crates/mira-server/src/hooks/subagent.rs`, after context_parts are assembled and before writing output, add:

```rust
if !context_parts.is_empty() {
    crate::hooks::emit_activity(
        "SubagentStart",
        &format!(
            "pre-loaded {} items ({} chars) for {} subagent",
            context_parts.len(),
            context.len(),
            start_input.subagent_type,
        ),
    );
}
```

**Step 6: Build and verify**

Run: `cargo check`
Expected: compiles with no errors

**Step 7: Commit**

```bash
git add -p
git commit -m "feat: add stderr activity summaries to all injection hooks"
```

---

### Task 3: Content capture and categories in injection recording

**Files:**
- Modify: `crates/mira-server/src/hooks/session/mod.rs`
- Modify: `crates/mira-server/src/hooks/user_prompt.rs`
- Modify: `crates/mira-server/src/hooks/pre_tool.rs`
- Modify: `crates/mira-server/src/hooks/subagent.rs`

**Step 1: Add content and categories to SessionStart injection recording**

In `crates/mira-server/src/hooks/session/mod.rs` (around line 371), update the `InjectionRecord` to include `content` and `categories`. Derive categories from context_parts labels:

```rust
content: Some(ctx.clone()),
categories: vec!["session_context".to_string()],
```

**Step 2: Add content and categories to UserPromptSubmit injection recording**

In `crates/mira-server/src/hooks/user_prompt.rs`, both in `assemble_output_from_ipc` (line ~418) and `run_direct` (line ~583), update the `InjectionRecord`:

```rust
content: Some(final_context.clone()),
categories: budget_result.kept_sources.clone(),
```

For the deduped case, set `content: None` (nothing was actually injected).

**Step 3: Add content and categories to PreToolUse injection recording**

In `crates/mira-server/src/hooks/pre_tool.rs` (line ~330 for Read, line ~480 for Edit/Write), update:

```rust
content: Some(context.clone()),
categories: hint_types.iter().map(|s| s.to_string()).collect(),
```

For Edit/Write patterns:
```rust
content: Some(context.clone()),
categories: vec!["change_patterns".to_string()],
```

**Step 4: Add content and categories to SubagentStart injection recording**

In `crates/mira-server/src/hooks/subagent.rs`, update the injection recording to include:

```rust
content: Some(context.clone()),
categories: context_parts_labels.clone(), // derive from what was included
```

Track what was included (goals, bundle, etc.) as category labels.

**Step 5: Build and test**

Run: `cargo check && cargo test --lib injection`
Expected: compiles and all tests pass

**Step 6: Commit**

```bash
git add -p
git commit -m "feat: capture injection content and categories in all hooks"
```

---

### Task 4: Activity context tag appended to injected content

**Files:**
- Create: helper function in `crates/mira-server/src/hooks/mod.rs`
- Modify: `crates/mira-server/src/hooks/user_prompt.rs`
- Modify: `crates/mira-server/src/hooks/session/mod.rs`
- Modify: `crates/mira-server/src/hooks/subagent.rs`

**Step 1: Write test for activity tag builder**

In `crates/mira-server/src/hooks/mod.rs` tests:

```rust
#[test]
fn test_build_activity_tag() {
    let tag = build_activity_tag(&["goals", "reactive"], 342);
    assert!(tag.contains("[Mira/activity]"));
    assert!(tag.contains("goals"));
    assert!(tag.contains("342 chars"));
}

#[test]
fn test_build_activity_tag_empty() {
    let tag = build_activity_tag(&[], 0);
    assert!(tag.is_empty());
}
```

**Step 2: Implement activity tag builder**

In `crates/mira-server/src/hooks/mod.rs`:

```rust
/// Build a compact activity tag for appending to injected context.
/// Returns empty string if no categories provided.
pub fn build_activity_tag(categories: &[&str], chars: usize) -> String {
    if categories.is_empty() {
        return String::new();
    }
    format!(
        "\n[Mira/activity] Injected: {} | {} chars",
        categories.join(", "),
        chars,
    )
}
```

**Step 3: Append tag to UserPromptSubmit output**

In `assemble_output_from_ipc` and `run_direct`, after building `final_context` and before writing output, append the tag:

```rust
let tag = crate::hooks::build_activity_tag(
    &budget_result.kept_sources.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
    final_context.len(),
);
let final_with_tag = if tag.is_empty() {
    final_context
} else {
    format!("{}{}", final_context, tag)
};
```

Use `final_with_tag` in the output JSON.

**Step 4: Append tag to SessionStart output**

In `crates/mira-server/src/hooks/session/mod.rs`, append the tag to the context string before writing hook output.

**Step 5: Append tag to SubagentStart output**

In `crates/mira-server/src/hooks/subagent.rs`, append the tag to the context string before writing hook output.

Note: PreToolUse does NOT get the tag since its hints are short and self-describing.

**Step 6: Build and test**

Run: `cargo check && cargo test --lib hooks`
Expected: compiles and tests pass

**Step 7: Commit**

```bash
git add -p
git commit -m "feat: append [Mira/activity] tag to injected context"
```

---

### Task 5: Enhanced status dashboard with injection stats

**Files:**
- Modify: `crates/mira-server/src/db/injection.rs` (new query functions)
- Modify: `crates/mira-server/src/tools/core/session/mod.rs` or `storage.rs`

**Step 1: Write tests for new query functions**

In `crates/mira-server/src/db/injection.rs` tests:

```rust
#[test]
fn test_get_injection_categories_breakdown() {
    let conn = setup_db();
    let mut r1 = make_record("UserPromptSubmit", Some("s1"));
    r1.categories = vec!["goals".into(), "reactive".into()];
    insert_injection_sync(&conn, &r1).unwrap();

    let mut r2 = make_record("SubagentStart", Some("s1"));
    r2.categories = vec!["goals".into()];
    insert_injection_sync(&conn, &r2).unwrap();

    let breakdown = get_category_breakdown_sync(&conn, Some("s1"), None).unwrap();
    assert_eq!(breakdown.get("goals"), Some(&2));
    assert_eq!(breakdown.get("reactive"), Some(&1));
}
```

**Step 2: Implement category breakdown query**

```rust
use std::collections::HashMap;

/// Get a breakdown of injection categories (how many times each category was injected).
pub fn get_category_breakdown_sync(
    conn: &Connection,
    session_id: Option<&str>,
    project_id: Option<i64>,
) -> Result<HashMap<String, usize>> {
    let mut sql = String::from(
        "SELECT categories FROM context_injections WHERE categories IS NOT NULL AND categories != ''"
    );
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(sid) = session_id {
        sql.push_str(" AND session_id = ?");
        params_vec.push(Box::new(sid.to_string()));
    }
    if let Some(pid) = project_id {
        sql.push_str(" AND project_id = ?");
        params_vec.push(Box::new(pid));
    }

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| row.get::<_, String>(0))?;

    let mut counts: HashMap<String, usize> = HashMap::new();
    for row in rows.flatten() {
        for cat in row.split(',') {
            let cat = cat.trim();
            if !cat.is_empty() {
                *counts.entry(cat.to_string()).or_default() += 1;
            }
        }
    }
    Ok(counts)
}
```

**Step 3: Build the dashboard output**

Add a new function in `crates/mira-server/src/tools/core/session/mod.rs` or a new `analytics` submodule. Wire it into the `session(action="status")` path by extending the existing status output (or creating a new `SessionAction::ActivityReport` action).

The dashboard queries:
1. `get_injection_stats_for_session` -- session injection stats
2. `get_injection_stats_cumulative` -- all-time stats
3. `get_category_breakdown_sync` -- category counts
4. `count_tracked_sessions` -- session count
5. Existing table counts (goals, insights, tool_history)

Format as the designed dashboard output.

**Step 4: Run tests**

Run: `cargo test --lib injection && cargo test --lib session`
Expected: all pass

**Step 5: Commit**

```bash
git add -p
git commit -m "feat: enhanced status dashboard with injection activity stats"
```

---

### Task 6: Value heuristics -- correlation queries

**Files:**
- Modify: `crates/mira-server/src/db/injection.rs` (new query functions)

**Step 1: Write test for stale file re-read correlation**

```rust
#[test]
fn test_stale_reread_correlation() {
    let conn = setup_db();
    // Insert a PreToolUse injection with file hint category
    let mut record = make_record("PreToolUse", Some("s1"));
    record.categories = vec!["reread_advisory".into()];
    record.content = Some("[Mira/efficiency] You already read tasks.rs".into());
    insert_injection_sync(&conn, &record).unwrap();

    // Insert a subsequent Read tool call for the same-ish file
    conn.execute(
        "INSERT INTO tool_history (session_id, tool_name, success, created_at) VALUES ('s1', 'Read', 1, datetime('now'))",
        [],
    ).unwrap();

    let stats = compute_value_heuristics_sync(&conn, Some("s1"), None).unwrap();
    assert!(stats.file_reread_hints > 0);
}
```

**Step 2: Implement value heuristics**

```rust
#[derive(Debug, Default)]
pub struct ValueHeuristics {
    /// PreToolUse injections with reread_advisory category
    pub file_reread_hints: u64,
    /// SubagentStart injections (subagents that got pre-loaded context)
    pub subagent_context_loads: u64,
    /// Total SubagentStart hooks (including ones with no injection)
    pub subagent_total: u64,
    /// Sessions that had goal injection AND subsequent goal tool calls
    pub goal_aware_sessions: u64,
    /// Sessions that had goal injection
    pub goal_injected_sessions: u64,
}

pub fn compute_value_heuristics_sync(
    conn: &Connection,
    session_id: Option<&str>,
    project_id: Option<i64>,
) -> Result<ValueHeuristics> {
    let mut h = ValueHeuristics::default();

    // 1. File reread hints count
    h.file_reread_hints = conn.query_row(
        "SELECT COUNT(*) FROM context_injections
         WHERE hook_name = 'PreToolUse' AND categories LIKE '%reread%'
         AND (?1 IS NULL OR session_id = ?1)
         AND (?2 IS NULL OR project_id = ?2)",
        params![session_id, project_id],
        |row| row.get::<_, i64>(0),
    )? as u64;

    // 2. Subagent context loads vs total
    h.subagent_context_loads = conn.query_row(
        "SELECT COUNT(*) FROM context_injections
         WHERE hook_name = 'SubagentStart' AND chars_injected > 0
         AND (?1 IS NULL OR session_id = ?1)
         AND (?2 IS NULL OR project_id = ?2)",
        params![session_id, project_id],
        |row| row.get::<_, i64>(0),
    )? as u64;

    h.subagent_total = conn.query_row(
        "SELECT COUNT(*) FROM context_injections
         WHERE hook_name = 'SubagentStart'
         AND (?1 IS NULL OR session_id = ?1)
         AND (?2 IS NULL OR project_id = ?2)",
        params![session_id, project_id],
        |row| row.get::<_, i64>(0),
    )? as u64;

    // 3. Goal awareness: sessions with goal injection that also had goal tool calls
    h.goal_injected_sessions = conn.query_row(
        "SELECT COUNT(DISTINCT session_id) FROM context_injections
         WHERE categories LIKE '%goals%' AND chars_injected > 0
         AND (?1 IS NULL OR session_id = ?1)
         AND (?2 IS NULL OR project_id = ?2)",
        params![session_id, project_id],
        |row| row.get::<_, i64>(0),
    )? as u64;

    h.goal_aware_sessions = conn.query_row(
        "SELECT COUNT(DISTINCT ci.session_id) FROM context_injections ci
         INNER JOIN tool_history th ON th.session_id = ci.session_id AND th.tool_name = 'goal'
         WHERE ci.categories LIKE '%goals%' AND ci.chars_injected > 0
         AND (?1 IS NULL OR ci.session_id = ?1)
         AND (?2 IS NULL OR ci.project_id = ?2)",
        params![session_id, project_id],
        |row| row.get::<_, i64>(0),
    )? as u64;

    Ok(h)
}
```

**Step 3: Wire heuristics into the dashboard**

Add the value signals section to the status output from Task 5.

**Step 4: Run tests**

Run: `cargo test --lib injection`
Expected: all pass

**Step 5: Commit**

```bash
git add -p
git commit -m "feat: add value heuristic correlation queries for status dashboard"
```

---

### Task 7: Update /mira:status skill template

**Files:**
- Modify: skill file at `skills/status/SKILL.md` in the Mira plugin

**Step 1: Update the skill to include the new activity section**

The skill template should call the new dashboard. Depending on how the dashboard is exposed (as part of existing `session(action="storage_status")` or as a new action), update the template accordingly.

If added as a new section in storage_status output, no skill change needed.
If added as a separate action, add a new section to the skill:

```markdown
## Activity

!`mira tool session '{"action":"activity_report"}'`
```

**Step 2: Test the skill manually**

Run: `mira tool session '{"action":"storage_status"}'` (or the new action)
Verify the output includes session stats, cumulative stats, and value signals.

**Step 3: Commit**

```bash
git add -p
git commit -m "feat: update /mira:status skill with activity dashboard"
```

---

### Task 8: Build and verify end-to-end

**Step 1: Full build**

Run: `cargo build`
Expected: compiles with no errors

**Step 2: Full test suite**

Run: `cargo test`
Expected: all tests pass

**Step 3: Manual verification**

1. Run `mira tool session '{"action":"recap"}'` -- verify it shows correct project
2. Run `mira tool session '{"action":"storage_status"}'` -- verify activity stats appear
3. Start a Claude Code session and check stderr for `[Mira]` lines
4. Check context for `[Mira/activity]` tags

**Step 4: Final commit (if any fixups needed)**

```bash
git add -p
git commit -m "fix: observability end-to-end verification fixes"
```
