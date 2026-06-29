# Jyowo Backend Runtime

This document defines backend runtime ownership for the in-process agent harness.

## Runtime Positioning

The Rust backend is the Policy authority.

React displays state, validates UI payloads, and asks for operations. Rust owns final decisions for Tool execution, Permission resolution, filesystem access, network access, sandbox policy, MCP tool exposure, Memory writes, model calls, Journal persistence, Replay data, Redactor behavior, and Audit records.

The backend is not a thin Tauri command wrapper. It is the runtime boundary for agent execution.

## Domain Ownership

Core objects:

```text
Workspace
Session
Run
TurnInput
Event
ConversationWorktreePage
Tool
Permission
MCP server
Memory item
Model provider
Journal record
Replay cursor
Audit record
Secret
```

Ownership rules:

- `Run` execution belongs to Rust.
- `Tool` registration, routing, approval, execution, timeout, result budget, and error mapping belong to Rust.
- `PermissionBroker` owns policy checks, request deduplication, persistence, and decision scope.
- `MCP` tools enter the system through backend-owned registration and permission checks.
- `Memory` recall and writes run through backend-owned tenant and visibility rules.
- `Model` providers are backend capabilities and must not expose raw provider credentials to React.
- `Journal` stores structured `Event` values after redaction.
- Conversation worktree projection belongs to Rust. It converts redacted journal
  events into `ConversationTurn` trees for the UI.
- `Replay` reads from backend-owned journal cursors and snapshots.
- `Audit` data is derived from backend-controlled events and permission decisions.
- `Redactor` runs before event persistence, logs, traces, export, and Replay output.
- `Secret` values must remain outside prompts, logs, traces, test snapshots, screenshots, and serialized UI state.

## Event Semantics

`harness-contracts` is the canonical source for public event contracts and the
UI-facing conversation projection contract.

Backend events MUST be structured `Event` variants. Plain text logs are diagnostic data, not product trace data.

Every persisted event SHOULD carry enough identity to answer:

```text
tenant
workspace or session
run
tool use
actor
time
result
risk or permission decision
```

Ordering rules:

- A `PermissionRequested` event MUST exist before user-facing approval UI.
- A `PermissionResolved` event MUST exist before an approved destructive action executes.
- Tool completion or failure events MUST be emitted after the execution attempt finishes.
- Redaction MUST run before the event reaches a durable `Journal`.
- Replay MUST return redacted or withheld payloads according to event visibility.
- Plugin loaded, rejected, and failed lifecycle events are activity/replay events.
- Plugin failure events MUST carry a redacted failure summary, not raw sidecar RPC,
  environment, credential, path, or process error text.

Conversation worktree paging rules:

- `page_conversation_worktree` returns complete turns, not raw event pages.
- `pageCursor` is a turn cursor.
- `eventCursor` is the latest consumed journal cursor.
- the projection may read materialized worktree rows, or replay from session
  start into a complete in-memory projection before slicing by turn.
- it must never project a partial raw-event page and then slice that result.
- the current SQLite read-model implementation uses complete replay from the
  start of the session, then slices the projected `ConversationTurn[]`.
- for `direction = After`, `pageCursor` points at the last returned turn.
- for `direction = Before`, `pageCursor` points at the first returned turn.
- repeated page requests must not repeat the cursor boundary turn.
- `eventCursor` reports the latest replayed journal event, not the latest event
  referenced by the selected turn page.
- thinking summaries must be status-derived, explicitly safe, or withheld.
- tool failure summaries must be user-safe and must not expose raw payloads,
  private paths, or withheld placeholders.

## Failure Defaults

Security and privacy default to fail-closed.

Fail-closed applies to:

- missing permission decision
- malformed command payload
- unknown tool name
- unknown MCP tool origin
- missing sandbox capability
- secret exposure risk
- tampered permission persistence
- event redaction failure
- invalid tenant or workspace scope

fail-open is allowed only for non-security telemetry. Examples:

- metrics export failure must not block a completed local run
- tracing sink failure must not reveal secrets or bypass policy
- UI healthcheck failure must not grant additional access

Each fail-open path MUST be documented in code near the branch and covered by tests.

## Process Boundary

Some guarantees are single-process guarantees. Others must survive restart.

Single-process guarantees:

- in-memory tool registry contents
- active stream subscriptions
- pending async task handles
- live MCP connection objects
- in-memory permission request deduplication

Restart-stable guarantees:

- persisted permission decisions
- journaled events
- audit query results
- replay cursors and snapshots
- tenant and workspace identifiers
- durable redaction of persisted event content

Runtime code MUST not describe an in-memory guard as a durable guarantee.

## Forbidden Runtime Behavior

Forbidden:

```text
React makes final security decisions
Tool execution bypasses PermissionBroker
MCP tools bypass backend registration
Memory writes bypass tenant and visibility checks
Model calls receive raw Secret values through prompts
Journal append stores unredacted sensitive content
Replay returns withheld payloads
Tests assert raw API keys, bearer tokens, or private paths
```
