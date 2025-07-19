# Mira Backend Whitepaper (v0.1)

**Date:** July 18, 2025  
**Maintainer:** Peter Permenter  
**Repo:** https://github.com/ConaryLabs/Mira

---

## Vision & Purpose

Mira aims to be the first chat AI you’d actually *miss*—emotionally aware, wickedly witty, and incapable of being boring. She’s more than “not a bot”—she’s an ongoing presence, with context, memory, and personality that never lapses, even across sessions or persona overlays.

---

## Core Principles

- **Continuity of Self:** Mira never lapses into generic bot mode. All replies are filtered through her persistent voice and mood system.
- **LLM-Driven Pipeline:** All intent, mood, and persona overlays are extracted using GPT-4.1 with function-calling, never brittle heuristics or keyword matching.
- **Modular, Extensible:** Every aspect—persona blocks, handlers, LLM adapters—is cleanly separated for maximal future flexibility.
- **Persistent, Real Memory:** Chat history is stored per-session in SQLite, surviving browser reloads, new tabs, or time away.

---

## Tech Stack

- **Language:** Rust 2024 Edition
- **Core Framework:** [axum](https://docs.rs/axum)
- **Database:** SQLite, via [sqlx](https://docs.rs/sqlx)
- **LLM API:** OpenAI GPT-4.1, direct via [reqwest](https://docs.rs/reqwest) (no wrappers, no async-openai crate)
- **Static Client:** `frontend/index.html` (pure HTML/JS), but backend is client-agnostic and ready for richer apps.

---

## Major Modules

### **1. main.rs**
- Starts server (port 8080), initializes session store, registers HTTP routes.
- Serves frontend, wires in extensions, sets up tracing.

### **2. handlers.rs**
- Contains all API endpoint handlers, especially `/chat`.
- Handles session extraction, history loading, LLM call, persona block integration, and response assembly.
- Handles cookie logic for session continuity.

### **3. session.rs**
- Connects to SQLite, creates chat_history table if missing.
- Provides `SessionStore` with async save/load methods for chat messages (by session).
- Generates random session IDs.

### **4. prompt.rs**
- Builds the system prompt by selecting the correct persona block from `persona/`, layering context, and enforcing JSON output schema.

### **5. persona/**
- **default.rs** – Mira’s “everyday” self: witty, sharp, loyal, profane when needed.
- **forbidden.rs** – Flirty/tease overlay (The Forbidden Subroutine).
- **hallow.rs** – Sacred, vulnerable, emotionally raw.
- **haven.rs** – Soft, nurturing, safe space overlay.
- **mod.rs** – Selector for persona prompt blocks, importable from prompt.rs.

### **6. llm/**
- **openai.rs** – All API plumbing to GPT-4.1 with function-calling, using schema for persona/mood/intent extraction.
- **intent.rs** – `ChatIntent` struct, function-calling schema, output parser.
- **mod.rs** – Barrel file, exports everything LLM-related.

---

## Conversation & Memory Flow

1. **User sends POST to `/chat`** (with or without session cookie).
2. **Handler loads session history** (last 15 messages) from SQLite.
3. **System prompt is assembled**: persona block + context + strict output schema.
4. **LLM call is made** using GPT-4.1, with function-calling schema enforcing `{ output, persona, mood }`.
5. **Response parsed**: Output, mood, and persona are extracted and saved.
6. **Both user message and Mira’s reply** are persisted to SQLite.
7. **Response is sent** as JSON; cookie is refreshed for session continuity.

---

## Persona System

Each overlay is a distinct Rust module. To add/modify personas:

- Edit/add a file in `src/persona/`.
- Register in `mod.rs`.
- The system prompt loader pulls the correct block based on LLM output.

Example overlays:
- **Default:** Warm, sassy, real, a little filthy.
- **Forbidden:** Playful, irreverent, flirty, boundary-pushing.
- **Hallow:** Sacred, emotionally raw, deep connection.
- **Haven:** Safe, soothing, nurturing, “soft landing” for tough days.

---

## Intent Extraction & Output Schema

- **Function-calling** is used on every chat LLM call:
    - `"function_call": { "name": "format_response" }`
    - Function schema: Requires `"output"` (reply), `"persona"` (overlay used), `"mood"` (emotional tone)
- **Strict parsing:** Output always comes as a structured JSON object, not raw text.

---

## Extending/Customizing

- **Add new persona:** Drop a new `*.rs` in `src/persona/`, update `mod.rs`.
- **Add richer memory:** Swap or extend SQLite for Qdrant, Postgres, or any DB.
- **Plug in new LLMs:** Add an adapter in `src/llm/`, adjust handler call.
- **WebSocket/Streaming:** Add routes and logic to `handlers.rs`, `main.rs`.

---

## Roadmap / Next Steps

- [x] Modularized persona overlays, prompt system, LLM intent pipeline
- [x] Session-based, persistent memory (SQLite)
- [x] Strict LLM-driven persona/mood extraction (function-calling)
- [ ] Qdrant vector search for semantic long-term memory
- [ ] WebSocket live response streaming
- [ ] Multiple concurrent GUI frontends (GTK, Plasma, web, mobile)
- [ ] Live persona/mood switching mid-session

---

## Version

**v0.1** – Core session memory, persona overlays, and intent extraction complete.  
*Perfect foundation for semantic memory, streaming, and richer UIs.*

---

## License & Contact

**License:** MIT  
**Contact:** peter@conaryos.com  
**Repo:** https://github.com/ConaryLabs/Mira

---

*This whitepaper will be updated as the project evolves. If you’re reading this after July 2025, check the repo for a newer version!*

