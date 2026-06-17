# Jyowo RunEvent Schema

## Source Of Truth

Rust contracts are canonical. Frontend `RunEvent` is a rendering model.

- MUST treat `jyowo-harness-contracts` as the canonical event source.
- MUST map Rust events into frontend `RunEvent` at an adapter boundary.
- MUST NOT make the frontend schema the backend persistence format.
- SHOULD keep frontend events stable for rendering, filtering, and virtualization.

## Required Fields

Every frontend `RunEvent` MUST include:

```ts
{
  id: string
  runId: string
  sequence: number
  timestamp: string
  type: RunEventType
  source: RunEventSource
  visibility: 'public' | 'redacted' | 'withheld'
  summary?: string
  payload?: Record<string, unknown>
}
```

`sequence` MUST be monotonic inside a Run. `timestamp` MUST be an ISO datetime with offset.

## MVP Event Types

- `run.started`
- `run.ended`
- `assistant.delta`
- `assistant.completed`
- `tool.requested`
- `tool.approved`
- `tool.denied`
- `tool.completed`
- `tool.failed`
- `permission.requested`
- `permission.resolved`
- `engine.failed`

## Redaction

- `public` payload MAY be rendered normally.
- `redacted` payload MAY be rendered only as already-redacted data.
- `withheld` payload MUST NOT be rendered.
- Raw JSON views MUST only display redacted payloads.

DEFERRED: full Rust-to-frontend event adapter and replay timeline.
