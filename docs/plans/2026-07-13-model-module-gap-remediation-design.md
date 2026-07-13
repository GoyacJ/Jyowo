# Model Module Gap Remediation Design

## Goal

Close the confirmed model-module gaps without replacing the current frontend, Tauri, daemon, SDK, Engine, or provider boundaries.

## Constraints

- Preserve the existing provider profile, secret, selection, and daemon IPC formats.
- Keep provider secrets in owner-only JSON files and retain the one-time reveal-token flow.
- Treat daemon journal events as the source of truth for task execution and model usage.
- Resolve configuration once per run segment and keep the resulting runtime snapshot immutable.
- Add regression tests before each production change.

## Architecture

The existing layered architecture remains in place:

```text
React settings and task UI
  -> Tauri settings commands and global configuration files
  -> daemon submit_message queue and journal
  -> per-segment RuntimeConfigSnapshot
  -> SDK/Harness and Engine
  -> ModelProvider adapter
  -> ModelStreamEvent
  -> domain events and daemon projection
```

The remediation closes boundaries rather than merging layers.

### Provider continuation

Both daemon and desktop Harness builders will receive a `FileProviderContinuationStore` rooted in their existing runtime directory. The store remains scoped by provider, model config, protocol, dialect, tenant, session, and message. Models requiring private reasoning replay will fail only when the required record is genuinely absent, not because runtime assembly omitted the store.

### Usage

`UsageAccumulatedEvent` remains the canonical accounting event. The desktop usage summary will consume durable daemon events and maintain an incremental rollup with a durable cursor. Rebuilding replays source events instead of resetting to an empty summary. Probe usage stays excluded through its diagnostic flag.

Engine accounting will distinguish initial usage snapshots from later deltas, record tool-call counts before emitting the model-call usage event, and avoid reporting incomplete streams as successful calls.

### Authentication-free providers

Provider authentication requirements will be derived from the provider catalog in every layer. `auth_scheme=None` providers may have no secret entry and an empty API key. Task configuration lists will use provider executability rather than `hasApiKey` alone. Authenticated providers remain fail-closed.

### Stream completion

A model call completes only after the normalized stream reaches an explicit terminal event. An ordinary EOF before terminal state becomes a provider stream error. Provider codecs must not turn an unverified transport EOF into success.

### Provider configuration commits

Profile, secret, and selection changes will be prepared as one logical update. The implementation will validate and build the prospective runtime before making the new default visible. Multi-file persistence will use staged temporary files and rollback or recovery metadata so a failed write cannot expose a mixed generation.

### Catalog

Catalog entries marked runnable must be buildable by the runtime registry. Dynamic Anthropic entries that cannot yet be constructed remain visible as unsupported inventory. DeepSeek refresh data will either participate in catalog merging or no longer be fetched and persisted; unused remote state will not remain in the refresh contract.

### Frontend

An empty task override means inheritance in both capability calculation and submission. Settings mutations invalidate both settings and task model queries. Task model query failures receive an explicit error and retry path. Mutation failures remain visible. Usage aggregation uses structured keys rather than slash-delimited strings. Rebuilding slices poll until ready.

## Error handling

- Configuration errors remain redacted and fail closed.
- Auth-free providers are the only exception to secret presence checks.
- Truncated streams produce a terminal error event and never an assistant-completed event.
- Usage rebuild failures return a local slice error without blocking catalog or provider settings.
- Failed settings persistence leaves the previous generation active.

## Verification

Regression coverage will include:

- continuation persistence and replay through daemon runtime construction;
- auth-free provider save, list, selection, and daemon resolution;
- truncated model streams;
- non-stream usage and tool-call usage;
- usage rollup ingestion and rebuild;
- transactional provider settings failure paths;
- dynamic catalog executability;
- task override inheritance and query failure UI;
- cross-query invalidation and slash-containing model IDs;
- model crate default-feature compilation.
