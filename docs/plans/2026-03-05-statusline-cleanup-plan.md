# Status Line Redesign and Recipe Cleanup Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove stale recipe references and redesign the status line to show value-focused metrics from the injection tracking system.

**Architecture:** Clean up dead references in README/skills/settings, then rewrite `statusline.rs` to query `context_injections` and `server_state` tables for session-scoped (with all-time fallback) value signals. Same stdin/stdout architecture, same < 50ms target.

**Tech Stack:** Rust, SQLite (rusqlite), ANSI escape codes.

**Design doc:** `docs/plans/2026-03-05-statusline-cleanup-design.md`

---

### Task 1: Remove stale recipe references

**Files:**
- Modify: `plugin/README.md:82-93`
- Modify: `plugin/skills/qa-hardening/SKILL.md:82-83`
- Modify: `.claude/settings.local.json:315`

**Step 1: Update README MCP tools table**

In `plugin/README.md`, change line 82 from `8 MCP tools` to `7 MCP tools`, and remove the recipe row (line 93):

```
| recipe | list, get | Agentic team workflow recipes |
```

Replace the table with these 7 tools (the `launch` tool replaced recipe):

```markdown
| Tool | Actions | Purpose |
|------|---------|---------|
| `code` | search, symbols, callers, callees | Code intelligence: semantic search + call graph |
| `diff` | *(single purpose)* | Semantic git diff analysis with impact assessment |
| `project` | start, get | Project/session management |
| `session` | current_session, recap | Session context and management |
| `insights` | insights, dismiss_insight | Background analysis digest and health dashboard |
| `goal` | create, list, update, add_milestone, complete_milestone, ... | Cross-session goal tracking |
| `index` | project, file, status | Code indexing |
| `launch` | *(single purpose)* | Launch agent teams from `.claude/agents/` definitions |
```

**Step 2: Update qa-hardening skill example**

In `plugin/skills/qa-hardening/SKILL.md`, lines 82-83 show a recipe example:

```
/mira:qa-hardening Review the recipe system
-> All 4 agents review the recipe code for production readiness
```

Replace with a generic example:

```
/mira:qa-hardening Review the authentication module
-> All 4 agents review the auth code for production readiness
```

**Step 3: Remove recipe permission from settings**

In `.claude/settings.local.json`, line 315, remove:

```
"mcp__plugin_mira_mira__recipe"
```

Make sure the comma on the preceding line is handled correctly (line 314 `"Bash(test:*)"` should NOT have a trailing comma if recipe was the last item; check the JSON structure).

**Step 4: Verify**

Run: `grep -rn "recipe" plugin/ .claude/settings.local.json --include="*.md" --include="*.json" | grep -v CHANGELOG | grep -v node_modules`
Expected: no results (CHANGELOG mentions are fine, they're historical)

**Step 5: Commit**

```bash
git add plugin/README.md plugin/skills/qa-hardening/SKILL.md .claude/settings.local.json
git commit -m "chore: remove stale recipe references from README, skills, settings"
```

---

### Task 2: Add injection query functions to statusline

**Files:**
- Modify: `crates/mira-server/src/cli/statusline.rs`

**Step 1: Add session ID query function**

After the existing `query_pending` function (line 104), add:

```rust
/// Get the active session ID from server_state.
fn query_active_session(conn: &Connection) -> Option<String> {
    conn.query_row(
        "SELECT value FROM server_state WHERE key = 'active_session_id'",
        [],
        |r| r.get(0),
    )
    .ok()
}
```

**Step 2: Add assists count query**

```rust
/// Count non-deduped context injections (assists).
/// If session_id is Some, scopes to that session; otherwise uses project_id.
fn query_assists(conn: &Connection, session_id: Option<&str>, project_id: i64) -> i64 {
    if let Some(sid) = session_id {
        conn.query_row(
            "SELECT COUNT(*) FROM context_injections \
             WHERE session_id = ?1 AND was_deduped = 0",
            [sid],
            |r| r.get(0),
        )
        .unwrap_or(0)
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM context_injections \
             WHERE project_id = ?1 AND was_deduped = 0",
            [project_id],
            |r| r.get(0),
        )
        .unwrap_or(0)
    }
}
```

**Step 3: Add subagent context stats query**

```rust
/// Query subagent context hit rate.
/// Returns (loads_with_context, total_subagent_starts).
fn query_subagent_stats(conn: &Connection, session_id: Option<&str>, project_id: i64) -> (i64, i64) {
    let (loads, total) = if let Some(sid) = session_id {
        let loads = conn.query_row(
            "SELECT COUNT(*) FROM context_injections \
             WHERE hook_name = 'SubagentStart' AND chars_injected > 0 AND session_id = ?1",
            [sid],
            |r| r.get::<_, i64>(0),
        ).unwrap_or(0);
        let total = conn.query_row(
            "SELECT COUNT(*) FROM context_injections \
             WHERE hook_name = 'SubagentStart' AND session_id = ?1",
            [sid],
            |r| r.get::<_, i64>(0),
        ).unwrap_or(0);
        (loads, total)
    } else {
        let loads = conn.query_row(
            "SELECT COUNT(*) FROM context_injections \
             WHERE hook_name = 'SubagentStart' AND chars_injected > 0 AND project_id = ?1",
            [project_id],
            |r| r.get::<_, i64>(0),
        ).unwrap_or(0);
        let total = conn.query_row(
            "SELECT COUNT(*) FROM context_injections \
             WHERE hook_name = 'SubagentStart' AND project_id = ?1",
            [project_id],
            |r| r.get::<_, i64>(0),
        ).unwrap_or(0);
        (loads, total)
    };
    (loads, total)
}
```

**Step 4: Verify compilation**

Run: `cargo check`
Expected: compiles (functions exist but aren't called yet)

**Step 5: Commit**

```bash
git add crates/mira-server/src/cli/statusline.rs
git commit -m "feat: add injection query functions for status line"
```

---

### Task 3: Rewrite status line display

**Files:**
- Modify: `crates/mira-server/src/cli/statusline.rs`

**Step 1: Remove rainbow_mira function and RAINBOW constant**

Delete the `RAINBOW` constant (lines 17-22) and the `rainbow_mira()` function (lines 27-36). They are no longer needed.

**Step 2: Update the `run()` function**

Replace the display section of `run()` (from line 140 `let mira_label = rainbow_mira();` through line 201 `Ok(())`) with:

```rust
    let mira_label = format!("{DIM}Mira{RESET}");

    if !main_db.exists() {
        return Ok(());
    }

    let main_conn = match open_readonly(&main_db) {
        Some(c) => c,
        None => return Ok(()),
    };

    // Resolve project from cwd
    let project = cwd
        .as_deref()
        .and_then(|cwd| resolve_project(&main_conn, cwd));

    let (project_id, _) = match project {
        Some((id, name)) => (id, name),
        None => {
            println!("{mira_label} {DIM}no project{RESET}");
            return Ok(());
        }
    };

    // Determine session scope: active session first, fall back to project-wide
    let session_id = query_active_session(&main_conn);

    // Query value-focused stats from main DB
    let assists = query_assists(&main_conn, session_id.as_deref(), project_id);
    let (subagent_loads, subagent_total) = query_subagent_stats(&main_conn, session_id.as_deref(), project_id);
    let goals = query_goals(&main_conn, project_id);
    let alerts = query_alerts(&main_conn, project_id);

    // Query stats from code DB
    let code_conn = open_readonly(&code_db);
    let indexed = code_conn
        .as_ref()
        .map(|c| query_indexed_files(c, project_id))
        .unwrap_or(0);
    let pending = code_conn
        .as_ref()
        .map(|c| query_pending(c, project_id))
        .unwrap_or(0);

    // Build output: assists, subagent ctx, goals, indexed, pending, alerts
    let mut parts = Vec::new();

    if assists > 0 {
        parts.push(format!("{GREEN}{assists}{RESET} assists"));
    }

    if subagent_total > 0 {
        let pct = (subagent_loads as f64 / subagent_total as f64 * 100.0).round() as i64;
        parts.push(format!("{GREEN}{pct}%{RESET} subagent ctx"));
    }

    if goals > 0 {
        parts.push(format!("{GREEN}{goals}{RESET} goals"));
    }

    if indexed > 0 {
        parts.push(format!("{DIM}{indexed}{RESET} indexed"));
    }

    if pending > 0 {
        parts.push(format!("{YELLOW}{pending} pending{RESET}"));
    }

    if alerts > 0 {
        parts.push(format!("{YELLOW}{alerts} alerts{RESET}"));
    }

    if parts.is_empty() {
        println!("{mira_label}");
    } else {
        let joined = parts.join(DOT);
        println!("{mira_label}{DOT}{joined}");
    }

    Ok(())
```

**Step 3: Update tests**

Remove `test_rainbow_mira_contains_all_chars` test (lines 236-249) since `rainbow_mira()` no longer exists. The `format_duration` tests can stay (they're `#[cfg(test)]` utility tests).

**Step 4: Verify**

Run: `cargo check && cargo test --lib statusline`
Expected: compiles and remaining tests pass

**Step 5: Commit**

```bash
git add crates/mira-server/src/cli/statusline.rs
git commit -m "feat: redesign status line with value-focused metrics and dim prefix"
```

---

### Task 4: Build and verify end-to-end

**Step 1: Full build**

Run: `cargo build`
Expected: compiles with no errors

**Step 2: Full test suite**

Run: `cargo test`
Expected: all tests pass

**Step 3: Manual verification**

Run: `echo '{"cwd":"/home/peter/Mira"}' | ./target/debug/mira statusline`

Expected output (approximate, depends on DB state):
```
Mira . 8 assists . 89% subagent ctx . 3 goals . 1204 indexed
```

Verify:
- "Mira" is dim (not rainbow)
- Assists count appears (green number)
- Subagent context percentage appears if any subagent hooks have fired
- Goals, indexed shown as before
- No recipe references anywhere

**Step 4: Verify recipe cleanup**

Run: `grep -rn "recipe" plugin/ .claude/settings.local.json --include="*.md" --include="*.json" | grep -v CHANGELOG`
Expected: no results

**Step 5: Commit (if any fixups needed)**

```bash
git add -p
git commit -m "fix: end-to-end verification fixes"
```
