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

`SessionCreated.options_hash` is a session identity hash. It is limited to workspace, tenant, session, user, and team identity fields. Conversation resume paths compare this identity hash only. Model selection, protocol, tool search, tool profile, permission mode, interactivity, system prompt addendum, context compression, max iterations, model extra, agent tool policy, runtime prompt context hash, and effective prompt input hash are run-level execution config. `RunStarted.effective_config_hash` records the effective config for that run. `SessionCreated.effective_config_hash` is historical session creation metadata and must not be used to reject a later conversation run with a different model or run config.

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
- `Tool` registration, routing, approval, execution, timeout, result budget, and
  error mapping belong to Rust. Tools produce `ToolActionPlan` values; execution
  requires a validated `AuthorizationTicket`.
- `AuthorizationService` (L3, `jyowo-harness-execution`) is the cross-domain
  execution authority. It turns action plans into permission requests, applies
  hard policy, resolves consent, mints one-time authorization tickets, runs
  sandbox preflight, and executes authorized actions.
- Sandbox lifecycle is mandatory. `execute_with_lifecycle` calls `before_execute`
  exactly once, then the backend, then `after_execute`. Backends must not call
  `before_execute` internally. Sandbox `preflight_exec` validates capability
  before execution.
- Local no-isolation mode (`LocalIsolation::None`) is not OS sandbox enforcement.
  It must not claim OS isolation capability.
- Tool execution plans declare an execution channel. Process tools are enforced
  by process sandbox preflight. HTTP and provider tools are enforced by the
  authorized HTTP broker. External backend capabilities are checked against
  registered runtime capabilities. Direct authorized Rust paths still require a
  valid authorization ticket.
- Sandbox capability reporting is policy-specific. A backend must not report
  restricted network or workspace policy support unless it can enforce that
  exact policy.
- Desktop process sandbox routing, Docker fallback, HTTP broker permits, and
  frontend display-only status are defined in
  [harness-sandbox](../architecture/harness/crates/harness-sandbox.md).
- `BypassPermissions` and `DontAsk` skip interactive permission prompts only.
  They do not bypass tenant scope, workspace scope, hard policy, sandbox
  preflight, HTTP broker validation, authorization ticket validation, Redactor,
  event ordering, or capability presence.
- `PermissionBroker` owns policy checks, request deduplication, persistence, and decision scope.
- The permission authority stack owns hard policy, dedup, history, persistence,
  and interactive resolution as one pipeline. `PermissionContext` does not carry
  caller-owned rule snapshots as authority.
- `PermissionAuthority` is the single decision authority for production
  permission resolution. A `PermissionBroker` alone is not accepted as the
  production authority.
- `Tool` execution requires an authorization ticket minted by the execution
  authority. File, network, command, MCP, and sandbox-backed tools must not
  execute without a validated authorization ticket.
- `MCP` tools enter the system through backend-owned registration and permission checks.
- `Memory` recall and writes run through backend-owned tenant and visibility rules.
- `Model` providers are backend capabilities and must not expose raw provider credentials to React.
- Conversation identity and run execution config are separate runtime policies.
  Conversation identity covers workspace, tenant, user/team, and session scope.
  Model choice, protocol, permission mode, tool profile/search, context
  compression, agent tool policy, and prompt addenda belong to the run effective
  config.
- The run model and provider capability routes are separate runtime policies.
  The run model controls chat input modalities and tool-calling eligibility.
  Capability routes control which provider service tools are exposed and which
  provider profile credentials they use.
- Draft conversations are desktop metadata records. Creating an empty
  conversation must not write `SessionCreated` or any runtime journal event.
  The runtime journal starts when `start_run` succeeds.
- `Journal` stores structured `Event` values after redaction.
- Conversation worktree projection belongs to Rust. It converts redacted journal
  events into `ConversationTurn` trees for the UI.
- `Replay` reads from backend-owned journal cursors and snapshots.
- `Audit` data is derived from backend-controlled events and permission decisions.
- `Redactor` runs before event persistence, logs, traces, export, and Replay output.
- `Secret` values must remain outside prompts, logs, traces, test snapshots, screenshots, and serialized UI state.

## Agent Orchestration Runtime

Agent orchestration is backend-owned runtime behavior. React may expose controls
and render projected state, but Rust owns capability resolution, start policy,
background persistence, supervisor coordination, worktree isolation, and
permission attribution.

Domain ownership:

- `jyowo-harness-agent-runtime` owns agent profiles, capability policy inputs,
  durable background agent records, team persistence, and workspace write leases.
- `jyowo-harness-subagent` owns child agent lifecycle and permission forwarding.
- `jyowo-harness-team` owns run-scoped team membership, member routing,
  mailbox/task state, quotas, and team termination.
- `jyowo-harness-sdk` assembles these domains for application-facing calls.
- Tauri commands expose IPC only. They must delegate agent orchestration behavior
  through the SDK facade.

Capability resolution:

- settings switches for subagents, agent teams, and background agents must be
  backed by Rust capability resolution.
- capability availability must account for desktop feature flags, runtime
  support, provider/model support, workspace state, and isolation requirements.
- unavailable capability reasons are backend payloads. React must not invent
  availability state.
- agent tools are installed only after Rust validates settings and runtime
  capabilities into the resolved agent tool policy.

Background agent persistence:

- background agents are started only when the model calls the
  `background_agent` tool.
- the durable registry is the source of truth for background agent list, detail,
  input request, pause, resume, cancel, archive, and delete operations.
- registry state changes must be journaled or auditable before the UI observes
  them.
- user-visible background status is projected from backend records and events,
  not from frontend-local timers.

Supervisor process boundary:

- the agent supervisor sidecar is a process boundary for waking and executing
  durable background work.
- the supervisor may request work and report status, but it does not own policy.
- Rust validates persisted payloads, workspace scope, live token scope, and
  permission state before execution.
- invalid queued payloads fail closed and record a redacted failure reason.

Restart semantics:

- active in-memory handles are single-process state and may be lost on restart.
- durable background registry rows, journal events, audit state, permission
  decisions, task/mailbox state, and worktree leases are restart-stable state.
- startup recovery must classify interrupted background records and either
  resume them through the runtime or mark them with a redacted terminal failure.
- recovery must not replay unsafe child actions without a valid permission
  decision.

Worktree isolation:

- write-capable parallel agent work requires an isolation lease for the target
  checkout.
- a same-checkout write conflict fails closed.
- read-only agent work may run without a write lease only when policy marks it
  read-only.
- isolation enforcement belongs to Rust and must not be bypassed by command
  payloads or UI state.

Permission source attribution:

- every agent-originated permission request must carry its authoritative actor
  source.
- subagent requests identify the child agent and parent session/run.
- team member requests identify the team, member agent, role, and parent run
  when present.
- background agent requests identify the background agent and parent
  conversation/run context.
- role names and prompts are redacted before persistence and before UI
  projection.

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
  Raw thinking is projected as `ProcessStep` with `UiVisibility::UserSafe` or
  `UiVisibility::Withheld`. It must never serialize `kind: "thinking"`.
- tool failure summaries must be user-safe and must not expose raw payloads,
  private paths, or withheld placeholders.

## Evidence Ref Store

`crates/jyowo-harness-journal/src/evidence.rs` owns the `EvidenceRefStore` — a
durable, conversation-scoped registry for large evidence content.

- `EvidenceRefId` is an opaque id minted only by Rust.
- SQLite evidence rows live in the same runtime SQLite database as the
  conversation read model. Desktop must not keep a separate evidence registry
  database for conversation evidence.
- The registry stores `kind`, `conversation_id`, `run_id`, source event refs,
  content type, byte length, content hash, redaction state, redaction provenance,
  and retention for each ref.
- Evidence refs are retained with their owning conversation. Conversation
  deletion and SDK prune paths make refs unreadable for removed sessions.
- Blob-backed writes store bytes first, then the registry row. If registry
  write fails after blob write, the orphan blob is deleted before returning
  the error.
- Journal-backed refs store an event id plus JSON pointer. Reads reload the
  source event payload, extract the pointed value, and re-check byte length and
  BLAKE3 hash before returning bytes.
- Read order is registry row first, then source validation: conversation
  ownership, kind, retention, redaction state, redaction provenance, byte
  length, and content hash.
- Full command output, full diff patches, and artifact content are fetched by
  opaque ref through dedicated Tauri commands:
  `get_conversation_command_output(conversation_id, full_output_ref, cursor, max_bytes)`,
  `get_conversation_diff_patch(conversation_id, full_patch_ref, cursor, max_bytes)`,
  `get_artifact_revision_content(conversation_id, content_ref, cursor, max_bytes)`.
- Each fetch command validates conversation ownership, ref kind, redaction state,
  and retention before returning bytes.
- React must never construct, mutate, or authorize an evidence ref.
- The projector includes only `EvidenceRefSummary` or opaque `EvidenceRefId`
  values in `ConversationTurn`. Full content is never embedded.

## Permission Decision Options

Permission decisions are backend-authored with opaque option ids:

- `PermissionRequestedEvent.presented_options` is `Vec<PermissionDecisionOption>`,
  not bare `Decision` values.
- Each `PermissionDecisionOption` carries `option_id`, `decision`, `scope`,
  `lifetime`, `matcher_summary`, `label`, `requires_confirmation`,
  `action_plan_hash`, and optional `fingerprint`.
- The option id is minted by Rust when the pending request is created. It binds
  to the request id, action plan hash, scope, decision, and fingerprint.
- React submits only `requestId`, `decision`, backend-issued `optionId`, and
  optional `confirmationText`. React never submits matcher internals, policy
  fields, sandbox state, risk level, or data exposure as authority.
- Rust resolves `(conversation_id, request_id, option_id)` against the
  still-pending backend-authored decision option. Missing, stale, mismatched,
  or already-resolved options fail closed.

## Artifact Revision Workspace

Artifacts are versioned workspace entities:

- `ArtifactCreatedEvent` and `ArtifactUpdatedEvent` carry a required
  `revision_id: ArtifactRevisionId`.
- The projector emits `ArtifactRevisionSummary` with `artifact_id`, `revision_id`,
  `kind`, `status`, `source_run_id`, `title`, `summary`, `preview_ref`,
  `content_ref`, and optional `media`.
- Artifact content bytes are fetched through
  `get_artifact_revision_content(conversationId, contentRef)`.
- Image artifact preview bytes are fetched through
  `get_artifact_media_preview(conversationId, artifactId, revisionId, contentRef)`.
  The command validates the selected artifact, revision, status, kind, projected
  content or preview ref, evidence owner, retention, redaction provenance, byte
  length, and hash before returning an image data URL.
- HTML and code previews are sandboxed. Updates create revisions instead of
  mutating user-visible history.

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

Conversation metadata and provider settings live in:

```text
.jyowo/runtime/conversation-metadata.json
.jyowo/runtime/provider-settings.json
```

Runtime flow:

```text
user message
  -> start_run request.model_config_id
  -> run model snapshot
  -> optional tool call
  -> ToolPool exposes only route-enabled service tools
  -> tool carries ToolServiceBinding metadata
  -> route resolver selects configured provider profile
  -> credential resolver returns run-scoped or operation-scoped credential
  -> provider service adapter executes operation
  -> BlobStore stores media output
  -> engine creates ArtifactCreated / ArtifactUpdated from typed tool output
  -> read model projects artifact
```

Ownership rules:

- Run model selection stays separate from service route selection.
- User attachments are validated only against the run model input modalities.
- Media generation is a routed provider service, not `ConversationModelCapability.output_modalities`.
- A provider profile may be both the run model and a service route only through explicit route config.
- Unconfigured service tools must not be exposed to the run model.
- Run models without tool calling must not receive autonomous service tools.
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
else if model_config_id is present:
  find provider config by model_config_id
  validate provider id and API key
  return run model credential
else:
  fail closed
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
**Memory Platform:** Memory is auxiliary context below system, runtime policy,
workspace instructions, and explicit user request. Memory references hydrate
through the Rust resolver and use the same untrusted-context fencing as recalled
memory. Model request preview is built from the final redacted request assembly
path. Memory is not a source of truth for current external facts. Memory
settings, inbox, traces, and previews are displayed by frontend UI; memory
policy decisions, export assembly, raw export permission, and audit emission are
owned by the Rust backend.
