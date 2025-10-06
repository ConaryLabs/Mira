# Holistic Codebase Understanding System
## Implementation Guide

---

## Problem Statement

We need a system that can provide LLMs (and users) with comprehensive, multi-level views of a codebase to support:

- **Documentation Generation**: Full-context understanding to write meaningful docs
- **Duplication Detection**: Finding redundant code/patterns across the codebase
- **Architectural Analysis**: Big-picture discussions about structure, patterns, opportunities
- **Refactoring Opportunities**: Identifying places where rewrites or consolidation would help
- **Dependency Analysis**: Understanding what depends on what, impact analysis

The existing code intelligence system extracts structured data (functions, complexity, calls), but we need a layer that aggregates and presents this data for holistic reasoning.

---

## Goals

1. **Multi-level Abstraction**: From 10,000-foot overview down to specific implementation details
2. **Context-Aware Querying**: Pull the right level of detail for the question being asked
3. **LLM-Optimized Output**: Format data in ways that LLMs can reason about effectively
4. **Incremental Updates**: Keep the view fresh as code changes
5. **Scalability**: Work with small repos (100 files) and large ones (10k+ files)

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    User Query/Request                       │
│          "Find duplication" / "Generate docs"               │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                   Query Orchestrator                        │
│       - Determines scope and detail level needed            │
│       - Decides which layers to fetch                       │
└───────────────────────────┬─────────────────────────────────┘
                            │
        ┌───────────────────┼───────────────────┐
        │                   │                   │
        ▼                   ▼                   ▼
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│   Summary    │    │    Graph     │    │    Detail    │
│    Layer     │    │    Layer     │    │    Layer     │
│              │    │              │    │              │
│ - Module     │    │ - Call       │    │ - Full       │
│   overviews  │    │   graphs     │    │   function   │
│ - Key        │    │ - Dependency │    │   bodies     │
│   patterns   │    │   trees      │    │ - Complexity │
│ - Stats      │    │ - Data flow  │    │   metrics    │
└──────────────┘    └──────────────┘    └──────────────┘
        │                   │                   │
        └───────────────────┴───────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│              Code Intelligence Database                     │
│   (Existing: files, functions, calls, complexity, etc.)     │
└─────────────────────────────────────────────────────────────┘
```

---

## Component Breakdown

### 1. Summary Layer
**Purpose**: High-level overview of the codebase structure and patterns

**Data Structures**:
```rust
struct CodebaseSummary {
    repo_id: String,
    total_files: usize,
    languages: HashMap<String, LanguageStats>,
    module_structure: Vec<ModuleSummary>,
    key_patterns: Vec<Pattern>,
    generated_at: DateTime<Utc>,
}

struct ModuleSummary {
    path: String,
    purpose: Option<String>,  // AI-generated or from comments
    entry_points: Vec<String>,
    key_types: Vec<String>,
    external_dependencies: Vec<String>,
    complexity_score: f32,
}

struct Pattern {
    pattern_type: PatternType,  // Singleton, Factory, Observer, etc.
    locations: Vec<String>,
    confidence: f32,
}
```

**Generation Strategy**:
- Run periodically or on-demand
- Use LLM to summarize module purposes from file structure + comments
- Extract patterns from code intelligence data
- Cache aggressively

### 2. Graph Layer
**Purpose**: Relationships and dependencies

**Data Structures**:
```rust
struct DependencyGraph {
    nodes: HashMap<NodeId, Node>,
    edges: Vec<Edge>,
}

struct Node {
    id: NodeId,
    node_type: NodeType,  // File, Function, Struct, Trait
    name: String,
    file_path: String,
}

struct Edge {
    from: NodeId,
    to: NodeId,
    edge_type: EdgeType,  // Calls, Imports, Implements, References
    weight: f32,  // Frequency or importance
}
```

**Capabilities**:
- **Call graph**: Who calls what
- **Import graph**: File dependencies
- **Type hierarchy**: Trait implementations, inheritance
- **Data flow**: Where data structures are created/consumed

### 3. Detail Layer
**Purpose**: Full information about specific code elements

**Sources**:
- Existing code intelligence tables (functions, structs, etc.)
- Full file contents when needed
- Parsed AST for deep queries

**Query Types**:
- "Give me full details on this function"
- "Show me all implementations of this trait"
- "What are the complexity metrics for this file?"

---

## Implementation Phases

### Phase 1: Summary Layer MVP
**Goal**: Generate basic codebase summaries

**Tasks**:
- [ ] Create `codebase_summaries` table
- [ ] Build summary generator that aggregates code intelligence data
- [ ] Create module-level summaries (file count, key functions, complexity)
- [ ] Add endpoint: `GET /api/repos/{id}/summary`
- [ ] Test with existing mira-backend repo

**Success Criteria**: Can generate a 1-page overview of a codebase's structure

### Phase 2: Graph Layer
**Goal**: Build queryable dependency graphs

**Tasks**:
- [ ] Design graph storage (adjacency list? Neo4j? In-memory?)
- [ ] Extract call relationships from code intelligence data
- [ ] Build graph query API
- [ ] Visualizations (optional, but helpful)
- [ ] Add endpoint: `GET /api/repos/{id}/graph?type=calls&root=function_name`

**Success Criteria**: Can answer "what calls this function?" and "what does this function call?"

### Phase 3: LLM Integration
**Goal**: Feed codebase understanding to LLM contexts

**Tasks**:
- [ ] Create context builder that selects appropriate detail level
- [ ] Format output for LLM consumption (structured prompts)
- [ ] Implement query orchestrator logic
- [ ] Add intelligent context expansion (start summary, drill down as needed)
- [ ] Test with real queries like "find duplication" or "generate docs"

**Success Criteria**: LLM can reason about codebase architecture with limited context window

### Phase 4: Pattern Detection & Refactoring Analysis
**Goal**: Identify opportunities and problems

**Tasks**:
- [ ] Duplication detector (AST-based similarity)
- [ ] Pattern recognizer (design patterns, anti-patterns)
- [ ] Refactoring opportunity finder (high complexity + high duplication)
- [ ] Add endpoint: `GET /api/repos/{id}/analysis?type=duplication`

**Success Criteria**: Can identify duplicate code and suggest consolidation

---

## Key Technical Decisions

### 1. Graph Storage

**Options**:
- **PostgreSQL with recursive CTEs**: Keep everything in existing DB, use SQL for graph queries
- **Neo4j/Graph DB**: Purpose-built for graph queries, but adds complexity
- **In-memory (petgraph)**: Fast, but needs rebuilding on startup

**Recommendation**: Start with PostgreSQL, move to graph DB if performance demands it

### 2. Summary Generation Frequency

**Options**:
- **On-demand**: Generate when requested
- **On-push**: Regenerate when code changes
- **Periodic**: Background job every N hours

**Recommendation**: On-demand with aggressive caching, invalidate on code changes

### 3. LLM Context Strategy

**Options**:
- **Kitchen sink**: Give LLM everything, let it figure it out
- **Progressive disclosure**: Start with summary, let LLM request more detail
- **Pre-filtered**: Use heuristics to select relevant files before LLM sees anything

**Recommendation**: Progressive disclosure - Summary → Graph → Detail as needed

### 4. Duplication Detection Approach

**Options**:
- **Token-based**: Compare sequences of tokens (fast, simple)
- **AST-based**: Compare tree structures (more accurate, slower)
- **Semantic**: Use embeddings to find functionally similar code

**Recommendation**: Start with AST-based, add semantic as enhancement

---

## Open Questions

1. **Monorepo handling**: Do we summarize the whole thing or treat subdirectories as separate projects?

2. **UX layer**: Is this API-only, or do we need a frontend visualization layer?

3. **Module boundaries**: Cargo.toml for Rust is obvious, but what about JS/TS?

4. **Pattern detection accuracy**: Do we need training data, or can we use rule-based detection?

5. **Multi-language support**: Do we build this for one language first, or design for multiple from the start?

---

## Next Steps

- [ ] Review and refine this document
- [ ] Decide on Phase 1 scope
- [ ] Prototype summary generation on mira-backend
- [ ] Test LLM consumption of summary data
- [ ] Iterate based on real usage