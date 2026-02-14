---
name: status
description: This skill should be used when the user asks "mira status", "show status", "how is mira doing", "system status", or wants a quick overview of Mira's health and configuration.
---

# Mira Status

## Index

!`mira tool index '{"action":"status"}'`

## Storage

!`mira tool session '{"action":"storage_status"}'`

## Active Goals

!`mira tool goal '{"action":"list"}'`

## Instructions

Present a concise status dashboard:

- **Index**: Symbol count, embedded chunk count
- **Embeddings**: Whether OpenAI embeddings are active (infer from chunk count > 0)
- **Storage**: Database sizes, memory count, session count
- **Goals**: Active goal count with summary of in-progress items

Keep it brief — this is a quick health check, not a deep dive.
