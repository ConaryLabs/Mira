---
name: diff
description: This skill should be used when the user asks "analyze my changes", "what did I change", "review my diff", "impact of changes", or wants semantic analysis of code changes.
---

# Semantic Diff Analysis

Analyze git changes semantically with classification, impact analysis, and risk assessment.

**Arguments:** $ARGUMENTS

## Instructions

1. Parse optional arguments:
   - `--from REF` → Starting git ref (default: HEAD~1 or staged changes)
   - `--to REF` → Ending git ref (default: HEAD)
   - `--no-impact` → Skip impact analysis (faster)

2. Use the `mcp__mira__code` tool:
   ```
   code(action="diff", from_ref="...", to_ref="...", include_impact=true)
   ```

3. Present results in sections:

### Change Classification
- **NewFunction**: Entirely new functions/methods
- **ModifiedFunction**: Changed existing functions
- **DeletedFunction**: Removed functions
- **NewFile**: New files added
- **Refactored**: Structural changes without behavior change

### Impact Analysis
- What callers are affected by these changes
- Which modules depend on modified code
- Potential ripple effects

### Risk Assessment
- **Breaking changes**: API signature changes, removed exports
- **Security relevance**: Auth, input validation, crypto changes
- **Test coverage**: Are changes covered by tests?

## Examples

```
/mira:diff
→ Analyzes staged/working changes vs HEAD

/mira:diff --from main
→ Analyzes current branch vs main

/mira:diff --from v1.0 --to v1.1
→ Analyzes changes between tags

/mira:diff --no-impact
→ Quick classification without call graph analysis
```

## Example Output

```
## Semantic Diff Analysis

### Changes (5 files, +142 -38 lines)

**New Functions:**
- `validate_token()` in src/auth.rs:45

**Modified Functions:**
- `handle_login()` in src/auth.rs:23 (added rate limiting)
- `create_session()` in src/session.rs:67 (changed return type)

### Impact Analysis
- `handle_login()` is called by 3 endpoints
- `create_session()` change affects 12 callers

### Risk Assessment
⚠️ **Breaking**: `create_session()` return type changed
🔒 **Security**: Rate limiting added (positive)
```
