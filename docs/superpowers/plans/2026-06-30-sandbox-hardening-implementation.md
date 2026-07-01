# Sandbox Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring Jyowo sandbox execution to a fail-closed, policy-owned, OS-isolated design comparable to Codex, Claude Code, and OpenClaw, without leaving compatibility shims or fake security boundaries.

**Architecture:** Rust remains the policy authority. `jyowo-harness-contracts` owns stable policy and diagnostic shapes, `jyowo-harness-sandbox` compiles those policies into backend-specific enforcement, `jyowo-harness-tool` maps tools into precise `ExecSpec`s, and the desktop shell only assembles the selected backend and exposes typed diagnostics through existing IPC boundaries. Destructive refactors are allowed when they remove duplicated policy state or false compatibility, but each refactor must reduce ambiguity and be covered by tests.

**Tech Stack:** Rust 1.96, Tauri 2, React 19, TypeScript 6, Zod, Tokio process execution, macOS Seatbelt, Linux bubblewrap, Windows process isolation only when enforceable, Docker, SSH, `cargo test`, `pnpm check:rust`, `pnpm check:desktop`, `pnpm check:docs`, `pnpm check`.

## 2026-07-01 Revalidation Baseline

This plan was rechecked after `goya/agent-orchestration` was merged into `main`.

Current implementation facts:

- `crates/jyowo-harness-agent-runtime` exists and owns agent profile storage, runtime policy merge, workspace isolation leases, team state, and background agent state.
- `apps/desktop/src-tauri/src/commands/agents.rs` and `background_agents.rs` exist and are registered command domains.
- `apps/desktop/src-tauri/src/commands/conversations.rs` now merges `AgentRunOptions`, starts foreground/background agent runs, and projects subagent, team, and background events.
- `apps/desktop/src-tauri/src/commands/providers.rs` exposes agent capability availability through `AgentCapabilityResolver`.
- `crates/jyowo-harness-sdk/src/harness/session_runtime.rs` installs run-scoped subagent capability wiring and always passes the configured sandbox into the engine builder.
- `crates/jyowo-harness-engine` already contains subagent sandbox inheritance behavior and tests. Task 10 must consolidate this behavior into the shared sandbox policy merge design rather than adding a parallel inheritance model.

Implementation rules from this revalidation:

- Create the sandbox hardening worktree from the current `main` after agent orchestration, not from `origin/main` or a pre-orchestration branch.
- Do not remove or bypass existing agent profile, subagent, team, background-agent, and run-scoped model-selection contracts while hardening sandbox behavior.
- Treat agent orchestration paths as production sandbox call sites. Every task that changes `ExecSpec`, `SandboxPolicy`, `PermissionBroker`, or `ProcessNetworkAccess` must re-check `jyowo-harness-agent-runtime`, `session_runtime.rs`, `commands/conversations.rs`, `commands/providers.rs`, `commands/runtime.rs`, and `commands/background_agents.rs`.
- Task 10 is now a refactor-and-unification task. It must replace or wrap existing ad hoc child sandbox checks with the shared L1 merge helper and preserve existing public behavior except where the plan explicitly requires fail-closed tightening.

---

## Authority References

Use these verified design facts as constraints. Do not implement behavior that contradicts them.

- Codex separates sandbox enforcement from approval policy. Local execution uses OS-level sandboxing and defaults network off. Cloud setup can have network and secrets; agent phase defaults offline and secrets are removed before agent execution.
- Codex permissions support filesystem read/write/deny, deny-first behavior, protected dot-directories, `.env` deny globs, network allow/deny, local/private IP blocking, and fail-closed unsupported policy.
- Claude Code separates tool permission rules from Bash sandboxing. Tool allow rules are not filesystem isolation. Bash sandbox uses Seatbelt or bubblewrap, and credential files and secret env require explicit sandbox credential policy.
- Claude Code treats full sandbox runtime / container / VM as stronger than Bash-only sandbox because hooks and MCP servers otherwise run outside the Bash sandbox.
- OpenClaw separates sandbox placement, tool policy, and elevated host execution. Deny wins. Docker sandbox network defaults to `none`. Bind mounts must reject credential roots, dangerous sources, and symlink-parent escapes.

## Required Worktree

Implementation MUST happen in an isolated git worktree. Do not implement this plan in the user's active working tree.

Run from the original repository:

```bash
git status --short
git worktree add ../Jyowo-sandbox-hardening -b goya/sandbox-hardening
cd ../Jyowo-sandbox-hardening
```

Rules:

- If `git worktree add` fails because the branch already exists, use `git worktree list` and switch to the existing `../Jyowo-sandbox-hardening` worktree.
- The worktree base must include `crates/jyowo-harness-agent-runtime` and `apps/desktop/src-tauri/src/commands/background_agents.rs`. If either path is missing, the base branch is stale and implementation must stop.
- Do not run `git reset --hard`, `git checkout --`, or equivalent destructive commands against user changes.
- Every task must commit independently after passing its gates and subagent audits.

## Mandatory Reading

Before implementation, read these files in the worktree:

```text
AGENTS.md
docs/backend/agent-harness-backend-development-guidelines.md
docs/backend/backend-runtime.md
docs/backend/backend-engineering.md
docs/backend/backend-quality.md
docs/frontend/agent-harness-frontend-development-guidelines.md
docs/frontend/frontend-product-ux.md
docs/frontend/frontend-engineering.md
docs/frontend/frontend-quality.md
```

## Hard Bans

- No mock data.
- No fake sandbox backend for new security behavior.
- No feature flag that silently restores unsafe behavior.
- No "best effort" security boundary.
- No frontend-only security decision.
- No raw secret, token, credential path, private absolute path, provider payload, or sandbox profile internals in prompt, event, log, trace, screenshot, frontend state, snapshot, or fixture.
- No broad `Unrestricted` fallback when the requested policy cannot be enforced.
- No compatibility duplicate where both old and new policy fields stay authoritative.

Tests may use temporary directories, real local processes, real `LocalSandbox`, real contract serialization, and controlled local files. They must not use fake implementations to prove policy enforcement.

## Per-Task Protocol

Every task has the same required entry and exit protocol.

### Entry Protocol

Before editing files for a task, the implementer MUST write a short task note in the plan or in the task-local commit message draft:

```text
Task N intent:
- Required behavior:
- Files to change:
- Security invariants:
- Tests that prove it:
- Unsupported behavior that must fail closed:
```

This note is mandatory. It prevents the implementer from inventing scope while coding.

### Exit Analysis

Before marking a task complete, the implementer MUST write:

```text
Task N exit analysis:
- Implemented behavior:
- Removed old behavior:
- Tests added or changed:
- Gates run with exit code 0:
- Remaining unsupported cases and why they fail closed:
- Secret / path / event leakage check:
```

### Subagent Audit

After the exit analysis and before commit, dispatch a fresh subagent audit. The subagent must receive this exact prompt shape:

```text
Audit Task N from docs/superpowers/plans/2026-06-30-sandbox-hardening-implementation.md.

Check only this task's intended scope.
Verify:
1. The implementation matches the Task N design and does not invent extra behavior.
2. Security failures are fail-closed.
3. No mock data, fake backend, or placeholder implementation proves security behavior.
4. PermissionBroker, Redactor, Journal, tenant scope, and sandbox policy are not bypassed.
5. Secret values and private paths cannot enter prompt, event, log, trace, frontend state, fixture, or snapshot.
6. Tests cover success and failure paths owned by the changed layer.
7. Required gates were run and passed.
8. Public contract, schema, and docs updates are complete.

Return PASS or FAIL.
For FAIL, list exact file and line findings.
```

Because all tasks are security-sensitive, also dispatch a security-review subagent with the same scope. The task cannot be committed until both audits return PASS or all findings are fixed and re-audited.

### Audit Record

Exit analysis and both audit results must be persisted before each task commit.

Create one file per task:

```text
docs/superpowers/audits/sandbox-hardening/task-N.md
```

Required format:

```text
# Sandbox Hardening Task N Audit

## Current Audit Status
Code Review: PASS|FAIL
Security Review: PASS|FAIL
Last Updated: YYYY-MM-DDTHH:MM:SSZ

## Intent
<copy the Task N intent note>

## Exit Analysis
<copy the Task N exit analysis>

## Code Review Subagent
Result: PASS|FAIL
Findings:

## Security Review Subagent
Result: PASS|FAIL
Findings:

## Gates
- <command>: exit 0
```

Rules:

- A task is not complete unless this file exists and `Current Audit Status` says both `Code Review: PASS` and `Security Review: PASS`.
- The audit file must not contain raw secrets, private absolute paths, provider payloads, or raw sandbox profile internals.
- If a task is re-audited, append a new dated section in the same file and update `Current Audit Status` to the latest result. Do not delete failed findings.
- Task commits must include the task audit file.

## Target Design

### Single Policy Source

`ExecSpec` must stop carrying split filesystem authority across `policy.scope`, `policy.denied_host_paths`, and `workspace_access`. The target model is one authoritative `SandboxPolicy`:

```rust
pub struct SandboxPolicy {
    pub mode: SandboxMode,
    pub phase: SandboxPhase,
    pub filesystem: FilesystemPolicy,
    pub network: NetworkAccess,
    pub secrets: SecretAccessPolicy,
    pub resource_limits: ResourceLimits,
}
```

Target contract additions:

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPhase {
    Setup,
    Agent,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct FilesystemPolicy {
    pub rules: Vec<FilesystemRule>,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct FilesystemRule {
    pub selector: FilesystemSelector,
    pub permission: FilesystemPermission,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemSelector {
    WorkspaceRoot,
    WorkspaceSubpath(PathBuf),
    WorkspaceSubtree(PathBuf),
    WorkspaceRootFilePrefix(String),
    TempDir,
    HomeSubpath(PathBuf),
    HomeSubtree(PathBuf),
    Absolute(PathBuf),
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemPermission {
    Read,
    Write,
    Deny,
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SecretAccessPolicy {
    None,
    AllowList(Vec<String>),
}
```

All public contract structs added in this plan must use:

```rust
#[serde(deny_unknown_fields)]
```

Schema tests must prove exported JSON Schema rejects unknown object properties. Do not rely on TypeScript/Zod strictness to protect Rust input boundaries.

Resolution semantics:

- Deny wins over write and read.
- Write implies read.
- Rules are resolved after lexical normalization and canonicalization where host paths exist.
- Symlink-parent escape is denied.
- Workspace write does not override protected deny paths.
- `WorkspaceSubpath` and `HomeSubpath` match one exact normalized path.
- `WorkspaceSubtree` and `HomeSubtree` match the normalized path and every descendant.
- `WorkspaceRootFilePrefix(".env.")` matches future and existing files directly under the workspace root whose file name starts with `.env.`.
- Missing or unsupported selector fails closed.
- Missing final path components are evaluated lexically before creation so deny rules still block future files.
- Secret env is empty by default in agent phase.
- Setup phase may use explicit `SecretAccessPolicy::AllowList`, never inherited host env.
- If a backend cannot enforce a deny selector under a writable parent for future creates, it must not bind the host workspace read-write. It must use a staged workspace or overlay and copy allowed changes back through `ensure_path_allowed`.

Staged workspace copy-back must be transactional at the file-manifest level:

```rust
pub struct WorkspaceChangeManifest {
    pub entries: Vec<WorkspaceChangeEntry>,
}

pub struct WorkspaceBaseline {
    pub entries: BTreeMap<WorkspaceRelativePath, WorkspaceBaselineEntry>,
}

pub struct WorkspaceBaselineEntry {
    pub id: WorkspaceBaselineEntryId,
    pub kind: WorkspaceBaselineEntryKind,
}

pub enum WorkspaceBaselineEntryKind {
    File { digest: ContentDigest, executable: bool },
    Directory,
}

pub struct WorkspaceBaselineEntryId(u64);
pub struct WorkspaceManifestEntryId(u64);

pub struct WorkspaceChangeEntry {
    pub id: WorkspaceManifestEntryId,
    pub change: WorkspaceChange,
}

pub enum WorkspaceChange {
    CreateFile { path: WorkspaceRelativePath, staged_digest: ContentDigest, staged_executable: bool },
    ModifyFile {
        path: WorkspaceRelativePath,
        baseline_id: WorkspaceBaselineEntryId,
        before_digest: ContentDigest,
        before_executable: bool,
        staged_digest: ContentDigest,
        staged_executable: bool,
    },
    DeleteFile {
        path: WorkspaceRelativePath,
        baseline_id: WorkspaceBaselineEntryId,
        before_digest: ContentDigest,
        before_executable: bool,
    },
    CreateDirectory { path: WorkspaceRelativePath },
    RemoveDirectory { path: WorkspaceRelativePath, baseline_id: WorkspaceBaselineEntryId },
}

pub struct WorkspaceRelativePath(PathBuf);
pub struct ContentDigest([u8; 32]);
```

Rules:

- The manifest is built by comparing the staged root with a captured host baseline. Every non-create mutation must reference the exact captured `WorkspaceBaselineEntryId`.
- `WorkspaceRelativePath` rejects absolute paths, `..`, empty components, Windows drive prefixes, and platform path separators inside file names.
- `ContentDigest` is computed from regular file bytes with a stable hash chosen in `workspace_staging.rs`; tests must not compare timestamps.
- Every manifest entry must pass `ensure_path_allowed` before any host mutation.
- If the host entry digest, executable bit, directory/file kind, existence, or baseline id no longer matches the captured baseline, copy-back aborts with a redacted conflict error.
- Host writes go to a temp path under the same parent and then use atomic rename where the platform supports it.
- The implementation must not partially apply a manifest. If any preflight check fails, no host path is mutated.
- If an apply step fails after mutation starts, the operation must return a typed partial-apply error containing only redacted path categories and the manifest entry id. It must not continue applying later entries.
- Symlink, hardlink, device, fifo, socket, and special-file entries are rejected unless a later task explicitly adds typed, policy-checked support.
- Metadata preservation is limited to regular file contents and executable bit where supported. Ownership, extended attributes, ACLs, and timestamps are not copied from the sandbox.

### Default Profiles

Add explicit constructors in `jyowo-harness-sandbox`, not in desktop:

```rust
SandboxPolicy::default_agent_workspace_write()
SandboxPolicy::default_agent_workspace_read_only()
SandboxPolicy::setup_workspace_write_with_network(
    network: NetworkAccess,
    secret_env_allowlist: Vec<String>,
)
```

Default agent workspace write:

- phase: `Agent`
- mode: OS-level where assembled by backend
- filesystem:
  - write workspace root
  - write temp dir
  - deny protected paths
- network: `NetworkAccess::None`
- secrets: `SecretAccessPolicy::None`
- resource limits: existing defaults

Protected deny paths:

```text
WorkspaceSubtree(".git")
WorkspaceSubpath(".jyowo/runtime/provider-settings.json")
WorkspaceSubpath(".jyowo/runtime/mcp-servers.json")
WorkspaceSubtree(".jyowo/runtime/permissions")
WorkspaceSubtree(".jyowo/runtime/events")
WorkspaceSubpath(".env")
WorkspaceSubpath(".env.local")
WorkspaceRootFilePrefix(".env.")
HomeSubtree(".ssh")
HomeSubtree(".aws")
HomeSubtree(".config/gcloud")
HomeSubtree(".docker")
HomeSubtree(".kube")
HomeSubpath(".npmrc")
HomeSubpath(".pypirc")
HomeSubpath(".netrc")
```

Do not represent protected paths with unchecked glob strings. Prefix and subtree selectors must be evaluated by typed Rust matching before execution and again during staged workspace copy-back.

### Backend Enforcement

Local backend:

- macOS: Seatbelt is valid only when `sandbox-exec` exists and the requested filesystem/network policy compiles.
- Linux: bubblewrap is valid only when `bwrap` exists and the requested filesystem/network policy compiles.
- Windows: existing `JobObject` is not enough for filesystem or network isolation. It may limit process lifetime only. It must not advertise enforcement for `NetworkAccess::None` or filesystem read/write/deny. Unsupported policy fails closed until a real Windows filesystem/network isolation backend exists.
- Desktop on Windows must keep settings and non-process UI usable, but local agent Bash/process execution must return a typed "local process sandbox unavailable on Windows" error unless the user selects an enforcing non-local backend such as Docker or SSH. Do not silently downgrade to unsandboxed local execution.

Docker backend:

- Default network mode for agent phase is `none`.
- Bind mounts are read-only unless the policy grants write.
- Bind mount sources must canonicalize under allowed roots.
- Credential roots, dangerous system paths, and symlink-parent escapes fail closed.

SSH backend:

- Remote execution must use a remote workspace root that is absolute, normalized, and not `/`.
- Workspace sync must exclude protected paths by default.
- Pull sync must not overwrite protected runtime files.
- Pull sync must reject delete, rename, symlink, hardlink, device, and special-file operations that target protected local paths.
- Unsupported filesystem/network policy fails closed.

### Network Policy

For arbitrary process execution, only claim support for policies the backend can enforce.

- `NetworkAccess::None` must be enforced by OS/container isolation or fail closed.
- `NetworkAccess::Unrestricted` must require explicit policy and a runtime-only process network grant derived from a separate `PermissionBroker` allow decision for process network access.
- `NetworkAccess::LoopbackOnly` and `NetworkAccess::AllowList` must fail closed for process backends until an enforcing backend advertises support.
- Jyowo-owned HTTP tools may keep their existing network permission path, but that does not authorize Bash or arbitrary process network.
- Host/domain allowlist must not be simulated by env vars such as `HTTP_PROXY` unless direct network is also blocked by sandbox enforcement.

Process network grant:

Add this variant to the existing `PermissionSubject` enum:

```rust
ProcessNetworkAccess {
    command: String,
    argv: Vec<String>,
    cwd: Option<PathBuf>,
    fingerprint: ExecFingerprint,
    network: NetworkAccess,
}
```

Add runtime-only sandbox authorization types:

```rust
pub struct ProcessGrantAuthority {
    pub issuer: ProcessGrantIssuer,
    pub verifier: ProcessGrantVerifier,
}

pub struct ProcessNetworkGrant {
    decision_id: DecisionId,
    subject_fingerprint: ExecFingerprint,
    granted_network: NetworkAccess,
    expires_at: DateTime<Utc>,
    proof: ProcessGrantProof,
}

pub struct ProcessGrantIssuer { /* private key material */ }
pub struct ProcessGrantVerifier { /* private verifier material */ }
struct ProcessGrantProof { /* private MAC/proof bytes */ }
```

Rules:

- The grant is produced only by trusted Rust execution orchestration after `PermissionBroker` returns an allow decision for `PermissionSubject::ProcessNetworkAccess`.
- The `PermissionBroker` result must include the stable `DecisionId` for that decision; grants, permission events, tool approval events, and persisted permission records must all reference the same id.
- `PermissionSubject::CommandExec` authorizes command execution only. It must not authorize process network.
- `issue_process_network_grant` authorizes only `NetworkAccess::Unrestricted` until an enforcing process loopback or allowlist backend exists.
- A grant must be issued by the runtime's `ProcessGrantIssuer` and validated by the matching `ProcessGrantVerifier` in `ExecContext`.
- The grant must not be accepted from frontend payloads, plugin payloads, MCP payloads, or serialized test fixtures.
- Process execution with `NetworkAccess::Unrestricted` must fail closed when the grant is missing, has an invalid proof, is mismatched, expired, or for a different fingerprint.
- The execution-started event may include the decision id and network summary, but not raw private paths or secret values.

### Shell Command Model

`BashTool` input remains user-facing shell script text. Execution must be explicit:

```text
input.command = user shell script
ExecSpec.command = /bin/sh
ExecSpec.args = ["-lc", input.command]
PermissionSubject::CommandExec.command = input.command
PermissionSubject::CommandExec.argv = ["/bin/sh", "-lc", input.command]
DecisionScope::ExactCommand.command = input.command
```

Dangerous pattern detection runs on the user shell script, not on `/bin/sh`.

### Phase Separation

Setup and agent execution are distinct policies.

- Setup phase can have explicit network and explicit secret env allowlist.
- Agent phase defaults to no network and no secret env.
- Secret env allowlist is copied into the child environment only after redaction-safe validation.
- Setup output is redacted before Journal and Replay.
- Agent phase cannot read setup-only secret files or env values.

Setup lifecycle:

```text
1. Runtime builds a SetupPlan from explicit trusted configuration. Desktop default start_run has no SetupPlan.
2. Setup runs before agent execution in a separate sandbox execution with SandboxPhase::Setup.
3. Setup receives only base passthrough env plus exact secret allowlist names.
4. Setup output and setup artifact metadata pass through Redactor before Journal, Replay, events, logs, traces, and exports.
5. Setup artifacts are copied into the agent workspace only through ResolvedFilesystemPolicy and never include setup env, setup secret files, or denied paths.
6. Agent execution rebuilds env from scratch with SandboxPhase::Agent and SecretAccessPolicy::None.
```

If no real setup entrypoint exists in current product code, Task 6 must implement the typed lifecycle and prove every existing run path uses an empty SetupPlan. It must not add a dead "future" constructor that is never validated.

### Diagnostics

Add a typed sandbox explain payload:

```rust
pub struct SandboxExplainPayload {
    pub backend_id: String,
    pub mode: SandboxMode,
    pub phase: SandboxPhase,
    pub filesystem: FilesystemPolicySummary,
    pub network: NetworkPolicySummary,
    pub secrets: SecretPolicySummary,
    pub capabilities: SandboxCapabilitySummary,
    pub fail_closed_reasons: Vec<String>,
}
```

Rules:

- Payload must be user-safe.
- It must not include raw private absolute paths.
- It may include path categories such as `workspace_root`, `workspace_subpath:.git`, `home_subpath:.ssh`.
- It must be available to backend tests and desktop execution settings.
- Saving execution settings must not require a live sandbox binary probe. Runtime availability probing belongs to `get_execution_settings` or an explicit status command, not to `set_execution_settings`.

## File Map

Create:

- `docs/architecture/harness/crates/harness-sandbox.md`
- `crates/jyowo-harness-sandbox/src/filesystem_policy.rs`
- `crates/jyowo-harness-sandbox/src/process_authorization.rs`
- `crates/jyowo-harness-sandbox/src/policy_merge.rs`
- `crates/jyowo-harness-sandbox/src/setup_plan.rs`
- `crates/jyowo-harness-sandbox/src/workspace_staging.rs`
- `crates/jyowo-harness-sandbox/src/explain.rs`
- `crates/jyowo-harness-sandbox/tests/filesystem_policy.rs`
- `crates/jyowo-harness-sandbox/tests/process_authorization.rs`
- `crates/jyowo-harness-sandbox/tests/policy_merge.rs`
- `crates/jyowo-harness-sandbox/tests/setup_plan.rs`
- `crates/jyowo-harness-sandbox/tests/workspace_staging.rs`
- `crates/jyowo-harness-sandbox/tests/explain.rs`
- `crates/jyowo-harness-sandbox/tests/phase_policy.rs`
- `docs/superpowers/audits/sandbox-hardening/task-1.md` through `task-12.md`

Modify:

- `crates/jyowo-harness-contracts/src/enums.rs`
- `crates/jyowo-harness-contracts/src/events/types.rs`
- `crates/jyowo-harness-contracts/src/schema_export.rs`
- `crates/jyowo-harness-contracts/tests/m1_contracts.rs`
- `crates/jyowo-harness-contracts/tests/provider_capability_routes.rs` only if schema export helpers require broad snapshot updates
- `crates/jyowo-harness-sandbox/src/backend.rs`
- `crates/jyowo-harness-sandbox/src/lib.rs`
- `crates/jyowo-harness-sandbox/src/local/mod.rs`
- `crates/jyowo-harness-sandbox/src/local/exec.rs`
- `crates/jyowo-harness-sandbox/src/docker.rs`
- `crates/jyowo-harness-sandbox/src/ssh.rs`
- `crates/jyowo-harness-permission/src/broker.rs`
- `crates/jyowo-harness-permission/src/dedup.rs`
- `crates/jyowo-harness-permission/src/aux_llm.rs`
- `crates/jyowo-harness-permission/src/chain.rs`
- `crates/jyowo-harness-permission/src/direct.rs`
- `crates/jyowo-harness-permission/src/rule_engine.rs`
- `crates/jyowo-harness-permission/src/stream.rs`
- `crates/jyowo-harness-permission/src/testing.rs`
- `crates/jyowo-harness-sandbox/tests/local.rs`
- `crates/jyowo-harness-sandbox/tests/docker.rs`
- `crates/jyowo-harness-sandbox/tests/ssh.rs`
- `crates/jyowo-harness-permission/tests/contract.rs`
- `crates/jyowo-harness-tool/src/builtin/bash.rs`
- `crates/jyowo-harness-tool/src/context.rs`
- `crates/jyowo-harness-tool/src/orchestrator.rs`
- `crates/jyowo-harness-tool/tests/builtin_exec.rs`
- `crates/jyowo-harness-tool/tests/orchestrator.rs`
- `crates/jyowo-harness-engine/src/engine.rs`
- `crates/jyowo-harness-engine/src/turn.rs`
- `crates/jyowo-harness-agent-runtime/src/policy.rs`
- `crates/jyowo-harness-agent-runtime/src/subagents.rs`
- `crates/jyowo-harness-agent-runtime/src/teams.rs`
- `crates/jyowo-harness-agent-runtime/src/background.rs`
- `crates/jyowo-harness-team/src/lib.rs`
- `crates/jyowo-harness-session/src/turn.rs`
- `crates/jyowo-harness-sdk/src/builder.rs`
- `crates/jyowo-harness-sdk/src/builtin.rs`
- `crates/jyowo-harness-sdk/src/ext.rs`
- `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- `crates/jyowo-harness-sdk/src/harness/permissions.rs`
- `crates/jyowo-harness-sdk/src/harness/tool_pool.rs`
- `crates/jyowo-harness-sdk/tests/agents_team.rs`
- `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`
- `apps/desktop/src-tauri/src/commands/mod.rs`
- `apps/desktop/src-tauri/src/commands/runtime.rs`
- `apps/desktop/src-tauri/src/commands/conversations.rs`
- `apps/desktop/src-tauri/src/commands/providers.rs`
- `apps/desktop/src-tauri/src/commands/background_agents.rs`
- `apps/desktop/src-tauri/src/lib.rs`
- `apps/desktop/src-tauri/tests/commands.rs`
- `apps/desktop/src-tauri/tests/commands/runs_permissions.rs`
- `apps/desktop/src-tauri/tests/commands/providers.rs`
- `apps/desktop/src-tauri/tests/commands/background_agents.rs`
- `apps/desktop/src-tauri/tests/commands/automations.rs` only if Task 2 intentionally migrates automation workspace policy.
- `apps/desktop/src/shared/tauri/commands.ts`
- `apps/desktop/src/shared/tauri/commands.test.ts`
- `apps/desktop/src/testing/command-client.ts`
- `apps/desktop/src/features/settings/ExecutionSettings.tsx`
- `apps/desktop/src/features/settings/ExecutionSettings.test.tsx`
- `docs/backend/backend-runtime.md`
- `docs/backend/backend-engineering.md`
- `docs/backend/backend-quality.md`
- `docs/frontend/frontend-engineering.md`
- `docs/frontend/frontend-quality.md`
- `scripts/check-backend-docs.mjs`

Do not create a new crate unless a task's entry analysis proves the existing L1 sandbox crate cannot own the code without circular dependencies.

## Current Refactored Module Layout

The desktop command design is modular. Do not recreate or target the old monolithic `apps/desktop/src-tauri/src/commands.rs` file.

- `apps/desktop/src-tauri/src/commands/mod.rs` owns public `#[tauri::command]` wrappers, command re-exports, and IPC boundary validation that remains in the shell.
- `apps/desktop/src-tauri/src/commands/runtime.rs` owns `build_desktop_harness`, desktop sandbox construction, diagnostics runner assembly, and plugin sidecar sandbox assembly.
- `apps/desktop/src-tauri/src/commands/conversations.rs` owns `start_run_with_runtime_state`, conversation run validation, permission resolution routing, and conversation runtime calls.
- `apps/desktop/src-tauri/src/commands/providers.rs` owns execution settings persistence, provider settings, provider capability routes, and `GetExecutionSettingsResponse` / `SetExecutionSettingsResponse` construction.
- `apps/desktop/src-tauri/src/lib.rs` owns `tauri::generate_handler!` registration when a new command is exposed.
- command tests are split under `apps/desktop/src-tauri/tests/commands/*.rs`; edit root `apps/desktop/src-tauri/tests/commands.rs` only for shared test helpers, imports, or registering a new test module.

The SDK facade design is also modular. Do not push new implementation into a monolithic `crates/jyowo-harness-sdk/src/harness.rs`; that file is now the module root.

- `crates/jyowo-harness-sdk/src/harness/session_runtime.rs` owns session engine assembly, `Engine::builder()`, `.with_sandbox(...)`, and `EngineSessionTurnRunner`.
- `crates/jyowo-harness-sdk/src/harness/conversation.rs` owns conversation-session facade methods such as `submit_conversation_turn`.
- `crates/jyowo-harness-sdk/src/harness/permissions.rs` owns stream permission facade wiring.
- `crates/jyowo-harness-sdk/src/harness/accessors.rs` owns facade accessors and feature availability reporting.
- `crates/jyowo-harness-sdk/src/harness/tool_pool.rs` owns tool filtering and ToolPool assembly helpers.

---

## Task 1: Write The Normative Sandbox Spec

**Purpose:** Lock the design before code changes so implementation agents do not invent sandbox semantics.

**Files:**

- Create: `docs/architecture/harness/crates/harness-sandbox.md`
- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`
- Modify: `scripts/check-backend-docs.mjs`

- [ ] **Step 1: Task intent check**

Write the required Task 1 intent note. Include the exact invariant: "sandbox policy is a Rust-owned security boundary; unsupported policy fails closed."

- [ ] **Step 2: Add the sandbox spec**

Create `docs/architecture/harness/crates/harness-sandbox.md` with these required sections:

```text
# harness-sandbox

## Ownership
## Threat Model
## Policy Model
## Filesystem Policy
## Network Policy
## Secret Policy
## Setup Phase And Agent Phase
## Local Backend
## Docker Backend
## SSH Backend
## Tool Integration
## Diagnostics
## Failure Defaults
## Test Requirements
```

The document must state:

- `PermissionBroker` authorizes operations; sandbox enforces process boundaries.
- Tool allow rules are not filesystem isolation.
- `NetworkAccess::AllowList` for arbitrary processes must fail closed until an enforcing backend exists.
- Windows `JobObject` is not a filesystem or network sandbox.
- Windows desktop must keep settings readable but local agent Bash/process execution must be unavailable unless an enforcing backend is selected.
- Setup phase may use explicit secrets; agent phase defaults to no secrets.
- Protected paths are denied even when workspace root is writable.
- Writable workspaces with protected dynamic denies require native future-create enforcement or staged workspace copy-back with manifest preflight, conflict detection, and redacted partial-apply errors.
- `NetworkAccess::Unrestricted` for arbitrary processes requires a runtime-only `ProcessNetworkGrant` derived from a separate `PermissionSubject::ProcessNetworkAccess` allow decision.
- `PermissionSubject::CommandExec` does not authorize process network.
- Public Rust contract payloads reject unknown fields.
- Each task audit record is stored under `docs/superpowers/audits/sandbox-hardening/`.

- [ ] **Step 3: Update backend docs**

Add sandbox-specific wording to:

- `docs/backend/backend-runtime.md`: Rust owns sandbox policy and phase separation.
- `docs/backend/backend-engineering.md`: `jyowo-harness-sandbox` owns filesystem, network, secret, and backend enforcement policy.
- `docs/backend/backend-quality.md`: add required tests for policy compilation, unsupported capability failure, protected path denies, network fail-closed, setup/agent secret separation, and diagnostics.

- [ ] **Step 4: Enforce docs gate**

Update `scripts/check-backend-docs.mjs` so the missing `docs/architecture/harness/crates/harness-sandbox.md` file is checked when sandbox docs are referenced.

If the docs gate rejects new architecture docs as a policy violation, do not bypass the gate. Move the normative sandbox content into the existing backend docs and update this plan before implementation continues.

- [ ] **Step 5: Run gates**

```bash
pnpm check:backend-docs
pnpm check:docs
```

Expected: both exit 0.

- [ ] **Step 6: Task exit analysis and subagent audits**

Run the required exit analysis, code-review-expert audit, and security-review audit.

- [ ] **Step 7: Commit**

```bash
git add docs/architecture/harness/crates/harness-sandbox.md docs/backend/backend-runtime.md docs/backend/backend-engineering.md docs/backend/backend-quality.md scripts/check-backend-docs.mjs docs/superpowers/audits/sandbox-hardening/task-1.md
git commit -m "docs(sandbox): define fail-closed sandbox policy"
```

## Task 2: Replace Split Filesystem Authority With A Single Policy

**Purpose:** Remove duplicated policy state so filesystem authority has one source of truth.

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/enums.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/types.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `crates/jyowo-harness-contracts/tests/m1_contracts.rs`
- Modify: `crates/jyowo-harness-sandbox/src/backend.rs`
- Create: `crates/jyowo-harness-sandbox/src/filesystem_policy.rs`
- Modify: `crates/jyowo-harness-sandbox/src/lib.rs`
- Test: `crates/jyowo-harness-sandbox/tests/filesystem_policy.rs`
- Modify only if automation workspace policy is intentionally migrated: `crates/jyowo-harness-contracts/src/automation.rs`
- Modify only if automation workspace policy is intentionally migrated: `apps/desktop/src-tauri/src/commands/validation.rs`

- [ ] **Step 1: Task intent check**

Write the Task 2 intent note. Include the exact removed behavior: "`ExecSpec.workspace_access`, `SandboxPolicy.scope`, and `SandboxPolicy.denied_host_paths` must stop being independent authority."

- [ ] **Step 2: Write failing contract tests**

Add tests proving:

- `SandboxPolicy::default()` serializes with `phase`, `filesystem`, `network`, `secrets`, and `resource_limits`.
- `FilesystemPermission::Deny` beats workspace write.
- `WorkspaceSubtree(".git")` denies `.git/config` even when workspace root is writable.
- `WorkspaceRootFilePrefix(".env.")` denies both existing and future `.env.production`.
- `SecretAccessPolicy::None` is the default.
- old `workspace_access`, `scope`, and `denied_host_paths` fields are not emitted by new serialization.
- deserializing `SandboxPolicy` with unknown fields fails.
- deserializing old `workspace_access`, `scope`, or `denied_host_paths` as authoritative policy fails.
- exported schemas for new sandbox contract structs use `additionalProperties: false` or the repository's equivalent strict-object schema encoding.
- `SandboxExecutionStartedEvent`, Journal replay payloads, and exported schemas use the new policy summary shape.
- Any existing test fixture containing old sandbox policy fields is intentionally updated or removed in the same task.

Run:

```bash
cargo test -p jyowo-harness-contracts sandbox -- --nocapture
```

Expected before implementation: FAIL.

- [ ] **Step 3: Implement contract shapes**

Add the target contract types from "Single Policy Source". Update `SandboxPolicy` to:

```rust
#[serde(deny_unknown_fields)]
pub struct SandboxPolicy {
    pub mode: SandboxMode,
    pub phase: SandboxPhase,
    pub filesystem: FilesystemPolicy,
    pub network: NetworkAccess,
    pub secrets: SecretAccessPolicy,
    pub resource_limits: ResourceLimits,
}
```

Remove `SandboxScope` only if no other crate owns a public non-sandbox use. If it remains for subagent compatibility, it must not appear in `SandboxPolicy`.

`FilesystemSelector` must include these exact variants:

```rust
WorkspaceRoot,
WorkspaceSubpath(PathBuf),
WorkspaceSubtree(PathBuf),
WorkspaceRootFilePrefix(String),
TempDir,
HomeSubpath(PathBuf),
HomeSubtree(PathBuf),
Absolute(PathBuf),
```

Do not encode `.env.*` as `WorkspaceSubpath(".env.*")`.

- [ ] **Step 4: Update `ExecSpec`**

Remove:

```rust
pub workspace_access: WorkspaceAccess
```

from `ExecSpec`.

This removal applies to process sandbox authority. `Automation.workspace_access` is a separate scheduler contract and must not be treated as process sandbox authority. If Task 2 keeps `Automation.workspace_access`, document that boundary in the Task 2 audit file and keep the final grep gate scoped so automation validation is not a false positive. If Task 2 migrates automation too, migrate it explicitly with contract tests; do not remove it as a side effect of fixing `ExecSpec`.

Update fingerprinting to hash:

```text
command
args
filtered env
cwd
SandboxPolicy.phase
SandboxPolicy.filesystem
SandboxPolicy.network
SandboxPolicy.secrets shape
```

Do not hash raw secret env values.

- [ ] **Step 5: Implement policy resolution**

Create `filesystem_policy.rs` with:

```rust
pub struct ResolvedFilesystemPolicy {
    pub readable_paths: Vec<PathBuf>,
    pub writable_paths: Vec<PathBuf>,
    pub denied_matchers: Vec<ResolvedDenyMatcher>,
}

pub enum ResolvedDenyMatcher {
    Exact(PathBuf),
    Subtree(PathBuf),
    WorkspaceRootFilePrefix {
        workspace_root: PathBuf,
        prefix: String,
    },
}
```

Add functions:

```rust
pub fn default_protected_rules() -> Vec<FilesystemRule>;

pub fn resolve_filesystem_policy(
    workspace_root: &Path,
    temp_dir: &Path,
    home_dir: Option<&Path>,
    policy: &FilesystemPolicy,
) -> Result<ResolvedFilesystemPolicy, SandboxError>;

pub fn ensure_path_allowed(
    resolved: &ResolvedFilesystemPolicy,
    path: &Path,
    required: FilesystemPermission,
) -> Result<(), SandboxError>;
```

Rules:

- Deny wins.
- Write implies read.
- Relative path selectors are rejected.
- Empty absolute path is rejected.
- `WorkspaceSubtree` and `HomeSubtree` match descendants without requiring the final path to exist.
- `WorkspaceRootFilePrefix` accepts only ASCII file-name prefixes with no slash, no backslash, no `..`, and no empty string.
- Symlink-parent escape is rejected.
- Private absolute paths are redacted in returned `SandboxError`.
- `ensure_path_allowed` checks lexical normalized paths before creation, then canonicalizes existing parents to reject symlink-parent escape.
- `ResolvedFilesystemPolicy` is an internal enforcement type. Do not derive `Serialize`, `Deserialize`, or `JsonSchema` for it.
- `ResolvedFilesystemPolicy` and `ResolvedDenyMatcher` must not be emitted through events, Journal, Replay, logs, traces, frontend payloads, screenshots, snapshots, or fixtures.
- User-facing diagnostics must use a separate redacted summary type, such as `FilesystemPolicySummary`, with path categories only.
- Tests must fail if a raw private absolute path from `readable_paths`, `writable_paths`, or `denied_matchers` appears in any sandbox error, explain payload, event snapshot, or frontend fixture.

- [ ] **Step 6: Update all compile errors intentionally**

Update call sites in sandbox, engine, SDK, tool, plugin, and subagent code to use `spec.policy.filesystem`.

Also update agent runtime, desktop run assembly, and any team-run construction path that currently derives filesystem authority from `workspace_access`, `SandboxScope`, or `denied_host_paths`.

Do not add adapter methods named like `workspace_access_compat`.

- [ ] **Step 7: Update event, replay, and schema boundaries**

Required changes:

```text
SandboxExecutionStartedEvent.policy must serialize the new policy summary shape.
Journal and Replay readers must not synthesize old workspace_access/scope/denied_host_paths authority.
Schema export tests must fail if old fields reappear.
Schema export tests must fail if new public sandbox objects allow unknown fields.
Existing old fixtures may be deleted or rewritten because this project is in development.
If any persisted migration is still required by current tests, write a one-way migration test and keep it outside runtime policy authority.
```

- [ ] **Step 8: Run gates**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-contracts sandbox -- --nocapture
cargo test -p jyowo-harness-contracts m1_contracts -- --nocapture
cargo test -p jyowo-harness-sandbox filesystem_policy -- --nocapture
cargo test -p jyowo-harness-sandbox filesystem_policy_default_protected_future_env_create -- --nocapture
pnpm check:rust
```

Expected: all exit 0.

- [ ] **Step 9: Task exit analysis and subagent audits**

Run the required exit analysis, code-review-expert audit, and security-review audit.

- [ ] **Step 10: Commit**

```bash
git add crates/jyowo-harness-contracts crates/jyowo-harness-sandbox crates/jyowo-harness-engine crates/jyowo-harness-sdk crates/jyowo-harness-tool crates/jyowo-harness-plugin crates/jyowo-harness-subagent docs/superpowers/audits/sandbox-hardening/task-2.md
git commit -m "refactor(sandbox): centralize filesystem policy"
```

## Task 3: Compile Local OS Isolation Fail-Closed

**Purpose:** Make local sandbox startup enforce real OS isolation by default and reject unsupported host capabilities.

**Files:**

- Modify: `crates/jyowo-harness-sandbox/src/local/mod.rs`
- Modify: `crates/jyowo-harness-sandbox/src/local/exec.rs`
- Modify: `crates/jyowo-harness-sandbox/src/backend.rs`
- Modify: `crates/jyowo-harness-sandbox/tests/local.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Test: `apps/desktop/src-tauri/tests/commands/runs_permissions.rs`

- [ ] **Step 1: Task intent check**

Write the Task 3 intent note. Include this invariant: "main desktop Harness must not use `LocalIsolation::None` for agent Bash execution."

- [ ] **Step 2: Write failing tests**

Add tests proving:

- `LocalSandbox::new(workspace)` remains a low-level constructor but `LocalSandbox::required_for_current_platform(workspace)` returns OS isolation or `SandboxError::Unavailable`.
- `LocalIsolation::JobObject` does not satisfy filesystem or network isolation.
- `NetworkAccess::None` with `LocalIsolation::None` fails closed.
- Desktop harness assembly uses `LocalSandbox::required_for_current_platform`.
- On Windows, desktop command assembly returns a typed unavailable error for local process execution and does not crash settings reads.
- Plugin sidecar and main agent use the same isolation resolver.
- A command that tries to create `.env.production` under writable workspace cannot create or modify that file on the host.
- staged workspace copy-back aborts before host mutation when any manifest entry is denied.
- staged workspace copy-back aborts on host digest/metadata conflict and reports only redacted path categories.
- staged workspace file replacement uses a temp path under the destination parent and atomic rename where supported.
- staged workspace rejects symlink, hardlink, device, fifo, socket, and special-file entries.
- Seatbelt and bubblewrap compiler tests snapshot the safe profile or argv shape without private absolute paths.

Run:

```bash
cargo test -p jyowo-harness-sandbox local_sandbox -- --nocapture
cargo test -p jyowo-harness-sandbox workspace_staging -- --nocapture
cargo test -p jyowo-desktop-shell sandbox -- --nocapture
```

Expected before implementation: FAIL.

- [ ] **Step 3: Add required constructor**

Add:

```rust
impl LocalSandbox {
    pub fn required_for_current_platform(root: impl Into<PathBuf>) -> Result<Self, SandboxError> {
        let isolation = LocalIsolation::for_current_platform();
        let sandbox = Self::new(root).with_isolation(isolation);
        sandbox.validate_startup_policy()?;
        Ok(sandbox)
    }
}
```

`validate_startup_policy` must check host binaries for Seatbelt and bubblewrap and must reject `LocalIsolation::None`.

Windows rule:

```rust
#[cfg(target_os = "windows")]
pub fn required_for_current_platform(root: impl Into<PathBuf>) -> Result<Self, SandboxError> {
    Err(SandboxError::Unavailable {
        backend: "local".to_owned(),
        reason: "local process sandbox does not enforce filesystem or network policy on Windows".to_owned(),
    })
}
```

Desktop must convert this into a safe `CommandErrorPayload` for local agent/process start. Settings read/write commands must remain usable.

- [ ] **Step 4: Split isolation capabilities**

Replace `LocalIsolation::is_os_level()` with explicit methods:

```rust
pub(crate) fn enforces_filesystem_policy(self) -> bool;
pub(crate) fn enforces_network_none(self) -> bool;
pub(crate) fn enforces_process_lifetime(self) -> bool;
```

Expected:

```text
Bubblewrap: filesystem yes, network none yes, lifetime yes
Seatbelt: filesystem yes, network none yes, lifetime yes
JobObject: filesystem no, network none no, lifetime yes
None: filesystem no, network none no, lifetime no
```

- [ ] **Step 5: Compile local filesystem rules**

Update Seatbelt and bubblewrap compilers to use `ResolvedFilesystemPolicy`.

For bubblewrap:

- bind system root read-only only when needed for executable runtime.
- bind temp according to resolved policy.
- bind workspace read-write only if the backend proves every deny selector under workspace write is enforced for future creates.
- otherwise run against a staged workspace or overlay and copy allowed changes back through `ensure_path_allowed`.
- hide existing denied paths with empty read-only mounts only as a defense-in-depth measure.
- fail closed when neither native enforcement nor staged copy-back is available.

For Seatbelt:

- deny default.
- allow process execution.
- allow read according to resolved readable paths.
- allow write according to resolved writable paths.
- add explicit deny for resolved exact and subtree deny matchers.
- use a staged workspace or overlay when deny selectors cannot be proven to block future creates under a writable parent.

- [ ] **Step 6: Implement staged workspace copy-back**

Create `crates/jyowo-harness-sandbox/src/workspace_staging.rs`.

Required API:

```rust
pub struct WorkspaceBaseline {
    pub entries: BTreeMap<WorkspaceRelativePath, WorkspaceBaselineEntry>,
}

pub struct WorkspaceBaselineEntry {
    pub id: WorkspaceBaselineEntryId,
    pub kind: WorkspaceBaselineEntryKind,
}

pub enum WorkspaceBaselineEntryKind {
    File { digest: ContentDigest, executable: bool },
    Directory,
}

pub struct WorkspaceChangeManifest {
    pub entries: Vec<WorkspaceChangeEntry>,
}

pub struct WorkspaceBaselineEntryId(u64);
pub struct WorkspaceManifestEntryId(u64);

pub struct WorkspaceChangeEntry {
    pub id: WorkspaceManifestEntryId,
    pub change: WorkspaceChange,
}

pub enum WorkspaceChange {
    CreateFile { path: WorkspaceRelativePath, staged_digest: ContentDigest, staged_executable: bool },
    ModifyFile {
        path: WorkspaceRelativePath,
        baseline_id: WorkspaceBaselineEntryId,
        before_digest: ContentDigest,
        before_executable: bool,
        staged_digest: ContentDigest,
        staged_executable: bool,
    },
    DeleteFile {
        path: WorkspaceRelativePath,
        baseline_id: WorkspaceBaselineEntryId,
        before_digest: ContentDigest,
        before_executable: bool,
    },
    CreateDirectory { path: WorkspaceRelativePath },
    RemoveDirectory { path: WorkspaceRelativePath, baseline_id: WorkspaceBaselineEntryId },
}

pub struct WorkspaceRelativePath(PathBuf);
pub struct ContentDigest([u8; 32]);

pub struct WorkspaceStage {
    pub staged_root: PathBuf,
    pub host_root: PathBuf,
    pub baseline: WorkspaceBaseline,
}

pub fn stage_workspace_for_policy(
    host_root: &Path,
    temp_root: &Path,
    policy: &ResolvedFilesystemPolicy,
) -> Result<WorkspaceStage, SandboxError>;

pub fn build_workspace_change_manifest(
    stage: &WorkspaceStage,
    policy: &ResolvedFilesystemPolicy,
) -> Result<WorkspaceChangeManifest, SandboxError>;

pub fn copy_allowed_changes_back(
    stage: &WorkspaceStage,
    policy: &ResolvedFilesystemPolicy,
) -> Result<(), SandboxError>;
```

Rules:

- The stage must not copy `.git`, `.jyowo/runtime/**`, `.env`, `.env.local`, `.env.*`, or home credential paths.
- Copy-back must call `ensure_path_allowed` for every created, modified, deleted, renamed, symlink, and hardlink candidate.
- `WorkspaceRelativePath` must reject absolute paths, `..`, empty components, Windows drive prefixes, and platform path separators inside file names.
- `ContentDigest` must be computed from regular file bytes only. Conflict tests must use digest/metadata, not timestamps.
- Every manifest non-create mutation must reference a captured `WorkspaceBaselineEntryId`.
- Copy-back preflight must reject host-state mismatches for digest, executable bit, directory/file kind, existence, or baseline id before mutating any host path.
- Symlinks and hardlinks pointing outside the staged workspace fail closed.
- Delete operations against denied host paths fail closed.
- Private absolute host paths must be redacted in errors.
- `copy_allowed_changes_back` must first build and validate a `WorkspaceChangeManifest`; if any entry is denied or conflicts with current host state, no host path is mutated.
- File writes must use temp files under the destination parent plus atomic rename. Directory creation/removal must be ordered from the manifest and stop on first error.
- Host-side conflict detection must compare captured baseline entry kind, digest, executable bit, and id with current host state before each mutation.
- Partial apply errors must identify only a redacted path category and manifest entry id. They must not include raw absolute paths.

- [ ] **Step 7: Wire desktop main Harness**

In `apps/desktop/src-tauri/src/commands/runtime.rs`, replace the current main harness sandbox construction:

```rust
let sandbox = Arc::new(LocalSandbox::new(workspace_root)) as Arc<dyn SandboxBackend>;
```

with a helper:

```rust
let (main_sandbox, main_sandbox_mode) = desktop_required_local_sandbox(workspace_root)?;
```

The helper must return `CommandErrorPayload` on unavailable isolation. Pass `main_sandbox` into `.with_sandbox_arc(main_sandbox)`. Use `main_sandbox_mode` for diagnostics/explain payloads or an explicitly named runtime state field; do not compute a second sandbox mode through a parallel path. The user-facing message must be safe and must not include private absolute paths.

- [ ] **Step 8: Run gates**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-sandbox --features local local -- --nocapture
cargo test -p jyowo-harness-sandbox workspace_staging -- --nocapture
cargo test -p jyowo-desktop-shell sandbox -- --nocapture
pnpm check:rust
```

Expected: all exit 0.

- [ ] **Step 9: Task exit analysis and subagent audits**

Run the required exit analysis, code-review-expert audit, and security-review audit.

- [ ] **Step 10: Commit**

```bash
git add crates/jyowo-harness-sandbox apps/desktop/src-tauri docs/superpowers/audits/sandbox-hardening/task-3.md
git commit -m "feat(sandbox): require local OS isolation"
```

## Task 4: Enforce Process Network Policy Without False Allowlist Claims

**Purpose:** Make process network behavior explicit, enforce `None`, and fail closed for unimplemented process allowlists.

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/enums.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/types.rs`
- Modify: `crates/jyowo-harness-sandbox/src/backend.rs`
- Create: `crates/jyowo-harness-sandbox/src/process_authorization.rs`
- Modify: `crates/jyowo-harness-sandbox/src/local/exec.rs`
- Modify: `crates/jyowo-harness-sandbox/src/docker.rs`
- Modify: `crates/jyowo-harness-sandbox/src/ssh.rs`
- Modify: `crates/jyowo-harness-permission/src/broker.rs`
- Modify: `crates/jyowo-harness-permission/src/dedup.rs`
- Modify: `crates/jyowo-harness-permission/src/aux_llm.rs`
- Modify: `crates/jyowo-harness-permission/src/chain.rs`
- Modify: `crates/jyowo-harness-permission/src/direct.rs`
- Modify: `crates/jyowo-harness-permission/src/rule_engine.rs`
- Modify: `crates/jyowo-harness-permission/src/stream.rs`
- Modify: `crates/jyowo-harness-permission/src/testing.rs`
- Modify: `crates/jyowo-harness-tool/src/context.rs`
- Modify: `crates/jyowo-harness-tool/src/orchestrator.rs`
- Modify: `crates/jyowo-harness-engine/src/turn.rs`
- Modify: `crates/jyowo-harness-session/src/turn.rs`
- Modify: `crates/jyowo-harness-sdk/src/builder.rs`
- Modify: `crates/jyowo-harness-sdk/src/ext.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/permissions.rs`
- Modify: `crates/jyowo-harness-subagent/src/lib.rs`
- Modify: `crates/jyowo-harness-mcp/src/sampling.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/src/commands/conversations.rs`
- Test: `crates/jyowo-harness-sandbox/tests/process_authorization.rs`
- Modify: `crates/jyowo-harness-sandbox/tests/local.rs`
- Modify: `crates/jyowo-harness-sandbox/tests/docker.rs`
- Modify: `crates/jyowo-harness-sandbox/tests/ssh.rs`
- Modify: `crates/jyowo-harness-permission/tests/contract.rs`
- Modify: `crates/jyowo-harness-tool/tests/orchestrator.rs`
- Modify: `crates/jyowo-harness-tool/tests/builtin_exec.rs`
- Modify: `crates/jyowo-harness-sdk/tests/facade.rs`
- Modify: `crates/jyowo-harness-sdk/tests/agents_team.rs`
- Modify: `crates/jyowo-harness-subagent/tests/permission_bridge.rs`
- Modify: `crates/jyowo-harness-mcp/tests/sampling.rs`
- Modify: `crates/jyowo-harness-agent-runtime/tests/subagents.rs`
- Modify affected desktop command tests under `apps/desktop/src-tauri/tests/commands`
- Modify affected `PermissionBroker` test implementations under `crates/jyowo-harness-tool-search/tests`

- [ ] **Step 1: Task intent check**

Write the Task 4 intent note. Include this invariant: "`AllowList` must not be simulated by env vars or permission prompts for arbitrary processes."

- [ ] **Step 2: Write failing tests**

Add tests proving:

- Local bubblewrap compiles `NetworkAccess::None` to `--unshare-net`.
- Local Seatbelt compiles `NetworkAccess::None` without `(allow network*)`.
- Local `LoopbackOnly` and `AllowList` return `SandboxError::CapabilityMismatch`.
- Docker agent-phase default produces `--network none`.
- SSH rejects `NetworkAccess::None`, `LoopbackOnly`, and `AllowList` unless remote backend enforcement exists.
- `NetworkAccess::Unrestricted` fails closed without a matching `ProcessNetworkGrant`.
- `PermissionSubject::CommandExec` allow does not authorize `NetworkAccess::Unrestricted`.
- process network grant is produced only after an allow decision for `PermissionSubject::ProcessNetworkAccess` with the exact command fingerprint.
- the `ProcessNetworkAccess` decision returns a stable `DecisionId` and the same id is used in `ProcessNetworkGrant`, `PermissionResolved`, `ToolUseApproved`, and persisted permission records.
- no production path creates `DecisionId::new()` after the broker has already returned a permission decision record.
- A grant for a different command fingerprint does not authorize process network.
- An expired grant does not authorize process network.
- A grant signed by a different `ProcessGrantIssuer` does not authorize process network.
- `issue_process_network_grant` rejects every `NetworkAccess` value except `NetworkAccess::Unrestricted` until a backend can enforce process loopback or allowlist policy.
- `NetworkAccess::Unrestricted` with a matching grant remains visible in `SandboxExecutionStarted`.

Run:

```bash
cargo test -p jyowo-harness-sandbox process_network_policy -- --nocapture
cargo test -p jyowo-harness-sandbox process_authorization -- --nocapture
cargo test -p jyowo-harness-permission process_network_grant -- --nocapture
cargo test -p jyowo-harness-permission permission_decision_id -- --nocapture
cargo test -p jyowo-harness-tool process_network_orchestration -- --nocapture
```

Expected before implementation: FAIL.

- [ ] **Step 3: Add network capability summary**

Extend `SandboxCapabilities` with:

```rust
pub supports_network_none: bool,
pub supports_network_loopback_only: bool,
pub supports_network_allowlist: bool,
pub supports_network_unrestricted: bool,
```

Update all backend capability implementations.

Remove the old broad `supports_network` field from process-sandbox capability checks. If any non-process feature still needs a broad network capability, give it a different domain-specific name and keep it out of arbitrary process policy validation.

Update `RequiredSandboxCapabilities` and every call site that currently reads `supports_network` to use the precise network capability required by the policy. `NetworkAccess::None` must check `supports_network_none`; `NetworkAccess::Unrestricted` must check `supports_network_unrestricted` plus a verified process grant; `LoopbackOnly` and `AllowList` must check their own precise flags and fail closed while those flags are false.

Add a review assertion that production code has no broad `supports_network` matches after this task, except renamed non-process capability text that the task explicitly documents. The assertion must exclude tests, fixtures, generated output, docs, and comments-only audit text; it must not fail because migration tests still mention the removed field.

- [ ] **Step 4: Return stable permission decision records**

Change the permission decision API so trusted orchestration receives the decision and its identity together:

```rust
pub struct PermissionDecision {
    pub decision_id: DecisionId,
    pub decision: Decision,
    pub scope: DecisionScope,
    pub fingerprint: ExecFingerprint,
    pub decided_at: DateTime<Utc>,
}
```

Rules:

- `PermissionBroker::decide` must return `PermissionDecision`, not a bare `Decision`.
- Update every `PermissionBroker` implementor and wrapper in the workspace, including permission, SDK, subagent, MCP sampling, tool-search tests, agent-runtime tests, desktop command tests, and any local test broker. Do not add a compatibility trait or adapter returning bare `Decision`.
- `PersistedDecision`, `PermissionResolvedEvent`, `ToolUseApprovedEvent`, permission audit records, and process network grants must use `PermissionDecision.decision_id`.
- Deduplicated or reused allow decisions must carry the original decision id or an explicit `original_decision_id`; they must not allocate a fresh approval id.
- `DecisionId::new()` is allowed only at the point where a new broker decision record is created.
- Engine and session permission event builders must not synthesize approval ids after decision resolution.
- Tests must fail on production matches of `decision_id: DecisionId::new()` inside permission resolved / tool approved event construction.

- [ ] **Step 5: Add non-forgeable process network grants**

Create `crates/jyowo-harness-sandbox/src/process_authorization.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessNetworkGrant {
    decision_id: DecisionId,
    subject_fingerprint: ExecFingerprint,
    granted_network: NetworkAccess,
    expires_at: DateTime<Utc>,
    proof: ProcessGrantProof,
}

#[derive(Debug, Clone)]
pub struct ProcessGrantAuthority {
    pub issuer: ProcessGrantIssuer,
    pub verifier: ProcessGrantVerifier,
}

#[derive(Debug, Clone)]
pub struct ProcessGrantIssuer {
    key_id: ProcessGrantKeyId,
    key: Arc<[u8; 32]>,
}

#[derive(Debug, Clone)]
pub struct ProcessGrantVerifier {
    key_id: ProcessGrantKeyId,
    key: Arc<[u8; 32]>,
}

#[derive(Debug, Clone, PartialEq)]
struct ProcessGrantProof {
    key_id: ProcessGrantKeyId,
    mac: [u8; 32],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExecAuthorization {
    process_network_grant: Option<ProcessNetworkGrant>,
}

impl ProcessGrantIssuer {
    pub fn issue_process_network_grant(
        &self,
        decision_id: DecisionId,
        command_fingerprint: ExecFingerprint,
        granted_network: NetworkAccess,
        expires_at: DateTime<Utc>,
    ) -> Result<ProcessNetworkGrant, SandboxError>;
}

impl ProcessGrantVerifier {
    pub fn verify_process_network_grant(
        &self,
        grant: &ProcessNetworkGrant,
        expected_fingerprint: ExecFingerprint,
        requested_network: &NetworkAccess,
        now: DateTime<Utc>,
    ) -> Result<(), SandboxError>;
}

impl ExecAuthorization {
    pub fn none() -> Self;
    pub fn with_process_network_grant(grant: ProcessNetworkGrant) -> Self;
}
```

Extend `ExecContext` with:

```rust
pub authorization: ExecAuthorization,
pub process_grant_verifier: ProcessGrantVerifier,
```

Rules:

- Do not derive `Serialize`, `Deserialize`, or `JsonSchema` for `ProcessNetworkGrant`.
- `ProcessNetworkGrant`, `ProcessGrantIssuer`, `ProcessGrantVerifier`, and `ProcessGrantProof` fields are private except the `ProcessGrantAuthority` pair returned by runtime assembly.
- The grant issuer is created only by trusted runtime assembly and is not stored in tool, plugin, MCP, frontend, or serialized state.
- Trusted Rust execution orchestration in `crates/jyowo-harness-tool/src/orchestrator.rs` requests both `PermissionSubject::CommandExec` and `PermissionSubject::ProcessNetworkAccess` when `ExecSpec.policy.network == NetworkAccess::Unrestricted`.
- The permission resolution path must create the `DecisionId` once in the returned `PermissionDecision`, persist/emit that decision id, and pass the same id into `issue_process_network_grant`.
- Trusted Rust execution orchestration attaches the grant to `ToolContext`/`ExecContext.authorization` only after `PermissionBroker` returns an allow `PermissionDecision` for `PermissionSubject::ProcessNetworkAccess`.
- `issue_process_network_grant` must reject `NetworkAccess::None`, `NetworkAccess::LoopbackOnly`, and `NetworkAccess::AllowList(_)`. Until an enforcing process allowlist backend exists, runtime grants authorize only `NetworkAccess::Unrestricted`.
- Add an `rg`-backed test or review assertion that production calls to `issue_process_network_grant` exist only after a `ProcessNetworkAccess` allow decision.
- Tool implementations may request `NetworkAccess::Unrestricted` in `ExecSpec.policy.network`, but they must not be able to authorize it.
- Tests must prove frontend, plugin, MCP, and serde payloads cannot construct this grant.

- [ ] **Step 6: Enforce unsupported policies**

Centralize validation:

```rust
pub fn ensure_network_policy_supported(
    backend_id: &str,
    capabilities: &SandboxCapabilities,
    network: &NetworkAccess,
    authorization: &ExecAuthorization,
    verifier: &ProcessGrantVerifier,
    fingerprint: ExecFingerprint,
    now: DateTime<Utc>,
) -> Result<(), SandboxError>
```

Use it in each backend `execute` entry before process spawn. Helpers such as `command_for_execute` may receive a prevalidated policy summary, but must not make authorization decisions without `ExecContext`.

- [ ] **Step 7: Preserve Jyowo-owned HTTP tool behavior**

Do not route WebSearch/WebFetch through process sandbox. They remain explicit network tools checked by `PermissionBroker`.

Add a test in `crates/jyowo-harness-tool/tests/builtin_exec.rs` proving `WebSearchTool` still asks `PermissionSubject::NetworkAccess` and does not change `BashTool` process policy.

- [ ] **Step 8: Run gates**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-sandbox process_network_policy -- --nocapture
cargo test -p jyowo-harness-sandbox process_authorization -- --nocapture
cargo test -p jyowo-harness-permission process_network_grant -- --nocapture
cargo test -p jyowo-harness-permission permission_decision_id -- --nocapture
cargo test -p jyowo-harness-tool process_network_orchestration -- --nocapture
cargo test -p jyowo-harness-tool web_search_uses_network_permission_and_backend -- --nocapture
cargo test -p jyowo-harness-sdk permission_decision_id -- --nocapture
cargo test -p jyowo-harness-subagent permission_bridge -- --nocapture
cargo test -p jyowo-harness-mcp sampling_permission_decision_id -- --nocapture
cargo test -p jyowo-desktop-shell permission_decision_id -- --nocapture
pnpm check:rust
```

Expected: all exit 0.

- [ ] **Step 9: Task exit analysis and subagent audits**

Run the required exit analysis, code-review-expert audit, and security-review audit.

- [ ] **Step 10: Commit**

```bash
git add crates/jyowo-harness-contracts crates/jyowo-harness-sandbox crates/jyowo-harness-permission crates/jyowo-harness-tool crates/jyowo-harness-engine crates/jyowo-harness-session crates/jyowo-harness-sdk crates/jyowo-harness-subagent crates/jyowo-harness-mcp crates/jyowo-harness-tool-search crates/jyowo-harness-agent-runtime apps/desktop/src-tauri docs/superpowers/audits/sandbox-hardening/task-4.md
git commit -m "feat(sandbox): enforce process network capabilities"
```

## Task 5: Fix BashTool Shell Execution Semantics

**Purpose:** Make Bash permission, fingerprinting, and actual process execution refer to the same command.

**Files:**

- Modify: `crates/jyowo-harness-tool/src/builtin/bash.rs`
- Modify: `crates/jyowo-harness-tool/tests/builtin_exec.rs`
- Modify: `crates/jyowo-harness-sandbox/tests/local.rs`

- [ ] **Step 1: Task intent check**

Write the Task 5 intent note. Include this invariant: "Bash input command is a shell script; local backend must never try to execute the entire script string as a program name."

- [ ] **Step 2: Replace fake execution tests**

Update BashTool tests so command mapping is checked against a real `ExecSpec` and real `LocalSandbox` for executable semantics.

Required assertions:

```text
input command: "echo hello"
spec.command: "/bin/sh"
spec.args: ["-lc", "echo hello"]
permission subject command: "echo hello"
permission subject argv: ["/bin/sh", "-lc", "echo hello"]
decision scope command: "echo hello"
dangerous command detection sees "rm -rf /"
```

Do not use `FakeSandbox` for the shell execution behavior test.

- [ ] **Step 3: Implement BashTool mapping**

In `exec_spec` and `exec_spec_for_input`, set:

```rust
command: "/bin/sh".to_owned(),
args: vec!["-lc".to_owned(), command(input)?.to_owned()],
```

Set the default policy to `SandboxPolicy::default_agent_workspace_write()`.

- [ ] **Step 4: Keep user-facing permission exact**

`check_permission` must use the raw user script for:

- dangerous pattern detection
- `PermissionSubject::CommandExec.command`
- `DecisionScope::ExactCommand.command`

`argv` must include the shell wrapper.

- [ ] **Step 5: Run gates**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-tool bash_ -- --nocapture
cargo test -p jyowo-harness-sandbox local_sandbox_reports_cwd_marker_over_side_fd_without_polluting_stdout -- --nocapture
pnpm check:rust
```

Expected: all exit 0.

- [ ] **Step 6: Task exit analysis and subagent audits**

Run the required exit analysis, code-review-expert audit, and security-review audit.

- [ ] **Step 7: Commit**

```bash
git add crates/jyowo-harness-tool crates/jyowo-harness-sandbox docs/superpowers/audits/sandbox-hardening/task-5.md
git commit -m "fix(tool): execute Bash input through explicit shell"
```

## Task 6: Add Setup And Agent Phase Separation

**Purpose:** Prevent setup-time network and secrets from leaking into normal agent execution.

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/enums.rs`
- Modify: `crates/jyowo-harness-sandbox/src/backend.rs`
- Modify: `crates/jyowo-harness-sandbox/src/local/exec.rs`
- Create: `crates/jyowo-harness-sandbox/src/setup_plan.rs`
- Modify: `crates/jyowo-harness-sandbox/tests/phase_policy.rs`
- Test: `crates/jyowo-harness-sandbox/tests/setup_plan.rs`
- Modify: `crates/jyowo-harness-sdk/src/builder.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- Modify: `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/src/commands/conversations.rs`
- Test: `apps/desktop/src-tauri/tests/commands/runs_permissions.rs`

- [ ] **Step 1: Task intent check**

Write the Task 6 intent note. Include this invariant: "agent phase cannot inherit setup secrets or setup network."

- [ ] **Step 2: Write failing tests**

Add tests proving:

- agent phase has `SecretAccessPolicy::None` and `NetworkAccess::None`.
- setup phase accepts only explicit env allowlist keys.
- host env is not inherited except `SandboxBaseConfig.passthrough_env_keys`.
- secret-like keys not in allowlist are absent from child env.
- setup phase output redaction runs before events and spilled blobs.
- desktop default harness uses agent phase for Bash.
- existing desktop `start_run` builds an empty `SetupPlan`.
- a non-empty setup plan runs before agent execution and copies only allowed artifacts through filesystem policy.
- agent env is rebuilt from scratch after setup and contains no setup-only secret.
- `run_setup_plan` passes each `SetupStep.authorization` into the step execution context.
- setup steps that request `NetworkAccess::Unrestricted` receive authorization only from the same `PermissionSubject::ProcessNetworkAccess` flow used by Task 4, with the setup command fingerprint and stable `PermissionDecision.decision_id`.
- agent execution after setup uses `ExecAuthorization::none()` unless its own permission resolution creates a fresh grant.

Run:

```bash
cargo test -p jyowo-harness-sandbox phase_policy -- --nocapture
cargo test -p jyowo-harness-sandbox setup_plan -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly -- --nocapture
cargo test -p jyowo-desktop-shell sandbox_phase -- --nocapture
```

Expected before implementation: FAIL.

- [ ] **Step 3: Implement phase-specific env filtering**

Update child environment construction:

```text
base passthrough env keys: PATH, LANG, LC_ALL, TERM
setup secret allowlist: exact env names only
agent secret allowlist: empty unless a future explicit policy adds names
```

Validation:

- env names must be ASCII uppercase, digits, or underscore.
- empty env names are rejected.
- values are never logged.
- rejected env names fail closed.

- [ ] **Step 4: Add setup plan lifecycle**

Create `crates/jyowo-harness-sandbox/src/setup_plan.rs`.

Required API:

```rust
pub struct SetupPlan {
    pub steps: Vec<SetupStep>,
}

pub struct SetupStep {
    pub spec: ExecSpec,
    pub authorization: ExecAuthorization,
    pub artifact_policy: FilesystemPolicy,
}

impl SetupPlan {
    pub fn empty() -> Self;
    pub fn is_empty(&self) -> bool;
}

pub async fn run_setup_plan(
    backend: Arc<dyn SandboxBackend>,
    plan: SetupPlan,
    ctx: ExecContext,
    agent_policy: &ResolvedFilesystemPolicy,
) -> Result<SetupArtifacts, SandboxError>;
```

Rules:

- `SetupStep.spec.policy.phase` must be `SandboxPhase::Setup`.
- `SetupStep.spec.policy.secrets` may only be `SecretAccessPolicy::AllowList` with exact env names.
- `SetupStep.spec.policy.network` may be unrestricted only with a matching `SetupStep.authorization` grant issued from an allow `PermissionDecision` for `PermissionSubject::ProcessNetworkAccess`.
- Setup configuration alone is not authorization. `SetupStep.authorization` must not be accepted from frontend payloads, serialized setup config, plugin payloads, MCP payloads, or test fixtures.
- If the product has no trusted setup permission orchestration path yet, non-empty setup steps requesting unrestricted network must fail closed.
- `run_setup_plan` must clone or rebuild `ExecContext` per step and replace `ctx.authorization` with `SetupStep.authorization` before calling `backend.execute`.
- Setup output must pass through `Redactor` before Journal, Replay, events, logs, traces, exports, and spilled blobs.
- `SetupArtifacts` must contain only redacted metadata and allowed file changes.
- Agent execution must receive a new `ExecContext`/env path with no setup secret env and `authorization: ExecAuthorization::none()`. Setup authorization must never be reused by agent phase.

- [ ] **Step 5: Add setup policy constructor**

Add:

```rust
pub fn setup_workspace_write_with_network(
    network: NetworkAccess,
    secret_env_allowlist: Vec<String>,
) -> SandboxPolicy
```

`secret_env_allowlist` uses exact env names only. `NetworkAccess::AllowList` remains fail-closed for arbitrary local process execution until an enforcing backend exists. `NetworkAccess::Unrestricted` still requires `SetupStep.authorization` issued from the Task 4 `ProcessNetworkAccess` decision path. The constructor is used only by `SetupPlan` assembly. Tests must assert local backend rejects unsupported network policy.

- [ ] **Step 6: Wire desktop agent phase**

Desktop default run assembly must use `SetupPlan::empty()` and `SandboxPolicy::default_agent_workspace_write()` for Bash and plugin sidecar agent commands. In the current layout, default harness assembly belongs in `apps/desktop/src-tauri/src/commands/runtime.rs`, start-run request orchestration belongs in `apps/desktop/src-tauri/src/commands/conversations.rs`, and session engine wiring belongs in `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`. There must be no setup secret path in normal `start_run`.

- [ ] **Step 7: Run gates**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-sandbox phase_policy -- --nocapture
cargo test -p jyowo-harness-sandbox setup_plan -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly -- --nocapture
cargo test -p jyowo-desktop-shell sandbox_phase -- --nocapture
pnpm check:rust
```

Expected: all exit 0.

- [ ] **Step 8: Task exit analysis and subagent audits**

Run the required exit analysis, code-review-expert audit, and security-review audit.

- [ ] **Step 9: Commit**

```bash
git add crates/jyowo-harness-contracts crates/jyowo-harness-sandbox crates/jyowo-harness-sdk apps/desktop/src-tauri docs/superpowers/audits/sandbox-hardening/task-6.md
git commit -m "feat(sandbox): separate setup and agent policy"
```

## Task 7: Harden Docker Bind Mounts And Container Defaults

**Purpose:** Make Docker sandbox safe when it is enabled later, and prevent it from claiming unsafe bind behavior.

**Files:**

- Modify: `crates/jyowo-harness-sandbox/src/docker.rs`
- Modify: `crates/jyowo-harness-sandbox/tests/docker.rs`
- Modify: `docs/architecture/harness/crates/harness-sandbox.md`

- [ ] **Step 1: Task intent check**

Write the Task 7 intent note. Include this invariant: "Docker bind mounts must fail closed on credential roots, dangerous host paths, and symlink-parent escape."

- [ ] **Step 2: Write failing tests**

Add tests proving:

- default agent Docker network is `none`.
- workspace mount is read-write only when filesystem policy grants workspace write.
- protected paths are not mounted writable.
- creating `.env.production` inside a writable Docker workspace does not create or modify that file on the host.
- source `/`, `/etc`, `/proc`, `/sys`, `/dev`, `/var/run/docker.sock`, home `.ssh`, home `.aws`, home `.docker`, and home `.kube` are rejected.
- symlink parent in a mount source is rejected.
- unsupported `NetworkAccess::AllowList` fails closed.

Run:

```bash
cargo test -p jyowo-harness-sandbox --features docker docker -- --nocapture
```

Expected before implementation: FAIL.

- [ ] **Step 3: Implement mount validation**

Add Docker helpers:

```rust
fn validate_volume_mount_source(source: &Path, policy: &ResolvedFilesystemPolicy) -> Result<PathBuf, SandboxError>;
fn docker_mounts_for_policy(policy: &ResolvedFilesystemPolicy) -> Result<Vec<VolumeMount>, SandboxError>;
```

Rules:

- canonicalize existing sources.
- reject non-existing sources for bind mounts.
- reject any symlink component before canonicalization.
- reject dangerous roots before constructing docker args.
- construct docker args only from validated `PathBuf`s.
- do not bind the host workspace read-write when protected deny selectors exist under the writable parent unless Docker enforcement blocks future creates.
- otherwise mount a staged workspace and copy allowed changes back through `copy_allowed_changes_back`.

- [ ] **Step 4: Apply network and resource policy**

`DockerSandbox::execute` must call the shared network capability validator with `ExecContext.authorization`, `ExecContext.process_grant_verifier`, the command fingerprint, and current time before constructing or spawning the Docker command. `command_for_execute` may receive the prevalidated network summary only; it must not make authorization decisions because it does not own `ExecContext`. Ephemeral per-exec may apply resource limits. Managed or bring-your-own containers must fail closed for per-exec limits they cannot enforce.

- [ ] **Step 5: Run gates**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-sandbox --features docker docker -- --nocapture
pnpm check:rust
pnpm check:docs
```

Expected: all exit 0.

- [ ] **Step 6: Task exit analysis and subagent audits**

Run the required exit analysis, code-review-expert audit, and security-review audit.

- [ ] **Step 7: Commit**

```bash
git add crates/jyowo-harness-sandbox docs/architecture/harness/crates/harness-sandbox.md docs/superpowers/audits/sandbox-hardening/task-7.md
git commit -m "feat(sandbox): harden docker mounts"
```

## Task 8: Harden SSH Workspace Sync And Remote Policy

**Purpose:** Make SSH sandbox honest about what it can enforce and prevent remote sync from overwriting protected local runtime files.

**Files:**

- Modify: `crates/jyowo-harness-sandbox/src/ssh.rs`
- Modify: `crates/jyowo-harness-sandbox/tests/ssh.rs`
- Modify: `docs/architecture/harness/crates/harness-sandbox.md`

- [ ] **Step 1: Task intent check**

Write the Task 8 intent note. Include this invariant: "SSH backend cannot claim local filesystem or network enforcement unless it implements a concrete remote policy."

- [ ] **Step 2: Write failing tests**

Add tests proving:

- remote workspace must be absolute.
- remote workspace `/` is rejected.
- `RsyncPush` excludes protected workspace paths by default.
- `RsyncBidi` pull refuses to overwrite `.jyowo/runtime`, `.git`, `.env*`, and credential files.
- pull sync refuses delete, rename, symlink, hardlink, device, fifo, and special-file operations that target protected local paths.
- `NetworkAccess::None`, `LoopbackOnly`, and `AllowList` fail closed for SSH process execution.
- snapshot restore rejects absolute and parent traversal archive entries.

Run:

```bash
cargo test -p jyowo-harness-sandbox --features ssh ssh -- --nocapture
```

Expected before implementation: FAIL.

- [ ] **Step 3: Validate remote workspace**

Builder validation must reject:

```text
empty remote workspace
relative remote workspace
remote workspace "/"
remote workspace containing ".."
```

- [ ] **Step 4: Add protected rsync excludes**

Default excludes must include:

```text
.git/**
.jyowo/runtime/**
.env
.env.*
**/.ssh/**
**/.aws/**
**/.docker/**
**/.kube/**
```

User-provided excludes may add entries, but cannot remove defaults.

- [ ] **Step 5: Protect pull sync**

Before pull sync writes into local workspace, validate the target path against `ResolvedFilesystemPolicy`. Pull sync must not write denied paths.

Required implementation:

```text
rsync pull must run into a staging directory first
then copy allowed changes into the real workspace through WorkspaceChangeManifest and copy_allowed_changes_back
do not pass --delete directly against the real workspace
reject symlink, hardlink, device, fifo, and special-file entries unless the target stays inside staged workspace and the final host path is allowed
abort before host mutation if the manifest contains a denied path or host-state conflict
stop on first partial-apply error and report only redacted path category plus manifest entry id
```

- [ ] **Step 6: Run gates**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-sandbox --features ssh ssh -- --nocapture
pnpm check:rust
pnpm check:docs
```

Expected: all exit 0.

- [ ] **Step 7: Task exit analysis and subagent audits**

Run the required exit analysis, code-review-expert audit, and security-review audit.

- [ ] **Step 8: Commit**

```bash
git add crates/jyowo-harness-sandbox docs/architecture/harness/crates/harness-sandbox.md docs/superpowers/audits/sandbox-hardening/task-8.md
git commit -m "feat(sandbox): harden ssh workspace policy"
```

## Task 9: Add Sandbox Explain Diagnostics

**Purpose:** Make effective sandbox policy visible and testable without exposing secrets or private paths.

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/enums.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Create: `crates/jyowo-harness-sandbox/src/explain.rs`
- Modify: `crates/jyowo-harness-sandbox/src/lib.rs`
- Test: `crates/jyowo-harness-sandbox/tests/explain.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/commands/providers.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs` only if adding `get_sandbox_status`
- Test: `apps/desktop/src-tauri/tests/commands/providers.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Modify: `apps/desktop/src/testing/command-client.ts`
- Modify: `apps/desktop/src/features/settings/ExecutionSettings.tsx`
- Modify: `apps/desktop/src/features/settings/ExecutionSettings.test.tsx`

- [ ] **Step 1: Task intent check**

Write the Task 9 intent note. Include this invariant: "sandbox diagnostics are explanatory only; Rust policy remains authoritative."

- [ ] **Step 2: Write failing Rust tests**

Add tests proving:

- explain payload includes backend id, mode, phase, network summary, filesystem summary, secret summary, and fail-closed reasons.
- private absolute workspace path is not serialized.
- protected paths are reported by category, not raw host path.
- unavailable isolation reports safe fail-closed reason.
- `set_execution_settings` persists valid settings even when local sandbox binary probing would fail.
- `get_execution_settings` or `get_sandbox_status` reports sandbox availability without raw private paths.

Run:

```bash
cargo test -p jyowo-harness-sandbox explain -- --nocapture
cargo test -p jyowo-desktop-shell execution_settings -- --nocapture
```

Expected before implementation: FAIL.

- [ ] **Step 3: Implement explain module**

Add:

```rust
pub fn explain_sandbox_policy(
    backend: &dyn SandboxBackend,
    policy: &SandboxPolicy,
) -> SandboxExplainPayload;
```

The function must summarize categories, not raw paths. Use redacted labels:

```text
workspace_root
workspace_subpath:<relative>
temp_dir
home_subpath:<relative>
absolute:<redacted>
```

- [ ] **Step 4: Extend execution settings read path**

Add to `GetExecutionSettingsResponse`:

```rust
pub sandbox: SandboxExplainPayload,
```

`get_execution_settings` is exposed from `apps/desktop/src-tauri/src/commands/mod.rs`, but the response helper lives in `apps/desktop/src-tauri/src/commands/providers.rs`. Compute this payload through a helper in `apps/desktop/src-tauri/src/commands/runtime.rs` that reuses the same sandbox assembly facts as `build_desktop_harness` and `desktop_plugin_sidecar_sandbox`. Do not build a separate fake payload.

Do not add a live sandbox probe to `SetExecutionSettingsResponse`. If frontend needs a refreshed status after save, add a separate read-only command:

```rust
pub async fn get_sandbox_status(...) -> Result<SandboxExplainPayload, CommandErrorPayload>
```

If this command is added, expose its wrapper from `commands/mod.rs`, register it in `apps/desktop/src-tauri/src/lib.rs`, and add command tests in `apps/desktop/src-tauri/tests/commands/providers.rs`, which owns provider and execution-settings command behavior. Do not put sandbox diagnostics tests in `automations.rs` unless the implementation also changes automation commands.

`set_execution_settings` may validate settings shape and persist config, but it must not fail only because `sandbox-exec`, `bwrap`, Docker, or SSH is unavailable.

- [ ] **Step 5: Update frontend Zod schemas**

In `apps/desktop/src/shared/tauri/commands.ts`, add strict schemas for:

```ts
const sandboxExplainSchema = z.object({
  backendId: z.string().min(1),
  mode: sandboxModeSchema,
  phase: sandboxPhaseSchema,
  filesystem: sandboxFilesystemSummarySchema,
  network: sandboxNetworkSummarySchema,
  secrets: sandboxSecretSummarySchema,
  capabilities: sandboxCapabilitySummarySchema,
  failClosedReasons: z.array(z.string()),
}).strict()
```

All nested schemas must be strict. Do not use `z.any()`.

- [ ] **Step 6: Render in ExecutionSettings**

Add a compact "Sandbox" section under existing execution settings:

- status row: backend, mode, phase
- network row: `none`, `unrestricted`, or `unsupported`
- secret row: `none` or allowlist count
- fail-closed reasons row when non-empty

Do not render raw paths.

- [ ] **Step 7: Run gates**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-contracts sandbox -- --nocapture
cargo test -p jyowo-harness-sandbox explain -- --nocapture
cargo test -p jyowo-desktop-shell execution_settings -- --nocapture
pnpm -C apps/desktop test shared/tauri/commands.test.ts -- --run
pnpm -C apps/desktop test ExecutionSettings -- --run
pnpm check:desktop
pnpm check:rust
```

Expected: all exit 0.

- [ ] **Step 8: Task exit analysis and subagent audits**

Run the required exit analysis, code-review-expert audit, and security-review audit.

- [ ] **Step 9: Commit**

```bash
git add crates/jyowo-harness-contracts crates/jyowo-harness-sandbox apps/desktop docs/superpowers/audits/sandbox-hardening/task-9.md
git commit -m "feat(sandbox): expose safe policy diagnostics"
```

## Task 10: Update Subagent, Plugin, And Engine Sandbox Inheritance

**Purpose:** Ensure all execution paths use the same sandbox policy model and do not bypass hard denies.

**2026-07-01 baseline adjustment:** Agent orchestration is already implemented. This task must include the existing agent runtime and desktop run paths, not just the older engine/plugin/session paths. The target is one shared child-policy merge rule used by existing subagent, team, background-agent, plugin sidecar, and session-turn execution.

**Files:**

- Create: `crates/jyowo-harness-sandbox/src/policy_merge.rs`
- Test: `crates/jyowo-harness-sandbox/tests/policy_merge.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/policy.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/subagents.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/teams.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/background.rs`
- Modify: `crates/jyowo-harness-agent-runtime/tests/agent_orchestration_policy.rs`
- Modify: `crates/jyowo-harness-agent-runtime/tests/subagents.rs`
- Modify: `crates/jyowo-harness-agent-runtime/tests/agents_team.rs`
- Modify: `crates/jyowo-harness-agent-runtime/tests/agent_orchestration_background.rs`
- Modify: `crates/jyowo-harness-team/src/lib.rs`
- Modify: `crates/jyowo-harness-team/tests/contract.rs`
- Modify: `crates/jyowo-harness-team/tests/api.rs`
- Modify: `crates/jyowo-harness-engine/src/engine.rs`
- Modify: `crates/jyowo-harness-engine/src/turn.rs`
- Modify: `crates/jyowo-harness-subagent/src/lib.rs`
- Modify: `crates/jyowo-harness-subagent/tests/contract.rs`
- Modify: `crates/jyowo-harness-plugin/src/cargo_extension.rs`
- Modify: `crates/jyowo-harness-plugin/tests/sources.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/tool_pool.rs`
- Modify: `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`
- Modify: `crates/jyowo-harness-sdk/tests/agents_team.rs`
- Modify: `crates/jyowo-harness-session/src/turn.rs`
- Modify: `crates/jyowo-harness-session/tests/run_turn.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify: `apps/desktop/src-tauri/src/commands/background_agents.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/background_agents.rs`

- [ ] **Step 1: Task intent check**

Write the Task 10 intent note. Include this invariant: "subagents, plugins, and session turns cannot weaken parent sandbox policy unless an explicit Rust policy override grants a stricter or equal policy."

- [ ] **Step 2: Write failing tests**

Add tests proving:

- `merge_sandbox_policy_for_child` lives in `jyowo-harness-sandbox` and is used by child execution paths rather than reimplemented in engine, session, plugin, or subagent crates.
- `policy_merge.rs` does not import `harness_subagent`, `harness_engine`, `harness_plugin`, or `harness_session`.
- agent runtime policy merge cannot produce write-capable subagent, team, or background execution without the hardened child sandbox policy.
- subagent `Require` fails closed if parent sandbox lacks network-none or filesystem enforcement.
- subagent `Override` cannot remove protected filesystem denies.
- team member and background-agent runs inherit the parent run sandbox through the same merge helper.
- `TeamSandboxPolicy::Empty` cannot call `EngineBuilder::without_sandbox`. Because the project is still in development, either remove the variant from the public team contract and update schema/tests, or make it a fail-closed request error. Do not keep a compatibility path that disables sandbox inheritance.
- plugin sidecar uses the same protected default policy.
- session turn execution passes the configured sandbox into tool context.
- no path calls `without_sandbox` except tests that intentionally assert missing sandbox rejection.

Name the new agent-runtime, team, and SDK team tests with a `sandbox_inheritance_` prefix so the targeted cargo filters below execute the intended coverage instead of passing with zero matching tests.

Run:

```bash
cargo test -p jyowo-harness-sandbox policy_merge -- --nocapture
cargo test -p jyowo-harness-agent-runtime sandbox_inheritance -- --nocapture
cargo test -p jyowo-harness-team sandbox_inheritance -- --nocapture
cargo test -p jyowo-harness-subagent sandbox -- --nocapture
cargo test -p jyowo-harness-plugin sandbox -- --nocapture
cargo test -p jyowo-harness-session sandbox -- --nocapture
cargo test -p jyowo-harness-sdk sandbox_inheritance -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly -- --nocapture
```

Expected before implementation: FAIL.

- [ ] **Step 3: Add policy merge rule**

Create `crates/jyowo-harness-sandbox/src/policy_merge.rs` and export it from `crates/jyowo-harness-sandbox/src/lib.rs`.

Layering rule:

- `policy_merge.rs` is an L1 helper and may depend only on L0 contracts and L1-local sandbox types.
- It must not import or pattern-match on `SandboxInheritance`, `RequiredSandboxCapabilities`, subagent lifecycle types, plugin types, session turn types, or engine builder types.
- Mapping `SandboxInheritance::{Inherit, Empty, Require, Override}`, `TeamSandboxPolicy::{Inherit, Empty, RequireBackend}`, and agent profile `AgentProfileSandboxInheritance` to parent/child `SandboxPolicy` values belongs in engine/subagent/session/plugin/agent-runtime/team/SDK call sites.
- Higher layers may call `merge_sandbox_policy_for_child(parent, child)`, but the sandbox crate must remain unaware of which higher-level domain requested the merge.

Implement:

```rust
pub fn merge_sandbox_policy_for_child(
    parent: &SandboxPolicy,
    child: &SandboxPolicy,
) -> Result<SandboxPolicy, SandboxError>
```

Rules:

- child may add denies.
- child may reduce network from unrestricted to none.
- child may not remove parent denies.
- child may not add secret env names absent from parent policy.
- child may not move from agent phase to setup phase.
- unsupported merged policy fails closed.

- [ ] **Step 4: Update inheritance call sites**

Replace ad hoc sandbox capability checks in engine, subagent, team, background-agent, plugin, and session paths with the shared capability and policy merge helpers. Update the SDK team-member runner path that currently maps `TeamSandboxPolicy::Empty` to `builder.without_sandbox()`; this path is part of production sandbox inheritance.

- [ ] **Step 5: Run gates**

```bash
cargo fmt --all --check
cargo test -p jyowo-harness-sandbox policy_merge -- --nocapture
cargo test -p jyowo-harness-agent-runtime sandbox_inheritance -- --nocapture
cargo test -p jyowo-harness-team sandbox_inheritance -- --nocapture
cargo test -p jyowo-harness-subagent sandbox -- --nocapture
cargo test -p jyowo-harness-plugin sandbox -- --nocapture
cargo test -p jyowo-harness-session sandbox -- --nocapture
cargo test -p jyowo-harness-sdk sandbox_inheritance -- --nocapture
cargo test -p jyowo-harness-sdk runtime_assembly -- --nocapture
if ! rg -n "sandbox_inheritance_" crates/jyowo-harness-agent-runtime/tests crates/jyowo-harness-team/tests crates/jyowo-harness-sdk/tests/agents_team.rs; then
  echo "Task 10 targeted tests must use the sandbox_inheritance_ prefix in agent-runtime, team, and SDK team tests."
  exit 1
fi
if rg -n "harness_(subagent|engine|plugin|session)|SandboxInheritance|RequiredSandboxCapabilities" crates/jyowo-harness-sandbox/src/policy_merge.rs; then
  echo "L1 sandbox policy merge production code must not depend on higher-layer inheritance types."
  exit 1
fi
pnpm check:rust
```

Expected: all exit 0.

- [ ] **Step 6: Task exit analysis and subagent audits**

Run the required exit analysis, code-review-expert audit, and security-review audit.

- [ ] **Step 7: Commit**

```bash
git add crates/jyowo-harness-sandbox crates/jyowo-harness-agent-runtime crates/jyowo-harness-team crates/jyowo-harness-engine crates/jyowo-harness-subagent crates/jyowo-harness-plugin crates/jyowo-harness-sdk crates/jyowo-harness-session apps/desktop/src-tauri docs/superpowers/audits/sandbox-hardening/task-10.md
git commit -m "feat(sandbox): enforce inherited child policy"
```

## Task 11: Final Documentation And Schema Gate

**Purpose:** Align docs, schema exports, and public contract snapshots after the sandbox refactor.

**Files:**

- Modify: `docs/architecture/harness/crates/harness-sandbox.md`
- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`
- Modify: `docs/frontend/frontend-engineering.md`
- Modify: `docs/frontend/frontend-quality.md`
- Modify: `crates/jyowo-harness-contracts/tests/m1_contracts.rs`
- Modify: schema snapshot files only if the repository already stores generated schema snapshots

- [ ] **Step 1: Task intent check**

Write the Task 11 intent note. Include this invariant: "documentation must describe the implemented behavior, not future behavior."

- [ ] **Step 2: Update docs**

Docs must include:

- single `SandboxPolicy` authority
- OS isolation requirement
- Windows enforcement limitation
- Windows desktop local process unavailability behavior
- Docker/SSH status and failure modes
- staged workspace or overlay copy-back requirement, including manifest preflight, conflict detection, atomic file replacement, and redacted partial-apply errors
- process network grant requirement for unrestricted process network
- `PermissionSubject::ProcessNetworkAccess` is distinct from `PermissionSubject::CommandExec`
- Rust public contract structs use `#[serde(deny_unknown_fields)]` and exported schemas reject unknown properties
- setup/agent phase behavior
- setup lifecycle and empty desktop default SetupPlan
- Bash shell wrapper behavior
- sandbox explain payload
- settings save path must not require live sandbox probing
- persisted task audit records
- exact quality gates

- [ ] **Step 3: Export schemas**

Run the repository's existing schema export command if present. If there is no export command, update schema export tests only through Rust test execution.

Do not hand-write generated schema files.

- [ ] **Step 4: Run gates**

```bash
pnpm check:backend-docs
pnpm check:frontend-docs
pnpm check:docs
cargo test -p jyowo-harness-contracts -- --nocapture
```

Expected: all exit 0.

- [ ] **Step 5: Task exit analysis and subagent audits**

Run the required exit analysis, code-review-expert audit, and security-review audit.

- [ ] **Step 6: Commit**

```bash
git add docs crates/jyowo-harness-contracts docs/superpowers/audits/sandbox-hardening/task-11.md
git commit -m "docs(sandbox): align policy and diagnostics contracts"
```

## Task 12: Full Verification And Regression Sweep

**Purpose:** Prove the full branch is coherent across Rust, desktop, docs, and frontend IPC schemas.

**Files:**

- All modified files.

- [ ] **Step 1: Task intent check**

Write the Task 12 intent note. Include this invariant: "full verification must run after all task commits and before handoff."

- [ ] **Step 2: Inspect diff**

```bash
git status --short
git diff --stat main...HEAD
git diff --check main...HEAD
```

Expected:

- no whitespace errors
- no unrelated generated noise
- no raw secret-like sample values

- [ ] **Step 3: Run root gates**

```bash
pnpm check:rust-deps
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
cargo test -p jyowo-harness-sandbox --features local local -- --nocapture
cargo test -p jyowo-harness-sandbox --features docker docker -- --nocapture
cargo test -p jyowo-harness-sandbox --features ssh ssh -- --nocapture
cargo test -p jyowo-harness-sandbox filesystem_policy -- --nocapture
cargo test -p jyowo-harness-sandbox workspace_staging -- --nocapture
cargo test -p jyowo-harness-sandbox process_network_policy -- --nocapture
cargo test -p jyowo-harness-sandbox setup_plan -- --nocapture
cargo test -p jyowo-harness-sandbox explain -- --nocapture
cargo test -p jyowo-harness-permission process_network_grant -- --nocapture
pnpm check:docs
pnpm check:desktop
pnpm check:rust
pnpm check
```

Expected: all exit 0.

- [ ] **Step 4: Run security searches**

```bash
if rg -n --pcre2 "(?i)\\b(api[_-]?key|token|secret|password)\\b\\s*[:=]\\s*['\"][^'\"]{8,}['\"]" crates apps docs \
  -g '!target/**' \
  -g '!node_modules/**' \
  -g '!dist/**' \
  -g '!storybook-static/**'; then
  echo "Potential raw secret assignment found. Review every match or move the benign example to a documented allowlist test fixture."
  exit 1
fi

if rg -n --pcre2 "(?i)\\b(authorization|cookie)\\b\\s*[:=]\\s*['\"][^'\"]{8,}['\"]|bearer\\s+[a-z0-9._~+/=-]{16,}" crates apps docs \
  -g '!target/**' \
  -g '!node_modules/**' \
  -g '!dist/**' \
  -g '!storybook-static/**'; then
  echo "Potential credential header or bearer value found. Review every match or move the benign example to a documented allowlist test fixture."
  exit 1
fi

unsafe_sandbox_matches="$(rg -n "LocalSandbox::new\\(workspace_root\\)|with_sandbox\\(LocalSandbox::new|NetworkAccess::AllowList\\([^)]*\\).*Ok\\(|workspace_access|denied_host_paths|pub scope:\\s*SandboxScope|SandboxScope::" crates apps \
  -g '!target/**' \
  -g '!node_modules/**' \
  -g '!**/tests/**' \
  -g '!**/test/**' \
  -g '!*.test.ts' \
  -g '!*.test.tsx' \
  -g '!*.spec.ts' \
  -g '!*.spec.tsx' \
  -g '!crates/jyowo-harness-contracts/src/automation.rs' \
  -g '!apps/desktop/src-tauri/src/commands/validation.rs' || true)"
if [ -n "$unsafe_sandbox_matches" ]; then
  printf '%s\n' "$unsafe_sandbox_matches"
  echo "Unsafe process sandbox compatibility names remain in production code. Automation workspaceAccess is a separate scheduler contract and must not authorize process sandbox policy."
  exit 1
fi

if rg -n "\\bsupports_network\\b" crates apps \
  -g '!target/**' \
  -g '!node_modules/**' \
  -g '!**/tests/**' \
  -g '!**/test/**' \
  -g '!*.test.ts' \
  -g '!*.test.tsx' \
  -g '!*.spec.ts' \
  -g '!*.spec.tsx'; then
  echo "Broad supports_network capability must not remain in production process sandbox code."
  exit 1
fi

grant_matches="$(rg -n "issue_process_network_grant" crates apps \
  -g '!target/**' \
  -g '!node_modules/**' || true)"
if [ -z "$grant_matches" ]; then
  echo "Process network grant issuer has no call sites."
  exit 1
fi
unexpected_grant_matches="$(printf '%s\n' "$grant_matches" | rg -v "crates/jyowo-harness-sandbox/src/process_authorization.rs|crates/jyowo-harness-sandbox/tests/process_authorization.rs|crates/jyowo-harness-tool/src/orchestrator.rs|tests/" || true)"
if [ -n "$unexpected_grant_matches" ]; then
  printf '%s\n' "$unexpected_grant_matches"
  echo "Process network grants may only be implemented in sandbox authorization, tool orchestration, or tests."
  exit 1
fi
if ! rg -n "PermissionSubject::ProcessNetworkAccess" crates/jyowo-harness-tool/src/orchestrator.rs >/dev/null; then
  echo "Tool orchestration must request ProcessNetworkAccess before issuing process network grants."
  exit 1
fi
if ! rg -n "issue_process_network_grant" crates/jyowo-harness-tool/src/orchestrator.rs >/dev/null; then
  echo "Tool orchestration must be the production grant issuance point."
  exit 1
fi
if rg -n "CommandExec.*issue_process_network_grant|issue_process_network_grant.*CommandExec" crates apps \
  -g '!target/**' \
  -g '!node_modules/**'; then
  echo "Process network grant must not be issued from CommandExec authorization."
  exit 1
fi

if rg -n "decision_id:\\s*DecisionId::new\\(\\)" crates/jyowo-harness-engine/src crates/jyowo-harness-session/src crates/jyowo-harness-tool/src \
  -g '!target/**'; then
  echo "Permission events and tool approvals must reuse PermissionDecision.decision_id instead of allocating a new id after resolution."
  exit 1
fi

for n in 1 2 3 4 5 6 7 8 9 10 11 12; do
  audit_file="docs/superpowers/audits/sandbox-hardening/task-$n.md"
  test -f "$audit_file"
  rg -n '^## Current Audit Status$' "$audit_file" >/dev/null
  code_status="$(awk '/^## Current Audit Status$/{inside=1; next} /^## /{inside=0} inside && /^Code Review:/{print $3; exit}' "$audit_file")"
  security_status="$(awk '/^## Current Audit Status$/{inside=1; next} /^## /{inside=0} inside && /^Security Review:/{print $3; exit}' "$audit_file")"
  test "$code_status" = "PASS"
  test "$security_status" = "PASS"
done
```

Expected:

- targeted secret searches have no raw secret assignment, credential header value, bearer token, or leaked credential; benign examples must live in reviewed allowlist fixtures.
- sandbox search has no unsafe main harness `LocalSandbox::new(workspace_root)`.
- old split authority names are gone from production process sandbox code. `Automation.workspace_access` may remain only as a scheduler contract, with tests proving it does not authorize process sandbox policy.
- the old broad `supports_network` capability is gone from production process sandbox code.
- remaining mentions are docs explaining removed behavior or tests asserting migration.
- `issue_process_network_grant` appears only after a `PermissionSubject::ProcessNetworkAccess` allow decision and in tests that prove rejection paths.
- permission event and tool approval code reuses `PermissionDecision.decision_id` instead of allocating a new id after resolution.
- every task audit file exists and its `Current Audit Status` says both `Code Review: PASS` and `Security Review: PASS`.

- [ ] **Step 5: Task exit analysis and subagent audits**

Run the required exit analysis, code-review-expert audit, and security-review audit over the full branch.

- [ ] **Step 6: Final commit**

If Task 12 changed only docs or snapshots:

```bash
{
  git diff --name-only
  git ls-files --others --exclude-standard
} | sort -u > /tmp/sandbox-hardening-files.txt
sed -n '1,200p' /tmp/sandbox-hardening-files.txt
# Review the list before staging. Delete unrelated files, temp files, logs, target output,
# and unknown generated noise from /tmp/sandbox-hardening-files.txt before continuing.
xargs git add -- < /tmp/sandbox-hardening-files.txt
git commit -m "test(sandbox): complete hardening verification"
```

Before staging, run `git status --short` and verify `/tmp/sandbox-hardening-files.txt` contains only current Task 12 unstaged or untracked implementation, tests, scripts, docs, schema, or lockfile changes. Do not use `git diff --name-only main...HEAD`, `git add .`, or broad directory staging, because earlier task commits on the feature branch must not be restaged as Task 12 changes.

If Task 12 changed only the task audit file, commit `docs/superpowers/audits/sandbox-hardening/task-12.md`.

If Task 12 changed nothing and the task audit file already exists in a previous commit, do not create an empty commit.

## Commit Boundaries

Expected commit sequence:

```text
docs(sandbox): define fail-closed sandbox policy
refactor(sandbox): centralize filesystem policy
feat(sandbox): require local OS isolation
feat(sandbox): enforce process network capabilities
fix(tool): execute Bash input through explicit shell
feat(sandbox): separate setup and agent policy
feat(sandbox): harden docker mounts
feat(sandbox): harden ssh workspace policy
feat(sandbox): expose safe policy diagnostics
feat(sandbox): enforce inherited child policy
docs(sandbox): align policy and diagnostics contracts
test(sandbox): complete hardening verification
```

## Acceptance Criteria

The implementation is not complete until all are true:

- Main desktop Harness uses required OS isolation or fails closed during startup.
- Windows desktop settings remain usable, while local agent Bash/process execution is unavailable unless an enforcing backend is selected.
- Agent phase defaults to no network and no secrets.
- Setup phase has a concrete lifecycle, empty desktop default, redacted outputs, filtered artifacts, and cannot leak secrets into agent phase.
- Filesystem policy has one authoritative source.
- Protected paths and future `.env.*` creations are denied under workspace write.
- Local, Docker, and SSH writable workspaces use native future-create enforcement or staged copy-back with manifest preflight, host conflict detection, redacted partial-apply errors, and atomic file replacement where supported.
- Windows `JobObject` does not claim filesystem or network sandboxing.
- `NetworkAccess::AllowList` is not falsely enforced for arbitrary processes.
- `NetworkAccess::Unrestricted` for arbitrary processes requires a matching runtime-only process network grant issued only after `PermissionSubject::ProcessNetworkAccess` allow.
- `PermissionSubject::CommandExec` does not authorize process network.
- Bash tool executes through `/bin/sh -lc` and asks permission on the user script.
- Docker mount validation rejects dangerous sources and symlink-parent escapes.
- SSH sync cannot overwrite, delete, rename, link, or materialize special files over protected local runtime paths.
- Sandbox explain is typed, redacted, strict, and visible in execution settings.
- Saving execution settings does not fail because a live sandbox backend is unavailable.
- Rust contract deserialization, exported contract schemas, and Zod schemas reject unknown fields.
- Every task has persisted exit analysis and subagent/security audit PASS records.
- `pnpm check` exits 0.

## Self-Review

Spec coverage:

- Problem 1 covered by Task 3.
- Problem 2 covered by Task 2.
- Problem 3 covered by Task 4.
- Problem 4 covered by Task 5.
- Problem 5 covered by Task 6.
- Problem 6 covered by Tasks 7 and 8.
- Problem 7 covered by Task 9.
- Problem 8 covered by Tasks 1 and 11.
- Cross-path inheritance and plugin/subagent execution covered by Task 10.
- Final branch safety covered by Task 12.

Placeholder scan:

- No task relies on fake implementation.
- No task accepts unsupported sandbox behavior as allowed.
- Unsupported network allowlist for arbitrary processes is explicitly fail-closed.

Risk boundary:

- This plan intentionally permits breaking refactors because the project is in development.
- Refactors are only valid when they remove split authority, false compatibility, or duplicated policy semantics.
- A task cannot proceed after failed tests, failed gates, failed audit, or unresolved security-review findings.
