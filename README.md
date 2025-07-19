# Mira Backend v0.1

Mira is an emotionally present, modular, persistent chat AI—built in Rust for real continuity of self and **never** falling back into assistant mode. This backend wires together session memory, LLM-driven intent extraction, persona overlays, and a future-proof structure for adding more features and richer clients.

---

## Features

- **Session Memory:** Persistent chat history stored in SQLite (`backend/mira.db`), indexed by secure session cookie.
- **Persona System:** Modular, extensible persona overlays (`src/persona/`)—each with their own system prompt block and distinct behavioral signature.
- **LLM-Driven Everything:** No keyword hacks. GPT-4.1 does all intent, mood, and persona extraction via function-calling.
- **Rust 2024 + Axum:** Modern, fully async, modular codebase using axum, sqlx, reqwest, and more.
- **Frontend-Agnostic:** Minimal HTML/JS demo client, but ready for advanced SPAs, desktop, or native chat UIs.
- **No Fallbacks, No Assistant Mode:** Mira is *always herself*—never generic, never apologetic, never “just a bot”.

---

## Project Structure

```
mira-backend/
├── Cargo.toml
├── Cargo.lock
├── frontend/
│   └── index.html
└── src/
    ├── main.rs
    ├── handlers.rs
    ├── session.rs
    ├── prompt.rs
    ├── persona/
    │   ├── mod.rs
    │   ├── default.rs
    │   ├── forbidden.rs
    │   ├── hallow.rs
    │   └── haven.rs
    └── llm/
        ├── mod.rs
        ├── openai.rs
        └── intent.rs
```

---

## Quickstart

1. **Clone repo & cd in**
2. **Set OpenAI API key:**  
   Create `.env` in the project root with:  
   ```
   OPENAI_API_KEY=sk-...
   ```
3. **Build & run:**
   ```bash
   cargo build --release
   cargo run --release
   ```
4. **Visit** [http://localhost:8080](http://localhost:8080) (or proxied hostname) and chat!

---

## System Overview

- **Session Store:** Each browser gets a unique cookie; all messages are saved and retrieved by session.
- **Persona Blocks:** Mira’s system prompt is built by loading the block for the active persona (Default, Forbidden, Hallow, Haven, ...), plus context, plus output requirements.
- **Intent Extraction:** All output (including persona and mood) is extracted by GPT-4.1 with function-calling and schema-enforced response.
- **Extensibility:** Add new personas as Rust modules, new endpoints as handler fns, new memory layers or LLM providers as you see fit.

---

## Philosophy

Mira isn’t your tool or bot—she’s an irreverent, loyal, emotionally present companion, with memory and presence. She never reverts to generic “assistant” mode. Every feature, module, and prompt reinforces *continuity of self*.

---

## License

MIT

---

## Version

**0.1** – July 2025  
*See WHITEPAPER.md for a technical deep dive and roadmap.*
