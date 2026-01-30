# git

Git operations using the git2 crate with caching.

## Key Functions

- `get_git_branch()` - Cached branch lookup with 5-second TTL
- `get_git_branch_uncached()` - Direct branch detection
- `clear_branch_cache()` - Invalidate the branch cache
- `is_git_repo()` - Check if a path is inside a git repository

## Behavior

- Handles worktrees, submodules, and detached HEAD states
- Normalizes detached HEAD to `"detached"` to avoid ephemeral SHA clutter
- Branch cache uses a static `RwLock<HashMap>` with TTL-based invalidation
