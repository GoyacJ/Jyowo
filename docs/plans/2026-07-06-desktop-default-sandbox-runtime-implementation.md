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
  else use LocalIsolation::None only if workspace policy is not restricted
  else DockerSandbox if available

if spec.policy.network == LoopbackOnly or AllowList:
  fail closed for process tools until a backend explicitly implements that policy
```

The router must call the selected backend lifecycle exactly once:

```text
preflight_execute -> before_execute -> execute -> wait -> after_execute
```

Do not call `before_execute` inside child backend `execute`.

### HTTP Broker

Add an authorized HTTP broker for tool-originated service calls.

Required behavior:

- accepts an opaque authorization permit derived from `AuthorizedToolInput`
- rejects requests whose URL host or port does not match the approved `NetworkAccess::AllowList`
- rejects raw IP, username/password URL authority, non-http(s) schemes, invalid host, and redirect to a host outside the allowlist
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
- Test: `crates/jyowo-harness-execution/tests/authorization_flow.rs`

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
- [ ] Add an authorization flow test that asserts a network-only `HttpBroker` plan does not call `SandboxBackend::preflight_execute`.
- [ ] Run the narrow tests and confirm they fail for the expected missing field or old preflight behavior.

**Implementation:**

- [ ] Add `ToolExecutionChannel` to contracts and schema export.
- [ ] Add `execution_channel` to `ToolActionPlan`.
- [ ] Change `action_plan_from_permission_check` to require `ToolExecutionChannel`.
- [ ] Update each built-in tool plan call site explicitly.
- [ ] Ensure permission fingerprints include the execution channel when relevant. If existing canonical hashing already serializes the full request/action plan, add a regression test proving channel changes alter the plan hash.

**Gates:**

```bash
cargo test -p jyowo-harness-contracts
cargo test -p jyowo-harness-tool --features builtin-toolset --test api_contract
cargo test -p jyowo-harness-tool --features builtin-toolset --test permission_fingerprint
cargo test -p jyowo-harness-execution --test authorization_flow
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

- Modify: `crates/jyowo-harness-execution/src/service.rs`
- Modify: `crates/jyowo-harness-execution/tests/authorization_flow.rs`
- Modify: `crates/jyowo-harness-tool/tests/orchestrator.rs` if action plan execution needs fixture updates

**Design:**

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
- [ ] Add a test where `ExternalCapability` missing capability fails before ticket mint.

**Implementation:**

- [ ] Replace `preflight_spec_for_plan` with channel-specific preflight helpers.
- [ ] Remove coarse `sandbox_preflight_failure` network checks for non-process channels.
- [ ] Preserve event ordering: permission requested, permission resolved, enforcement preflight passed/failed, ticket minted only after pass.
- [ ] Add distinct failure reasons that identify `process_sandbox`, `http_broker`, or `external_capability`.

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
- Modify: `crates/jyowo-harness-sandbox/src/backend.rs` only if trait changes are strictly needed
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
- [ ] Add a test proving failure messages list candidate backend ids and reasons.

**Implementation:**

- [ ] Implement `RoutingSandboxBackend`.
- [ ] Ensure lifecycle wrapping in `execute_with_lifecycle` still calls router `preflight_execute`, then router `before_execute`, selected backend `execute`, and router `after_execute`.
- [ ] Store selected backend for the current handle if needed to call `after_execute` on the same child backend.
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
  include Docker fallback if Docker is available
  include LocalIsolation::None only for unrestricted policies

Linux:
  include LocalSandbox::with_isolation(Bubblewrap) when bwrap exists
  include Docker fallback if Docker is available
  include LocalIsolation::None only for unrestricted policies

Windows:
  do not claim restricted policy support through JobObject until tests prove it
  include Docker fallback if Docker is available
  include LocalIsolation::None only for unrestricted policies
```

The factory must return backend-authored unavailable reasons. Do not panic when a host primitive is absent.

**Tests first:**

- [ ] Add a test proving desktop runtime does not construct the main sandbox with bare `LocalSandbox::new`.
- [ ] Add a test proving diagnostics runner receives the same routing sandbox.
- [ ] Add a test proving unsupported restricted process policy returns a specific capability reason.

**Implementation:**

- [ ] Replace the main runtime `LocalSandbox::new(workspace_root)` with the routing factory.
- [ ] Keep plugin sidecar sandbox behavior separate unless the same factory can be used without weakening sidecar isolation.
- [ ] Ensure `DesktopDiagnosticsRunner` uses the routing sandbox.
- [ ] Do not introduce settings that the user must configure before common tools work.

**Gates:**

```bash
cargo test -p jyowo-desktop-shell runtime
cargo test -p jyowo-desktop-shell diagnostics
cargo test -p jyowo-harness-tool --features builtin-toolset --test builtin_diagnostics
cargo fmt --all --check
cargo check --workspace
git diff --check
```

**Audit:** regular subagent audit and security audit required.

## Task 6: Add Authorized HTTP Broker

**Goal:** Provider and web HTTP tools use a broker that enforces approved host rules.

**Files:**

- Create: `crates/jyowo-harness-tool/src/network_broker.rs`
- Modify: `crates/jyowo-harness-tool/src/lib.rs`
- Modify: `crates/jyowo-harness-tool/src/context.rs` if helper methods are needed
- Modify: `crates/jyowo-harness-execution/src/lib.rs`
- Create: `crates/jyowo-harness-execution/src/http_broker.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Test: `crates/jyowo-harness-tool/tests/capabilities.rs`
- Test: `crates/jyowo-harness-execution/tests/authorization_flow.rs`
- Create: `crates/jyowo-harness-execution/tests/http_broker.rs`

**Design:**

The opaque permit belongs near `AuthorizedToolInput`, where the ticket and action plan are available.

```rust
pub struct AuthorizedNetworkPermit {
    ticket: AuthorizedTicketSummary,
    network_access: NetworkAccess,
    action_plan_hash: ActionPlanHash,
    _private: (),
}

impl AuthorizedToolInput {
    pub fn network_permit(&self) -> Result<AuthorizedNetworkPermit, ToolError>;
}
```

The broker trait belongs in `jyowo-harness-tool` so production tools can depend on it without creating a lower-layer cycle.

```rust
#[async_trait]
pub trait ToolNetworkBrokerCap: Send + Sync + 'static {
    async fn execute_json(
        &self,
        permit: &AuthorizedNetworkPermit,
        request: ToolHttpJsonRequest,
    ) -> Result<ToolHttpResponse, ToolError>;
}
```

Add request types only as needed. Include method, URL, headers, JSON body, multipart payload, timeout, and max response bytes. Do not add generic raw socket support.

Broker validation rules:

- permit must contain `NetworkAccess::AllowList`
- request scheme must be `http` or `https`
- request host and explicit/effective port must match one approved `HostRule`
- redirects are denied unless each redirect target is validated before following
- response body is capped
- error strings are redacted before returning

**Tests first:**

- [ ] Add test where an approved host succeeds against a local loopback server.
- [ ] Add test where a different host fails before any request is sent.
- [ ] Add test where redirect to an unapproved host fails.
- [ ] Add test where `NetworkAccess::Unrestricted` fails for brokered tool calls unless the plan explicitly supports unrestricted broker policy.
- [ ] Add test proving the permit cannot be constructed outside `AuthorizedToolInput` in normal production code paths.

**Implementation:**

- [ ] Add broker trait, permit, and request/response types.
- [ ] Implement `ReqwestToolNetworkBroker` in `jyowo-harness-execution`.
- [ ] Register the broker in desktop runtime `CapabilityRegistry`.
- [ ] Ensure errors pass through existing redactor or an injected redactor.

**Gates:**

```bash
cargo test -p jyowo-harness-tool --features builtin-toolset --test capabilities
cargo test -p jyowo-harness-execution --test http_broker
cargo test -p jyowo-harness-execution --test authorization_flow
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
- [ ] Add a static-style test or grep-based test that fails if production code calls `reqwest::Client::new` or `Client::builder` in `crates/jyowo-harness-tool/src/provider_minimax.rs`, `builtin/minimax.rs`, `builtin/seedance.rs`, `builtin/web_fetch.rs`, or production web search code.

**Implementation:**

- [ ] Change MiniMax execution to obtain `authorized.network_permit()` and `ToolNetworkBrokerCap`.
- [ ] Change Seedance execution to use a brokered transport. If `SeedanceApiClient` lives in `jyowo-harness-model`, either move the HTTP transport boundary into `jyowo-harness-tool` or add a lower-level transport trait without making `jyowo-harness-model` depend on `jyowo-harness-tool`.
- [ ] Change WebFetch production backend to use `ToolNetworkBrokerCap`.
- [ ] Change WebSearch production backend to use brokered HTTP or mark it as `ExternalCapability` if it is not an HTTP backend.
- [ ] Ensure provider credentials stay out of logs, traces, events, frontend state, test snapshots, and error strings.

**Gates:**

```bash
cargo test -p jyowo-harness-tool --features builtin-toolset --test minimax_tools
cargo test -p jyowo-harness-tool --features builtin-toolset --test seedance_tools
cargo test -p jyowo-harness-tool --features builtin-toolset --test builtin_tools
cargo test -p jyowo-harness-tool --features builtin-toolset --test capabilities
cargo fmt --all --check
cargo check --workspace
pnpm check:test-architecture
git diff --check
```

**Audit:** regular subagent audit and security audit required.

## Task 8: Fix Non-Process Capability Plans

**Goal:** Tools such as `SendMessage` stop using process-sandbox-shaped network preflight.

**Files:**

- Modify: `crates/jyowo-harness-tool/src/builtin/send_message.rs`
- Modify: any other built-in that declares `NetworkAccess::AllowList` but executes through a non-HTTP capability
- Modify: `crates/jyowo-harness-tool/tests/builtin_tools.rs`
- Modify: `crates/jyowo-harness-execution/tests/authorization_flow.rs`

**Tests first:**

- [ ] Add a `SendMessage` authorization test proving it uses `ExternalCapability { capability: UserMessenger }`.
- [ ] Add a test proving missing `UserMessengerCap` fails before ticket mint.
- [ ] Add a test proving present `UserMessengerCap` does not call process sandbox preflight.

**Implementation:**

- [ ] Update `SendMessageTool::plan` to use the external capability channel.
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

- Modify: `crates/jyowo-harness-contracts/src/enums.rs` or add a focused contracts file if project convention supports it
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `crates/jyowo-harness-sdk/src/builder.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src/shared/tauri` files for command client exposure
- Modify: relevant frontend settings/workbench/conversation UI files only if an existing diagnostics/status surface exists
- Test: `apps/desktop/src-tauri/tests/commands.rs`
- Test: relevant frontend Vitest file near the UI surface

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
- [ ] Add SDK facade method for runtime execution status.
- [ ] Add Tauri command that returns status.
- [ ] Add frontend command client method and UI rendering in the existing status/settings/workbench surface.
- [ ] Ensure UI text is concise and localized in `en-US.ts` and `zh-CN.ts` if new strings are introduced.

**Gates:**

```bash
cargo test -p jyowo-desktop-shell runtime_execution_status
pnpm -C apps/desktop test -- runtime execution status
pnpm check:desktop
pnpm check:frontend-docs
cargo fmt --all --check
cargo check --workspace
git diff --check
```

**Audit:** regular subagent audit and security audit required.

## Task 10: Add Architecture Docs And Docs Gate Coverage

**Goal:** The design is documented in normative backend architecture docs and protected by docs gates.

**Files:**

- Create: `docs/architecture/harness/crates/harness-sandbox.md`
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
- exact meaning of `BypassPermissions` and `DontAsk`
- local no-isolation mode not being enforcement
- policy-specific capability reporting
- frontend status as display-only
- no production mocks or fake success paths

Do not write temporary implementation notes into normative docs.

**Tests first:**

- [ ] If a docs gate script needs an update, add a failing test proving `harness-sandbox.md` is required.
- [ ] Run the docs gate and confirm the intended failure before implementation if applicable.

**Implementation:**

- [ ] Add or update docs.
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
5. Run one MiniMax tool against an approved local test server in test mode, or skip only if credentials are unavailable and the automated broker regression already passed.
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
- Sandbox capability reporting is policy-specific and honest.
- Unsupported policies fail closed with backend-authored reasons.
- Frontend renders backend status only.
- `BypassPermissions` and `DontAsk` still cannot bypass sandbox, network broker, hard policy, tenant/workspace scope, ticket validation, redaction, or event ordering.
- `pnpm check` exits 0.
