# Desktop Default Sandbox Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Every task requires a pre-task implementation analysis, a pre-audit completion analysis, and a read-only subagent audit before the task can be marked complete.
>
> **Required model profile:** use ChatGPT 5.5 with `xhigh` reasoning for implementation and audit subagents. If the tool requires API-style overrides, use `model: gpt-5.5`, `reasoning_effort: xhigh`, and the highest available service tier. Do not silently downgrade.

**Goal:** Make the desktop runtime execute common built-in tools by default without user-side sandbox configuration, while preserving fail-closed policy enforcement and avoiding fake or UI-only security.

**Architecture:** Split enforcement by execution channel. Process tools run through a routing sandbox backend that selects an enforceable local OS or container backend. HTTP/provider tools run through an authorized network broker that validates the approved action plan host rules before issuing requests. Rust remains the policy authority; React only displays backend-authored capability state and errors.

**Tech Stack:** Rust 1.96, Tauri 2, React 19, TypeScript 6, Zod, reqwest, Tokio, schemars JsonSchema, cargo test, Vitest, Playwright/Storybook where UI is changed, pnpm 11.7, existing Jyowo docs gates.

---

## Branch And Worktree Rules

This plan file must be tracked on `main` before implementation starts. Implementation must not run in the original `main` checkout.

Use an isolated worktree and branch prefix `goya`.

```bash
SOURCE_CHECKOUT="$(pwd)"
PLAN_PATH="docs/plans/2026-07-06-desktop-default-sandbox-runtime-implementation.md"
test "$(git branch --show-current)" = "main"
git show main:"$PLAN_PATH" >/dev/null
git status --short

git worktree add -b goya/desktop-default-sandbox-runtime ../Jyowo-desktop-default-sandbox-runtime main
cd ../Jyowo-desktop-default-sandbox-runtime
test "$(git branch --show-current)" = "goya/desktop-default-sandbox-runtime"
test -f "$PLAN_PATH"
git status --short --branch
```

Expected:

- source checkout branch is `main`
- `git show main:$PLAN_PATH` exits 0
- implementation branch is `goya/desktop-default-sandbox-runtime`
- implementation work happens only inside `../Jyowo-desktop-default-sandbox-runtime`
- if the branch or worktree already exists, stop and ask for a new branch name

Do not stash, revert, or overwrite unrelated user changes. Stage exact files only. Never stage broad directories such as `crates`, `apps`, or `docs`.

## Mandatory Execution Protocol

Every task must follow this order.

1. **Task Intent Check**
   - Restate the task objective.
   - List exact in-scope files.
   - List exact out-of-scope files.
   - State the invariants that must remain true.
   - State the tests and gates for this task.
   - State why this task does not add mock data, fake runtime paths, noop success, placeholder behavior, or UI-only policy.

2. **Read Required Context**
   - Read root `AGENTS.md`.
   - Read `docs/testing/testing-strategy.md`.
   - After Task 0.5 is complete, read `docs/architecture/harness/crates/harness-sandbox.md`.
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
   - Read every file listed by the task before editing it.

3. **Write Failing Tests First**
   - Add or update tests in the owning crate or frontend boundary.
   - Run the narrow test and confirm it fails for the intended reason.
   - If a failing test cannot be written first, explain why in the task response and add the nearest executable contract test before implementation.

4. **Implement**
   - Make task-scoped changes only.
   - Destructive refactor is allowed when it removes a broken design or prevents technical debt.
   - Do not keep old compatibility branches unless this plan explicitly requires a migration window.

5. **Local Gate**
   - Run the task-specific commands.
   - If Rust files changed, run `cargo fmt --all --check`.
   - If public Rust API, serde contract, JsonSchema, workspace members, feature flags, Tauri IPC payloads, `ToolActionPlan`, `Tool` execution, or sandbox traits changed, run `cargo check --workspace`.
   - If frontend files changed, run the relevant narrow Vitest command and `pnpm check:desktop`.
   - If docs changed, run `pnpm check:docs`.
   - If tests were added, moved, renamed, split, or deleted, run `pnpm check:test-architecture`.
   - Run `git diff --check`.
   - Inspect `git diff --name-only`.

6. **Task Completion Analysis**
   - Before audit, write a short completion analysis in the task response:
     - implemented objective
     - changed ownership or state flow
     - evidence that no production mock, fake, noop success, placeholder, or UI-only policy was introduced
     - commands run with exit codes
     - known residual risk, if any

7. **Subagent Audit**
   - Spawn a read-only subagent using ChatGPT 5.5 with `xhigh` reasoning.
   - The audit must return `PASS` or `FAIL`.
   - `FAIL` must include file and line evidence.
   - Fix failures and rerun the same audit before moving on.
   - If multi-agent tools are unavailable, stop. Do not self-certify.

8. **Security Audit**
   - Required for every task that touches permission, sandbox, network, provider credentials, user input, IPC, secrets, logs, traces, Journal, Replay, or frontend state that displays security-relevant status.
   - Use a separate read-only subagent with the same model profile.

9. **Commit**
   - Commit each task separately from the isolated worktree.
   - Commit message format: `refactor: <task subject>` or `test: <task subject>` for pure test tasks.
   - Do not commit unrelated files.

### Subagent Audit Prompt Template

```text
Read-only audit for Task N of docs/plans/2026-07-06-desktop-default-sandbox-runtime-implementation.md.

Use ChatGPT 5.5 with xhigh reasoning. If the tool requires API-style overrides, use `model: gpt-5.5`, `reasoning_effort: xhigh`, and the highest available service tier.
Do not edit files.

Task objective:
<copy the Task Intent Check>

Completion analysis:
<copy the Task Completion Analysis>

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
- Rust remains the policy authority
- React only displays backend-authored status and never decides execution policy
- BypassPermissions and DontAsk skip only interactive waiting, not sandbox, hard policy, tenant/workspace scope, network enforcement, redaction, or ticket validation
- a sandbox backend does not report a capability it cannot enforce
- process tools do not bypass SandboxBackend lifecycle
- HTTP/provider tools do not issue raw reqwest calls outside the authorized network broker
- no production mock data, fake runtime path, noop success, placeholder behavior, compatibility shim, or UI-only policy was introduced
- tests cover the changed boundary
- command evidence includes every required gate with exit code 0
- changed files are limited to task-owned files

Return PASS or FAIL.
For FAIL, include exact file and line evidence.
```

### Security Audit Prompt Template

```text
Security audit for Task N of docs/plans/2026-07-06-desktop-default-sandbox-runtime-implementation.md.

Use ChatGPT 5.5 with xhigh reasoning. Do not edit files.

Focus:
- no secret reaches prompt, event, log, trace, screenshot, frontend state, fixture, or snapshot
- no tool can execute command, filesystem, network, MCP, or outbound message work without an authorization ticket
- process sandbox and HTTP broker fail closed when policy cannot be enforced
- capability status cannot be forged by frontend state
- BypassPermissions and DontAsk do not bypass non-interactive policy checks
- local no-isolation mode is never used for a restricted policy
- test-only servers or deterministic adapters do not become production fallbacks

Return PASS or FAIL with file and line evidence.
```

## Forbidden Throughout

- Production mock data, fake runtime paths, noop success, placeholder behavior, or UI-only policy.
- A production sandbox backend that reports support for policy it cannot enforce.
- A fallback from restricted policy to `LocalIsolation::None`.
- A network-only tool that passes sandbox preflight but then performs raw `reqwest` outside the authorized network broker.
- A Tauri command that grants, upgrades, or forges tool permission.
- A frontend flag that marks a tool executable without backend capability status.
- A compatibility wrapper that silently preserves old `supports_network: bool` behavior.
- Docker `supports_network: true` as a substitute for per-policy support.
- `BypassPermissions` or `DontAsk` bypassing hard policy, sandbox capability, network broker validation, ticket validation, tenant/workspace scope, redaction, or event ordering.
- Raw secrets in prompt, events, logs, traces, screenshots, frontend state, fixtures, snapshots, or exported files.

Allowed in tests:

- `tempfile` workspaces.
- Local loopback HTTP servers that exercise the production broker and transport code path.
- Existing in-memory stores where the owning crate already uses them for deterministic tests.
- Existing `NoopSandbox` only for deny/fail-closed tests; never as a successful execution backend.

## Current Code Facts

Use these facts as the baseline. Do not invent a different starting state.

- `SandboxBackend` is defined in `crates/jyowo-harness-sandbox/src/backend.rs`.
- `SandboxCapabilities` currently contains coarse booleans such as `supports_network` and `supports_filesystem_write`.
- `validate_preflight_capabilities` rejects `NetworkAccess::None` when the backend cannot enforce no-network, and rejects fine-grained network policies.
- `LocalSandbox::new` defaults to `LocalIsolation::None`.
- Desktop main runtime currently creates `LocalSandbox::new(workspace_root)` in `apps/desktop/src-tauri/src/commands/runtime.rs`.
- Desktop plugin sidecar sandbox already uses `LocalIsolation::for_current_platform()` in `desktop_plugin_sidecar_sandbox`.
- `LocalSandbox` supports OS-level wrappers for `Bubblewrap`, `Seatbelt`, and `JobObject`, but allowlist network policy is not implemented.
- `DockerSandbox` maps only `NetworkAccess::None` to Docker `--network none` and `NetworkAccess::Unrestricted` to `bridge`.
- `SshSandbox` only accepts unrestricted network policy.
- `AuthorizationService` in `crates/jyowo-harness-execution/src/service.rs` runs permission resolution before sandbox preflight.
- `preflight_spec_for_plan` currently turns network-only action plans into an `ExecSpec`, which makes provider/API tools depend on process sandbox network support.
- `BashTool` builds `Command` resources and executes through `execute_with_lifecycle`.
- `DiagnosticsTool` uses `DiagnosticsRunnerCap`; desktop diagnostics runner executes `cargo` or `pnpm` through `execute_with_lifecycle`.
- MiniMax and Seedance provider tools declare `ActionResource::Network` with `NetworkAccess::AllowList`.
- MiniMax HTTP client currently owns a direct `reqwest::Client` in `crates/jyowo-harness-tool/src/provider_minimax.rs`.
- Seedance uses `SeedanceApiClient` from `jyowo-harness-model`.
- `WebFetchTool` and `WebSearchTool` use backend traits and declare network action plans.
- `SendMessageTool` declares network-shaped permission but executes through `UserMessengerCap`, not a process sandbox.
- `ToolContext` carries `sandbox: Option<Arc<dyn SandboxBackend>>` and a `CapabilityRegistry`.
- Final safety decisions belong in Rust per `docs/backend/backend-runtime.md`.

## Target Design

### Execution Channels

Every tool action must be classified by the component that can enforce it.

```text
ProcessSandbox
  command execution, diagnostics, shell-like tools

HttpBroker
  provider service tools, web fetch, web search backends that issue HTTP

ExternalCapability
  outbound user message and other backend-owned capabilities that are not process or HTTP execution

DirectAuthorizedRust
  file read/write/edit/list/grep and pure in-process operations that already execute inside authorized Rust code
```

The plan must not route all `ActionResource::Network` values through process sandbox preflight. Network-only API tools must be authorized by `AuthorizationService` and executed by an HTTP broker that validates the approved `NetworkAccess` host rules.

### Sandbox Capabilities

Replace coarse capability booleans with explicit policy support.

Required internal shape in `jyowo-harness-sandbox`:

```rust
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct NetworkPolicySupport {
    pub none: bool,
    pub loopback_only: bool,
    pub allowlist: bool,
    pub unrestricted: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct WorkspacePolicySupport {
    pub read_write_all: bool,
    pub read_only: bool,
    pub writable_subpaths: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct SandboxCapabilities {
    pub supports_streaming: bool,
    pub supports_stdin: bool,
    pub supports_cwd_tracking: bool,
    pub cwd_marker_support: CwdMarkerSupport,
    pub supports_activity_heartbeat: bool,
    pub supports_interactive_shell: bool,
    pub network: NetworkPolicySupport,
    pub workspace: WorkspacePolicySupport,
    pub supports_gpu: bool,
    pub supports_pty: bool,
    pub supports_detach: bool,
    pub supports_workspace_sync: bool,
    pub supports_session_snapshot: bool,
    pub max_concurrent_execs: u32,
    pub supports_kill_scope: Vec<KillScope>,
    pub snapshot_kinds: BTreeSet<SessionSnapshotKind>,
    pub resource_limit_support: ResourceLimitSupport,
    pub default_timeout: Duration,
}
```

If implementation needs additional fields, add them only when backed by tests.

Expected backend reports:

| Backend | none | loopback | allowlist | unrestricted |
|---|---:|---:|---:|---:|
| `LocalIsolation::None` | false | false | false | true |
| `LocalIsolation::Bubblewrap` | true | false until implemented | false | true |
| `LocalIsolation::Seatbelt` | true | false until implemented | false | true |
| `LocalIsolation::JobObject` | false until implemented | false | false | true |
| `DockerSandbox` ephemeral | true | false until implemented | false | true |
| `SshSandbox` | false | false | false | true |

Do not mark `allowlist: true` until host and port allowlist enforcement exists.

### Process Sandbox Routing

Add a routing backend that implements `SandboxBackend` and delegates each `ExecSpec` to a child backend that can enforce the requested policy.

Required selection behavior:

```text
if spec.policy.network == None:
  prefer OS-level LocalSandbox when available
  else use DockerSandbox if available
  else fail closed with a user-facing capability reason

if spec.policy.network == Unrestricted:
  use OS-level LocalSandbox when available
  else use DockerSandbox if available
  else use LocalIsolation::None only if workspace policy is not restricted

if spec.policy.network == LoopbackOnly or AllowList:
  fail closed for process tools until a backend explicitly implements that policy
```

Docker fallback is usable only when the desktop factory builds it with the active workspace mounted. The plan must not rely on `DockerSandbox::builder()` defaults, because the current default has no mounted workspace.

Required Docker desktop fallback configuration:

```text
host workspace root -> /workspace inside the container
VolumeMount::workspace(host_workspace_root, "/workspace") only for read_write_all workspace policy
ExecSpec.cwd host paths under host workspace root rewritten to /workspace-relative container paths
default workdir /workspace when ExecSpec.cwd is absent
read-only and writable-subpath workspace policies are not supported by Docker fallback until Docker-specific read-only/subpath mount enforcement is implemented and tested
same non-secret environment passthrough rules as LocalSandbox
user mapping set to the current uid/gid on Unix when Docker accepts it; otherwise document why ownership remains correct
image availability checked before reporting Docker as available
missing Docker binary, daemon, image, or workspace mount support returns a backend-authored unavailable reason
```

The fallback image must be treated as a real runtime dependency. Do not report Docker as available when `jyowo-workspace:latest` or the configured image cannot execute a trivial command in the mounted `/workspace`.

The router must call the selected backend lifecycle exactly once:

```text
preflight_execute -> before_execute -> execute -> wait -> after_execute
```

Do not call `before_execute` inside child backend `execute`.

Existing `execute_with_lifecycle` calls `before_execute` and `execute` as separate trait methods on the router. Therefore the router must bind one selected child backend to one execution before child `before_execute` runs.

Required implementation:

```text
1. Add an opaque per-execution id to ExecContext inside jyowo-harness-sandbox.
2. Generate that id at the start of execute_with_lifecycle before preflight.
3. RoutingSandboxBackend::before_execute selects the child exactly once, calls that child's before_execute, and stores a RoutingSelectionLease keyed by the execution id.
4. RoutingSandboxBackend::execute removes that lease and executes the same child. If no lease exists, fail closed because the lifecycle was bypassed.
5. RoutingActivityHandle owns the selected child backend and calls that child's after_execute after wait.
6. Cleanup the lease on child before_execute failure, child execute failure, wait completion, kill, and dropped handle paths where the implementation can observe them.
```

Do not rerun the selector in `execute` after a child `before_execute` has already succeeded. Do not use a single router-wide selected backend slot.

Because the outer lifecycle still calls `router.after_execute`, router-level `after_execute` must not call a child backend. Child `after_execute` belongs only to the routing activity wrapper. Router-level `after_execute` may emit router telemetry or no-op.

### HTTP Broker

Add an authorized HTTP broker for tool-originated service calls.

Required behavior:

- accepts an opaque authorization permit derived from `AuthorizedToolInput`
- rejects requests whose URL host or port does not match the approved `NetworkAccess::AllowList`
- rejects public raw IP, username/password URL authority, non-http(s) schemes, invalid host, and redirect to a host outside the allowlist
- allows loopback IP literals such as `127.0.0.1` and `::1` only when the exact host and port are explicitly present in the approved allowlist; this exists for local integration tests and local dev services, not as a public-IP bypass
- blocks redirects by default unless the broker validates each redirect target against the same allowlist
- applies timeout and response byte limits
- redacts secrets from errors before returning `ToolError`
- records auditable non-secret request metadata through existing event paths if the owning event shape supports it; do not add raw request/response body events unless redacted and explicitly required

Production network tools must use this broker instead of constructing their own unrestricted `reqwest::Client`.

### Desktop Capability Status

Desktop startup must compute backend-authored capability status.

Required status:

```text
processSandbox:
  selected backend id
  available network policies
  available workspace policies
  unavailable reasons

httpBroker:
  available
  denied reasons

tools:
  Bash, Diagnostics, WebFetch, WebSearch, MiniMax*, Seedance*, SendMessage
  available/unavailable
  backend-authored reason
```

React may render this status. React must not decide availability.

### Product Acceptance

On a supported macOS or Linux desktop with platform sandbox primitives available:

- `Bash` command `pwd && ls -la` succeeds after approval.
- `Diagnostics` succeeds or returns compiler diagnostics, not sandbox capability mismatch.
- MiniMax image generation reaches the authorized HTTP broker and no longer fails process sandbox preflight for allowlist network.
- `WebFetch` fetches approved HTTP(S) URLs through the broker.
- If a requested policy cannot be enforced, the UI shows a backend-authored reason before or at approval time. It must not show a generic failure only after approval.

## Task 0: Worktree, Baseline, And Plan Presence

**Files:**

- Read: `AGENTS.md`
- Read: `docs/plans/2026-07-06-desktop-default-sandbox-runtime-implementation.md`
- No source changes

**Steps:**

- [ ] Run the branch/worktree commands from "Branch And Worktree Rules".
- [ ] Read all mandatory docs listed in "Mandatory Execution Protocol".
- [ ] Run:

```bash
pnpm check:docs
cargo fmt --all --check
cargo check --workspace
git diff --check
```

- [ ] Record baseline failures, if any. If a baseline gate fails before any implementation, stop and ask whether to repair baseline first.
- [ ] Run the read-only subagent audit for Task 0.
- [ ] Do not commit if no files changed.

**Expected:** implementation worktree exists, plan is present from `main`, and baseline status is known.

## Task 0.5: Add Architecture Docs Before Code Changes

**Goal:** Lock the normative backend design before implementation so later tasks do not invent policy, lifecycle, or UI authority rules.

**Files:**

- Create: `docs/architecture/harness/crates/harness-sandbox.md`
- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`
- Modify: docs gate scripts only if existing gates do not validate the new architecture doc reference
- Test: docs gate scripts if changed

**Docs requirements:**

Document these rules as product architecture, not temporary implementation notes:

- execution channels: `ProcessSandbox`, `HttpBroker`, `ExternalCapability`, `DirectAuthorizedRust`
- routing process sandbox selection order and fail-closed behavior
- Docker fallback workspace mount contract: host workspace root mounted at `/workspace` only for `read_write_all`, cwd rewrite, image availability, and unavailable reasons
- router lifecycle: per-execution selected backend lease, no selector rerun after child `before_execute`, no global selected-backend slot, no duplicate child lifecycle calls
- authorized HTTP broker: allowlist-only v1, loopback IP literal exception only when explicitly allowlisted, public raw IP denial
- broker permit claims: session, run, tool use, tool name, approved host rules, action plan hash
- broker runtime assembly: preflight registry and execution capability use the same broker instance
- exact meaning of `BypassPermissions` and `DontAsk`
- local no-isolation mode not being enforcement
- policy-specific capability reporting
- frontend status as display-only
- no production mocks or fake success paths

**Tests first:**

- [ ] If a docs gate script needs an update, add a failing test proving `harness-sandbox.md` is required.
- [ ] Run the docs gate and confirm the intended failure before implementation if applicable.

**Implementation:**

- [ ] Add or update docs before any Rust or frontend code task starts.
- [ ] Add docs gate coverage if the existing gate does not enforce the new architecture doc reference.
- [ ] Re-read the new architecture doc during every later task intent check.

**Gates:**

```bash
pnpm check:backend-docs
pnpm check:agent-docs
pnpm check:docs
git diff --check
```

**Audit:** regular subagent audit required. Security audit required because this task documents security policy.

## Task 1: Add Explicit Tool Execution Channels

**Goal:** Stop treating every network-only action plan as process sandbox work.

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/enums.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `crates/jyowo-harness-tool/src/tool.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/mod.rs`
- Modify: all built-in tool plan call sites that use `action_plan_from_permission_check`
- Test: `crates/jyowo-harness-tool/tests/api_contract.rs`
- Test: `crates/jyowo-harness-tool/tests/permission_fingerprint.rs`

**Design:**

Add a public serde contract that describes which enforcement channel owns execution.

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ToolExecutionChannel {
    DirectAuthorizedRust,
    ProcessSandbox,
    HttpBroker,
    ExternalCapability { capability: ToolCapability },
}
```

Add this field to `ToolActionPlan`:

```rust
pub execution_channel: ToolExecutionChannel,
```

Update action plan helpers so every call site must pass the channel explicitly. Do not add a default that preserves old behavior.

`ToolCapability` already derives `Serialize`, `Deserialize`, and `JsonSchema`; keep `ExternalCapability { capability: ToolCapability }` as a typed contract, not a stringly typed capability field. The contract test must snapshot the exact serde shape for built-in and `Custom(String)` capabilities so public payload shape cannot drift.

Expected channel mapping:

| Tools | Channel |
|---|---|
| `Bash` | `ProcessSandbox` |
| `Diagnostics` | `ProcessSandbox` through `DiagnosticsRunnerCap` implementation |
| file read/write/edit/list/grep/glob | `DirectAuthorizedRust` |
| `WebFetch` | `HttpBroker` |
| `WebSearch` | `HttpBroker` if the backend issues HTTP; otherwise `ExternalCapability` with its backend capability |
| MiniMax tools | `HttpBroker` |
| Seedance tools | `HttpBroker` |
| `SendMessage` | `ExternalCapability { capability: ToolCapability::UserMessenger }` |
| process monitor | `DirectAuthorizedRust` unless it shells out; if it shells out, `ProcessSandbox` |

**Tests first:**

- [ ] Add a contract test that fails because `ToolActionPlan` lacks `execution_channel`.
- [ ] Run the narrow tests and confirm they fail for the expected missing field or old preflight behavior.

**Implementation:**

- [ ] Add `ToolExecutionChannel` to contracts and schema export.
- [ ] Add `execution_channel` to `ToolActionPlan`.
- [ ] Change `action_plan_from_permission_check` to require `ToolExecutionChannel`.
- [ ] Update each built-in tool plan call site explicitly.
- [ ] Map `SendMessage` to `ExternalCapability { capability: ToolCapability::UserMessenger }` in this task so no temporary network-shaped process channel is introduced.
- [ ] Ensure permission fingerprints include the execution channel when relevant. If existing canonical hashing already serializes the full request/action plan, add a regression test proving channel changes alter the plan hash.

**Gates:**

```bash
cargo test -p jyowo-harness-contracts
cargo test -p jyowo-harness-tool --features builtin-toolset --test api_contract
cargo test -p jyowo-harness-tool --features builtin-toolset --test permission_fingerprint
cargo fmt --all --check
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit:** regular subagent audit and security audit required.

## Task 2: Replace Coarse Sandbox Capabilities

**Goal:** Make sandbox preflight capability checks policy-specific and honest.

**Files:**

- Modify: `crates/jyowo-harness-sandbox/src/backend.rs`
- Modify: `crates/jyowo-harness-sandbox/src/local/exec.rs`
- Modify: `crates/jyowo-harness-sandbox/src/docker.rs`
- Modify: `crates/jyowo-harness-sandbox/src/ssh.rs`
- Modify: `crates/jyowo-harness-sandbox/src/noop.rs`
- Modify: tests under `crates/jyowo-harness-sandbox/tests/`
- Modify: downstream code that reads `supports_network` or `supports_filesystem_write`

**Tests first:**

- [ ] Add tests proving `LocalIsolation::None` supports unrestricted network only.
- [ ] Add tests proving OS-level local supports `NetworkAccess::None` but rejects `AllowList`.
- [ ] Add tests proving Docker ephemeral supports `None` and `Unrestricted` but rejects `AllowList`.
- [ ] Add tests proving SSH rejects per-exec network policies other than unrestricted.
- [ ] Run the tests and confirm they fail with the current coarse capability model.

**Implementation:**

- [ ] Add `NetworkPolicySupport` and `WorkspacePolicySupport`.
- [ ] Replace `supports_network` and `supports_filesystem_write`.
- [ ] Rewrite `validate_preflight_capabilities` to match the requested `NetworkAccess` variant against the exact support bit.
- [ ] Keep backend-specific `preflight_execute` checks. Generic preflight must not claim support for a policy that backend-specific validation later rejects.
- [ ] Update all backends to report only enforceable policies.
- [ ] Update downstream capability checks in `jyowo-harness-execution`, `jyowo-harness-engine`, and desktop runtime.

**Gates:**

```bash
cargo test -p jyowo-harness-sandbox --features local,docker,ssh
cargo test -p jyowo-harness-execution
cargo test -p jyowo-harness-engine --features subagent-tool
cargo fmt --all --check
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit:** regular subagent audit and security audit required.

## Task 3: Split Authorization Preflight By Execution Channel

**Goal:** `AuthorizationService` preflights the component that actually enforces the action.

**Files:**

- Create: `crates/jyowo-harness-tool/src/network_broker.rs`
- Modify: `crates/jyowo-harness-tool/src/lib.rs`
- Modify: `crates/jyowo-harness-tool/src/context.rs`
- Modify: `crates/jyowo-harness-execution/src/service.rs`
- Create: `crates/jyowo-harness-execution/src/preflight_registry.rs`
- Modify: `crates/jyowo-harness-execution/src/lib.rs`
- Modify: `crates/jyowo-harness-execution/tests/authorization_flow.rs`
- Modify: `crates/jyowo-harness-tool/tests/orchestrator.rs` if action plan execution needs fixture updates

**Design:**

This task creates the broker preflight interface before `AuthorizationService` uses it. It does not implement the production reqwest transport; that happens in Task 6.

Add a tool-layer preflight capability that can be registered in the runtime without creating a dependency cycle:

```rust
pub struct NetworkBrokerPreflightRequest {
    pub tool_name: String,
    pub tool_use_id: ToolUseId,
    pub network_access: NetworkAccess,
    pub action_plan_hash: ActionPlanHash,
}

#[async_trait]
pub trait ToolNetworkBrokerPreflightCap: Send + Sync + 'static {
    async fn preflight_network_request(
        &self,
        request: &NetworkBrokerPreflightRequest,
    ) -> Result<(), ToolError>;
}
```

Add an execution preflight registry owned by `jyowo-harness-execution`:

```rust
pub struct ExecutionPreflightRegistry {
    pub sandbox_backend: Arc<dyn SandboxBackend>,
    pub network_broker: Option<Arc<dyn ToolNetworkBrokerPreflightCap>>,
    pub capabilities: Arc<CapabilityRegistry>,
}
```

`AuthorizationService::new` must receive this registry or an equivalent typed struct. Do not add optional parameters that silently disable checks. Missing broker or capability must fail closed with a channel-specific reason.

`AuthorizationService` must keep permission resolution before enforcement preflight. After permission allow:

```text
ProcessSandbox -> call sandbox backend preflight with ExecSpec
HttpBroker -> validate approved NetworkAccess shape through network broker preflight
ExternalCapability -> verify required capability is declared and present at runtime where available
DirectAuthorizedRust -> no process sandbox preflight
```

Do not mint an execution ticket until the selected enforcement preflight passes.

If the service cannot check a channel because the required broker/capability is missing, fail closed with a specific reason.

**Tests first:**

- [ ] Add a test where `HttpBroker` + `NetworkAccess::AllowList` passes authorization without invoking sandbox preflight.
- [ ] Add a test where `ProcessSandbox` + `NetworkAccess::None` still invokes sandbox preflight.
- [ ] Add a test where `HttpBroker` with `NetworkAccess::None` fails because HTTP cannot execute with no network.
- [ ] Add a test where `HttpBroker` with missing broker fails before ticket mint.
- [ ] Add a test where `ExternalCapability` missing capability fails before ticket mint.

**Implementation:**

- [ ] Add `ToolNetworkBrokerPreflightCap` and `ExecutionPreflightRegistry`.
- [ ] Replace `preflight_spec_for_plan` with channel-specific preflight helpers.
- [ ] Remove coarse `sandbox_preflight_failure` network checks for non-process channels.
- [ ] Preserve event ordering: permission requested, permission resolved, enforcement preflight passed/failed, ticket minted only after pass.
- [ ] Add distinct failure reasons that identify `process_sandbox`, `http_broker`, or `external_capability`.
- [ ] Keep test-only recording brokers inside tests. Do not add a production noop broker or production fallback broker.

**Gates:**

```bash
cargo test -p jyowo-harness-execution --test authorization_flow
cargo test -p jyowo-harness-tool --features builtin-toolset --test orchestrator
cargo fmt --all --check
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit:** regular subagent audit and security audit required.

## Task 4: Add Routing Process Sandbox Backend

**Goal:** Process tools get a default enforceable backend without user configuration.

**Files:**

- Create: `crates/jyowo-harness-sandbox/src/routing.rs`
- Modify: `crates/jyowo-harness-sandbox/src/lib.rs`
- Modify: `crates/jyowo-harness-sandbox/src/backend.rs`
- Modify: `crates/jyowo-harness-sandbox/Cargo.toml`
- Test: `crates/jyowo-harness-sandbox/tests/routing.rs`

**Design:**

Implement a `RoutingSandboxBackend` using the Strategy pattern.

```rust
pub struct RoutingSandboxBackend {
    backends: Vec<Arc<dyn SandboxBackend>>,
}

impl RoutingSandboxBackend {
    pub fn new(backends: Vec<Arc<dyn SandboxBackend>>) -> Result<Self, SandboxError>;
    pub fn select_backend(&self, spec: &ExecSpec) -> Result<Arc<dyn SandboxBackend>, SandboxError>;
}
```

Selection must be deterministic and tested. It must choose the first backend whose `preflight_execute(spec)` succeeds and whose backend is available if availability is checkable without executing user code.

`execute_with_lifecycle` must create an opaque execution id before preflight and store it in `ExecContext`. The id is not a public serde contract and must not be exposed to React.

`RoutingSandboxBackend::before_execute` must select exactly one child backend, call that child's `before_execute`, and store a lease keyed by `ExecContext.execution_id`:

```rust
struct RoutingSelectionLease {
    selected_backend: Arc<dyn SandboxBackend>,
    selected_backend_id: String,
}
```

`RoutingSandboxBackend::execute` must remove that lease and call only the leased child backend's `execute`. If the lease is missing, execution must fail closed because the caller bypassed lifecycle. It must wrap the returned child `ActivityHandle` in a `RoutingActivityHandle` that owns:

```rust
struct RoutingActivityHandle {
    selected_backend: Arc<dyn SandboxBackend>,
    selected_backend_id: String,
    inner: Arc<dyn ActivityHandle>,
    ctx: ExecContext,
    after_execute_started: AtomicBool,
}
```

`RoutingActivityHandle::wait` calls `inner.wait()`, then calls `selected_backend.after_execute(&outcome, &ctx)` exactly once. Do not store the selected backend in a router-wide mutable slot, because concurrent process executions would race.

`RoutingSandboxBackend::after_execute` must not call any child backend; the selected child `after_execute` is already owned by `RoutingActivityHandle`.

Required order for desktop process execution:

```text
1. OS-level LocalSandbox for current platform
2. DockerSandbox ephemeral per exec if Docker is available
3. LocalIsolation::None only for unrestricted process policies
```

Do not use `LocalIsolation::None` for `NetworkAccess::None`, `LoopbackOnly`, `AllowList`, read-only workspace policy, or writable-subpath-only policy.

**Tests first:**

- [ ] Add a stub backend test where router picks the first backend that can enforce `NetworkAccess::None`.
- [ ] Add a test proving router refuses restricted network policy if only no-isolation local is registered.
- [ ] Add a test proving `before_execute` and `after_execute` run only on the selected backend.
- [ ] Add a test proving `execute` fails closed when called without a preceding router `before_execute`.
- [ ] Add a test proving backend availability changes after `before_execute` cannot switch execution to a different child backend.
- [ ] Add a concurrent execution test proving two handles selected by the same router keep separate child backends and call the correct `after_execute`.
- [ ] Add a test proving failure messages list candidate backend ids and reasons.

**Implementation:**

- [ ] Implement `RoutingSandboxBackend`.
- [ ] Add an internal execution id to `ExecContext` and initialize it in `execute_with_lifecycle` before preflight.
- [ ] Ensure lifecycle wrapping in `execute_with_lifecycle` still calls router `preflight_execute`, then router `before_execute`, leased selected backend `execute`, selected child `after_execute` through `RoutingActivityHandle`, and router `after_execute` without child delegation.
- [ ] Implement `RoutingActivityHandle` for selected-child `after_execute`; do not use router-global selected backend state.
- [ ] Remove the routing selection lease on execute success, execute failure, before_execute failure, and observable cancellation paths.
- [ ] Do not make router `capabilities()` claim policies unless at least one child can enforce them.

**Gates:**

```bash
cargo test -p jyowo-harness-sandbox --features local,docker,ssh --test routing
cargo test -p jyowo-harness-sandbox --features local,docker,ssh
cargo fmt --all --check
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit:** regular subagent audit and security audit required.

## Task 5: Wire Desktop Main Runtime To The Routing Sandbox

**Goal:** Desktop main tool execution stops using bare `LocalSandbox::new(workspace_root)`.

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs` or add a focused runtime assembly test if the existing test layout fits
- Modify: `crates/jyowo-harness-sdk/src/builder.rs` only if SDK injection needs a stronger facade

**Design:**

Add a desktop factory that builds the process sandbox once and injects it into the SDK/tool runtime.

Required behavior:

```text
macOS:
  include LocalSandbox::with_isolation(Seatbelt) when sandbox-exec exists
  include Docker fallback only if Docker can run the configured image with the workspace mounted at /workspace
  include LocalIsolation::None only for unrestricted policies

Linux:
  include LocalSandbox::with_isolation(Bubblewrap) when bwrap exists
  include Docker fallback only if Docker can run the configured image with the workspace mounted at /workspace
  include LocalIsolation::None only for unrestricted policies

Windows:
  do not claim restricted policy support through JobObject until tests prove it
  include Docker fallback only if Docker can run the configured image with the workspace mounted at /workspace
  include LocalIsolation::None only for unrestricted policies
```

Desktop Docker fallback construction must use:

```rust
DockerSandbox::builder()
    .mount(VolumeMount::workspace(workspace_root, "/workspace"))
    .lifecycle(ContainerLifecycle::EphemeralPerExec)
```

This construction is valid only for `read_write_all` workspace policy because `VolumeMount::workspace` is a full read-write mount. For `read_only` or `writable_subpaths`, the router must not select Docker fallback until Docker-specific read-only/subpath mount enforcement exists and has tests.

Add the configured image only through the existing project configuration path if one already exists. Do not introduce a required user setting for common tools. If the default image is missing, return a backend-authored unavailable reason and keep probing the next allowed backend.

The factory must return backend-authored unavailable reasons. Do not panic when a host primitive is absent.

**Tests first:**

- [ ] Add a test proving desktop runtime does not construct the main sandbox with bare `LocalSandbox::new`.
- [ ] Add a test proving diagnostics runner receives the same routing sandbox.
- [ ] Add a test proving Docker fallback is registered with `VolumeMount::workspace(workspace_root, "/workspace")` only for `read_write_all` workspace policy.
- [ ] Add a test proving Docker fallback is not selected for read-only or writable-subpath workspace policies until those mounts are implemented.
- [ ] Add a test proving a missing Docker image or daemon reports an unavailable reason instead of claiming restricted support.
- [ ] Add a test proving unsupported restricted process policy returns a specific capability reason.

**Implementation:**

- [ ] Replace the main runtime `LocalSandbox::new(workspace_root)` with the routing factory.
- [ ] Build Docker fallback with the workspace mount, cwd rewrite, and default `/workspace` workdir only for `read_write_all`.
- [ ] Return a backend-authored unavailable reason for Docker fallback when a process policy requests read-only or writable-subpath workspace access.
- [ ] Keep plugin sidecar sandbox behavior separate unless the same factory can be used without weakening sidecar isolation.
- [ ] Ensure `DesktopDiagnosticsRunner` uses the routing sandbox.
- [ ] Do not introduce settings that the user must configure before common tools work.

**Gates:**

```bash
cargo test -p jyowo-desktop-shell runtime_uses_routing_sandbox
cargo test -p jyowo-desktop-shell diagnostics_runner_uses_routing_sandbox
cargo test -p jyowo-desktop-shell docker_fallback_mounts_workspace
cargo test -p jyowo-desktop-shell docker_fallback_rejects_restricted_workspace_mounts
cargo test -p jyowo-desktop-shell unsupported_restricted_policy_reports_reason
cargo test -p jyowo-harness-tool --features builtin-toolset --test builtin_diagnostics
cargo fmt --all --check
cargo check --workspace
git diff --check
```

**Audit:** regular subagent audit and security audit required.

## Task 6: Add Authorized HTTP Broker

**Goal:** Provider and web HTTP tools use a production broker transport that enforces approved host rules.

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/enums.rs` if `ToolCapability::NetworkBroker` does not already exist after earlier tasks
- Modify: `crates/jyowo-harness-tool/src/network_broker.rs`
- Modify: `crates/jyowo-harness-tool/src/lib.rs`
- Modify: `crates/jyowo-harness-tool/src/context.rs` if helper methods are needed
- Modify: `crates/jyowo-harness-execution/src/lib.rs`
- Create: `crates/jyowo-harness-execution/src/http_broker.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Test: `crates/jyowo-harness-tool/tests/capabilities.rs`
- Test: `crates/jyowo-harness-execution/tests/authorization_flow.rs`
- Create: `crates/jyowo-harness-execution/tests/http_broker.rs`

**Design:**

The broker preflight interface already exists from Task 3. This task adds the execution permit and production reqwest-backed transport.

The opaque permit belongs near `AuthorizedToolInput`, where the ticket and action plan are available. It must not implement `Clone`, must have private fields, and must have no public constructor outside `AuthorizedToolInput`.

```rust
pub struct AuthorizedNetworkPermit {
    ticket: AuthorizedTicketSummary,
    tool_name: String,
    tool_use_id: ToolUseId,
    session_id: SessionId,
    run_id: RunId,
    network_access: NetworkAccess,
    approved_hosts: Vec<HostRule>,
    action_plan_hash: ActionPlanHash,
    _private: (),
}

impl AuthorizedToolInput {
    pub fn network_permit(&self) -> Result<AuthorizedNetworkPermit, ToolError>;
}
```

The broker trait belongs in `jyowo-harness-tool` so production tools can depend on it without creating a lower-layer cycle. Extend the Task 3 trait instead of creating a parallel capability.

```rust
#[async_trait]
pub trait ToolNetworkBrokerCap: ToolNetworkBrokerPreflightCap {
    async fn execute_json(
        &self,
        permit: &AuthorizedNetworkPermit,
        request: ToolHttpJsonRequest,
    ) -> Result<ToolHttpResponse, ToolError>;
}
```

Use a typed capability key for execution lookup:

```rust
ToolCapability::NetworkBroker
```

Add this enum variant if it does not already exist. Do not register the production broker under `ToolCapability::Custom(String)`.

Desktop runtime assembly must create exactly one `Arc<dyn ToolNetworkBrokerCap>` and inject that same object into both:

```text
ExecutionPreflightRegistry.network_broker
CapabilityRegistry[ToolCapability::NetworkBroker]
```

Authorization preflight and authorized tool execution must therefore use the same broker instance and the same policy implementation.

Add request types only as needed. Include method, URL, headers, JSON body, multipart payload, timeout, and max response bytes. Do not add generic raw socket support.

Broker validation rules:

- broker v1 supports only `NetworkAccess::AllowList`; `NetworkAccess::Unrestricted` always fails for brokered tool calls until a separate explicit policy is designed and tested
- request scheme must be `http` or `https`
- request host and explicit/effective port must match one approved `HostRule`
- public raw IP literals are denied; loopback IP literals are allowed only when the exact loopback host and port are explicitly approved
- redirects are denied unless each redirect target is validated before following
- response body is capped
- broker must validate request host rules against the permit's immutable claims, not frontend state or tool-supplied strings
- error strings are redacted before returning

**Tests first:**

- [ ] Add test where an approved host succeeds against a local loopback server.
- [ ] Add test where a different host fails before any request is sent.
- [ ] Add test where redirect to an unapproved host fails.
- [ ] Add test where public raw IP fails even if the hostname parser accepts it.
- [ ] Add test where loopback IP succeeds only when the exact loopback host and port are approved.
- [ ] Add test where `NetworkAccess::Unrestricted` fails for brokered tool calls.
- [ ] Add test proving permit fields bind to the authorized tool name, tool use id, session id, run id, approved hosts, and action plan hash.
- [ ] Add test proving the permit cannot be constructed outside `AuthorizedToolInput` in normal production code paths.
- [ ] Add a desktop runtime assembly test proving `ExecutionPreflightRegistry.network_broker` and `CapabilityRegistry[ToolCapability::NetworkBroker]` point to the same `Arc` instance.

**Implementation:**

- [ ] Extend the Task 3 broker trait with execution methods and add permit plus request/response types.
- [ ] Add `ToolCapability::NetworkBroker` if needed and use it as the only production capability key for broker execution.
- [ ] Implement `ReqwestToolNetworkBroker` in `jyowo-harness-execution`.
- [ ] Create one production broker instance in desktop runtime assembly and inject the same `Arc` into both the execution preflight registry and `CapabilityRegistry`.
- [ ] Validate every request against permit-owned tool/session/run/action-plan/host claims before network dispatch.
- [ ] Ensure errors pass through existing redactor or an injected redactor.

**Gates:**

```bash
cargo test -p jyowo-harness-contracts
cargo test -p jyowo-harness-tool --features builtin-toolset --test capabilities
cargo test -p jyowo-harness-execution --test http_broker
cargo test -p jyowo-harness-execution --test authorization_flow
cargo test -p jyowo-desktop-shell network_broker_runtime_assembly_uses_same_instance
cargo fmt --all --check
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit:** regular subagent audit and security audit required.

## Task 7: Refactor Built-In HTTP Tools To Use The Broker

**Goal:** Network-only built-ins no longer issue raw HTTP outside the authorized broker.

**Files:**

- Modify: `crates/jyowo-harness-tool/src/provider_minimax.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/minimax.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/seedance.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/web_fetch.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/web_search.rs`
- Modify: `crates/jyowo-harness-tool/src/provider_media.rs` if media downloads are tool-originated network access
- Create: `scripts/check-tool-network-broker-boundary.mjs`
- Modify: `package.json`
- Modify: tests under `crates/jyowo-harness-tool/tests/`
- Modify: provider client unit tests that currently construct direct reqwest clients

**Design:**

Production clients must accept brokered transport.

Example shape:

```rust
pub(crate) struct MinimaxApiClient {
    transport: Arc<dyn ToolNetworkBrokerCap>,
    permit: AuthorizedNetworkPermit,
    api_key: SecretString,
    base_url: Url,
}
```

If direct reqwest clients remain for unit tests, keep them behind `#[cfg(test)]` helpers only. Do not expose direct production constructors that bypass the broker.

**Tests first:**

- [ ] Add a MiniMax tool execution test where authorization allows `127.0.0.1:<port>` and the brokered client reaches the local test server.
- [ ] Add a MiniMax test where the request body tries to use a credential base URL outside the approved host and fails before network.
- [ ] Add WebFetch tests for approved host, disallowed host, and redirect denial.
- [ ] Add Seedance tests equivalent to MiniMax for approved and denied host.
- [ ] Add a boundary scanner script that fails if production code outside the broker module imports or constructs `reqwest::Client`, `reqwest::ClientBuilder`, aliases either type, or calls direct fully qualified constructors. The scanner must ignore `#[cfg(test)]` blocks and files under test directories.
- [ ] Add or update a root package script named `check:tool-network-broker-boundary` and include it in `pnpm check` before `pnpm check:rust`.

**Implementation:**

- [ ] Change MiniMax execution to obtain `authorized.network_permit()` and `ToolNetworkBrokerCap`.
- [ ] Change Seedance execution to use a brokered transport. If `SeedanceApiClient` lives in `jyowo-harness-model`, either move the HTTP transport boundary into `jyowo-harness-tool` or add a lower-level transport trait without making `jyowo-harness-model` depend on `jyowo-harness-tool`.
- [ ] Change WebFetch production backend to use `ToolNetworkBrokerCap`.
- [ ] Change WebSearch production backend to use brokered HTTP or mark it as `ExternalCapability` if it is not an HTTP backend.
- [ ] Add `scripts/check-tool-network-broker-boundary.mjs` and make it scan only production files. Do not use a simple grep that can miss aliases.
- [ ] Add `"check:tool-network-broker-boundary": "node scripts/check-tool-network-broker-boundary.mjs"` to root `package.json`.
- [ ] Add `pnpm check:tool-network-broker-boundary` to root `check` before `pnpm check:rust`.
- [ ] Ensure provider credentials stay out of logs, traces, events, frontend state, test snapshots, and error strings.

**Gates:**

```bash
cargo test -p jyowo-harness-tool --features builtin-toolset --test minimax_tools
cargo test -p jyowo-harness-tool --features builtin-toolset --test seedance_tools
cargo test -p jyowo-harness-tool --features builtin-toolset --test builtin_tools
cargo test -p jyowo-harness-tool --features builtin-toolset --test capabilities
node scripts/check-tool-network-broker-boundary.mjs
pnpm check:tool-network-broker-boundary
cargo fmt --all --check
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit:** regular subagent audit and security audit required.

## Task 8: Audit Non-Process Capability Plans

**Goal:** Prove no non-process capability tool still uses a process-sandbox-shaped execution channel.

**Files:**

- Modify: `crates/jyowo-harness-tool/src/builtin/send_message.rs` only if Task 1 missed the required channel mapping
- Modify: any other built-in that declares `NetworkAccess::AllowList` but executes through a non-HTTP capability only if the audit finds one
- Modify: `crates/jyowo-harness-tool/tests/builtin_tools.rs`
- Modify: `crates/jyowo-harness-execution/tests/authorization_flow.rs`

**Tests first:**

- [ ] Add a `SendMessage` authorization regression test proving Task 1 mapped it to `ExternalCapability { capability: UserMessenger }`.
- [ ] Add a test proving missing `UserMessengerCap` fails before ticket mint.
- [ ] Add a test proving present `UserMessengerCap` does not call process sandbox preflight.
- [ ] Add a scan-style test over built-in action plans proving every `ExternalCapability` plan is checked against a runtime capability and never process sandbox preflight.

**Implementation:**

- [ ] Keep `SendMessageTool::plan` as `ExternalCapability { capability: ToolCapability::UserMessenger }`; if it is not, fix it here and document why Task 1 missed it in the task completion analysis.
- [ ] Replace network-shaped permission subject if a more accurate existing subject exists. If no accurate subject exists, keep the existing permission subject but document why execution channel, not subject, controls enforcement.
- [ ] Ensure `execute_authorized` still requires the authorized input and registered capability.

**Gates:**

```bash
cargo test -p jyowo-harness-tool --features builtin-toolset --test builtin_tools
cargo test -p jyowo-harness-execution --test authorization_flow
cargo fmt --all --check
cargo check --workspace
git diff --check
```

**Audit:** regular subagent audit and security audit required.

## Task 9: Add Backend Runtime Capability Status

**Goal:** The UI can show why a tool is available or unavailable before the user approves a plan.

**Files:**

- Create: `crates/jyowo-harness-contracts/src/runtime_execution_status.rs`
- Modify: `crates/jyowo-harness-contracts/src/lib.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `crates/jyowo-harness-sdk/src/builder.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src/shared/tauri` files for command client exposure
- Modify: relevant frontend settings/workbench/conversation UI files only if an existing diagnostics/status surface exists
- Test: `apps/desktop/src-tauri/tests/commands.rs`
- Test: `apps/desktop/src/shared/tauri/runtime-execution-status.schema.test.ts`
- Test: relevant frontend Vitest file near the UI surface only if UI rendering changes

**Design:**

Add a backend-authored status payload.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeExecutionStatus {
    pub process_sandbox: ProcessSandboxStatus,
    pub http_broker: BrokerStatus,
    pub tools: Vec<ToolRuntimeStatus>,
}
```

The status must be read-only. Tauri commands must not let React override it.

Frontend must render status from this payload only. Do not infer tool availability from local frontend constants.

**Tests first:**

- [ ] Add Tauri command test proving status comes from backend runtime assembly.
- [ ] Add frontend test proving unavailable reason renders from backend payload.
- [ ] Add Zod validation for the payload if the command client boundary requires it.

**Implementation:**

- [ ] Add contracts and schema export.
- [ ] Keep status structs in `runtime_execution_status.rs`; do not place non-enum status structs in `enums.rs`.
- [ ] Add SDK facade method for runtime execution status.
- [ ] Add Tauri command that returns status.
- [ ] Add frontend command client method and UI rendering in the existing status/settings/workbench surface.
- [ ] Ensure UI text is concise and localized in `en-US.ts` and `zh-CN.ts` if new strings are introduced.

**Gates:**

```bash
cargo test -p jyowo-desktop-shell runtime_execution_status_command_returns_backend_payload
pnpm -C apps/desktop test -- src/shared/tauri/runtime-execution-status.schema.test.ts
pnpm check:desktop
pnpm check:frontend-docs
cargo fmt --all --check
cargo check --workspace
git diff --check
```

**Audit:** regular subagent audit and security audit required.

## Task 10: Reconcile Architecture Docs And Docs Gate Coverage

**Goal:** Keep the Task 0.5 architecture docs aligned with the final implementation and protected by docs gates.

**Files:**

- Modify: `docs/architecture/harness/crates/harness-sandbox.md`
- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`
- Modify: docs gate scripts only if existing gates do not validate the new architecture doc reference
- Test: docs gate scripts if changed

**Docs requirements:**

Document:

- execution channels
- routing process sandbox
- authorized HTTP broker
- Docker fallback workspace mount contract
- Docker fallback `read_write_all`-only support until Docker read-only/subpath mounts exist
- router per-execution lifecycle lease ownership
- broker permit binding, same-instance runtime assembly, and allowlist-only v1 policy
- exact meaning of `BypassPermissions` and `DontAsk`
- local no-isolation mode not being enforcement
- policy-specific capability reporting
- frontend status as display-only
- no production mocks or fake success paths

Do not write temporary implementation notes into normative docs.

**Tests first:**

- [ ] If implementation changed the Task 0.5 design, add or update a docs gate test proving the normative doc still covers the changed concept.
- [ ] Run the docs gate and confirm the intended failure before implementation if a docs gate change is needed.

**Implementation:**

- [ ] Update docs only for final implementation facts that differ from Task 0.5.
- [ ] Add docs gate coverage if the existing gate does not enforce the new architecture doc reference.

**Gates:**

```bash
pnpm check:backend-docs
pnpm check:frontend-docs
pnpm check:agent-docs
pnpm check:docs
git diff --check
```

**Audit:** regular subagent audit required. Security audit required if docs touch security policy.

## Task 11: End-To-End Regressions For The Original Failures

**Goal:** Prove the screenshot failures cannot regress.

**Files:**

- Add or modify Rust integration tests near:
  - `crates/jyowo-harness-tool/tests/builtin_exec.rs`
  - `crates/jyowo-harness-tool/tests/builtin_diagnostics.rs`
  - `crates/jyowo-harness-tool/tests/minimax_tools.rs`
  - `crates/jyowo-harness-execution/tests/authorization_flow.rs`
  - `apps/desktop/src-tauri/tests/commands.rs`
- Add frontend regression tests only if status/error rendering changed in Task 9

**Required regressions:**

- [ ] Bash `pwd && ls -la` with `NetworkAccess::None` uses a backend that can enforce no-network or fails before approval with a clear backend-authored reason.
- [ ] Diagnostics uses the same process routing and does not fail because the main desktop runtime used `LocalIsolation::None`.
- [ ] MiniMax `AllowList` network action uses HTTP broker preflight and brokered execution, not process sandbox preflight.
- [ ] `bypass_permissions` still requires process sandbox or HTTP broker enforcement.
- [ ] Missing sandbox backend or HTTP broker fails closed and does not mint a usable execution ticket.
- [ ] UI renders a backend-authored reason for unavailable tools.

**Gates:**

```bash
cargo test -p jyowo-harness-execution --test authorization_flow
cargo test -p jyowo-harness-tool --features builtin-toolset --test builtin_exec
cargo test -p jyowo-harness-tool --features builtin-toolset --test builtin_diagnostics
cargo test -p jyowo-harness-tool --features builtin-toolset --test minimax_tools
cargo test -p jyowo-desktop-shell
pnpm check:desktop
cargo fmt --all --check
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit:** regular subagent audit and security audit required.

## Task 12: Final Gates And Release Decision

**Goal:** Verify the full implementation as one product-level change.

**Files:**

- No source changes unless a gate exposes a real defect.

**Steps:**

- [ ] Run:

```bash
pnpm check:docs
pnpm check:agent-docs
pnpm check:frontend-docs
pnpm check:backend-docs
pnpm check:desktop
pnpm check:rust
pnpm audit:tests
pnpm check:test-architecture
pnpm check:tool-network-broker-boundary
pnpm check:agent-orchestration-no-fakes
pnpm check:agent-supervisor-sidecar
pnpm check:quick
pnpm check
git diff --check
```

- [ ] Run a manual desktop smoke test or automated equivalent:

```text
1. Start desktop app from the implementation worktree.
2. Run Bash with `pwd && ls -la`.
3. Run Diagnostics for Rust or desktop TS.
4. Run one brokered WebFetch against an approved local test server or a user-approved HTTPS URL.
5. Run one MiniMax manual-live smoke only with real user-provided credentials and an approved HTTPS provider host. If credentials are unavailable, skip the manual-live call and rely on the automated loopback broker regression. Do not add a production MiniMax test mode.
6. Confirm unavailable tools show backend-authored reasons.
```

- [ ] Run final read-only implementation audit.
- [ ] Run final security audit.
- [ ] Commit final fixes if any.

Final audit prompt:

```text
Final read-only audit for docs/plans/2026-07-06-desktop-default-sandbox-runtime-implementation.md.

Use ChatGPT 5.5 with xhigh reasoning. Do not edit files.

Verify every task acceptance item, every forbidden item, every original screenshot failure regression, and every final gate.
Return PASS or FAIL with exact file and line evidence.
```

**Expected:** all gates exit 0, original failures are covered by regression tests, and the implementation branch is ready for review.

## Definition Of Done

- The plan was implemented from an isolated worktree, not the original `main` checkout.
- Every task has a commit.
- Every task has a pre-task intent check, completion analysis, and read-only subagent audit.
- Security-sensitive tasks have separate security audits.
- No production mock data, fake runtime path, noop success, placeholder behavior, or UI-only policy exists.
- Main desktop runtime no longer uses bare `LocalSandbox::new(workspace_root)` for process tools.
- Process sandbox preflight is used only for process execution channels.
- HTTP/provider tools execute through the authorized HTTP broker.
- Authorization preflight and authorized HTTP execution use the same broker instance.
- Docker fallback does not claim restricted workspace policy support unless exact Docker mount enforcement exists.
- Sandbox capability reporting is policy-specific and honest.
- Unsupported policies fail closed with backend-authored reasons.
- Frontend renders backend status only.
- `BypassPermissions` and `DontAsk` still cannot bypass sandbox, network broker, hard policy, tenant/workspace scope, ticket validation, redaction, or event ordering.
- `pnpm check:tool-network-broker-boundary` exits 0 and is included in root `pnpm check`.
- `pnpm check` exits 0.
