# Migrations README

This repo is now on a “baseline-only + forward migrations” model. Since you’re fine nuking mira.db, keep the single baseline schema and add new migrations on top. Don’t keep the old stuff in the runner’s path.

TL;DR
- Keep only: migrations/20251010_baseline_v2.sql
- Move older .sql files to migrations/archive (or delete) so the runner won’t apply them.
- To reset: rm -f mira.db && sqlite3 mira.db < migrations/20251010_baseline_v2.sql
- Then start the backend and sanity-check the DB.
- Future changes: create new migrations (SQLx or manual .sql) and apply on top of the baseline.

Files we rely on
- migrations/20251010_baseline_v2.sql
  - Salience is 0.0–1.0 with CHECK constraints
  - routed_to_heads/topics default to '[]' and NOT NULL
  - websocket_* tables use INTEGER epoch created_at + indexes
  - message_embeddings head includes 'documents'
  - llm_metadata has reasoning_tokens
  - schema_metadata is populated with the current version (e.g., 2.2.0)

Fresh DB init
Option A: sqlite3 (simple, recommended for baseline)
1) Stop the backend
2) rm -f mira.db
3) sqlite3 mira.db < migrations/20251010_baseline_v2.sql
4) Start the backend

Option B: SQLx CLI (if you want the DB file created by SQLx)
1) Stop the backend
2) rm -f mira.db
3) sqlx database create --database-url sqlite://mira.db
4) Apply the baseline one of two ways:
   - EITHER via sqlite3 (still fine): sqlite3 mira.db < migrations/20251010_baseline_v2.sql
   - OR rename the baseline to run first under SQLx and then run: sqlx migrate run --database-url sqlite://mira.db
     Notes:
     - If you go this route, ensure only the baseline file is in migrations/ so SQLx doesn’t try to run legacy junk.
     - SQLx just cares about filename ordering; naming like 20251010_000001_baseline_v2.sql is acceptable.

Runner behavior (important)
- If your runner auto-applies every .sql in migrations/ by timestamp, keep ONLY the baseline there for a fresh DB.
- Archive old files:
  mkdir -p migrations/archive
  git mv migrations/*.sql migrations/archive/  # then move migrations/20251010_baseline_v2.sql back
- Alternatively, keep old files elsewhere so they don’t get picked up.

Sanity checks (after init)
- PRAGMA foreign_key_check;  → should be empty
- SELECT COUNT(*) FROM message_analysis WHERE salience > 1.0; → 0
- SELECT DISTINCT embedding_head FROM message_embeddings; → empty on a brand new DB (will populate later)
- SELECT typeof(created_at) FROM websocket_calls LIMIT 5; → 'integer'
- Schema version:
  SELECT version FROM schema_metadata ORDER BY applied_at DESC LIMIT 1;  → 2.2.0 (or whatever baseline sets)

Runtime PRAGMAs (SQLite best practice)
- Ensure the app sets these at startup (and you can also keep them in the baseline):
  PRAGMA foreign_keys = ON;
  PRAGMA journal_mode = WAL;
  PRAGMA synchronous = NORMAL;

Adding new migrations (post-baseline)
- Using SQLx:
  - Create a new migration:
    sqlx migrate add <short_name>
  - Edit the generated SQL to include your schema changes (up.sql and down.sql if you used -r/reversible).
  - Apply:
    sqlx migrate run --database-url sqlite://mira.db
- Manual .sql:
  - Drop a new timestamped .sql into migrations/ (make sure ordering is after the baseline).
  - Apply with sqlite3 or wire your runner to execute it.
- Keep the old baseline in place, don’t re-run it. Future migrations should assume the baseline schema is present.

Troubleshooting
- If SQLx complains about unknown migrations while you’ve already applied the baseline manually, that’s fine: SQLx only tracks files it has applied. As long as you keep only new migrations in migrations/, SQLx will create its own ledger and apply just those.
- If you accidentally left legacy migrations in the folder and the runner applied them first (bringing back the 0–10 salience), just nuke the DB and re-run the baseline correctly.
- If artifact viewer/migration runner ordering is weird, verify the folder contains only what you intend to run and filenames are strictly increasing.

Quick reset script (copy/paste)
- sqlite3 path:
  pkill mira-backend || true
  rm -f mira.db
  sqlite3 mira.db < migrations/20251010_baseline_v2.sql
  ./mira-backend &

- SQLx database create + sqlite3 apply:
  pkill mira-backend || true
  rm -f mira.db
  sqlx database create --database-url sqlite://mira.db
  sqlite3 mira.db < migrations/20251010_baseline_v2.sql
  ./mira-backend &

Notes for Future You
- Baseline is the source of truth for a fresh DB. Don’t let older migrations run on a new instance.
- Salience is 0.0–1.0 across the stack. routed_to_heads/topics default to non-null empty arrays.
- WebSocket tables indexed by created_at. Embedding heads include 'documents'.
- When in doubt: nuke, apply baseline, sanity check, proceed.
