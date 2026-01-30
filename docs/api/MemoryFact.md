# MemoryFact

Represents a stored memory in Mira's semantic memory system. Memories are facts, preferences, decisions, or context stored for recall across sessions.

**Crate:** `mira-types`
**Source:** `crates/mira-types/src/lib.rs`

## Definition

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFact {
    pub id: i64,
    pub project_id: Option<i64>,
    pub key: Option<String>,
    pub content: String,
    pub fact_type: String,
    pub category: Option<String>,
    pub confidence: f64,
    pub created_at: String,
    pub session_count: i32,
    pub first_session_id: Option<String>,
    pub last_session_id: Option<String>,
    pub status: String,
    pub user_id: Option<String>,
    pub scope: String,
    pub team_id: Option<i64>,
}
```

## Fields

### Core Fields

| Field | Type | Description |
|-------|------|-------------|
| `id` | `i64` | Database primary key |
| `project_id` | `Option<i64>` | Associated project (None for global memories) |
| `key` | `Option<String>` | Optional upsert key for deduplication |
| `content` | `String` | The memory content text |
| `fact_type` | `String` | Type: `preference`, `decision`, `context`, `general` |
| `category` | `Option<String>` | Optional category for filtering |
| `confidence` | `f64` | Confidence score (0.0-1.0) |
| `created_at` | `String` | ISO timestamp of creation |

### Evidence-Based Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `session_count` | `i32` | `1` | Number of sessions that have reinforced this memory |
| `first_session_id` | `Option<String>` | `None` | Session where this memory was first created |
| `last_session_id` | `Option<String>` | `None` | Most recent session that touched this memory |
| `status` | `String` | `"candidate"` | Memory status: `candidate`, `confirmed`, `archived` |

### Multi-User Sharing Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `user_id` | `Option<String>` | `None` | Creator's identity |
| `scope` | `String` | `"project"` | Visibility: `personal`, `project`, `team` |
| `team_id` | `Option<i64>` | `None` | Team ID for team-scoped memories |

## Defaults

- `session_count` defaults to `1`
- `status` defaults to `"candidate"`
- `scope` defaults to `"project"`

## Usage

Returned by the `recall` tool and stored by the `remember` tool:

```rust
let fact = MemoryFact {
    id: 42,
    project_id: Some(1),
    key: None,
    content: "Use builder pattern for Config struct".into(),
    fact_type: "decision".into(),
    category: Some("architecture".into()),
    confidence: 0.9,
    created_at: "2025-01-15T10:30:00Z".into(),
    session_count: 3,
    first_session_id: Some("abc-123".into()),
    last_session_id: Some("def-456".into()),
    status: "confirmed".into(),
    user_id: None,
    scope: "project".into(),
    team_id: None,
};
```

## See Also

- [remember](../tools/remember.md) - Store new memories
- [recall](../tools/recall.md) - Search memories by semantic similarity
- [forget](../tools/forget.md) - Delete a memory by ID
