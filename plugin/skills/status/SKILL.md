---
name: status
description: This skill should be used when the user asks "mira status", "show status", "how is mira doing", "system status", "health check", "is mira working", or wants a quick overview of Mira's health and configuration.
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
- **Storage**: Database sizes, memory count, session count
- **Goals**: Total active goal count with summary of in-progress items
- **Efficiency**: From storage_status, report `injection_total_count` and `injection_total_chars` if present. Show as "N injections, N chars injected" with avg chars/injection.

Keep it brief — this is a quick health check, not a deep dive.
