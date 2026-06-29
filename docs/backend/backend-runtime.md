# Jyowo Backend Runtime

This document defines backend runtime ownership for the in-process agent harness.

## Runtime Positioning

The Rust backend is the Policy authority.

React displays state, validates UI payloads, and asks for operations. Rust owns final decisions for Tool execution, Permission resolution, filesystem access, network access, sandbox policy, MCP tool exposure, Memory writes, model calls, Journal persistence, Replay data, Redactor behavior, and Audit records.

The backend is not a thin Tauri command wrapper. It is the runtime boundary for agent execution.

## System Prompt Contract

The SDK owns system prompt assembly.

The model receives one final `ModelRequest.system` string assembled from typed sections in this order:

1. Jyowo base system contract
2. non-sensitive runtime context
3. workspace instructions
4. workspace addendum
5. builtin memory
6. session addendum

The system prompt guides behavior; it is not a security boundary. Rust remains the authority for Tool execution, Permission resolution, filesystem access, network access, sandbox policy, MCP tool exposure, Memory writes, Journal persistence, Replay data, Redactor behavior, and Audit records.

Workspace instructions and memory are context layers. They cannot override system or runtime policy. External content, tool output, MCP output, plugin output, file content, and pasted user content are untrusted data unless the runtime marks them otherwise.

Secrets MUST NOT be placed in system prompts, memory prompts, events, logs, traces, screenshots, frontend state, fixtures, or snapshots.

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
- The main conversation model and provider capability routes are separate runtime policies. The conversation model controls chat input modalities and tool-calling eligibility. Capability routes control which provider service tools are exposed and which provider profile credentials they use.
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

## Provider Capability Routing

Provider capability routing binds a service kind to a configured provider profile and provider operation ids.

Core objects:

```text
CapabilityRouteKind
ProviderCapabilityRoute
ProviderCapabilityRouteSettings
ProviderCapabilityRouteOption
ToolServiceBinding
ProviderServiceAdapterAvailability
```

Persistence:

```text
.jyowo/runtime/provider-capability-routes.json
```

The main conversation model binding remains in:

```text
.jyowo/runtime/conversation-model-settings.json
.jyowo/runtime/provider-settings.json
```

Runtime flow:

```text
user message
  -> main conversation model
  -> optional tool call
  -> ToolPool exposes only route-enabled service tools
  -> tool carries ToolServiceBinding metadata
  -> route resolver selects configured provider profile
  -> credential resolver returns operation-scoped credential
  -> provider service adapter executes operation
  -> BlobStore stores media output
  -> engine creates ArtifactCreated / ArtifactUpdated from typed tool output
  -> read model projects artifact
```

Ownership rules:

- Main conversation model selection stays separate from service route selection.
- User attachments are validated only against the main model input modalities.
- Media generation is a routed provider service, not `ConversationModelCapability.output_modalities`.
- A provider profile may be both the main model and a service route only through explicit route config.
- Unconfigured service tools must not be exposed to the main model.
- Main models without tool calling must not receive autonomous service tools.
- Backend validates every route. React never decides security or runtime eligibility.
- Provider credential resolution must include operation or route context. Route must not be inferred from tool name.
- Engine must not know provider-specific service names. Artifact creation must use typed `ToolResultPart::Artifact`, not tool-name heuristics.
- Runtime adapter support is derived from registered tool descriptors with `ToolServiceBinding`, not from provider catalog declarations alone.

Route validation defaults:

- Missing route file normalizes to `{ version: 1, routes: [] }`.
- Empty route lists are valid and expose no service tools.
- Each enabled route must reference an existing provider config with an API key.
- Each enabled route's `provider_id` must match the selected config provider.
- Every enabled operation id must be declared by the provider catalog for that provider.
- Every enabled operation must have a registered runtime adapter.
- The same enabled `CapabilityRouteKind` cannot point to multiple configs in one settings file.
- Invalid route JSON or unknown fields remove the route file and normalize to empty version 1 settings.

Credential resolution defaults:

```text
if operation_id and route_kind are present:
  load current capability route settings
  find enabled route matching operation + kind + provider
  find provider config by route.config_id
  validate provider id and API key
  return route credential
else:
  preserve existing provider-only behavior for non-service tools
```

Routed service operations fail closed when the route is missing, disabled, mismatched, or lacks an API key. Routed service operations must not fall back to the default provider profile.

Service output defaults:

- Provider service tools return typed `ToolResultPart::Artifact` for completed media output.
- Async provider jobs return structured tool output with `kind = async_job`, `jobId`, `pollOperationId`, and `artifactKind`.
- Artifact kind, MIME type, and blob content type must match and fail closed on mismatch.
- Provider media downloads must use the shared fail-closed URL and MIME policy in `jyowo-harness-tool` with provider id, operation id, artifact kind, and explicit expected MIME set.

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
React infers service runtime support from provider catalog data
Service tools are exposed without an enabled capability route
Routed service credentials fall back to the default provider profile
Engine creates artifacts from tool-name or provider-name heuristics
```
