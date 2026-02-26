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

## Injection Stats

!`mira tool session '{"action":"report"}'`

## Instructions

Present a concise status dashboard:

- **Index**: Symbol count, embedded chunk count
- **Storage**: Database sizes, memory count, session count
- **Goals**: Total active goal count with summary of in-progress items
- **Injections**: Total injections, chars, efficiency ratio (if data exists)

Keep it brief — this is a quick health check, not a deep dive.
