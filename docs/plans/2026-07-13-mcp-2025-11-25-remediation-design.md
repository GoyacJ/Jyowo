# MCP 2025-11-25 Remediation Design

## Goal

Bring Jyowo's MCP client, runtime integration, desktop management UI, optional transports, and reverse server adapter into a coherent implementation of MCP 2025-11-25 while preserving Jyowo's existing authorization and task-isolation boundaries.

## Scope

This design covers:

- stdio and Streamable HTTP client transports;
- deprecated HTTP+SSE compatibility and custom WebSocket transport;
- protocol negotiation, capabilities, pagination, content types, sampling, elicitation, and shutdown;
- `McpRegistry`, reconnect behavior, list-changed propagation, and tool injection;
- daemon required/optional startup policy and task diagnostics;
- desktop global/project configuration and settings diagnostics;
- the reverse MCP Server Adapter;
- feature compilation and end-to-end contract tests.

It does not add MCP Tasks support. Jyowo must not advertise that capability until a task handler exists.

## Chosen Approach

Keep Jyowo's registry, tool wrapper, permission service, and runtime configuration model. Replace transport-local protocol logic with a shared `McpPeer`, negotiated `McpSession`, and typed message router.

The alternatives were rejected:

- patching each transport independently would retain duplicated initialization, pagination, inbound request, and lifecycle behavior;
- adopting an external Rust SDK would force a larger rewrite across Jyowo's authorization, diagnostics, reconnect, and wrapper boundaries.

## Architecture

```text
Protocol types
  -> McpMessage: request | notification | response
  -> InitializeResult and negotiated capabilities
  -> typed tool/resource/prompt/content models

McpPeer
  -> request id and pending responses
  -> inbound request router
  -> notification router
  -> timeout and cancellation

McpSession
  -> lifecycle state
  -> negotiated protocol version
  -> offered client capabilities
  -> accepted server capabilities

Transport codecs
  -> stdio newline codec
  -> Streamable HTTP POST/GET/SSE/session codec
  -> deprecated HTTP+SSE compatibility codec
  -> WebSocket custom codec

Runtime integration
  -> McpRegistry
  -> reconnect and stable change stream
  -> permission-wrapped Harness tools
  -> settings and task diagnostics
```

Transports only move MCP messages. Initialization, version checks, capability checks, pagination, inbound requests, and notification routing live above the transport codecs.

## Protocol Compatibility

The client requests `2025-11-25` and accepts these known revisions when returned by the server:

- `2025-11-25`
- `2025-06-18`
- `2025-03-26`
- `2024-11-05`

An unknown version ends initialization. The negotiated version is stored in the session and used for subsequent HTTP requests.

Client and server capabilities are modeled separately. The client only advertises handlers that are installed. Required server capabilities are checked after `initialize` and before `notifications/initialized` completes the transition to operation.

MCP Tasks are not advertised. Sampling, roots, and form/URL elicitation are only advertised when their corresponding inbound handlers exist.

## Bidirectional Message Handling

`McpMessage` distinguishes requests, notifications, success responses, and error responses. Request IDs may be numbers or strings.

`McpPeer` owns pending outbound requests. Incoming server requests are dispatched independently and their responses are sent through the same transport. Incoming notifications never enter the pending-response map.

Supported inbound client requests include:

- `ping`
- `sampling/createMessage`
- `elicitation/create`
- `roots/list`

List-changed, progress, cancellation, logging, resource updates, and elicitation completion are handled as notifications.

## Standard Transports

### stdio

stdio uses newline-delimited UTF-8 MCP messages. Shutdown closes stdin, waits for exit, then escalates to TERM and KILL within bounded timeouts. No MCP `shutdown` method is sent.

### Streamable HTTP

The HTTP transport implements:

- POST requests with `Accept: application/json, text/event-stream`;
- JSON responses and SSE responses to POST;
- optional GET SSE channel;
- `MCP-Session-Id` capture and reuse;
- `MCP-Protocol-Version` after initialization;
- DELETE session termination, accepting 405 as unsupported cleanup;
- `Last-Event-ID`, server retry values, and bounded stream resumption;
- server requests and notifications received on SSE;
- POSTing responses to server requests;
- reinitialization after session 404;
- existing authorization refresh, redirect, DNS pinning, timeout, and cancellation behavior.

The reverse HTTP Server Adapter validates `Origin`, binds locally by default, supports GET/POST/DELETE, isolates sessions, and returns 202 for accepted notifications and responses.

### Deprecated HTTP+SSE

`TransportChoice::Sse` remains as an explicit deprecated `2024-11-05` compatibility transport. It opens the configured SSE URL, waits for the server-provided `endpoint` event, and posts subsequent messages to that endpoint.

Streamable HTTP only falls back to this flow when initialization returns 400, 404, or 405.

### WebSocket

WebSocket remains a documented custom transport. It uses the shared peer and session but closes with a WebSocket close frame. It is not presented as a standard MCP transport.

## Protocol Data Models

List operations expose page-level APIs with optional cursors. High-level collection APIs iterate all pages with limits for page count, item count, and repeated cursors.

Tool and content models add:

- tool `title`, icons, annotations, `_meta`, `outputSchema`, and execution metadata;
- `structuredContent`;
- text, image, audio, resource-link, and embedded-resource content;
- unknown content preservation;
- resource blobs, sizes, and multiple `resources/read` contents;
- prompt arguments and pagination.

The existing non-standard `McpContent::Json` representation migrates to `structuredContent` plus a text fallback where the Harness requires text.

JSON Schema validation selects the declared dialect and otherwise defaults to 2020-12.

## Sampling and Elicitation

Sampling uses the MCP 2025-11-25 camelCase model: messages, model preferences, max tokens, tools, and tool choice. Jyowo's existing approval, budget, tenant, and permission controls remain around the protocol handler.

Elicitation is split into standard mechanisms:

- `elicitation/create` for form and URL modes;
- `-32042` only for URL elicitation required errors;
- `notifications/elicitation/complete` as a separate notification.

The current custom behavior that merges form data into the original tool arguments and retries is removed.

## Registry and Reconnect

The connection object is the single source of connection state. `ManagedMcpServer` does not retain an independent mutable connection-state snapshot.

Tool synchronization state remains separate from connection state. A schema or injection failure does not mark a healthy transport connection as disconnected.

`ManagedMcpConnection` owns a stable broadcast stream. Every successful initial connection or reconnect starts a generation-bound forwarder from the current transport connection. Replacing the connection aborts the previous forwarder.

Registry list-changed subscription is idempotent per server. Removing a server or shutting down the registry terminates its forwarder and tool-sync task.

`mark_unhealthy` enters the same reconnect path used for transport failures. It cannot leave the connection permanently in `Reconnecting` without a reconnect loop.

## Runtime Isolation

`DesktopSettingsRuntime`, foreground daemon runs, and child daemon runs keep separate registries and transport connections.

They cannot share a connection because they have different:

- authorization contexts;
- run and session identifiers;
- permission modes;
- workspace or worktree roots;
- lifecycle ownership.

The implementation unifies configuration semantics, state representation, event formats, and trust derivation instead of sharing connection instances.

## Required and Optional Servers

Persisted records add:

```rust
#[serde(default)]
required: bool
```

Old records become optional. Disabled records are never connected and never block startup.

Configuration parsing, validation, and unsafe working-directory errors remain fail-closed. After a valid record has entered runtime construction:

- optional connection, initialization, capability, list, or injection failures register a failed server, emit task diagnostics, skip tool injection, and allow the run to start;
- required failures emit the same diagnostics, close already-connected servers, and stop run construction with a redacted boundary error.

Foreground and child runs use the same policy but retain separate registries and authorization contexts.

## Diagnostics and Lifecycle

Settings diagnostics use `plane=settings`. Daemon diagnostics use `plane=task` and carry task, session, run, and segment identifiers when available.

The daemon replaces `NoopMcpEventSink` with a bounded asynchronous writer into the task event journal. Runtime shutdown follows this order:

1. stop accepting new MCP work;
2. close registry connections;
3. stop change subscriptions;
4. flush the diagnostic writer.

The primary run error is preserved. A shutdown error is returned only when no earlier error exists. Drop remains a cancellation and panic fallback, not the normal cleanup path.

The settings UI labels registry state as a settings connection check. It does not present that state as proof of task availability.

## Source and Trust

Global user records are `McpServerSource::User` and `UserControlled`. Project records are `McpServerSource::Project` and `UserControlled`.

`Workspace` is reserved for actual workspace or administrator policy. UI manageability is modeled independently from source and trust.

This makes trusted annotation handling identical in settings and daemon execution.

## Desktop Configuration

Configuration layer and runtime attribution are separate fields:

```text
configLayer: global | project
runtimeScope: global | session | agent
```

All get, save, delete, toggle, and restart commands address `(configLayer, id)`. The backend derives the active project root from desktop runtime state and never accepts an arbitrary project path from the renderer.

A project record with the same ID overrides its global record. Deleting the project record reveals the global record again. Inherited global records are read-only in the project view until copied into a project override.

The UI adds:

- global/project configuration views;
- required/optional control;
- explicit runtime-scope wording;
- settings/task diagnostic source badges and filters;
- settings-check status labeling;
- real `user` and `project` origins.

TypeScript validation is aligned with Rust contracts for lengths, NUL checks, URL userinfo and secret query rejection, environment names, header names, header values, and secret-bearing inherited variables.

The inherited environment example changes from `GITHUB_TOKEN` to `PATH`.

Browser presets use exact versions:

- `@playwright/mcp@0.0.78`
- `chrome-devtools-mcp@1.5.0`

## Server Adapter

The reverse adapter shares protocol types and the server dispatcher across stdio, Streamable HTTP, and custom WebSocket frontends.

The dispatcher enforces initialization before operation, accepts notifications without generating responses, negotiates capabilities, and does not accept a non-standard `shutdown` method.

Existing authentication, tenant mapping, tenant isolation, rate limiting, audit, and default-disabled write operations remain intact.

## Dependency Strategy

The MCP crate uses one workspace `reqwest` version. SSE parsing is implemented over byte streams rather than binding a second reqwest version through `reqwest-eventsource`.

WebSocket code is migrated to the `tokio-tungstenite 0.30` `Utf8Bytes` and `Bytes` API.

All feature combinations must compile, including `--all-features`.

## Test Strategy

Every behavior change follows red-green-refactor.

Protocol tests cover:

- official initialize fixtures and version negotiation;
- request, notification, and response classification;
- capability direction and gating;
- pagination limits and repeated cursor protection;
- all standard tool content blocks and unknown blocks;
- multiple resource contents;
- sampling and elicitation wire models.

Transport tests cover:

- stdio bidirectional requests and bounded shutdown;
- Streamable HTTP JSON, POST SSE, GET SSE, sessions, reinitialization, resumption, and DELETE;
- legacy endpoint discovery and constrained fallback;
- custom WebSocket frames and closure.

Runtime tests cover:

- dynamic connection state;
- reconnect generation change forwarding;
- idempotent tool synchronization;
- required and optional failures at connect and injection stages;
- foreground and child isolation;
- diagnostic flushing and cleanup on all exits.

Desktop tests cover:

- global/project isolation and override restoration;
- required-field backward compatibility;
- source and trust consistency;
- diagnostic plane compatibility for old JSONL records;
- strict TypeScript/Rust validation parity;
- UI configuration layer, status source, and diagnostic source behavior.

The final verification matrix is:

```text
cargo test -p jyowo-harness-mcp --features stdio
cargo test -p jyowo-harness-mcp --features http
cargo test -p jyowo-harness-mcp --features sse
cargo test -p jyowo-harness-mcp --features websocket
cargo test -p jyowo-harness-mcp --features server-adapter
cargo test -p jyowo-harness-mcp --features stdio,http,oauth
cargo test -p jyowo-harness-mcp --all-features
daemon MCP tests
desktop Rust MCP tests
frontend Vitest, typecheck, and lint
```

## Migration and Compatibility

- Missing `required` defaults to false.
- Historical diagnostic records default to `plane=settings`.
- Existing stdio and HTTP persisted records keep their shape except for additive fields.
- `TransportChoice::Sse` changes from the current non-standard `/events` assumption to documented legacy endpoint discovery.
- Removing the custom `shutdown` method is an intentional protocol correction.
- Unknown content is preserved instead of failing an entire result.
- Current task connections remain immutable snapshots; configuration changes affect new runs.
