# Expert-Driven Documentation Generation

## Current Flow

```
Background Service (continuous)
    │
    ├── detection.rs: scan_documentation_gaps()
    │   └── Finds undocumented tools, modules, stale docs
    │
    ├── generation.rs: generate_pending_drafts()
    │   └── Calls LLM (Gemini) to generate drafts
    │   └── Problem: produces low-quality "Not documented" placeholders
    │
    └── Stores drafts in doc_tasks table
```

## Proposed Flow

```
Background Service (cheap, continuous)
    │
    ├── detection.rs: scan_documentation_gaps()
    │   └── Same as before - find gaps
    │
    └── Creates tasks with status='pending' (NO draft generation)


User/Claude (on-demand)
    │
    ├── list_doc_tasks() → see what needs docs
    │
    ├── generate_doc_with_expert(task_id) ← NEW
    │   └── Calls documentation expert
    │   └── Expert explores codebase (35+ tool calls)
    │   └── Produces high-quality draft
    │   └── Stores draft, status='draft_ready'
    │
    ├── review_doc_draft(task_id) → review expert output
    │
    └── apply_doc_draft(task_id) → write to file
```

## Changes Required

### 1. New Expert Role: `DocumentationWriter`

**File:** `crates/mira-server/src/tools/core/experts.rs`

Add new expert role:

```rust
pub enum ExpertRole {
    Architect,
    PlanReviewer,
    ScopeAnalyst,
    CodeReviewer,
    Security,
    DocumentationWriter,  // NEW
}
```

**Prompt focus:**
- Write clear, comprehensive documentation
- Explore codebase to understand behavior (not just schema)
- Include examples, edge cases, limitations
- Follow consistent markdown format
- Link to related tools/modules

### 2. New Tool: `generate_doc_with_expert`

**File:** `crates/mira-server/src/tools/core/documentation.rs`

```rust
pub async fn generate_doc_with_expert<C: ToolContext>(
    ctx: &C,
    task_id: i64,
) -> Result<String, String> {
    // 1. Get task details from DB
    let task = get_doc_task(ctx.db(), task_id)?;

    // 2. Build context based on doc_type
    let context = match task.doc_type.as_str() {
        "mcp_tool" => build_tool_context(ctx, &task).await?,
        "module" => build_module_context(ctx, &task).await?,
        _ => return Err("Unknown doc type".into()),
    };

    // 3. Call documentation expert
    let expert = ExpertRole::DocumentationWriter;
    let question = format!(
        "Generate documentation for {} at {}",
        task.source_identifier, task.target_doc_path
    );

    let draft = consult_expert(ctx, expert, context, Some(question)).await?;

    // 4. Store draft
    store_doc_draft(ctx.db(), task_id, &draft)?;

    Ok(format!("Generated draft for {}. Review with review_doc_draft({})",
               task.target_doc_path, task_id))
}
```

### 3. Disable Background Draft Generation

**File:** `crates/mira-server/src/background/documentation/generation.rs`

Option A: Remove entirely (recommended)
- Delete `generate_pending_drafts()` and related functions
- Background service only does detection

Option B: Make it configurable
- Add config flag `enable_background_drafts: bool`
- Default to false

### 4. MCP Tool Registration

**File:** `crates/mira-server/src/mcp/tools.rs`

Add new tool:

```rust
ToolInfo {
    name: "generate_doc_with_expert",
    description: "Generate high-quality documentation using an expert agent",
    input_schema: json!({
        "type": "object",
        "properties": {
            "task_id": {
                "type": "integer",
                "description": "Task ID from list_doc_tasks"
            }
        },
        "required": ["task_id"]
    }),
}
```

## Documentation Expert Prompt

```
You are a technical documentation writer. Your task is to create clear,
comprehensive documentation for code components.

## Process

1. EXPLORE the codebase to understand how the component works
   - Read the implementation code
   - Find related components
   - Understand the data flow

2. DOCUMENT with these sections:
   - Purpose: What problem does this solve?
   - Parameters: All inputs with types, defaults, constraints
   - Behavior: How it works, including edge cases
   - Examples: 2-3 realistic usage examples
   - Errors: What can go wrong and why
   - Related: Links to related tools/modules

3. QUALITY standards:
   - Be specific, not generic
   - Explain the "why", not just the "what"
   - Include gotchas and limitations
   - Never say "not documented" - explore to find out

## Output Format

Return markdown suitable for a docs/ file. Use code blocks for examples.
```

## Benefits

1. **Quality**: Expert explores codebase, produces comprehensive docs
2. **Cost**: Only generate docs when user requests (not continuous)
3. **Control**: User decides what to document and when
4. **Consistency**: Single expert prompt ensures consistent style

## Migration

1. Existing `draft_ready` tasks: Keep them, user can regenerate with expert if unhappy
2. Existing `pending` tasks: Will use new expert flow
3. Background service: Continues scanning for new gaps

## Usage After Implementation

```
User: What docs need writing?
Claude: *calls list_doc_tasks()*

User: Generate docs for the remember tool
Claude: *calls generate_doc_with_expert(task_id=X)*
        *expert explores codebase, generates draft*
        Draft ready. Review with review_doc_draft(X)

User: Let me see it
Claude: *calls review_doc_draft(X)*
        [shows high-quality markdown]

User: Looks good, apply it
Claude: *calls apply_doc_draft(X)*
        Written to docs/tools/remember.md
```
