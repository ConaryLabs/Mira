# ProjectContext

Represents the active project context in a Mira session. Carries the project's database ID, filesystem path, and display name.

**Crate:** `mira-types`
**Source:** `crates/mira-types/src/lib.rs`

## Definition

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectContext {
    pub id: i64,
    pub path: String,
    pub name: Option<String>,
}
```

## Fields

| Field | Type | Description |
|-------|------|-------------|
| `id` | `i64` | Persistent database ID for this project. Used as foreign key across all project-scoped data. |
| `path` | `String` | Absolute filesystem path to the project root. Used for file operations and indexing. |
| `name` | `Option<String>` | Human-readable display name. Auto-detected from `Cargo.toml`, `package.json`, or directory name. |

## Usage

`ProjectContext` is the primary way project identity flows through the system. It's set during `project(action="start")` or `project(action="set")` and referenced by tools that need project-scoped data.

```rust
let ctx = ProjectContext {
    id: 5,
    path: "/home/user/myproject".into(),
    name: Some("myproject".into()),
};
```

## Serialization

```json
{
  "id": 5,
  "path": "/home/user/myproject",
  "name": "myproject"
}
```

The `name` field is omitted from JSON when `None`.

## See Also

- [project](../tools/project.md) - Initialize and manage project context
- [index](../tools/index.md) - Index a project for code intelligence
