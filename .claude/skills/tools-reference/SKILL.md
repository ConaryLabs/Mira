# Mira Consolidated Tools Reference

Mira uses action-based tools to reduce cognitive load. Reference for tool signatures and workflows.

## `project` - Project/Session Management

```
project(action="start", project_path="...", name="...")  # Initialize session
project(action="set", project_path="...", name="...")    # Change active project
project(action="get")                                     # Show current project
```

## `finding` - Code Review Findings

```
finding(action="list", status="pending")                  # List findings
finding(action="get", finding_id=123)                     # Get finding details
finding(action="review", finding_id=123, status="accepted", feedback="...")  # Review single
finding(action="review", finding_ids=[1,2,3], status="rejected")  # Bulk review
finding(action="stats")                                   # Get statistics
finding(action="patterns")                                # Get learned patterns
finding(action="extract")                                 # Extract patterns from accepted findings
```

## `documentation` - Documentation Tasks

Claude Code writes documentation directly (no expert system).

```
documentation(action="list", status="pending")            # List doc tasks
documentation(action="get", task_id=123)                  # Get task details + guidelines
documentation(action="complete", task_id=123)             # Mark done after writing
documentation(action="skip", task_id=123, reason="...")   # Skip a task
documentation(action="inventory")                         # Show doc inventory
documentation(action="scan")                              # Trigger doc scan
```

**Workflow:**
1. `documentation(action="list")` - See what needs docs
2. `documentation(action="get", task_id=N)` - Get source path, target path, guidelines
3. Read the source file, write the documentation
4. `documentation(action="complete", task_id=N)` - Mark done

## `consult_experts` - Expert Consultation

```
consult_experts(roles=["architect"], context="...", question="...")
consult_experts(roles=["code_reviewer", "security"], context="...")  # Multiple
```
