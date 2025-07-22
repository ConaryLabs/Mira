# Mira Project Roadmap

## Completed Sprints

### Sprint 1: LLM-Driven Semantic Memory & Modular Backend Refactor (**✅ Completed July 22, 2025**)

#### Objective
Implement a rich, emotionally aware memory system where **GPT-4.1 handles absolutely everything related to memory/recall**—no keywords, no brittle heuristics, no random prompt hacks, no "assistant mode" ever.

#### Delivered Features
- ✅ Dual memory system (SQLite for chronological, Qdrant for semantic)
- ✅ Full GPT-4.1 integration with function calling
- ✅ Memory evaluation pipeline (salience, tags, summary, type)
- ✅ Embeddings for all messages (text-embedding-3-large)
- ✅ Semantic search and recall
- ✅ Moderation API integration
- ✅ Modular backend structure
- ✅ Persona system with mood tracking
- ✅ Never drops into assistant mode

#### Key Technical Achievements
- Every message evaluated by GPT-4.1 before storage
- High-salience memories (≥3) stored in Qdrant
- Context building combines recent (15) + semantic (5) memories
- Clean module separation (api/, memory/, llm/, persona/, etc.)
- Proper error handling throughout

---

## Future Sprints

### Sprint 2: Real-Time UX & Persona System Polish

- WebSocket streaming (Axum, chunked output)
- Typing indicators, live persona/mood badges
- Persona overlay switching (live, LLM-triggered or manual)
- Chat UI refinement, API endpoint polish
- HTTP fallback endpoints for bots/scripts/batch/admin

### Sprint 3: Project/Artifact Management

- Upload/browse project files/artifacts (API endpoints)
- Project-aware chat and context
- Project switcher, file context separation

### Sprint 4: Scale-Out & Infrastructure

- Optional: Switch SQLite to Postgres for larger deployments
- Clustered Qdrant, multi-user support, API rate limiting
- Harden authentication, add API docs

---

## Technical Debt & Improvements

- [ ] Add comprehensive test coverage
- [ ] Implement proper logging (not just eprintln!)
- [ ] Add metrics/monitoring
- [ ] Document API with OpenAPI/Swagger
- [ ] Optimize embedding batch processing
- [ ] Add conversation summarization for long-term storage

---

## Version History

- **Sprint 1 (v0.2)**: Completed July 22, 2025 - Full semantic memory system
- **Sprint 0 (v0.1)**: Completed July 18, 2025 - Basic chat with personas

---

*This roadmap is a living document. Each sprint builds on the last, maintaining Mira's core identity: never a bot, always herself.*
