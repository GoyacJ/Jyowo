# Authorization, Permission, And Sandbox Refactor Implementation Plan

> **For agentic workers:** REQUIRED MODEL PROFILE: use GPT-5.5 Pro. When the tool requires API-style overrides, use `model: gpt-5.5`, `reasoning_effort: xhigh`, and `service_tier: priority` for implementation and audit subagents. Do not downgrade to a smaller model.
>
> **Required sub-skill:** use `superpowers:subagent-driven-development` task-by-task. Every task must end with a read-only subagent audit. Do not self-certify a task.

**Goal:** Refactor Jyowo authorization, permission, and sandbox execution into one explicit backend-owned enforcement pipeline.

**Architecture:** Tools produce typed action plans. A new L3 execution authority turns action plans into permission requests, applies hard policy, resolves user consent, mints one-time authorization tickets, runs sandbox preflight, and only then executes. React remains a display and input layer; Rust remains the policy authority.

**Tech Stack:** Rust 1.96, Tauri 2, React 19, TypeScript 6, Zod, schemars JsonSchema, cargo test, Vitest, pnpm 11.7, Git worktrees, existing Jyowo docs gates.

---

## Branch And Worktree Rules

This plan file must be tracked on `main` before implementation starts. Implementation must not run in the active checkout.

Start implementation from an isolated worktree:

```bash
SOURCE_CHECKOUT="$(pwd)"
PLAN_PATH="docs/superpowers/plans/2026-07-02-authorization-sandbox-refactor.md"
test -f "$PLAN_PATH"
test "$(git branch --show-current)" = "main"
test "$(git ls-files -- "$PLAN_PATH")" = "$PLAN_PATH"
test -z "$(git status --short -- "$PLAN_PATH")"
git status --short

git worktree add -b goya/authorization-sandbox-refactor ../Jyowo-authorization-sandbox-refactor main
cd ../Jyowo-authorization-sandbox-refactor
test -f "$PLAN_PATH"
test "$(git branch --show-current)" = "goya/authorization-sandbox-refactor"
git status --short
```

Expected:

- source checkout branch is `main`
- `docs/superpowers/plans/2026-07-02-authorization-sandbox-refactor.md` is tracked and clean on `main`
- implementation branch is `goya/authorization-sandbox-refactor`
- implementation happens only in `../Jyowo-authorization-sandbox-refactor`
- implementation worktree starts from tracked `main` content; do not copy an untracked plan file into it
- if branch or worktree already exists, stop and ask for a new branch name

Do not stash, revert, or overwrite unrelated user changes. Stage exact files only. Never stage broad directories such as `crates`, `apps`, or `docs`.

## Mandatory Execution Protocol

Every task must follow this exact order.

1. **Task Intent Check**
   - Restate the task objective.
   - List exact in-scope files.
   - List exact out-of-scope files.
   - State the invariant that must remain true.
   - State the tests and gates for this task.
   - State why this task does not add mock data, fake runtime paths, noop success, placeholder behavior, or UI-only policy.

2. **Read Required Context**
   - Read root `AGENTS.md`.
   - Read `docs/testing/testing-strategy.md`.
   - For backend changes, read:
     - `docs/backend/agent-harness-backend-development-guidelines.md`
     - `docs/backend/backend-runtime.md`
     - `docs/backend/backend-engineering.md`
     - `docs/backend/backend-quality.md`
   - For frontend changes, read:
     - `docs/frontend/agent-harness-frontend-development-guidelines.md`
     - `docs/frontend/frontend-product-ux.md`
     - `docs/frontend/frontend-engineering.md`
     - `docs/frontend/frontend-quality.md`
     - `docs/design2/antigravity_2_0_design_system_specification.md`
   - Read every file listed by the task before editing it.

3. **Write Failing Tests First**
   - Add or update tests in the owning crate or frontend boundary.
   - Run the narrow test and confirm it fails for the intended reason.
   - If a failing test cannot be written first, explain why in the task response and add the nearest executable contract test before implementation.

4. **Implement**
   - Make the smallest task-scoped implementation.
   - Destructive refactor is allowed when it removes a broken design or prevents technical debt.
   - Do not keep compatibility wrappers for old semantics unless the task explicitly requires a migration window.

5. **Local Gate**
   - Run the task-specific commands.
   - If the task changes Rust files, run `cargo fmt --all --check`.
   - If the task changes any public Rust API, event enum, serde contract, workspace member, feature flag, `Tool` trait, `PermissionContext`, or Tauri IPC payload, run `cargo check --workspace` before the task audit.
   - Do not create a task commit that leaves the workspace uncompilable. If the task-specific verification list omits `cargo check --workspace` for a public API break, add it for that task.
   - If the task adds, renames, moves, splits, or deletes tests, run `pnpm check:test-architecture`.
   - If the task changes the test inventory structurally, run `pnpm audit:tests` and commit the intended inventory update if the repository owns one.
   - Run `git diff --check`.
   - Inspect `git diff --name-only`.

6. **Subagent Audit**
   - Spawn a read-only subagent with GPT-5.5 Pro. If the tool requires API-style overrides, pass `model: gpt-5.5`, `reasoning_effort: xhigh`, `service_tier: priority`.
   - The audit must return `PASS` or `FAIL`.
   - `FAIL` must include file and line evidence.
   - Fix failures and rerun the same audit before moving on.

7. **Commit**
   - Commit each task separately.
   - Commit message format: `refactor: <task subject>`.
   - Do not commit unrelated files.

Subagent audit prompt template:

```text
Read-only audit for Task N of docs/superpowers/plans/2026-07-02-authorization-sandbox-refactor.md.

Use GPT-5.5 Pro. If the tool requires API-style overrides, use `model: gpt-5.5`, `reasoning_effort: xhigh`, `service_tier: priority`.
Do not edit files.

Task objective:
<copy the Task Intent Check>

Evidence:
- owned files changed:
  - <exact files>
- commands run:
  - command: <exact command>
    exit: <exit code>
- git diff --check: exit <exit code>
- git diff --name-only:
  - <exact files>

Verify:
- implementation matches this task and not a different design
- final policy decision remains in Rust
- PermissionBroker cannot be bypassed by Tool, filesystem, network, sandbox, MCP, subagent, team, background agent, or Tauri command paths
- BypassPermissions and DontAsk skip only interactive waiting, not hard policy, tenant/workspace scope, sandbox capability, or redaction
- no production mock data, fake runtime path, noop success, placeholder behavior, or UI-only policy was introduced
- no old compatibility branch was kept without an explicit requirement in the plan
- tests cover the changed boundary
- command evidence includes every required gate with exit code 0
- Rust tasks include `cargo fmt --all --check`
- test structure changes include `pnpm check:test-architecture`
- changed files are limited to task-owned files

Return PASS or FAIL.
For FAIL, include exact file and line evidence.
```

If multi-agent tools are unavailable, stop. Do not continue without the read-only subagent audit.

## Forbidden Throughout

- Production `allow_all` broker.
- Production compatibility broker that silently preserves old permission semantics.
- Production `NoopDecisionPersistence` pretending to provide integrity.
- Production fake, mock, fixture, or placeholder runtime paths.
- Production noop success returns.
- UI-only policy decisions.
- A Tauri command that grants or upgrades permission.
- A tool that reads, writes, shells out, calls network, or calls MCP without an authorization ticket.
- A sandbox backend that reports capability it cannot enforce.
- Docker `supports_network: true` as a substitute for per-exec network policy.
- `BypassPermissions` or `DontAsk` bypassing hard policy, tenant/workspace scope, sandbox capability, ticket validation, redaction, or event ordering.
- Raw secrets in prompt, events, logs, traces, screenshots, frontend state, fixtures, or snapshots.
- Test data named mock/fake/noop unless it is an existing deny-only safety type and not a success path.

Allowed in tests:

- `tempfile` directories.
- Existing in-memory event stores where the owning crate already uses them.
- Deterministic test providers that exercise the same production code path and are named as test adapters, not production mocks.
- Existing `NoopSandbox` only for deny/fail-closed tests; never for successful execution.

## Current Code Facts

These are observed facts from the current codebase and must guide the refactor.

- `ToolOrchestrator` currently performs `validate`, `check_permission`, `PermissionBroker::decide`, then calls `tool.execute`. File and network execution remain inside individual tools.
  - `crates/jyowo-harness-tool/src/orchestrator.rs`
- `PermissionContext.rule_snapshot` currently participates in hard policy checks. Engine and MCP sampling create empty snapshots in call paths.
  - `crates/jyowo-harness-permission/src/broker.rs`
  - `crates/jyowo-harness-engine/src/turn.rs`
  - `crates/jyowo-harness-mcp/src/sampling.rs`
- Permission dedup exists both in `jyowo-harness-permission::DedupGate` and inside engine `RecordingPermissionBroker`.
  - `crates/jyowo-harness-permission/src/dedup.rs`
  - `crates/jyowo-harness-engine/src/turn.rs`
- `execute_with_lifecycle` does not call `before_execute`. `LocalSandbox` calls it internally; Docker does not have the same lifecycle entry.
  - `crates/jyowo-harness-sandbox/src/backend.rs`
  - `crates/jyowo-harness-sandbox/src/local/exec.rs`
  - `crates/jyowo-harness-sandbox/src/docker.rs`
- `LocalSandbox::new` defaults to `LocalIsolation::None`. This is not OS isolation.
  - `crates/jyowo-harness-sandbox/src/local/mod.rs`
- Local `NetworkAccess::None` fails without OS isolation. `LoopbackOnly` and `AllowList` are not implemented for local backend.
  - `crates/jyowo-harness-sandbox/src/local/exec.rs`
- Docker network mode is configured on `docker run` / container lifecycle, not reliably per `docker exec`.
  - `crates/jyowo-harness-sandbox/src/docker.rs`
- File read/write built-ins currently call `std::fs` directly after permission.
  - `crates/jyowo-harness-tool/src/builtin/read.rs`
  - `crates/jyowo-harness-tool/src/builtin/write.rs`
- Web search currently asks for network permission, then executes through its backend.
  - `crates/jyowo-harness-tool/src/builtin/web_search.rs`
- Desktop `resolve_permission` validates pending request and window subscription, then delegates to resolver. It must remain UI resolution only.
  - `apps/desktop/src-tauri/src/commands/conversations.rs`
  - `apps/desktop/src-tauri/src/commands/contracts.rs`
  - `apps/desktop/src/shared/tauri/commands.ts`
- `DecisionPersistence` currently exposes `supports_integrity()` and `persist(...)` only. `FileDecisionPersistence::load_decisions()` exists on the concrete type but is not available through the persistence trait.
  - `crates/jyowo-harness-permission/src/broker.rs`
  - `crates/jyowo-harness-permission/src/persistence/file.rs`
- Existing tool events are `ToolUseRequested`, `ToolUseApproved`, `ToolUseDenied`, `ToolUseCompleted`, `ToolUseFailed`, and `ToolUseHeartbeat`. There is no current `ToolStarted`, `ToolCompleted`, or `ToolFailed` contract.
  - `crates/jyowo-harness-contracts/src/events/tool.rs`
  - `crates/jyowo-harness-contracts/src/events/mod.rs`
- Existing `PermissionMode` variants include `Default`, `Plan`, `AcceptEdits`, `BypassPermissions`, `DontAsk`, and `Auto`.
  - `crates/jyowo-harness-contracts/src/enums.rs`
- Existing Rust permission actor sources cover parent run, subagent, team member, and background agent. Conversation read-model projection exposes frontend `actorSource.type` values as camelCase.
  - `crates/jyowo-harness-contracts/src/events/permission.rs`
  - `crates/jyowo-harness-journal/src/conversation_read_model.rs`
  - `apps/desktop/src/shared/events/run-event-schema.ts`

## Target Architecture

The target runtime flow is fixed:

```text
ToolCall raw input
  -> existing ToolUseRequested event
  -> Tool validate_input
  -> ToolActionPlan
  -> AuthorizationService preflight
  -> hard policy and scope check
  -> PermissionBroker decision
  -> PermissionRequested / PermissionResolved events
  -> one-time AuthorizationTicket
  -> sandbox capability preflight
  -> execute_authorized
  -> existing ToolUseCompleted / ToolUseFailed / Sandbox events
```

Layer ownership:

- L0 `jyowo-harness-contracts`
  - public IDs, serde payloads, events, JsonSchema
  - `ToolActionPlan` and UI-safe permission review payloads
  - event shape for requested/resolved/preflight/audit data

- L1 `jyowo-harness-permission`
  - rule providers
  - rule engine
  - hard policy
  - decision history and dedup
  - signed persistence
  - interactive stream broker as a primitive, not the whole authority

- L1 `jyowo-harness-sandbox`
  - backend capability truth
  - sandbox policy preflight
  - lifecycle events
  - local/docker/noop/ssh execution backends
  - fail-closed when requested policy cannot be enforced

- L2 `jyowo-harness-tool`
  - tool descriptors
  - input validation
  - typed action planning
  - authorized execution adapters
  - no final allow/deny decision

- L3 `jyowo-harness-execution`
  - new crate
  - cross-domain authorization and execution pipeline
  - authorization tickets
  - ticket ledger
  - event ordering through an injected `AuthorizationEventSink`
  - orchestration of permission, sandbox, and tool execution
  - no direct `EventStore` ownership

- L3 `jyowo-harness-engine`
  - model/tool loop
  - run orchestration
  - no separate permission authority or dedup decision engine

- L4 `jyowo-harness-sdk`
  - application-facing assembly
  - desktop permission authority wiring
  - sandbox backend wiring

- Tauri shell `apps/desktop/src-tauri`
  - IPC boundary only
  - pending permission resolution only
  - no business policy

- React `apps/desktop/src`
  - Zod parsing
  - permission UI
  - command submission
  - no final policy

### Core Runtime Types

Implement these names unless a direct compile conflict requires a smaller local rename.

Public contract types belong in `crates/jyowo-harness-contracts`.

```rust
pub struct ToolActionPlan {
    pub plan_id: ActionPlanId,
    pub tool_use_id: ToolUseId,
    pub tool_name: String,
    pub actor_source: PermissionActorSource,
    pub subject: PermissionSubject,
    pub scope: DecisionScope,
    pub severity: Severity,
    pub resources: Vec<ActionResource>,
    pub sandbox_policy: SandboxPolicy,
    pub workspace_access: WorkspaceAccess,
    pub network_access: NetworkAccess,
    pub review: PermissionReview,
    pub plan_hash: ActionPlanHash,
    pub created_at: DateTime<Utc>,
}

pub enum ActionResource {
    FileRead { path: PathBuf },
    FileWrite { path: PathBuf, content_hash: String },
    FileDelete { path: PathBuf },
    Command { command: String, argv: Vec<String>, cwd: Option<PathBuf>, fingerprint: ExecFingerprint },
    Network { host: String, port: Option<u16> },
    McpTool { server_id: String, origin: ManifestOriginRef, tool_name: String },
    McpSampling { server_id: String, origin: ManifestOriginRef },
    McpResource { server_id: String, origin: ManifestOriginRef, operation: McpResourceOperation },
    McpPrompt { server_id: String, origin: ManifestOriginRef, operation: McpPromptOperation },
    McpTransport { server_id: String, origin: ManifestOriginRef, target: McpTransportTarget },
    Sandbox { backend_id: String, policy_hash: SandboxPolicyHash },
}

pub enum McpResourceOperation {
    List,
    Read { uri: String },
    Subscribe { uri: String },
    Unsubscribe { uri: String },
}

pub enum McpPromptOperation {
    List,
    Get { name: String },
}

pub struct McpTransportTarget {
    pub transport: String,
    pub endpoint_label: String,
    pub endpoint_fingerprint: String,
}

pub struct PermissionReview {
    pub summary: String,
    pub details: Vec<PermissionReviewDetail>,
    pub confirmation: PermissionConfirmation,
    pub redacted: bool,
}

pub enum PermissionConfirmation {
    None,
    ExplicitButton { label: String },
    TypeToConfirm { expected: String },
}
```

### Canonical Hashing Rules

All authorization hashes must be implemented in `crates/jyowo-harness-contracts` or in a single helper module re-exported from that crate. Do not duplicate hash logic in tool, execution, engine, SDK, or desktop code.

Hash algorithm:

- use BLAKE3 32-byte digest
- public hex string representation is lowercase hex
- domain separators are mandatory:
  - `jyowo.tool_action_plan.v1`
  - `jyowo.sandbox_policy.v1`
  - `jyowo.action_resource.v1`
  - `jyowo.file_content.v1`
  - `jyowo.exec_spec.v1`
  - `jyowo.mcp_transport_target.v1`
- never use Rust `Debug`, map iteration order, or non-canonical serde output as a hash input

Canonical inputs:

- `ActionPlanHash` covers every security-relevant `ToolActionPlan` field except `plan_hash` and `created_at`
- `SandboxPolicyHash` covers sandbox policy, backend id, backend capability version, workspace access, and network access
- `ActionResource` paths are canonicalized before hashing:
  - existing paths use filesystem canonicalization after workspace scope validation
  - new file writes canonicalize the parent directory and append the final file name
  - symlink escape fails closed
  - relative paths are rejected before hash creation
- file write `content_hash` is BLAKE3 over raw bytes with the `jyowo.file_content.v1` domain separator
- command fingerprints use canonical `ExecSpec` fields, not shell-rendered preview text
- network resources normalize host casing and explicit default ports before hashing
- MCP resource URI, prompt name, server id, transport id, and transport endpoint fingerprint are included before authorization
- MCP transport endpoint fingerprints use `jyowo.mcp_transport_target.v1` over transport kind, server id, origin, normalized endpoint without credentials, and stdio command fingerprint when applicable
- MCP transport endpoint labels must be redacted UI labels; raw bearer tokens, query secrets, or local absolute paths must not enter review text, logs, or frontend state

Tests must prove equivalent canonical inputs produce the same hash, security-relevant input changes produce different hashes, and non-canonical path or symlink escape fails closed.

### Confirmation Safety Rules

`PermissionConfirmation::TypeToConfirm.expected` is UI-visible and must be treated as non-secret output.

It must:

- be generated by Rust from fixed UI-safe phrases or redacted labels
- never contain raw command text, raw file content, raw secret, raw token, provider credential, full unredacted URL, or unredacted absolute path
- use a short deterministic phrase for destructive actions, such as `DELETE`, `OVERWRITE`, or a redacted resource label
- be covered by redaction tests and frontend state tests

If the only meaningful confirmation phrase would expose sensitive data, use a fixed phrase and put the sensitive resource identity only in hashed/redacted review details.

Actor source contract is fixed. Do not use free-form actor strings.

Rust contract must include these semantic variants:

```rust
pub enum PermissionActorSource {
    ParentRun,
    Subagent { subagent_id: SubagentId, parent_session_id: SessionId, parent_run_id: RunId, team_id: Option<TeamId>, team_member_profile_id: Option<String> },
    TeamMember { team_id: TeamId, agent_id: AgentId, role: String, parent_run_id: Option<RunId> },
    BackgroundAgent { background_agent_id: BackgroundAgentId, conversation_id: SessionId, attempt_id: Option<RunId> },
    Automation { automation_id: String, conversation_id: SessionId, run_id: Option<RunId> },
    McpServer { server_id: McpServerId, origin: ManifestOriginRef, scope: McpServerScope },
}
```

Frontend projection must expose `actorSource.type` as:

```text
parentRun
subagent
teamMember
backgroundAgent
automation
mcpServer
```

If raw Rust serde shape and frontend projection shape differ, Task 1 must add tested conversion in the owning projection layer. Rust contract tests and frontend Zod tests must both cover the two new actor sources.

Internal execution crate types belong in `crates/jyowo-harness-execution`.

```rust
pub struct AuthorizationRequest {
    pub plan: ToolActionPlan,
    pub tenant_id: TenantId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub permission_mode: PermissionMode,
    pub interactivity: InteractivityLevel,
    pub fallback_policy: FallbackPolicy,
}

pub struct AuthorizationTicket {
    pub ticket_id: AuthorizationTicketId,
    pub request_id: RequestId,
    pub decision_id: DecisionId,
    pub plan_hash: ActionPlanHash,
    pub sandbox_policy_hash: SandboxPolicyHash,
    pub scope: DecisionScope,
    pub expires_at: DateTime<Utc>,
    pub consumed: bool,
}
```

Ticket rules:

- ticket never crosses Tauri IPC
- ticket never appears in frontend state
- ticket is single-use
- ticket expires before a later run can reuse it
- ticket is bound to tenant, session, run, tool use, plan hash, and sandbox policy hash
- ticket mismatch fails closed

### Permission Mode Semantics

`PermissionMode::Default`:

- apply hard policy
- apply rule engine
- ask user when decision escalates and interactivity allows
- deny when interactivity is unavailable

`PermissionMode::Plan`:

- apply hard policy
- produce review and plan metadata only
- do not mint authorization tickets
- do not execute filesystem writes, process execution, network calls, MCP calls, or sandbox-backed commands
- record review-only events when useful, but never persist an allow decision from plan mode

`PermissionMode::AcceptEdits`:

- apply hard policy
- apply tenant/workspace scope
- apply rule engine
- auto-allow only planned low or medium risk file edit/write actions inside the validated workspace scope
- require explicit approval for delete, command execution, network, MCP, high, or critical actions unless a policy rule denies first
- never bypass sandbox capability, ticket validation, redaction, or event ordering

`PermissionMode::Auto`:

- apply hard policy
- apply rule engine
- auto-allow only rule-allowed low and medium risk actions
- high and critical risk still require explicit user approval unless a policy rule denies first

`PermissionMode::BypassPermissions` and `PermissionMode::DontAsk`:

- apply hard policy
- apply tenant/workspace scope
- apply sandbox capability preflight
- apply redaction and event ordering
- auto-resolve permission interaction as allow only after the above pass
- record requested and resolved events with `auto_resolved = true`
- never persist permanent approvals from bypass mode

### Event Ordering

Required order for actions that need approval:

```text
ToolUseRequested
PermissionRequested
PermissionResolved
AuthorizationTicketMinted or internal audit record
SandboxPreflightPassed or SandboxPreflightFailed
SandboxExecutionStarted when command-backed
ToolUseCompleted or ToolUseFailed
```

This plan reuses the existing `ToolUseRequested`, `ToolUseCompleted`, and `ToolUseFailed` contracts. Do not add `ToolStarted`, `ToolCompleted`, or `ToolFailed` unless Task 1 is explicitly rewritten to add those event variants, schema exports, journal projection, frontend Zod schema, and tests.

`jyowo-harness-execution` must not own durable journal append directly. It must emit ordered event batches through an injected `AuthorizationEventSink` trait. The engine or SDK adapter owns the `harness_journal::EventStore` append and redaction boundary.

If event append or redaction fails before execution, execution fails closed.

## Task 1: Add Authorization Contracts

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/ids.rs`
- Modify: `crates/jyowo-harness-contracts/src/enums.rs`
- Modify: `crates/jyowo-harness-contracts/src/tool.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/permission.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/sandbox.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/mod.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Modify: `crates/jyowo-harness-engine/src/turn.rs`
- Modify: `crates/jyowo-harness-mcp/src/sampling.rs`
- Create: `crates/jyowo-harness-contracts/tests/authorization_contracts.rs`
- Modify: `crates/jyowo-harness-contracts/tests/tool_contracts.rs`
- Modify: `crates/jyowo-harness-contracts/tests/core_contracts.rs`
- Modify: `crates/jyowo-harness-journal/tests/conversation_read_model.rs`
- Modify: `apps/desktop/src/shared/events/run-event-schema.ts`
- Modify: `apps/desktop/src/shared/events/run-event-schema.test.ts`

**Design:**

Add public serde and JsonSchema types for:

- `ActionPlanId`
- `ActionPlanHash`
- `SandboxPolicyHash`
- `AuthorizationTicketId`
- `ActionResource`
- `McpResourceOperation`
- `McpPromptOperation`
- `McpTransportTarget`
- `PermissionReview`
- `PermissionReviewDetail`
- `PermissionConfirmation`
- `ToolActionPlan`
- `SandboxPreflightStatus`
- `SandboxPolicySummary`

Extend permission events without exposing internal tickets to React:

- `PermissionRequestedEvent` includes:
  - `action_plan_hash`
  - `review`
  - `effective_mode`
  - `sandbox_policy`
  - keep existing `auto_resolved`
  - keep existing `actor_source`
- `PermissionResolvedEvent` includes:
  - `action_plan_hash`
  - `decision_id`
  - `auto_resolved`

Extend `PermissionActorSource` with:

- `Automation`
- `McpServer`

The frontend conversation projection must expose actor source payloads using existing camelCase `actorSource.type` values and add `automation` and `mcpServer`. Do not expose a second actor source naming convention in React.

Add sandbox preflight event shape:

- `SandboxPreflightPassedEvent`
- `SandboxPreflightFailedEvent`

Do not add raw command output, raw secrets, or full unredacted input payloads.

Do not add `ToolStarted`, `ToolCompleted`, or `ToolFailed`. This plan uses existing `ToolUseRequested`, `ToolUseCompleted`, and `ToolUseFailed`.

**Steps:**

- Write contract tests for serde shape and JsonSchema export.
- Write Rust tests for `PermissionActorSource::Automation` and `PermissionActorSource::McpServer` serialization plus conversation projection payloads.
- Write frontend Zod tests for valid and invalid permission review payloads and actor source payloads.
- Implement Rust contract types.
- Update schema export and all exhaustive actor source matches in engine, MCP, and journal projection.
- Update frontend Zod schema to parse new fields.
- Run gates.

**Verification:**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-contracts --test authorization_contracts
cargo test -p jyowo-harness-contracts --test tool_contracts
cargo test -p jyowo-harness-contracts --test core_contracts
cargo test -p jyowo-harness-journal --test conversation_read_model
pnpm -C apps/desktop test -- run-event-schema
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit focus:** Contract shapes are stable, UI-safe, and do not expose internal authorization tickets or secrets.

## Task 2: Refactor Permission Into A Single Authority Stack

**Files:**

- Modify: `crates/jyowo-harness-permission/src/broker.rs`
- Modify: `crates/jyowo-harness-permission/src/chain.rs`
- Modify: `crates/jyowo-harness-permission/src/dedup.rs`
- Modify: `crates/jyowo-harness-permission/src/direct.rs`
- Modify: `crates/jyowo-harness-permission/src/stream.rs`
- Modify: `crates/jyowo-harness-permission/src/rule.rs`
- Modify: `crates/jyowo-harness-permission/src/rule_engine.rs`
- Modify: `crates/jyowo-harness-permission/src/persistence/file.rs`
- Modify: `crates/jyowo-harness-permission/src/persistence/mod.rs`
- Modify: `crates/jyowo-harness-permission/src/lib.rs`
- Create: `crates/jyowo-harness-permission/src/authority.rs`
- Modify: `crates/jyowo-harness-permission/tests/contract.rs`
- Modify: `crates/jyowo-harness-permission/tests/dedup.rs`
- Modify: `crates/jyowo-harness-permission/tests/stream.rs`
- Modify: `crates/jyowo-harness-permission/tests/rule_engine.rs`
- Modify: `crates/jyowo-harness-permission/tests/rule_engine_dangerous.rs`
- Modify: `crates/jyowo-harness-permission/tests/file_persistence.rs`
- Modify: `crates/jyowo-harness-permission/tests/integrity_signer.rs`
- Modify as part of the same commit if `PermissionContext.rule_snapshot` is removed:
  - `crates/jyowo-harness-engine/src/engine.rs`
  - `crates/jyowo-harness-engine/src/turn.rs`
  - `crates/jyowo-harness-session/src/turn.rs`
  - `crates/jyowo-harness-mcp/src/sampling.rs`
  - `crates/jyowo-harness-subagent/tests/permission_bridge.rs`
  - `crates/jyowo-harness-tool/tests/orchestrator.rs`
  - `crates/jyowo-harness-tool/tests/result_budget.rs`
  - `crates/jyowo-harness-tool/tests/permission_fingerprint.rs`
  - `crates/jyowo-harness-tool/tests/builtin_exec.rs`
  - `crates/jyowo-harness-sdk/tests/facade.rs`
  - `crates/jyowo-harness-sdk/tests/mcp_server_adapter.rs`
  - `apps/desktop/src-tauri/tests/commands/support.rs`
  - `crates/jyowo-harness-agent-runtime/tests/subagents.rs`

**Design:**

Remove `rule_snapshot` from `PermissionContext`.

Hard policy must be owned by the broker stack, not by arbitrary call sites. Delete or make private any helper that lets callers supply a local snapshot as authority.

Add `PermissionAuthority`:

```rust
pub struct PermissionAuthority {
    policy_broker: Arc<dyn PermissionBroker>,
    interactive_broker: Option<Arc<dyn PermissionBroker>>,
    dedup: DedupGate,
    decision_store: Arc<dyn DecisionStore>,
}

#[async_trait]
pub trait DecisionHistory: Send + Sync + 'static {
    async fn find_scoped_decision(
        &self,
        lookup: DecisionLookup,
    ) -> Result<Option<PersistedDecision>, PermissionError>;
}

pub trait DecisionStore: DecisionPersistence + DecisionHistory {}
```

`DecisionLookup` must include tenant id, session id, requested scope, subject or fingerprint, decision source, permission mode, and lookup time. `FileDecisionPersistence::load_decisions()` must become reachable through the trait-object path used by production authority. Tampered or unreadable persistence must fail closed and emit the existing tamper event path.

Required decision order:

1. tenant/session context check
2. hard policy deny
3. rule allow/deny/default
4. previous persisted scoped decision
5. dedup for recent equivalent requests
6. permission mode semantics
7. interactive stream broker when needed
8. signed persistence for durable decisions
9. audit metadata returned to caller

`StreamBasedBroker` becomes an interaction primitive. It must not be the production authority by itself.

`NoopDecisionPersistence::supports_integrity()` must return `false`.

`BypassPermissions` and `DontAsk` may auto-allow only after hard policy and scope checks. They must never create permanent persisted approvals.

This task must complete before the execution crate is created. `jyowo-harness-execution` must depend on the final `PermissionAuthority` API, not on old `PermissionBroker` semantics or a temporary compatibility authority.

**Steps:**

- Write failing tests proving:
  - empty call-site snapshot cannot disable policy deny
  - bypass cannot override policy deny
  - stream broker alone is not accepted as production authority
  - Noop persistence does not satisfy integrity requirement
  - persisted scoped decisions are read through `DecisionHistory`, not by downcasting to `FileDecisionPersistence`
  - tampered persisted decisions fail closed and are not reused
  - duplicate high-risk allow is not silently reused as global allow
- Remove `rule_snapshot` from `PermissionContext`.
- Update every downstream `PermissionContext` constructor listed above in the same task.
- Update permission crate tests and helpers.
- Implement `PermissionAuthority`, `DecisionHistory`, `DecisionLookup`, and `DecisionStore`.
- Keep primitive brokers only as internal pipeline pieces or test adapters.
- Run gates.

**Verification:**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-permission --all-features
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit focus:** There is one authority stack, hard policy cannot depend on caller-provided empty snapshots, and bypass modes do not skip hard policy.

## Task 3: Create L3 Execution Authority Crate

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/jyowo-harness-execution/Cargo.toml`
- Create: `crates/jyowo-harness-execution/src/lib.rs`
- Create: `crates/jyowo-harness-execution/src/error.rs`
- Create: `crates/jyowo-harness-execution/src/service.rs`
- Create: `crates/jyowo-harness-execution/src/ticket.rs`
- Create: `crates/jyowo-harness-execution/src/event_sink.rs`
- Create: `crates/jyowo-harness-execution/src/audit.rs`
- Create: `crates/jyowo-harness-execution/tests/ticket.rs`
- Create: `crates/jyowo-harness-execution/tests/authorization_flow.rs`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`

**Design:**

Add new workspace crate:

```toml
[package]
name = "jyowo-harness-execution"

[lib]
name = "harness_execution"
```

Layer: L3.

Dependencies allowed:

- `jyowo-harness-contracts`
- `jyowo-harness-permission`
- `jyowo-harness-sandbox`
- `jyowo-harness-tool`

Dependencies forbidden:

- `jyowo-harness-sdk`
- `jyowo-harness-engine`
- `jyowo-harness-journal`
- `jyowo-desktop-shell`
- frontend packages

Expose:

```rust
pub struct AuthorizationService { ... }
pub struct AuthorizationContext { ... }
pub struct AuthorizationOutcome { ... }
pub struct TicketLedger { ... }
pub trait AuthorizationEventSink { ... }
pub enum ExecutionError { ... }
```

`AuthorizationService` must call the `PermissionAuthority` created in Task 2. It must not call a raw `PermissionBroker` as the final decision authority. A deny-only permission test adapter is allowed only inside tests and must not be exported as a production assembly path.

`AuthorizationEventSink` must accept ordered `harness_contracts::Event` batches plus tenant/session identity. It must not expose `harness_journal::EventStore` in the execution crate API. Engine or SDK code will provide the journal-backed adapter later.

`TicketLedger` must:

- mint ticket only for allow decisions
- reject unknown tickets
- reject expired tickets
- reject consumed tickets
- reject plan hash mismatch
- reject tenant/session/run/tool_use mismatch
- consume exactly once

No tool execution integration yet. This task creates compileable execution primitives and tests on top of the already-finished permission authority.

**Steps:**

- Write ticket ledger tests first.
- Write a minimal authorization flow test using the real `PermissionAuthority`, a real hard-deny policy rule, and real contract types.
- Write an event sink ordering test proving execution emits preflight and permission events through the sink without importing `jyowo-harness-journal`.
- Add crate and exports.
- Add workspace member.
- Update backend layer table and critical backend tests list.
- Run gates.

**Verification:**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-execution
cargo check --workspace
pnpm check:backend-docs
pnpm check:test-architecture
git diff --check
```

**Audit focus:** New crate follows layer direction and contains no SDK, desktop, or frontend dependency.

## Task 4: Fix Sandbox Lifecycle And Capability Truth

**Files:**

- Modify: `crates/jyowo-harness-sandbox/src/backend.rs`
- Modify: `crates/jyowo-harness-sandbox/src/policy.rs`
- Modify: `crates/jyowo-harness-sandbox/src/local/mod.rs`
- Modify: `crates/jyowo-harness-sandbox/src/local/exec.rs`
- Modify: `crates/jyowo-harness-sandbox/src/docker.rs`
- Modify: `crates/jyowo-harness-sandbox/src/noop.rs`
- Modify: `crates/jyowo-harness-sandbox/tests/api_contract.rs`
- Modify: `crates/jyowo-harness-sandbox/tests/local.rs`
- Modify: `crates/jyowo-harness-sandbox/tests/docker.rs`
- Modify: `crates/jyowo-harness-sandbox/tests/noop.rs`
- Modify: `crates/jyowo-harness-sandbox/tests/fingerprint.rs`

**Design:**

`execute_with_lifecycle` owns lifecycle:

```text
before_execute
  -> backend.execute
  -> after_execute through LifecycleActivity
```

Backends must not call `before_execute` internally after this task.

Add sandbox preflight API:

```rust
pub fn preflight_exec(
    backend: &dyn SandboxBackend,
    spec: &ExecSpec,
    ctx: &ExecContext,
) -> Result<SandboxPreflightReport, SandboxError>
```

Capability rules:

- local backend with `LocalIsolation::None` must not claim OS sandbox isolation
- local backend may allow unrestricted network only when policy requests unrestricted network
- local backend must fail closed for `NetworkAccess::None`, `LoopbackOnly`, and `AllowList` unless OS-level enforcement is available
- Docker `EphemeralPerExec` may enforce network via `docker run --network`
- Docker `CreatePerSession`, `ReusePooled`, and `BringYourOwn` must fail closed when a per-exec network policy differs from the container's configured network
- `NoopSandbox` remains deny-only and must never be a successful execution backend

**Steps:**

- Write lifecycle tests proving `before_execute` is called exactly once by `execute_with_lifecycle`.
- Write capability matrix tests for local and Docker network policy.
- Move `before_execute` invocation into `execute_with_lifecycle`.
- Remove duplicate local call.
- Add preflight report and events.
- Update Docker policy validation.
- Run gates.

**Verification:**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-sandbox --test api_contract
cargo test -p jyowo-harness-sandbox --test local
cargo test -p jyowo-harness-sandbox --test docker
cargo test -p jyowo-harness-sandbox --test noop
cargo test -p jyowo-harness-sandbox --test fingerprint
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit focus:** Sandbox capability reporting matches enforceable behavior; unsupported policy fails closed before process execution.

## Task 5: Refactor Tool API From Permission Checks To Action Plans

**Files:**

- Modify: `crates/jyowo-harness-tool/src/tool.rs`
- Modify: `crates/jyowo-harness-tool/src/context.rs`
- Modify: `crates/jyowo-harness-tool/src/orchestrator.rs`
- Modify: `crates/jyowo-harness-tool/src/lib.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/read.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/write.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/edit.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/list_dir.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/glob.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/grep.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/bash.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/execute_code.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/process_monitor.rs`
- Modify: `crates/jyowo-harness-tool/src/process_registry.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/web_search.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/web_fetch.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/task_stop.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/read_blob.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/clarify.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/diagnostics.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/send_message.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/todo.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/skills.rs`
- Modify: `crates/jyowo-harness-tool/src/registry.rs`
- Modify: `crates/jyowo-harness-tool/tests/api_contract.rs`
- Modify: `crates/jyowo-harness-tool/tests/builtin_io.rs`
- Modify: `crates/jyowo-harness-tool/tests/builtin_exec.rs`
- Modify: `crates/jyowo-harness-tool/tests/execute_code.rs`
- Modify: `crates/jyowo-harness-tool/tests/builtin_process_monitor.rs`
- Modify: `crates/jyowo-harness-tool/tests/orchestrator.rs`
- Modify: `crates/jyowo-harness-tool/tests/permission_fingerprint.rs`
- Modify: every additional file returned by:
  `rg -l "impl Tool for|async fn check_permission\\(" crates apps/desktop/src-tauri -S`
  when replacing the public `Tool` trait. Paste that exact file list into the Task Intent Check before editing. This includes command tools, feature-gated provider tools, plugin proxies, MCP wrappers, subagent tools, SDK test tools, desktop test tools, and all test-only implementors.

**Design:**

Replace tool permission API:

```rust
async fn check_permission(&self, input: &Value, ctx: &ToolContext) -> PermissionCheck;
```

with action planning:

```rust
async fn plan(&self, input: &Value, ctx: &ToolContext) -> Result<ToolActionPlan, ToolError>;
async fn execute_authorized(
    &self,
    authorized: AuthorizedToolInput,
    ctx: ToolContext,
) -> Result<ToolStream, ToolError>;
```

`AuthorizedToolInput` contains:

- original raw input
- validated and canonicalized action plan
- consumed authorization ticket summary

File tools must execute from canonical paths in `ToolActionPlan`, not from raw input re-resolution.

Network tools must include network host/resource in `ToolActionPlan`.

`jyowo-harness-tool` must not call `PermissionBroker::decide`.

`ToolOrchestrator` in this crate should either be removed or reduced to planning utilities. Cross-domain authorization orchestration belongs to `jyowo-harness-execution`.

Because this task changes the public `Tool` trait, it must leave zero implementors on `check_permission` or raw `execute`. Do not add default trait methods, compatibility blanket impls, or transitional adapters that preserve old semantics. If the migration scan returns a file not listed above, that file is in scope for this task and must be included in the task commit.

**Steps:**

- Write failing tests proving file read/write cannot execute without authorized input.
- Write tests proving planned path hash mismatch fails.
- Write failing tests proving command-backed tools cannot execute through old raw `execute` paths after the trait change.
- Run the `rg -l "impl Tool for|async fn check_permission\\(" crates apps/desktop/src-tauri -S` migration scan and add every returned file to the task-owned file list.
- Change `Tool` trait.
- Migrate every implementor found by the scan in the same task.
- Remove permission decision logic from tool orchestrator or move it to execution crate.
- Run a second `rg -n "check_permission\\(|async fn execute\\(&self, input: Value|PermissionBroker::decide|\\.decide\\(" crates/jyowo-harness-tool crates/jyowo-harness-tool-search crates/jyowo-harness-plugin crates/jyowo-harness-mcp crates/jyowo-harness-subagent crates/jyowo-harness-sdk apps/desktop/src-tauri -S` scan and fail the task if production tool execution still uses the old permission/execute path.
- Run gates.

**Verification:**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-tool --test api_contract
cargo test -p jyowo-harness-tool --test builtin_io
cargo test -p jyowo-harness-tool --test builtin_exec
cargo test -p jyowo-harness-tool --test execute_code
cargo test -p jyowo-harness-tool --test builtin_process_monitor
cargo test -p jyowo-harness-tool --test orchestrator
cargo test -p jyowo-harness-tool --test permission_fingerprint
cargo check -p jyowo-harness-tool --all-features
cargo test -p jyowo-harness-execution --test authorization_flow
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit focus:** No `Tool` implementor can compile or execute through the old permission-check/raw-execute path; file, network, command, MCP, plugin, subagent, provider, and SDK tool paths require authorized input.

## Task 6: Harden Command Execution Through Authorized Sandbox

**Files:**

- Modify: `crates/jyowo-harness-tool/src/builtin/bash.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/execute_code.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/process_monitor.rs`
- Modify: `crates/jyowo-harness-tool/src/process_registry.rs`
- Modify: `crates/jyowo-harness-tool/tests/builtin_exec.rs`
- Modify: `crates/jyowo-harness-tool/tests/execute_code.rs`
- Modify: `crates/jyowo-harness-tool/tests/builtin_process_monitor.rs`
- Modify: `crates/jyowo-harness-execution/src/service.rs`
- Modify: `crates/jyowo-harness-execution/tests/authorization_flow.rs`

**Design:**

Task 5 already migrated command tools to `plan` and `execute_authorized`. This task must not change the public `Tool` trait, add old-method compatibility shims, or leave a command execution path outside authorized input.

Command tools must produce a `ToolActionPlan` containing:

- canonical `ExecSpec`
- canonical fingerprint
- command preview
- severity
- sandbox policy
- workspace access
- network access

Execution crate must:

1. consume authorization ticket
2. run sandbox preflight
3. call sandbox lifecycle execution
4. stream process output through existing redaction and budget paths

Dangerous command detection remains Rust-side and must happen before any user approval request is treated as sufficient.

`BashTool::execute_authorized` must not rebuild a weaker `ExecSpec` from raw input.

**Steps:**

1. Write failing bash test: command cannot execute when ticket plan hash does not match ExecSpec.
2. Write failing bash test: hard-denied dangerous command is denied even under bypass mode.
3. Write failing process monitor test proving process start uses the same ticket and sandbox preflight path.
4. Harden bash authorized execution against ExecSpec rebuild or raw input downgrade.
5. Harden execute-code and process monitor paths against ticket, sandbox policy, and plan hash mismatch.
6. Run gates.

**Verification:**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-tool --test builtin_exec
cargo test -p jyowo-harness-tool --test execute_code
cargo test -p jyowo-harness-tool --test builtin_process_monitor
cargo test -p jyowo-harness-execution --test authorization_flow
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit focus:** Commands are authorized by canonical `ExecSpec` and sandbox preflight, not by raw command text after approval.

## Task 7: Integrate Execution Authority Into Engine

**Files:**

- Modify: `crates/jyowo-harness-engine/Cargo.toml`
- Modify: `crates/jyowo-harness-engine/src/turn.rs`
- Modify: `crates/jyowo-harness-engine/src/engine.rs`
- Modify: `crates/jyowo-harness-engine/src/runner.rs`
- Modify: `crates/jyowo-harness-engine/tests/permission.rs`
- Modify: `crates/jyowo-harness-engine/tests/permission_hooks.rs`
- Modify: `crates/jyowo-harness-engine/tests/contract.rs`
- Modify: `crates/jyowo-harness-engine/tests/main_loop.rs`
- Modify: `crates/jyowo-harness-engine/tests/e2e_engine.rs`

**Design:**

Engine no longer owns permission dedup or final decision logic.

Remove or shrink `RecordingPermissionBroker` so it only records already-authoritative outcomes for event projection. It must not:

- inspect previous decisions to allow/deny
- apply bypass
- apply hard policy
- mint decisions

Engine calls `harness_execution::AuthorizationService`.

Engine must provide:

- tenant id
- session id
- run id
- tool use id
- actor source
- permission mode
- interactivity
- redactor
- event store
- journal-backed `AuthorizationEventSink`
- sandbox backend

If the injected `AuthorizationEventSink` fails to append or redact an event before execution, execution fails closed.

**Steps:**

1. Write failing engine test proving repeated permission request uses execution authority dedup, not engine-local dedup.
2. Write failing engine test proving policy deny survives bypass and MCP/programmatic tool call paths.
3. Integrate `AuthorizationService`.
4. Implement the engine-side journal adapter for `AuthorizationEventSink`.
5. Remove engine-owned decision branches.
6. Preserve event projection shape.
7. Run gates.

**Verification:**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-engine --test permission
cargo test -p jyowo-harness-engine --test permission_hooks
cargo test -p jyowo-harness-engine --test contract
cargo test -p jyowo-harness-engine --test main_loop
cargo test -p jyowo-harness-engine --test e2e_engine
cargo test -p jyowo-harness-execution
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit focus:** Engine delegates authorization to execution crate and does not contain a second permission authority.

## Task 8: Cover MCP, Subagent, Team, Background, And Automation Paths

**Files:**

- Modify: `crates/jyowo-harness-mcp/src/sampling.rs`
- Modify: `crates/jyowo-harness-mcp/src/registry.rs`
- Modify: `crates/jyowo-harness-mcp/src/client.rs`
- Modify: `crates/jyowo-harness-mcp/src/transport.rs`
- Modify: `crates/jyowo-harness-mcp/src/reconnect.rs`
- Modify: `crates/jyowo-harness-mcp/src/server.rs`
- Modify: `crates/jyowo-harness-mcp/src/transports/mod.rs`
- Modify: `crates/jyowo-harness-mcp/src/transports/http.rs`
- Modify: `crates/jyowo-harness-mcp/src/transports/sse.rs`
- Modify: `crates/jyowo-harness-mcp/src/transports/websocket.rs`
- Modify: `crates/jyowo-harness-mcp/src/transports/stdio.rs`
- Modify: `crates/jyowo-harness-mcp/src/transports/in_process.rs`
- Modify: `crates/jyowo-harness-mcp/tests/sampling.rs`
- Modify: `crates/jyowo-harness-mcp/tests/tenant_isolation.rs`
- Modify: `crates/jyowo-harness-mcp/tests/server_protocol.rs`
- Modify: `crates/jyowo-harness-mcp/tests/contract.rs`
- Modify: `crates/jyowo-harness-mcp/tests/http.rs`
- Modify: `crates/jyowo-harness-mcp/tests/sse.rs`
- Modify: `crates/jyowo-harness-mcp/tests/websocket.rs`
- Modify: `crates/jyowo-harness-mcp/tests/stdio.rs`
- Modify: `crates/jyowo-harness-mcp/tests/in_process.rs`
- Modify: `crates/jyowo-harness-subagent/src/lib.rs`
- Modify: `crates/jyowo-harness-subagent/tests/permission_bridge.rs`
- Modify: `crates/jyowo-harness-team/src/lib.rs`
- Modify: `crates/jyowo-harness-team/tests/contract.rs`
- Modify: `crates/jyowo-harness-team/tests/team_e2e.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/background.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/policy.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/subagents.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/teams.rs`
- Modify: `crates/jyowo-harness-agent-runtime/tests/agent_orchestration_background_permission.rs`
- Modify: `crates/jyowo-harness-agent-runtime/tests/agent_orchestration_policy.rs`
- Modify: `crates/jyowo-harness-agent-runtime/tests/agents_team.rs`
- Modify: `apps/desktop/src-tauri/src/agent_supervisor.rs`
- Modify: `apps/desktop/src-tauri/src/commands/automations.rs`
- Modify: `apps/desktop/src-tauri/tests/agent_orchestration_e2e.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/background_agents.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/automations.rs`

**Design:**

Every non-foreground actor must carry authoritative actor source through authorization:

- foreground run: Rust `PermissionActorSource::ParentRun`, frontend `actorSource.type = "parentRun"`
- subagent: Rust `PermissionActorSource::Subagent`, frontend `actorSource.type = "subagent"`
- team member: Rust `PermissionActorSource::TeamMember`, frontend `actorSource.type = "teamMember"`
- background agent: Rust `PermissionActorSource::BackgroundAgent`, frontend `actorSource.type = "backgroundAgent"`
- automation: Rust `PermissionActorSource::Automation`, frontend `actorSource.type = "automation"`
- MCP server: Rust `PermissionActorSource::McpServer`, frontend `actorSource.type = "mcpServer"`

MCP sampling and MCP tools must not construct empty policy snapshots or bypass authorization. They must request authorization through the same execution authority.

MCP coverage is not limited to `tools/call` and `sampling/createMessage`. These MCP operations are in scope and must either route through execution authority or fail closed when authority, tenant/session context, actor source, server origin, or workspace scope is unavailable:

- outbound `tools/call`
- outbound `sampling/createMessage`
- outbound `resources/list`
- outbound `resources/read`
- outbound `resources/subscribe`
- outbound `resources/unsubscribe`
- outbound `prompts/list`
- outbound `prompts/get`
- transport-backed connect/request paths for HTTP, SSE, WebSocket, stdio, and in-process transports
- inbound MCP server adapter methods that proxy to Jyowo tools, resources, or prompts

`resources/list` and `prompts/list` may be classified as low-risk metadata only by the Rust authority. They must still create an action plan with `PermissionActorSource::McpServer`, server origin, and scope. A raw list call without authority context is not allowed.

`McpConnectContext` must carry the authority context needed to build action plans for transport, resource, prompt, tool, and sampling requests. A default context may exist only for tests that assert fail-closed behavior; it must not silently allow external network, process, resource, prompt, tool, or sampling calls.

Background supervisor recovery must revalidate persisted payloads and permission state before running. It must not replay unsafe child actions from a previous approval.

Automations must fail closed when sandbox mode, permission mode, model config, or workspace scope cannot be validated.

**Steps:**

1. Write failing MCP sampling test for policy deny under bypass.
2. Write failing subagent permission bridge test for actor source preservation and deny behavior.
3. Write failing automation test proving automation-originated permissions carry `PermissionActorSource::Automation`.
4. Write failing MCP tool or sampling test proving MCP-originated permissions carry `PermissionActorSource::McpServer` with origin and scope.
5. Write failing MCP resource tests for `resources/list` and `resources/read` proving policy deny survives bypass and missing authority context fails closed.
6. Write failing MCP prompt tests for `prompts/list` and `prompts/get` proving policy deny survives bypass and missing authority context fails closed.
7. Write failing transport tests proving HTTP, SSE, WebSocket, stdio, and in-process connect/request paths fail closed without authority context and preserve server origin when authorized.
8. Write failing inbound server adapter test proving proxied resources, prompts, and tools cannot bypass authority.
9. Write failing background recovery test for stale permission denial.
10. Route MCP and agent-originated permission requests through execution authority.
11. Update automation validation.
12. Run gates.

**Verification:**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-mcp --test sampling
cargo test -p jyowo-harness-mcp --test tenant_isolation
cargo test -p jyowo-harness-mcp --test server_protocol
cargo test -p jyowo-harness-mcp --test contract
cargo test -p jyowo-harness-mcp --test http
cargo test -p jyowo-harness-mcp --test sse
cargo test -p jyowo-harness-mcp --test websocket
cargo test -p jyowo-harness-mcp --test stdio
cargo test -p jyowo-harness-mcp --test in_process
cargo test -p jyowo-harness-subagent --test permission_bridge
cargo test -p jyowo-harness-team --test contract
cargo test -p jyowo-harness-team --test team_e2e
cargo test -p jyowo-harness-agent-runtime --features agents-subagent,agents-team --test agent_orchestration_background_permission
cargo test -p jyowo-harness-agent-runtime --features agents-subagent,agents-team --test agent_orchestration_policy
cargo test -p jyowo-harness-agent-runtime --features agents-subagent,agents-team --test agents_team
cargo test -p jyowo-desktop-shell --test agent_orchestration_e2e
cargo test -p jyowo-desktop-shell --test commands background_agents
cargo test -p jyowo-desktop-shell --test commands automations
cargo test -p jyowo-desktop-shell --test commands agent_run_policy
cargo check -p jyowo-harness-mcp --all-features
cargo check --workspace
pnpm check:test-architecture
pnpm check:agent-orchestration-no-fakes
pnpm check:agent-supervisor-sidecar
git diff --check
```

**Audit focus:** No actor path can bypass the same authorization pipeline; MCP tools, sampling, resources, prompts, and transport-backed requests are authorized or fail closed; actor source attribution is preserved and redacted.

## Task 9: Wire SDK And Desktop Runtime To Production Authority

**Files:**

- Modify: `crates/jyowo-harness-sdk/Cargo.toml`
- Modify: `crates/jyowo-harness-sdk/src/builder.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/permissions.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- Modify: `crates/jyowo-harness-sdk/tests/facade.rs`
- Modify: `crates/jyowo-harness-sdk/tests/runtime_assembly_tools.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/permissions.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/runs.rs`

**Design:**

SDK builder exposes production authority wiring:

```rust
with_permission_authority(...)
with_authorization_service(...)
```

The old `with_permission_broker` can remain only as a low-level test adapter if all production assembly uses `with_permission_authority`. If it remains public, docs and tests must prove it cannot be used by desktop production runtime to bypass policy.

Desktop production features must enable:

- `stream-permission`
- `rule-engine-permission`
- `integrity`

Desktop runtime must assemble:

- real rule providers
- signed file decision persistence
- stream interactive resolver
- permission authority
- execution authorization service
- local sandbox with explicit platform isolation decision

If platform isolation is unavailable, desktop must surface capability unavailable and fail closed for policies requiring network/file isolation.

`resolve_permission` remains pending request resolution. It must:

- validate conversation id
- validate request id
- validate pending request belongs to session
- validate subscribed window
- validate typed confirmation text when the pending backend-authored review requires it
- call resolver only
- never create a decision without pending request
- never grant a new scope

**Steps:**

1. Write failing desktop test proving production runtime does not use stream broker alone.
2. Write failing desktop test proving stale/non-pending request cannot resolve.
3. Write failing SDK assembly test proving rule provider policy deny works through full runtime.
4. Wire SDK and desktop runtime.
5. Update features.
6. Run gates.

**Verification:**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-sdk --test facade
cargo test -p jyowo-harness-sdk --test runtime_assembly_tools
cargo test -p jyowo-desktop-shell --test commands permissions
cargo test -p jyowo-desktop-shell --test commands runs
cargo test -p jyowo-desktop-shell --test commands execution_settings
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit focus:** Desktop production runtime uses the full authority stack with signed persistence and cannot resolve arbitrary permissions from IPC.

## Task 10: Update Frontend Permission UX And IPC Schemas

**Files:**

- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/permissions.rs`
- Modify: `apps/desktop/src/shared/events/run-event-schema.ts`
- Modify: `apps/desktop/src/shared/events/run-event-schema.test.ts`
- Modify: `apps/desktop/src/features/conversation/timeline/permission-inline-panel.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/tool-attempt-row.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.permission.test.tsx`
- Modify: `apps/desktop/src/features/activity/PermissionDialog.tsx`
- Modify: `apps/desktop/src/features/activity/RunEventDetails.tsx`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`
- Modify: `apps/desktop/src/features/conversation/Composer.tsx`
- Modify: `apps/desktop/src/features/conversation/Composer.test.tsx`
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`

**Design:**

Frontend displays backend-authored permission review:

- summary
- risk/severity
- scope
- sandbox policy summary
- effective permission mode
- auto-resolved state
- required confirmation

Frontend must not infer:

- whether action is safe
- whether sandbox can enforce
- whether bypass applies
- whether a network/file scope is allowed

Approval payload remains narrow:

```ts
{
  conversationId: string
  requestId: string
  decision: 'approve' | 'deny'
  confirmationText?: string
}
```

Rust revalidates confirmation text. UI disables approve when required confirmation text is missing, but that is not a security boundary.

Backend IPC requirements:

- `ResolvePermissionRequest` in `apps/desktop/src-tauri/src/commands/contracts.rs` must add `confirmation_text: Option<String>` with camelCase serde as `confirmationText`.
- `resolve_permission_with_runtime_state` must validate `confirmationText` against the pending request's backend-authored `PermissionReview.confirmation`.
- Missing or mismatched confirmation text must return invalid payload before calling the resolver.
- Deny decisions must not require confirmation text.
- Approval without a pending request must still fail before confirmation validation can create any decision.
- Confirmation text must never be logged, journaled, or persisted as raw sensitive data.

UX:

- low/medium risk can use inline panel
- high/critical uses `PermissionDialog`
- destructive approve button has no default focus
- status uses icon plus text, never color only
- raw JSON remains drill-down only
- no secrets in UI state

**Steps:**

1. Write failing Rust IPC tests for missing, mismatched, and correct `confirmationText` on approve, plus deny without confirmation.
2. Write failing Zod tests for new payloads and unknown fields.
3. Write failing component tests for high/critical confirmation.
4. Update Rust `ResolvePermissionRequest` and confirmation validation.
5. Update frontend schemas.
6. Update UI components.
7. Update i18n keys.
8. Run gates.

**Verification:**

```bash
cargo fmt --all --check
pnpm -C apps/desktop test -- commands run-event-schema ConversationWorkspace Composer conversation-timeline.permission
cargo test -p jyowo-desktop-shell --test commands permissions
cargo check --workspace
pnpm -C apps/desktop typecheck
pnpm -C apps/desktop lint
pnpm check:test-architecture
git diff --check
```

**Audit focus:** React displays backend policy and submits user intent only; it does not make allow/deny decisions.

## Task 11: Update Docs And Anti-Fake Gates

**Files:**

- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`
- Modify: `docs/frontend/frontend-engineering.md`
- Modify: `docs/frontend/frontend-quality.md`
- Modify: `docs/testing/testing-strategy.md`
- Modify: `scripts/check-backend-docs.mjs`
- Modify: `scripts/backend-docs-policy.test.mjs`
- Modify: `scripts/check-agent-orchestration-no-fakes.mjs`
- Modify: `scripts/check-agent-orchestration-no-fakes.test.mjs`
- Modify: `scripts/check-test-architecture.mjs`
- Modify: `package.json` only if a new gate command is required

**Design:**

Docs must state:

- execution authority crate exists at L3
- tool execution requires authorization ticket
- `PermissionContext` does not carry rule snapshots as authority
- permission authority owns hard policy, dedup, persistence, and interactive resolution
- sandbox lifecycle preflight is mandatory
- local no-isolation mode is not OS sandbox enforcement
- frontend permission UI is display-only
- subagent audit is required for each task touching authorization/sandbox

Anti-fake scanner must catch production paths near authorization/sandbox/permission context containing:

- mock
- fake
- noop success
- placeholder
- TODO near production authorization
- allow all
- bypass policy
- unimplemented permission

The scanner must exclude tests and docs where appropriate, but it must scan production Rust and frontend IPC surfaces.

**Steps:**

1. Write failing docs/scanner tests for prohibited production markers.
2. Update backend and frontend docs.
3. Update scripts.
4. Run docs and scanner gates.

**Verification:**

```bash
pnpm check:docs
pnpm check:test-architecture
pnpm check:agent-orchestration-no-fakes
git diff --check
```

**Audit focus:** Documentation matches implementation and gates prevent reintroducing fake authorization/sandbox paths.

## Task 12: Full Integration Gate And Security Review

**Files:**

- No planned source edits.
- If failures require fixes, edit only the owning files and rerun the relevant task audit.

**Required Review:**

Run a read-only security review subagent with GPT-5.5 Pro before final completion. If the tool requires API-style overrides, use `model: gpt-5.5`, `reasoning_effort: xhigh`, and `service_tier: priority`.

Security review prompt:

```text
Read-only security review for the completed authorization, permission, and sandbox refactor.
Use GPT-5.5 Pro. If the tool requires API-style overrides, use `model: gpt-5.5`, `reasoning_effort: xhigh`, and `service_tier: priority`.
Do not edit files.

Verify:
- Rust remains final policy authority
- PermissionBroker / PermissionAuthority cannot be bypassed by tools, filesystem, network, sandbox, MCP, subagent, team, background agent, automation, or Tauri commands
- BypassPermissions and DontAsk skip only user interaction
- unsupported sandbox capability fails closed
- authorization tickets are single-use and never cross IPC
- event ordering reuses existing ToolUse events and is ToolUseRequested -> PermissionRequested -> PermissionResolved -> ticket/preflight -> ToolUseCompleted or ToolUseFailed
- Redactor runs before Journal, Replay, logs, traces, export, and UI-visible raw payloads
- no secrets can enter prompt, event, log, trace, screenshot, frontend state, fixture, or snapshot
- no production mock/fake/noop/placeholder runtime path exists
- docs and gates match implementation

Return PASS or FAIL with file and line evidence.
```

**Full Gates:**

```bash
cargo fmt --all --check
pnpm check:test-architecture
pnpm check:quick
pnpm check:desktop
pnpm check:rust
pnpm check
git diff --check
```

If `pnpm check` fails because of a known external environment dependency, capture:

- command
- exit code
- exact failing output summary
- why the failure is external
- narrower gates that passed

Do not claim completion without a passing security review subagent.

## Final Acceptance Criteria

The refactor is complete only when all are true:

- `crates/jyowo-harness-execution` is the only cross-domain authorization/execution orchestrator.
- Tool code plans actions and executes only with authorized input.
- Engine no longer owns duplicate permission dedup or bypass decision logic.
- `PermissionContext` no longer carries caller-owned rule snapshots as hard policy authority.
- Full production permission authority includes hard policy, rules, dedup/history, interaction, signed persistence, and audit metadata.
- Desktop production runtime wires full authority, not stream broker alone.
- `resolve_permission` only resolves a pending request and cannot grant a new scope.
- Sandbox lifecycle calls `before_execute` through `execute_with_lifecycle` exactly once.
- Sandbox capability report matches actual enforcement.
- Local no-isolation mode is not represented as OS sandbox enforcement.
- Docker network policy is enforced or fails closed.
- MCP tools, sampling, resources, prompts, transport-backed requests, subagent, team, background, and automation paths use the same authority.
- Frontend Zod schemas parse backend payloads and reject unknown fields.
- Frontend permission UI submits user intent only.
- No production mock data, fake implementation, noop success, placeholder runtime path, or UI-only policy remains.
- Required docs and gates are updated.
- Every task has a passing read-only subagent audit.
- Final security review subagent returns `PASS`.
- `pnpm check` passes.
