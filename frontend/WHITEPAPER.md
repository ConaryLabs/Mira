# Mira Frontend Whitepaper (LLM-Facing)

Audience: model/tool authors and frontend integrators. This tells you exactly what messages to send/expect, how streaming works, how artifacts show up, and how the router treats unknown types. Keep to the contract and nothing gets dropped on the floor.

---

## 1) Scope
- WebSocket message contracts the frontend expects
- Two coexisting protocols (legacy vs operations) and when they fire
- Router architecture (store + handlers), not a mythical single switch
- Artifact flow and file writes
- Dev-mode enforcement to catch protocol drift fast

---

## 2) Core Concepts
- Turn: a single user → assistant exchange, optionally with tools
- Stream: incremental assistant tokens (legacy: `stream`; ops: `operation.streaming`)
- Artifacts: code/doc payloads shown in the Artifact Viewer and saved via `files.write`

---

## 3) Protocols That Coexist

There are two active protocols. Backend chooses per-turn.

Legacy chat protocol (simple conversational turns)
- `{ type: 'status', status: 'thinking' }`
- `{ type: 'stream', delta: '...' }`
- `{ type: 'chat_complete', content: '...', artifacts: [...] }`

Operations protocol (complex/multi-step engine)
- `{ type: 'operation.started', operation_id: '...' }`
- `{ type: 'operation.status_changed', status: '...' }`
- `{ type: 'operation.streaming', delta: '...' }`  // canonical field is `delta`; some servers send `content`
- `{ type: 'operation.artifact_preview', path: '...' }`  // optional hint that an artifact is incoming
- `{ type: 'operation.artifact_completed', artifact: {...} }`
- `{ type: 'operation.completed', result: '...' }`

Selection rule (done by backend)
- Simple chat → legacy protocol
- Complex ops → operations protocol
Frontend handles both concurrently.

---

## 4) Router Architecture

We don't have one global exhaustive switch. We have a store that fans out to subscribers.

Diagram:
```
WS Frame → useWebSocketStore.handleMessage()
          → Notifies subscribers (optionally filtered by type)
              ├─ useMessageHandler (legacy: status/stream/chat_complete)
              └─ useWebSocketMessageHandler (data envelope + operation.* + misc)
```

---

## 5) Dev-Mode Enforcement (Don't drop unknowns)

Unknown types must scream in dev. Warn-only in prod.

In useWebSocketStore.handleMessage():
```ts
handleMessage: (message: WebSocketMessage) => {
  // ... existing notify logic first

  if (!KNOWN_MESSAGE_TYPES.has(message.type)) {
    console.warn(`[WS] Unknown message type: ${message.type}`, message);
    if (import.meta?.env?.DEV || process.env.NODE_ENV !== 'production') {
      throw new Error(`[WS-Dev] Unhandled message type: ${message.type}`);
    }
  }
}
```

In useWebSocketMessageHandler for data payloads:
```ts
const handleDataMessage = (data: any) => {
  const dtype = data?.type;
  if (!dtype) return;

  if (!KNOWN_DATA_TYPES.has(dtype)) {
    console.warn('[WS-Global] Unknown data type:', dtype, data);
    if (import.meta?.env?.DEV || process.env.NODE_ENV !== 'production') {
      throw new Error(`[WS-Dev] Unhandled data type: ${dtype}`);
    }
  }

  // ... existing switch
}
```

---

## 6) Streaming Semantics (Delta vs Content)

Canonical field is `delta`. Some servers send `content`. Normalize to `delta` and always attach to the in-flight buffer keyed by `turn_id` or `operation_id`.

Example (operations):
```ts
case 'operation.streaming': {
  const delta = data.delta ?? data.content ?? '';
  const id = data.turn_id ?? data.operation_id ?? 'unknown';
  appendStreamContent(id, delta, data.seq);
  return;
}
```

Legacy streaming:
```ts
case 'stream': {
  const delta = data.delta ?? data.content ?? '';
  appendStreamContent(currentTurnId(), delta, data.seq);
  return;
}
```

---

## 7) Artifact Timing

- Operations protocol: artifacts can arrive mid-stream via `operation.artifact_completed` (and may be hinted by `operation.artifact_preview`).
- Legacy protocol: artifacts arrive bundled on `chat_complete` as an `artifacts: [...]` array.

Both paths hydrate the same artifact UI.

---

## 8) Artifact Storage Model (Generalized)

We don't prescribe a single store; the app may use a slice and hooks.

Reality we support/document:
- Artifacts live under a useAppState slice (e.g., useArtifactState) and helper hooks (e.g., useArtifacts)
- Required fields on an artifact: `{ id, path, content, language?, status }`
- Actions typically exposed: `importArtifacts(list)`, `save(id)`, `apply(id)`

---

## 9) File Write Patterns (Both valid)

Direct:
```json
{ "type": "files.write", "data": { "path": "src/a.ts", "content": "..." } }
```

Envelope:
```json
{ "type": "file_system_command", "method": "files.write", "data": { "path": "src/a.ts", "content": "..." } }
```

Frontend must support both. Save/Apply should respect the full `path` exactly (no flattening or "helpful" renames).

---

## 10) Handler Skeletons (Prescriptive)

useMessageHandler (legacy):
```ts
switch (type) {
  case 'status': {
    startStreaming(); // show thinking
    return;
  }
  case 'stream': {
    const delta = data.delta ?? data.content ?? '';
    appendStreamContent(currentTurnId(), delta, data.seq);
    return;
  }
  case 'chat_complete': {
    endStreaming();
    if (Array.isArray(data.artifacts) && data.artifacts.length) {
      artifacts.importArtifacts(data.artifacts);
    }
    addAssistantMessage({ content: data.content });
    return;
  }
}
```

useWebSocketMessageHandler (operations + data envelope):
```ts
switch (data.type) {
  case 'operation.started': {
    markOpStarted(data.operation_id);
    return;
  }
  case 'operation.status_changed': {
    markOpStatus(data.operation_id, data.status);
    return;
  }
  case 'operation.streaming': {
    const delta = data.delta ?? data.content ?? '';
    const id = data.turn_id ?? data.operation_id;
    appendStreamContent(id, delta, data.seq);
    return;
  }
  case 'operation.artifact_preview': {
    console.log('[WS-Global] Artifact preview:', data.path);
    // optional: show loading state in artifact panel for data.path
    return;
  }
  case 'operation.artifact_completed': {
    const a = data.artifact;
    if (a) artifacts.importArtifacts([a]);
    return;
  }
  case 'operation.completed': {
    markOpCompleted(data.operation_id, data.result);
    // Some servers echo artifacts again here; import if present.
    if (Array.isArray(data.artifacts) && data.artifacts.length) {
      artifacts.importArtifacts(data.artifacts);
    }
    return;
  }
  case 'document_list': {
    console.log('[WS-Global] Document list received:', data.documents?.length || 0);
    routeDocumentsToStore(data);
    return;
  }
  default: {
    // Dev enforcement handled by KNOWN_DATA_TYPES check wrapping this switch
    return;
  }
}
```

---

## 11) Known Type Registries (Keep current)

Add these to your sets so dev-mode enforcement is truthful.

```ts
export const KNOWN_MESSAGE_TYPES = new Set<string>([
  // legacy
  'status', 'stream', 'chat_complete',
  // operations wrapper (some envs deliver op events at top-level message.type)
  'operation.started', 'operation.status_changed', 'operation.streaming',
  'operation.artifact_preview', 'operation.artifact_completed', 'operation.completed',
  // envelopes
  'data', 'error'
]);

export const KNOWN_DATA_TYPES = new Set<string>([
  // operations (data.type)
  'operation.started', 'operation.status_changed', 'operation.streaming',
  'operation.artifact_preview', 'operation.artifact_completed', 'operation.completed',
  // app data
  'project_list', 'file_tree', 'document_list',
]);
```

---

## 12) Artifact Contract (LLM-facing)

When creating artifacts, send complete payloads. No commentary inside `content`.

Example:
```json
{
  "type": "operation.artifact_completed",
  "artifact": {
    "id": "art_123",
    "path": "src/utils/date.ts",
    "content": "export function formatDate(...) {\n  ...\n}\n",
    "language": "typescript",
    "status": "draft"
  }
}
```

Legacy bundling on chat_complete:
```json
{
  "type": "chat_complete",
  "content": "Explanation text...",
  "artifacts": [ { "id": "art_123", "path": "...", "content": "..." } ]
}
```

---

## 13) PR Checklist (Do this now)
- Add dev-mode router guard to `useWebSocketStore.handleMessage()` (throws on unknown top-level types in dev)
- Add dev-mode guard to `useWebSocketMessageHandler` for `data.type`
- Add `'document_list'` to `KNOWN_DATA_TYPES` and handle case (log + route)
- Add `'operation.artifact_preview'` to `KNOWN_MESSAGE_TYPES`/`KNOWN_DATA_TYPES` and handle case (log + optional UI hint)
- Normalize streaming deltas exactly like the skeletons (`delta ?? content`), including `seq` + `turn_id/operation_id`

---

## 14) Testing
- Legacy flow: send `status` → many `stream` with `delta` → `chat_complete` with optional `artifacts`
- Operations flow: send `operation.started` → many `operation.streaming` with `delta` → `operation.artifact_completed` → `operation.completed`
- Artifact preview: send `operation.artifact_preview` before the completed event; UI should not crash and may show a loading stub
- Data: push `document_list` and confirm it is routed (no dev throw)
- Bad types: push `type: 'wat'` and confirm dev build throws

---

## 15) Notes on Reality vs Prescription
- Our actual code historically appended from `data.content`; the contract is now prescriptive: treat `delta` as canonical and normalize when only `content` is present.
- Router is store + subscribers; don't centralize into a single mega switch. Keep the two-handler mental model.
