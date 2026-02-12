<!-- docs/modules/mira-server/git.md -->
# git

Git operations using git2 (branch detection) and git CLI (commits, diffs).

## Sub-modules

| Module | Purpose |
|--------|---------|
| `branch` | Branch detection via git2 with caching |
| `commit` | Commit history via git CLI (`get_recent_commits`, `get_commits_with_files`, `resolve_ref`) |
| `diff` | Diff operations via git CLI (`get_unified_diff`, `get_staged_diff`, `get_working_diff`) |

## Key Functions (branch)

- `get_git_branch()` - Cached branch lookup with 5-second TTL
- `get_git_branch_uncached()` - Direct branch detection
- `clear_branch_cache()` - Invalidate the branch cache
- `is_git_repo()` - Check if a path is inside a git repository

## Behavior

- Handles worktrees, submodules, and detached HEAD states
- Normalizes detached HEAD to `"detached"` to avoid ephemeral SHA clutter
- Branch cache uses a static `RwLock<HashMap>` with TTL-based invalidation
