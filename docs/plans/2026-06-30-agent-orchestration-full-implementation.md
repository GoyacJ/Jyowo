# Agent Orchestration Full Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Each task also requires an independent read-only subagent audit before the task can be marked complete.

**Goal:** Make subagents, run-scoped agent teams, and background agents first-class Jyowo desktop capabilities with real backend runtime support, durable contracts, UI surfaces, security gates, recovery behavior, and tests.

**Architecture:** Keep Rust as the policy and runtime authority. Use existing `jyowo-harness-subagent` and `jyowo-harness-team` as L3 primitives, add a new L3 `jyowo-harness-agent-runtime` crate for cross-domain orchestration, and keep `jyowo-harness-sdk` as the L4 facade and assembly layer only. Settings toggles only permit use; per-run options request use; backend policy decides availability and fails closed.

**Tech Stack:** Rust 1.96, serde, schemars, rusqlite, JSONL journal, Tokio, Tauri 2, React 19, TypeScript 6, Zod, TanStack Query, React Hook Form, Vitest, Testing Library, cargo test, pnpm gates.

---

## Required Execution Mode

Implementation must happen in an isolated git worktree. Do not implement in the original `main` workspace.

Use branch prefix `goya`.

```bash
git status --short
git worktree add ../Jyowo-agent-orchestration -b goya/agent-orchestration
cd ../Jyowo-agent-orchestration
git status --short --branch
```

Expected:

```text
## goya/agent-orchestration
```

If the branch name already exists:

```bash
git worktree add ../Jyowo-agent-orchestration-2 -b goya/agent-orchestration-2
```

All commits for this plan must be created from the isolated worktree path.

## Mandatory Reading

Before editing files, read these files in the worktree:

```text
AGENTS.md
docs/frontend/agent-harness-frontend-development-guidelines.md
docs/frontend/frontend-product-ux.md
docs/frontend/frontend-engineering.md
docs/frontend/frontend-quality.md
docs/backend/agent-harness-backend-development-guidelines.md
docs/backend/backend-runtime.md
docs/backend/backend-engineering.md
docs/backend/backend-quality.md
docs/plans/2026-06-30-agent-orchestration-full-implementation.md
```

For any task touching frontend code, reread the frontend docs above.

For any task touching Rust backend code, reread the backend docs above.

## External Product References

These references informed scope. They do not override Jyowo security rules.

- OpenAI Codex subagents: https://developers.openai.com/codex/subagents
- Claude Code subagents: https://code.claude.com/docs/en/sub-agents
- Claude Code agent teams: https://code.claude.com/docs/en/agent-teams
- Claude Code agent view and background management: https://code.claude.com/docs/en/agent-view
- Claude Code git worktrees: https://code.claude.com/docs/en/git-worktrees

Reference principles to keep:

- Subagents are explicit, bounded delegation. They return summarized results to the parent.
- Subagents isolate context and tool scope from the parent.
- Agent teams need a lead, members, task coordination, and a mailbox or shared work queue.
- Background agents must be inspectable, cancellable, and restart-aware.
- Write-capable parallel agents need workspace isolation. Same checkout parallel writes are forbidden.

Jyowo decisions:

- Do not label subagents or agent teams as experimental after this plan is complete.
- Do not mark a capability available until the backend runtime, IPC, UI, projection, tests, and recovery semantics exist.
- Agent teams are run-scoped in this plan. There is no standalone persistent team management product surface beyond run invocation, timeline display, and audit/replay.
- Background agents are durable and detachable from the active conversation UI. If all Jyowo processes exit, running background agents must be recovered or marked interrupted on restart. Continuous execution after full app quit requires the supervisor task in this plan; until that task passes, `backgroundAgentsAvailable` remains false.
- Background agents have one user-facing start path: `start_run` with `agentOptions.background = background`. Dedicated background commands operate on an existing background agent record; they do not start a second kind of run.

## Current Code Facts

Use these facts as the baseline. Do not invent a different starting state.

- `crates/jyowo-harness-subagent/src/lib.rs` already contains `SubagentSpec`, runner logic, admin, permission bridge, watchdog, quota, events, and `AgentTool`.
- `crates/jyowo-harness-team/src/lib.rs` already contains `TeamSpec`, topologies, message bus, lifecycle, quota, watchdog, member runner, and journal integration.
- `crates/jyowo-harness-team/src/lib.rs` explicitly says the team message bus is single-process and has no cross-process ordering or delivery.
- `crates/jyowo-harness-sdk/Cargo.toml` already has `agents-subagent` and `agents-team` features.
- `crates/jyowo-harness-sdk/src/builder.rs` already has `with_subagent_runner(...)` behind `agents-subagent`.
- `crates/jyowo-harness-engine/src/engine.rs` already has `with_subagent_tool()`.
- `crates/jyowo-harness-sdk/src/harness/team_runtime.rs` already exposes `Harness::create_team(...)` behind `agents-team`; do not move team runtime logic back into the `harness.rs` module root.
- There is no `crates/jyowo-harness-agent-runtime` crate yet. This plan must add it as an L3 workspace member.
- `crates/jyowo-harness-contracts/src/capability.rs` already has `AgentCapabilityKind::{Subagents, AgentTeams, BackgroundAgents}` and `AgentCapabilityUnavailableReason::NotCompiled`.
- `crates/jyowo-harness-contracts/src/events/subagent.rs` and `crates/jyowo-harness-contracts/src/events/team.rs` already define public event shapes.
- `crates/jyowo-harness-contracts/src/schema_export.rs` already exports subagent and team event schemas.
- `apps/desktop/src-tauri/Cargo.toml` does not enable `agents-subagent` or `agents-team` for `jyowo-harness-sdk`.
- Desktop Tauri commands are split by domain under `apps/desktop/src-tauri/src/commands/`; `commands/mod.rs` owns command wrappers and re-exports, not domain business logic.
- `apps/desktop/src-tauri/src/commands/contracts.rs` defines IPC request/response structs including `StartRunRequest` and execution settings responses.
- `apps/desktop/src-tauri/src/commands/providers.rs` stores `ExecutionSettingsRecord::{subagents_enabled, agent_teams_enabled, background_agents_enabled}` and currently hardcodes agent capability availability to false in `agent_capabilities_available()`.
- `apps/desktop/src-tauri/src/commands/conversations.rs` owns `start_run_payload(...)` and `start_run_with_runtime_state(...)`; `StartRunRequest` does not include agent orchestration options.
- `apps/desktop/src-tauri/src/lib.rs` registers no Tauri commands for agent profile, team, or background agent management.
- `apps/desktop/src/shared/tauri/commands.ts` has current Zod settings schema for agent capabilities but no agent runtime IPC schemas.
- `apps/desktop/src/features/settings/ExecutionSettings.tsx` reads agent capability settings but does not expose complete General settings switches.
- `crates/jyowo-harness-contracts/src/conversation.rs` `AssistantSegment` has no subagent, team, or background agent activity segment.
- `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs` does not project subagent, team, or background activity into `ConversationWorktreePage`.
- There is no real background agent runtime module in an owning L3 crate, Tauri command surface, durable registry, supervisor, restart recovery, or UI.

## Product Contract

### Settings

Settings > General must expose three switches:

```text
Subagents
Agent teams
Background agents
```

Rules:

- A switch means "allow this capability when a run requests it."
- A switch does not mean "auto-spawn agents on every run."
- A disabled switch always prevents the capability, even if a per-run request asks for it.
- An unavailable runtime disables the switch and shows a backend-supplied reason.
- Frontend state never grants capability. Rust validates every request.

### Per-run invocation

Composer/run options must allow:

```text
subagents: off | allowed
agent team: off | allowed + teamConfig
background: off | run in background
```

Rules:

- Defaults come from settings.
- The user can turn a capability off for a single run.
- The user cannot turn a disabled global setting on for a single run.
- Agent teams are allowed only at run start for this plan.
- `agentTeam = allowed` requires `teamConfig` in the same `StartRunRequest`.
- `agentTeam = off` requires `teamConfig = null`.
- `teamConfig` must name a topology, one lead profile, one or more member profiles, max turns per goal, and shared memory policy.
- Nested teams are not allowed.
- Subagents inside team members are allowed only when both `subagents` and `agentTeams` are allowed and depth/concurrency policy permits it.
- Background start is canonical through `start_run(agentOptions.background = background)`.
- No public `start_background_agent` Tauri command may start a separate run path.

### Capability availability

`AgentCapabilitiesPayload` must distinguish:

```text
compiled
runtime storage open
permission runtime available
agent profiles valid
background supervisor available
workspace isolation available for write mode
```

Availability is true only when all required runtime components for that capability exist.

### Background agent semantics

Background agents must support:

```text
start
list
get detail
stream updates through journal projection
send follow-up input when waiting for user
cancel
pause
resume after interruption
delete archived record
```

States:

```text
queued
running
waiting_for_permission
waiting_for_input
paused
cancelling
cancelled
succeeded
failed
interrupted
recoverable
archived
```

Rules:

- A background agent is not just `tokio::spawn`.
- A background agent has a durable registry entry before model/tool execution starts.
- On process restart, `running`, `waiting_for_permission`, and `waiting_for_input` records are recovered by durable state, not by in-memory handles.
- If a task cannot safely resume, it becomes `interrupted` with a safe reason and explicit user action.
- No raw child transcript, raw provider request, secret, private path error, or tool payload is stored in UI state.

Lifecycle operations:

| Operation | Valid source states | Result state | Notes |
|---|---|---|---|
| `start_run(background)` | none | `queued` then `running` | Canonical user-facing start path. Creates durable background record before execution. |
| `pause` | `queued`, `running`, `waiting_for_input` | `paused` | Does not discard pending input or permission metadata. |
| `resume` | `paused`, `interrupted`, `recoverable` | `queued` then `running` or `waiting_for_input` | Creates a new attempt id when resuming after `interrupted`. |
| `cancel` | `queued`, `running`, `waiting_for_permission`, `waiting_for_input`, `paused`, `recoverable` | `cancelling` then `cancelled` | Cancels active handles when present and records a terminal journal event. |
| `send_input` | `waiting_for_input`, `recoverable` with input request | `queued` then `running` | Input is redacted before journal write. |
| `resolve_permission` | `waiting_for_permission`, `recoverable` with pending decision | `queued` then `running`, or `failed` on denial policy | Uses the existing permission surface with background agent attribution. |
| `archive` | `cancelled`, `succeeded`, `failed`, `interrupted` | `archived` | Hides the record from default lists but preserves audit and replay. |
| `delete` | `archived` | deleted row tombstone | Deletes only archived records. Audit, journal, and redacted replay remain immutable. |
| startup recovery | `running`, `cancelling` | `interrupted` | Used when no live supervisor/attempt can be reattached. |
| startup recovery | `waiting_for_permission`, `waiting_for_input` | `recoverable` | Requires durable pending request metadata. |

There is no public `recover_background_agent` command. Recovery is a startup manager action; user-initiated continuation uses `resume_background_agent`, `send_background_agent_input`, or the existing permission resolution command.

## Non-Negotiable Design Rules

- Rust backend is the policy authority.
- Frontend can request capability use; frontend cannot decide runtime eligibility.
- Tauri commands are IPC boundaries only.
- Public payloads live in `crates/jyowo-harness-contracts` unless they are desktop-only shell wrappers.
- Stable public payloads derive `Serialize`, `Deserialize`, and `JsonSchema`.
- Zod schemas mirror Rust contract shapes.
- `unsafe_code = "forbid"` remains unchanged.
- No production fake implementations.
- No no-op command that returns success.
- No hardcoded capability true response.
- No setting-only implementation.
- No UI-only implementation.
- No raw secret in prompt, event, log, trace, screenshot, frontend state, support bundle, replay, or test snapshot.
- No same-checkout parallel writes by multiple agents.
- `jyowo-harness-agent-runtime` may depend on L0-L2 crates and on the existing L3 `jyowo-harness-subagent` / `jyowo-harness-team` primitives. `jyowo-harness-subagent` and `jyowo-harness-team` must not depend back on `jyowo-harness-agent-runtime`.
- `jyowo-harness-sdk` must not own agent runtime state machines, storage, or policy decisions.
- No cross-process guarantee may be claimed for `jyowo-harness-team` until a durable bus exists. This plan keeps team execution run-scoped and explicitly local.
- No background agent available flag may become true until restart recovery tests pass.

## Target Runtime Model

```text
Settings record
  -> backend capability resolver
  -> per-run AgentRunOptions
  -> jyowo-harness-agent-runtime resolved AgentRuntimePolicy
  -> SDK facade harness session assembly
  -> optional subagent tool
  -> optional run-scoped team lead/member orchestration
  -> optional background registry/supervisor
  -> journal events
  -> conversation/background read models
  -> Zod-validated frontend rendering
```

## Target Storage

Runtime storage belongs under workspace `.jyowo/runtime`.

```text
.jyowo/runtime/execution-settings.json
.jyowo/runtime/agent-profiles.json
.jyowo/runtime/agent-runtime.sqlite
.jyowo/runtime/agent-worktrees/
.jyowo/runtime/events/
.jyowo/runtime/blobs/
```

`agent-runtime.sqlite` owns durable state for:

```text
agent profiles cache metadata
background agent registry
background agent lifecycle attempts
agent team task list
agent team mailbox
workspace isolation leases
restart recovery markers
```

JSONL journal remains the public event source for replay and conversation projection.

## Target Contract Sketch

The final implementation may split these into focused files, but the shape must remain stable and Zod-backed.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilitiesPayload {
    pub subagents_enabled: bool,
    pub agent_teams_enabled: bool,
    pub background_agents_enabled: bool,
    pub subagents_available: bool,
    pub agent_teams_available: bool,
    pub background_agents_available: bool,
    pub unavailable_reasons: Vec<AgentCapabilityUnavailableReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentCapabilityUnavailableReason {
    NotCompiled { capability: AgentCapabilityKind },
    RuntimeStoreUnavailable { capability: AgentCapabilityKind, message: String },
    PermissionRuntimeUnavailable { capability: AgentCapabilityKind },
    InvalidAgentProfiles { capability: AgentCapabilityKind, message: String },
    BackgroundSupervisorUnavailable { message: String },
    WorkspaceIsolationUnavailable { capability: AgentCapabilityKind, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    pub id: String,
    pub scope: AgentProfileScope,
    pub role: String,
    pub description: String,
    pub model_config_override: Option<AgentProfileModelOverride>,
    pub tool_allowlist: Option<Vec<String>>,
    pub tool_blocklist: Vec<String>,
    pub sandbox_inheritance: AgentProfileSandboxInheritance,
    pub memory_scope: AgentProfileMemoryScope,
    pub context_mode: AgentProfileContextMode,
    pub max_turns: u32,
    pub max_depth: u8,
    pub default_workspace_isolation: AgentWorkspaceIsolationMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileModelOverride {
    pub provider_config_id: Option<String>,
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentProfileScope {
    Builtin,
    User,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentProfileSandboxInheritance {
    InheritParent,
    NarrowOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentProfileMemoryScope {
    None,
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentProfileContextMode {
    Minimal,
    Focused,
    FullWorkspace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunOptions {
    pub subagents: AgentUsePolicy,
    pub agent_team: AgentUsePolicy,
    pub team_config: Option<AgentTeamRunConfig>,
    pub background: BackgroundRunPolicy,
    pub workspace_isolation: AgentWorkspaceIsolationMode,
    pub max_depth: u8,
    pub max_concurrent_subagents: u32,
    pub max_team_members: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentTeamRunConfig {
    pub topology: AgentTeamTopology,
    pub lead_profile_id: String,
    pub member_profile_ids: Vec<String>,
    pub max_turns_per_goal: u32,
    pub shared_memory_policy: AgentTeamSharedMemoryPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentTeamTopology {
    CoordinatorWorker,
    PeerToPeer,
    RoleRouted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentTeamSharedMemoryPolicy {
    None,
    SummariesOnly,
    RedactedMailbox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentUsePolicy {
    Off,
    Allowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundRunPolicy {
    Foreground,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspaceIsolationMode {
    ReadOnly,
    PatchOnly,
    GitWorktree,
}
```

All Rust and Zod enum serialized values in this contract use `snake_case`.
Examples: `coordinator_worker`, `peer_to_peer`, `role_routed`, `git_worktree`.

## File Map

Create or modify only these areas unless a task explicitly identifies an additional file.

### Contracts

```text
crates/jyowo-harness-contracts/src/capability.rs
crates/jyowo-harness-contracts/src/conversation.rs
crates/jyowo-harness-contracts/src/events/background_agent.rs
crates/jyowo-harness-contracts/src/events/mod.rs
crates/jyowo-harness-contracts/src/lib.rs
crates/jyowo-harness-contracts/src/schema_export.rs
crates/jyowo-harness-contracts/tests/agent_orchestration_contracts.rs
crates/jyowo-harness-contracts/tests/m1_contracts.rs
```

### Journal and Projection

```text
crates/jyowo-harness-journal/src/conversation_worktree_projector.rs
crates/jyowo-harness-journal/src/conversation_read_model.rs
crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs
crates/jyowo-harness-journal/tests/conversation_read_model.rs
```

### Agent Runtime L3

```text
Cargo.toml
crates/jyowo-harness-agent-runtime/Cargo.toml
crates/jyowo-harness-agent-runtime/src/lib.rs
crates/jyowo-harness-agent-runtime/src/store.rs
crates/jyowo-harness-agent-runtime/src/migrations.rs
crates/jyowo-harness-agent-runtime/src/policy.rs
crates/jyowo-harness-agent-runtime/src/profiles.rs
crates/jyowo-harness-agent-runtime/src/subagents.rs
crates/jyowo-harness-agent-runtime/src/teams.rs
crates/jyowo-harness-agent-runtime/src/background.rs
crates/jyowo-harness-agent-runtime/src/isolation.rs
crates/jyowo-harness-agent-runtime/tests/agent_orchestration_profiles.rs
crates/jyowo-harness-agent-runtime/tests/agent_orchestration_policy.rs
crates/jyowo-harness-agent-runtime/tests/agent_orchestration_background.rs
crates/jyowo-harness-agent-runtime/tests/agent_orchestration_isolation.rs
crates/jyowo-harness-agent-runtime/tests/agent_runtime_store.rs
crates/jyowo-harness-agent-runtime/tests/agents_team.rs
```

### SDK Facade and Runtime Assembly

Use the existing split `harness/` modules. Keep `harness.rs` as the module root and core `Harness` definition; do not add new agent orchestration business logic to the root file when a focused module exists.

```text
crates/jyowo-harness-sdk/Cargo.toml
crates/jyowo-harness-sdk/src/lib.rs
crates/jyowo-harness-sdk/src/harness.rs
crates/jyowo-harness-sdk/src/harness/accessors.rs
crates/jyowo-harness-sdk/src/harness/conversation.rs
crates/jyowo-harness-sdk/src/harness/session_runtime.rs
crates/jyowo-harness-sdk/src/harness/team_runtime.rs
crates/jyowo-harness-sdk/src/harness/tool_pool.rs
crates/jyowo-harness-sdk/src/harness/types.rs
crates/jyowo-harness-sdk/src/builder.rs
crates/jyowo-harness-sdk/tests/facade.rs
crates/jyowo-harness-sdk/tests/runtime_assembly.rs
```

### Existing Subagent and Team Crates

```text
crates/jyowo-harness-subagent/src/lib.rs
crates/jyowo-harness-subagent/tests/contract.rs
crates/jyowo-harness-subagent/tests/default_runner.rs
crates/jyowo-harness-subagent/tests/permission_bridge.rs
crates/jyowo-harness-team/src/lib.rs
crates/jyowo-harness-team/tests/contract.rs
crates/jyowo-harness-team/tests/routing.rs
```

### Desktop Backend

Use the existing split `commands/` modules. `commands/mod.rs` is the command wrapper and re-export layer; request/response structs belong in `commands/contracts.rs`, settings and capability availability in `commands/providers.rs`, run start flow in `commands/conversations.rs`, runtime assembly in `commands/runtime.rs`, and new agent/background command domains in focused modules.

```text
apps/desktop/src-tauri/Cargo.toml
apps/desktop/src-tauri/src/lib.rs
apps/desktop/src-tauri/src/commands/**
apps/desktop/src-tauri/src/commands/stores/mod.rs
apps/desktop/src-tauri/src/agent_supervisor.rs
apps/desktop/src-tauri/src/bin/jyowo-agent-supervisor.rs
apps/desktop/src-tauri/build.rs
apps/desktop/src-tauri/binaries/README.md
apps/desktop/src-tauri/capabilities/default.json
apps/desktop/src-tauri/tauri.conf.json
apps/desktop/src-tauri/tests/commands.rs
apps/desktop/src-tauri/tests/commands/agents.rs
apps/desktop/src-tauri/tests/commands/background_agents.rs
apps/desktop/src-tauri/tests/commands/automations.rs
apps/desktop/src-tauri/tests/commands/runs_permissions.rs
apps/desktop/src-tauri/tests/commands/conversations.rs
apps/desktop/src-tauri/tests/agent_orchestration_e2e.rs
```

### Frontend

```text
apps/desktop/src/shared/tauri/commands.ts
apps/desktop/src/shared/tauri/commands.test.ts
apps/desktop/src/testing/command-client.ts
apps/desktop/src/features/settings/ExecutionSettings.tsx
apps/desktop/src/features/settings/ExecutionSettings.test.tsx
apps/desktop/src/features/conversation/Composer.tsx
apps/desktop/src/features/conversation/Composer.test.tsx
apps/desktop/src/features/conversation/use-conversation.ts
apps/desktop/src/features/conversation/use-agent-profiles.ts
apps/desktop/src/features/conversation/use-agent-profiles.test.ts
apps/desktop/src/features/conversation/ConversationWorkspace.tsx
apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx
apps/desktop/src/features/conversation/timeline/conversation-timeline-selectors.ts
apps/desktop/src/features/conversation/AgentActivitySegment.tsx
apps/desktop/src/features/conversation/AgentActivitySegment.test.tsx
apps/desktop/src/features/background-agents/BackgroundAgentsPanel.tsx
apps/desktop/src/features/background-agents/BackgroundAgentsPanel.test.tsx
apps/desktop/src/features/background-agents/use-background-agents.ts
apps/desktop/src/routes/background-agents.tsx
apps/desktop/src/routes/background-agents.lazy.tsx
apps/desktop/src/app/router.tsx
apps/desktop/src/routeTree.gen.ts
```

`apps/desktop/src/routeTree.gen.ts` is generated by TanStack Router tooling. Do not edit it by hand. If route files change, run the existing desktop route generation path through the normal desktop check/build command and commit the generated output only when the tool updates it.

### Docs and Gates

```text
docs/backend/backend-runtime.md
docs/backend/backend-engineering.md
docs/backend/backend-quality.md
docs/frontend/frontend-engineering.md
docs/frontend/frontend-quality.md
scripts/check-agent-orchestration-no-fakes.mjs
scripts/check-agent-orchestration-no-fakes.test.mjs
scripts/build-agent-supervisor-sidecar.mjs
package.json
```

## Required Task Close Gate

Every task below inherits this close gate. A task is not complete until all items pass.

- [ ] Run the task-specific tests listed in the task.
- [ ] Run the listed package gate for the touched area.
- [ ] Run the anti-fake search gate after any task that changes production code:

```bash
pnpm check:agent-orchestration-no-fakes
```

Expected:

```text
exit code 0
```

- [ ] Dispatch a read-only subagent audit using this prompt, replacing `N` with the task number:

```text
Audit Task N from docs/plans/2026-06-30-agent-orchestration-full-implementation.md.

Read the task section, inspect the diff, and verify:
- every file listed in the task was implemented or intentionally left untouched with evidence
- no placeholder, no no-op command, no hardcoded success, and no production fake remains
- tests prove the behavior and include negative/fail-closed cases
- Rust remains the policy authority
- frontend schemas match Rust contracts
- permission, sandbox, MCP, journal, replay, and redaction boundaries are preserved
- background/team/subagent behavior is not merely a setting flag

Return PASS or FAIL.
Use file and line evidence.
Do not modify files.
```

- [ ] Record the subagent result in the implementation notes for the task:

```text
Task N subagent audit: PASS
Agent id: <id>
Evidence: <one-line summary with file paths>
```

If the subagent returns FAIL, fix the issue, rerun tests, and rerun the subagent audit.

## Task 0: Initial Anti-fake Gate

**Files:**

- Create: `scripts/check-agent-orchestration-no-fakes.mjs`
- Create: `scripts/check-agent-orchestration-no-fakes.test.mjs`
- Modify: `package.json`
- Modify: `docs/backend/backend-quality.md`
- Modify: `docs/frontend/frontend-quality.md`

**Required behavior:**

Create the anti-fake gate before any production implementation task. Later tasks may harden the pattern list, but the command must exist before Task 1.

The initial gate scans only agent-orchestration production surfaces:

```text
apps/desktop/src-tauri/src/commands/**
apps/desktop/src/features/settings/ExecutionSettings.tsx
apps/desktop/src/features/conversation/Composer.tsx
apps/desktop/src/features/conversation/AgentActivitySegment.tsx
apps/desktop/src/features/background-agents/**
crates/jyowo-harness-agent-runtime/**
crates/jyowo-harness-subagent/**
crates/jyowo-harness-team/**
```

It fails those scoped production files if it finds:

- Tauri agent command handlers returning fixed success without calling SDK/runtime code
- user-facing future-tense placeholder labels, experimental labels, unimplemented labels, or unfinished-work markers only when agent-related context appears near the match
- production files named or described as fake agent runners, fake background providers, or mock agent runtimes

Agent-related context means one of:

```text
subagent
agent team
background agent
agent runtime
agent orchestration
```

Non-agent generic placeholder UI, normal test fakes, fixture mocks, and unrelated product placeholder text must not fail this gate.

Do not add `*_available = false` production scans in Task 0. The current baseline contains hardcoded false capability values before the runtime is wired. Task 3 must add and pass strict scans for `subagents_available = false` and `agent_teams_available = false` after those capabilities are wired. Task 12 must add and pass the strict scan for `background_agents_available = false` after supervisor recovery is implemented. Temporary allowlists for these hardcoded availability fields are forbidden after the relevant task passes.

The gate excludes:

```text
docs/**
**/*.test.ts
**/*.test.tsx
**/tests/**
target/**
node_modules/**
dist/**
storybook-static/**
```

**Steps:**

- [ ] Implement the script with the scoped production path list above, agent-context proximity checks, and ignore rules.
- [ ] Add node tests that create temporary files and prove forbidden patterns fail.
- [ ] Add node tests proving unrelated placeholder/fake/mock text outside agent-orchestration production surfaces does not fail.
- [ ] Add package script:

```json
"check:agent-orchestration-no-fakes": "node --test scripts/check-agent-orchestration-no-fakes.test.mjs && node scripts/check-agent-orchestration-no-fakes.mjs"
```

- [ ] Add this script into root `pnpm check` after docs and before desktop/rust gates.
- [ ] Document the gate in frontend and backend quality docs.
- [ ] Run the new script once before starting Task 1.

**Verification:**

```bash
pnpm check:agent-orchestration-no-fakes
pnpm check:docs
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 1: Contract Baseline and Capability Reasons

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/capability.rs`
- Modify: `crates/jyowo-harness-contracts/src/lib.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Create: `crates/jyowo-harness-contracts/tests/agent_orchestration_contracts.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`

**Steps:**

- [ ] Add contract types for `AgentProfile`, `AgentProfileScope`, `AgentProfileModelOverride`, `AgentProfileSandboxInheritance`, `AgentProfileMemoryScope`, `AgentProfileContextMode`, `AgentRunOptions`, `AgentTeamRunConfig`, `AgentTeamTopology`, `AgentTeamSharedMemoryPolicy`, `AgentUsePolicy`, `BackgroundRunPolicy`, `AgentWorkspaceIsolationMode`, and expanded `AgentCapabilityUnavailableReason`.
- [ ] Export schemas for every new contract type from `schema_export.rs`.
- [ ] Add Rust tests that serialize and deserialize representative payloads:
  - capability unavailable because not compiled
  - capability unavailable because runtime store failed
  - capability unavailable because background supervisor failed
  - builtin profile with read-only scope
  - user profile with model override, tool allowlist, memory scope, context mode, and default workspace isolation
  - run options with subagents allowed, team off, foreground
  - run options with agent team allowed and complete `teamConfig`
  - invalid run options with `agentTeam = allowed` and missing `teamConfig`
  - invalid run options with `agentTeam = off` and non-null `teamConfig`
  - run options with background and git worktree isolation
- [ ] Mirror the same shapes in Zod.
- [ ] Add Zod tests for valid and invalid payloads. Invalid cases:
  - unknown capability reason type
  - unknown isolation mode
  - unknown team topology
  - unknown profile scope
  - invalid profile id
  - empty team member list
  - negative concurrency
  - background requested with invalid policy string

**Verification:**

```bash
cargo test -p jyowo-harness-contracts agent_orchestration_contracts --test agent_orchestration_contracts
pnpm -C apps/desktop test -- commands.test.ts
pnpm check:rust
pnpm check:desktop
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 2: Agent Runtime Store and Profile Registry

**Files:**

- Modify: `Cargo.toml`
- Modify: `crates/jyowo-harness-contracts/src/capability.rs`
- Modify: `crates/jyowo-harness-contracts/src/lib.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `crates/jyowo-harness-contracts/tests/agent_orchestration_contracts.rs`
- Create: `crates/jyowo-harness-agent-runtime/Cargo.toml`
- Create: `crates/jyowo-harness-agent-runtime/src/lib.rs`
- Create: `crates/jyowo-harness-agent-runtime/src/store.rs`
- Create: `crates/jyowo-harness-agent-runtime/src/migrations.rs`
- Create: `crates/jyowo-harness-agent-runtime/src/profiles.rs`
- Create: `crates/jyowo-harness-agent-runtime/tests/agent_runtime_store.rs`
- Create: `crates/jyowo-harness-agent-runtime/tests/agent_orchestration_profiles.rs`
- Modify: `crates/jyowo-harness-sdk/src/lib.rs`
- Modify: `crates/jyowo-harness-sdk/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Create: `apps/desktop/src-tauri/src/commands/agents.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Create: `apps/desktop/src-tauri/tests/commands/agents.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Modify: `docs/backend/backend-engineering.md`

**Required behavior:**

- Public profile payloads live in `crates/jyowo-harness-contracts` and export `JsonSchema`; `jyowo-harness-agent-runtime` imports those types and owns validation/storage.
- Frontend mirrors profile payloads in `apps/desktop/src/shared/tauri/commands.ts` with Zod and rejects unknown fields.
- `AgentRuntimeStore` owns `.jyowo/runtime/agent-runtime.sqlite`.
- `AgentRuntimeStore::open(...)` runs versioned migrations before any profile, background, team, or isolation operation can use the store.
- No task may open ad hoc `rusqlite` connections or create ad hoc `agent-runtime.sqlite` tables outside `AgentRuntimeStore`.
- Initial migrations create at least:
  - schema version tracking
  - agent profile metadata cache
  - background agent registry
  - background agent attempts
  - agent team task list
  - agent team mailbox
  - workspace isolation leases
  - restart recovery markers
- Profile storage path is `.jyowo/runtime/agent-profiles.json`.
- Supported scopes are `builtin`, `user`, and `project`.
- Profile ids are stable strings with lowercase letters, digits, `_`, and `-`.
- A profile defines:
  - role
  - description
  - model config override optional
  - tool allowlist optional
  - tool blocklist
  - sandbox inheritance
  - memory scope
  - context mode
  - max turns
  - max depth
  - default workspace isolation
- Builtin profiles are read-only and never written into the user file.
- User/project profile files are validated before persistence.
- Invalid profile files are quarantined by rename and capability availability reports `InvalidAgentProfiles`.

**Steps:**

- [ ] Add `jyowo-harness-agent-runtime` as an L3 workspace member.
- [ ] Add `jyowo-harness-agent-runtime` to the backend layer table as L3 cross-domain orchestration.
- [ ] Define `agents-subagent` and `agents-team` features in `jyowo-harness-agent-runtime`, and make SDK features delegate to the runtime crate features instead of owning runtime behavior.
- [ ] Add or update profile contract types in `jyowo-harness-contracts`; export schemas and mirror them in frontend Zod.
- [ ] Implement `AgentRuntimeStore` and `migrations.rs` with idempotent open/reopen behavior.
- [ ] Add migration tests for create, open, reopen, idempotence, missing runtime directory creation, and incompatible schema rejection.
- [ ] Implement profile validation inside `jyowo-harness-agent-runtime` using the public contract types.
- [ ] Add SDK facade methods that delegate to `jyowo-harness-agent-runtime`; do not place profile storage or validation logic in SDK.
- [ ] Implement atomic load/save using the same symlink and temp-file safety pattern as provider settings.
- [ ] Add desktop commands:
  - `list_agent_profiles`
  - `save_agent_profile`
  - `delete_agent_profile`
- [ ] Register commands in `apps/desktop/src-tauri/src/lib.rs`.
- [ ] Add command tests for valid save/list/delete, unknown payload rejection, readonly builtin delete rejection, and invalid file quarantine.

**Verification:**

```bash
cargo test -p jyowo-harness-contracts agent_orchestration_contracts --test agent_orchestration_contracts
cargo test -p jyowo-harness-agent-runtime agent_runtime_store --test agent_runtime_store
cargo test -p jyowo-harness-agent-runtime agent_orchestration_profiles --test agent_orchestration_profiles
cargo test -p jyowo-desktop-shell agent_profile
pnpm -C apps/desktop test -- commands.test.ts
pnpm check:backend-docs
pnpm check:rust
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 3: Backend Capability Resolver and Settings Semantics

**Files:**

- Create: `crates/jyowo-harness-agent-runtime/src/policy.rs`
- Create: `crates/jyowo-harness-agent-runtime/tests/agent_orchestration_policy.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/accessors.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/types.rs`
- Modify: `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/providers.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/automations.rs`
- Modify: `scripts/check-agent-orchestration-no-fakes.mjs`
- Modify: `scripts/check-agent-orchestration-no-fakes.test.mjs`

**Required behavior:**

- Enable `agents-subagent` and `agents-team` for desktop `jyowo-harness-sdk` dependency.
- Remove the bare hardcoded `agent_capabilities_available()` implementation from `commands/providers.rs`. The desktop settings command must delegate to `AgentCapabilityResolver`.
- Subagents available when:
  - desktop is compiled with `agents-subagent`
  - execution settings store can load
  - agent profile registry is valid
  - stream permission runtime exists
- Agent teams available when:
  - subagents are available
  - desktop is compiled with `agents-team`
  - team runtime policy can be created
- Background agents available when:
  - background registry opens
  - supervisor is available
  - restart recovery marker check passes
  - permission runtime exists
- Settings save must fail closed if a requested enabled capability is unavailable.
- Existing invalid settings files must be handled the same way as current execution settings: remove or quarantine and return defaults.
- After this task, the anti-fake gate fails production code that hardcodes `subagents_available = false` or `agent_teams_available = false`.
- Do not add or keep a temporary allowlist for subagent/team hardcoded false availability after this task passes.
- `background_agents_available` may resolve unavailable until Task 12 only through a typed `BackgroundSupervisorUnavailable` resolver branch. A naked `background_agents_available = false` assignment outside the resolver branch is forbidden.

**Steps:**

- [ ] Add `ResolvedAgentCapabilityPolicy` and `AgentCapabilityResolver` in `jyowo-harness-agent-runtime`.
- [ ] Add SDK facade wiring that delegates capability resolution to `jyowo-harness-agent-runtime`.
- [ ] Wire desktop settings validation in `commands/providers.rs` to the resolver.
- [ ] Add tests for each unavailable reason.
- [ ] Add tests that enabling unavailable capabilities returns `invalid_payload`.
- [ ] Add tests that settings toggles persist only after backend validation.
- [ ] Extend the anti-fake gate and tests to forbid hardcoded subagent/team unavailable values in production code.
- [ ] Add scanner tests proving a typed `BackgroundSupervisorUnavailable` resolver branch is allowed before Task 12, while unrelated naked background false assignments fail.

**Verification:**

```bash
cargo test -p jyowo-harness-agent-runtime agent_orchestration_policy --test agent_orchestration_policy
cargo test -p jyowo-desktop-shell execution_settings_agent_capabilities
pnpm check:agent-orchestration-no-fakes
pnpm check:rust
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 4: Run Options Contract and Composer IPC

**Files:**

- Modify: `crates/jyowo-harness-agent-runtime/src/policy.rs`
- Modify: `crates/jyowo-harness-agent-runtime/tests/agent_orchestration_policy.rs`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/runs_permissions.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Create: `apps/desktop/src/features/conversation/use-agent-profiles.ts`
- Create: `apps/desktop/src/features/conversation/use-agent-profiles.test.ts`
- Modify: `apps/desktop/src/features/conversation/Composer.tsx`
- Modify: `apps/desktop/src/features/conversation/Composer.test.tsx`
- Modify: `apps/desktop/src/features/conversation/use-conversation.ts`

**Required behavior:**

- `StartRunRequest` accepts optional `agentOptions`.
- If omitted, backend derives options from execution settings.
- Per-run options cannot enable a capability disabled in settings.
- Per-run options cannot use unavailable runtime capabilities.
- Agent teams can only be requested at run start.
- `agentOptions.agentTeam = allowed` requires a valid `teamConfig`.
- `agentOptions.agentTeam = off` rejects non-null `teamConfig`.
- Background mode cannot be used with missing `conversationId`.
- Background mode starts through `start_run` only and returns the background agent id in the start response when a record is created.
- No Composer or frontend code calls a separate `start_background_agent` command.
- Invalid `maxDepth`, `maxConcurrentSubagents`, or `maxTeamMembers` fails closed.
- Composer renders compact controls only when backend says the capability is available.
- Composer submits validated `agentOptions` through `shared/tauri` only.

**Steps:**

- [ ] Add `agent_options: Option<AgentRunOptions>` to Rust `StartRunRequest` in `commands/contracts.rs`.
- [ ] Add backend merge function in `jyowo-harness-agent-runtime` and call it from `commands/conversations.rs` before run execution:

```text
ExecutionSettingsRecord + optional AgentRunOptions + AgentCapabilitiesPayload
  -> ResolvedAgentRuntimePolicy
```

- [ ] Add negative command tests for disabled settings, unavailable runtime, and invalid numeric limits.
- [ ] Add command tests for missing team config, invalid topology, empty member profile list, invalid lead profile id, and background start response id.
- [ ] Add Zod schema and tests.
- [ ] Add Composer controls:
  - subagent allow switch
  - agent team allow switch
  - background run switch
  - workspace isolation selector when write-capable agent mode is on
- [ ] Add Composer tests for available, unavailable, disabled, and submit payload states.

**Verification:**

```bash
cargo test -p jyowo-harness-agent-runtime agent_orchestration_policy --test agent_orchestration_policy
cargo test -p jyowo-desktop-shell start_run_agent_options
pnpm -C apps/desktop test -- commands.test.ts Composer.test.tsx
pnpm check:desktop
pnpm check:rust
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 5: Workspace Isolation Manager

**Files:**

- Modify: `crates/jyowo-harness-agent-runtime/src/store.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/migrations.rs`
- Create: `crates/jyowo-harness-agent-runtime/src/isolation.rs`
- Modify: `crates/jyowo-harness-agent-runtime/tests/agent_runtime_store.rs`
- Create: `crates/jyowo-harness-agent-runtime/tests/agent_orchestration_isolation.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- Modify: `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/runs_permissions.rs`

**Required behavior:**

- Support modes:
  - `ReadOnly`: no write tools for child agents.
  - `PatchOnly`: child agent can produce patch artifacts but cannot apply to workspace.
  - `GitWorktree`: child agent writes in a dedicated git worktree.
- Worktrees live under `.jyowo/runtime/agent-worktrees/`.
- Each worktree lease records:
  - lease id
  - parent conversation id
  - parent run id
  - agent id
  - path
  - branch
  - base commit
  - status
  - created at
  - updated at
- Non-git workspaces fail closed for `GitWorktree`.
- Dirty workspace handling:
  - allow read-only
  - allow patch-only
  - reject git worktree unless a safe base commit is available
- Same branch cannot be checked out by two active write-capable agents.
- Cleanup never deletes a worktree with uncommitted changes unless it first creates a patch artifact and marks the lease `cleanup_blocked`.
- Isolation persistence uses `AgentRuntimeStore` from Task 2. Do not open a separate SQLite database or create tables outside the versioned migration module.

**Steps:**

- [ ] Implement lease repository methods on `AgentRuntimeStore` using the Task 2 workspace isolation tables.
- [ ] Implement git discovery using non-interactive `git` commands through a bounded backend helper.
- [ ] Add tests with temporary git repositories:
  - create lease
  - reject non-git workspace
  - reject duplicate branch lease
  - detect dirty worktree on cleanup
  - resume lease metadata after reopening store
- [ ] Add migration tests proving the isolation tables exist after a fresh Task 2 store open and remain readable after reopening.
- [ ] Wire policy resolver so write-capable subagent/team/background modes require isolation.

**Verification:**

```bash
cargo test -p jyowo-harness-agent-runtime agent_orchestration_isolation --test agent_orchestration_isolation
cargo test -p jyowo-desktop-shell workspace_isolation
pnpm check:rust
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 6: Subagent Runtime Wiring

**Files:**

- Create: `crates/jyowo-harness-agent-runtime/src/subagents.rs`
- Modify: `crates/jyowo-harness-sdk/src/builder.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/tool_pool.rs`
- Modify: `crates/jyowo-harness-sdk/tests/facade.rs`
- Modify: `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`
- Modify: `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/runs_permissions.rs`

**Required behavior:**

- Desktop harness installs a real subagent runner when resolved policy allows subagents.
- Engine exposes `agent` tool only when:
  - feature is compiled
  - runner capability is installed
  - global setting allows subagents
  - per-run policy allows subagents
  - depth and concurrency limits allow delegation
- Child subagent inherits or narrows permissions. It never expands permissions.
- Child MCP servers are inherited only when trusted and allowed by policy.
- Child sandbox mode cannot be less restrictive than parent unless a backend policy explicitly rejects or approves it through `PermissionBroker`.
- Subagent transcript is summarized before parent visibility.
- Raw child transcript is stored only as redacted blob/replay reference, never inline conversation text.

**Steps:**

- [ ] Implement L3 adapter that creates `DefaultSubagentRunner` from current harness dependencies.
- [ ] Add SDK assembly code that installs the L3 adapter without owning runner policy.
- [ ] Add policy checks before installing `ToolCapability::SubagentRunner`.
- [ ] Add runtime tests proving `agent` tool appears only when allowed.
- [ ] Add runtime tests proving disabled global settings remove the tool.
- [ ] Add permission bridge tests for subagent source attribution.
- [ ] Add command test that a real start run can invoke subagent tool in a scripted model flow.

**Verification:**

```bash
cargo test -p jyowo-harness-sdk facade --features agents-subagent
cargo test -p jyowo-harness-sdk runtime_assembly --features agents-subagent
cargo test -p jyowo-harness-agent-runtime subagents --features agents-subagent
cargo test -p jyowo-desktop-shell subagent_runtime
pnpm check:rust
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 7: Subagent Projection and Frontend Rendering

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/conversation.rs`
- Modify: `crates/jyowo-harness-contracts/tests/m1_contracts.rs`
- Modify: `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify: `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Modify: `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
- Modify: `crates/jyowo-harness-journal/tests/conversation_read_model.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Create: `apps/desktop/src/features/conversation/AgentActivitySegment.tsx`
- Create: `apps/desktop/src/features/conversation/AgentActivitySegment.test.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline-selectors.ts`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`

**Required behavior:**

- Add `AssistantSegment::AgentActivity`.
- Segment supports activity kind:
  - `subagent`
  - `agentTeam`
  - `backgroundAgent`
- Subagent projection displays:
  - role
  - task summary
  - status
  - safe result summary
  - permission state if waiting
  - event refs
- Projection is derived from journal events, not frontend-only state.
- Renderer covers loading, running, waiting, completed, failed, cancelled, and redacted states.

**Steps:**

- [ ] Add Rust segment contract and schema export.
- [ ] Project existing `SubagentSpawnedEvent`, `SubagentAnnouncedEvent`, `SubagentTerminatedEvent`, `SubagentStalledEvent`, and permission forwarded/resolved events.
- [ ] Update read-model paging so `ConversationWorktreePage` can return the new segment without dropping event refs or cursor semantics.
- [ ] Add projection tests from real event sequences.
- [ ] Add read-model tests proving agent activity segments survive page slicing and replay.
- [ ] Mirror segment schema in Zod and add invalid payload tests.
- [ ] Add renderer and component tests.

**Verification:**

```bash
cargo test -p jyowo-harness-contracts m1_contracts
cargo test -p jyowo-harness-journal conversation_worktree_projector
cargo test -p jyowo-harness-journal conversation_read_model
pnpm -C apps/desktop test -- commands.test.ts AgentActivitySegment.test.tsx ConversationWorkspace.test.tsx
pnpm check:desktop
pnpm check:rust
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 8: Run-scoped Agent Team Runtime

**Files:**

- Modify: `crates/jyowo-harness-subagent/src/lib.rs`
- Modify: `crates/jyowo-harness-subagent/tests/contract.rs`
- Modify: `crates/jyowo-harness-subagent/tests/default_runner.rs`
- Modify: `crates/jyowo-harness-subagent/tests/permission_bridge.rs`
- Create: `crates/jyowo-harness-agent-runtime/src/teams.rs`
- Create: `crates/jyowo-harness-agent-runtime/tests/agents_team.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/team_runtime.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- Modify: `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`
- Modify: `crates/jyowo-harness-team/src/lib.rs`
- Modify: `crates/jyowo-harness-team/tests/contract.rs`
- Modify: `crates/jyowo-harness-team/tests/routing.rs`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/runs_permissions.rs`

**Required behavior:**

- Agent teams are formal, non-experimental, run-scoped capability.
- A run can request a team lead and members through backend-approved profiles.
- `AgentRunOptions.teamConfig` is the only team configuration contract for run invocation.
- Runtime rejects `agentTeam = allowed` without `teamConfig`.
- Runtime rejects `teamConfig` when `agentTeam = off`.
- Runtime validates `leadProfileId`, every `memberProfileId`, `maxTurnsPerGoal`, `sharedMemoryPolicy`, and topology before any member starts.
- Supported topologies for desktop:
  - `coordinator_worker`
  - `peer_to_peer`
  - `role_routed`
- `Topology::Custom` remains rejected unless there is executable backend routing and tests.
- Team task list and mailbox are persisted in `agent-runtime.sqlite`.
- Team persistence uses `AgentRuntimeStore` from Task 2. Do not open a separate SQLite database or create team tables outside the versioned migration module.
- Team events are journaled:
  - created
  - member joined
  - message sent
  - message routed
  - task updated
  - turn completed
  - member left
  - terminated
- Team member permissions flow through parent broker with member attribution.
- Team cancellation cancels all active members and records terminal state.
- Team does not claim cross-process delivery.
- Existing subagent crate tests must be updated if team member delegation changes the subagent contract, runner behavior, or permission bridge attribution.

**Steps:**

- [ ] Add L3 run-scoped team coordinator that wraps existing `Harness::create_team(...)` from `crates/jyowo-harness-sdk/src/harness/team_runtime.rs`.
- [ ] Add SDK assembly code that delegates team startup to `jyowo-harness-agent-runtime`.
- [ ] Persist team task list and mailbox through `AgentRuntimeStore` before dispatch.
- [ ] Add missing contract events for task updates if they do not already exist.
- [ ] Update subagent runner/permission bridge contract only where team-member source attribution requires it.
- [ ] Add tests for each supported topology.
- [ ] Add tests for missing `teamConfig`, invalid profile ids, empty members, invalid `maxTurnsPerGoal`, and `teamConfig` supplied while team use is off.
- [ ] Add tests for permission attribution by team/member.
- [ ] Add cancellation tests proving member handles terminate.
- [ ] Add desktop `StartRunRequest` tests proving team is allowed only at run start.

**Verification:**

```bash
cargo test -p jyowo-harness-team
cargo test -p jyowo-harness-subagent
cargo test -p jyowo-harness-agent-runtime agents_team --features agents-team
cargo test -p jyowo-harness-sdk runtime_assembly --features agents-team
cargo test -p jyowo-desktop-shell agent_team_runtime
pnpm check:rust
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 9: Agent Team Projection and Invocation UI

**Files:**

- Modify: `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify: `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Modify: `apps/desktop/src/features/conversation/Composer.tsx`
- Modify: `apps/desktop/src/features/conversation/Composer.test.tsx`
- Modify: `apps/desktop/src/features/conversation/use-agent-profiles.ts`
- Modify: `apps/desktop/src/features/conversation/use-agent-profiles.test.ts`
- Modify: `apps/desktop/src/features/conversation/AgentActivitySegment.tsx`
- Modify: `apps/desktop/src/features/conversation/AgentActivitySegment.test.tsx`

**Required behavior:**

- Composer can request "allow agent team" for a run when available and enabled.
- Composer must send a complete `teamConfig` when the team toggle is on.
- Composer must clear `teamConfig` when the team toggle is off.
- Composer loads selectable lead/member profiles through `list_agent_profiles`, parsed by `shared/tauri` Zod schemas and cached with TanStack Query.
- Composer cannot submit a team config whose lead or member profile is missing from the latest backend profile list.
- UI does not expose standalone team creation outside run invocation.
- Team timeline segment displays:
  - topology
  - lead
  - members
  - current tasks
  - mailbox count
  - status
  - safe final summary
- Team mailbox details are safe summaries by default.
- Raw inter-agent messages are not shown unless redacted and explicitly safe for replay.

**Steps:**

- [ ] Project team lifecycle events into `AgentActivitySegment`.
- [ ] Add `use-agent-profiles.ts` backed by `list_agent_profiles`; cover loading, empty, error, and ready states in hook tests.
- [ ] Add Composer controls for topology, lead profile, member profiles, max turns per goal, and shared memory policy.
- [ ] Add Composer profile selection behavior for lead and members using backend profile ids only; never synthesize profile ids on the frontend.
- [ ] Add team renderer states for empty task list, active routing, failed member, cancelled team, completed team.
- [ ] Add Composer tests that team toggle is hidden when unavailable, disabled when settings are off, shows profile loading/error/empty states, submits valid `teamConfig`, rejects missing members, and rejects stale profile ids.
- [ ] Add Zod tests for projected team segment.

**Verification:**

```bash
cargo test -p jyowo-harness-journal conversation_worktree_projector
pnpm -C apps/desktop test -- commands.test.ts use-agent-profiles.test.ts Composer.test.tsx AgentActivitySegment.test.tsx
pnpm check:desktop
pnpm check:rust
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 10: Background Agent Durable Manager

**Files:**

- Modify: `crates/jyowo-harness-agent-runtime/src/store.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/migrations.rs`
- Create: `crates/jyowo-harness-agent-runtime/src/background.rs`
- Modify: `crates/jyowo-harness-agent-runtime/tests/agent_runtime_store.rs`
- Create: `crates/jyowo-harness-agent-runtime/tests/agent_orchestration_background.rs`
- Create: `crates/jyowo-harness-contracts/src/events/background_agent.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/mod.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Create: `apps/desktop/src-tauri/tests/commands/background_agents.rs`

**Required behavior:**

- Background record is durable before execution.
- Manager supports:
  - start
  - list
  - get
  - pause
  - resume
  - cancel
  - send input
  - archive
  - delete archived record
- Manager owns state transitions and rejects invalid transitions.
- Manager transition behavior must match the lifecycle table in Product Contract.
- Every transition writes a journal event and SQLite row update in a defined order.
- Background persistence uses `AgentRuntimeStore` from Task 2. Do not open a separate SQLite database or create background tables outside the versioned migration module.
- Active task handles are single-process only; durable state is restart-stable.
- On manager startup:
  - `running` becomes `interrupted`
  - `waiting_for_permission` remains recoverable only if pending decision exists
  - `waiting_for_input` remains recoverable
  - `cancelling` becomes `interrupted`
  - terminal states stay terminal
- Resuming an interrupted agent creates a new attempt id and links to prior attempt.
- Deleting is allowed only for `archived` records and must not delete immutable journal, audit, or replay data.
- Archive and delete operations emit explicit `BackgroundAgentArchived` and `BackgroundAgentDeleted` events. `BackgroundAgentStateChanged` alone is not enough for audit/replay.

**Steps:**

- [ ] Use the background registry and attempts tables created by the Task 2 migration; add only additive migrations if the table shape changes and update migration tests.
- [ ] Implement state machine with table-driven tests.
- [ ] Add table-driven tests for every lifecycle operation in the Product Contract transition table.
- [ ] Implement journal events:
  - `BackgroundAgentStarted`
  - `BackgroundAgentStateChanged`
  - `BackgroundAgentInputRequested`
  - `BackgroundAgentInputSubmitted`
  - `BackgroundAgentCancelled`
  - `BackgroundAgentCompleted`
  - `BackgroundAgentFailed`
  - `BackgroundAgentInterrupted`
  - `BackgroundAgentArchived`
  - `BackgroundAgentDeleted`
- [ ] Export schemas and add Rust contract tests for every background event variant, including archive and delete.
- [ ] Add redaction before journal write.
- [ ] Add tests for restart recovery, archive/delete audit behavior, archived-only delete, and invalid transitions.

**Verification:**

```bash
cargo test -p jyowo-harness-contracts background_agent
cargo test -p jyowo-harness-agent-runtime agent_orchestration_background --test agent_orchestration_background
cargo test -p jyowo-desktop-shell background_agent_manager
pnpm check:rust
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 11: Background Agent Tauri Commands and Frontend Surface

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Create: `apps/desktop/src-tauri/src/commands/background_agents.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/background_agents.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Create: `apps/desktop/src/features/background-agents/use-background-agents.ts`
- Create: `apps/desktop/src/features/background-agents/BackgroundAgentsPanel.tsx`
- Create: `apps/desktop/src/features/background-agents/BackgroundAgentsPanel.test.tsx`
- Create: `apps/desktop/src/routes/background-agents.tsx`
- Create: `apps/desktop/src/routes/background-agents.lazy.tsx`
- Modify: `apps/desktop/src/app/router.tsx`
- Generated: `apps/desktop/src/routeTree.gen.ts`

**Required behavior:**

- Background agent creation from the Composer uses `start_run` with `agentOptions.background = background`.
- This task must not add a public `start_background_agent` Tauri command.
- Tauri commands:
  - `list_background_agents`
  - `get_background_agent`
  - `pause_background_agent`
  - `resume_background_agent`
  - `cancel_background_agent`
  - `send_background_agent_input`
  - `archive_background_agent`
  - `delete_background_agent`
- Each command validates workspace, conversation, permission, and capability policy.
- `delete_background_agent` succeeds only for archived records.
- Frontend panel has loading, empty, error, ready, running, waiting, interrupted, terminal, and archived states.
- User can open a background agent from the conversation timeline and from the background agents route.
- Permission prompts show background agent id and parent conversation/run.

**Steps:**

- [ ] Add command request/response contracts in `commands/contracts.rs`, command implementations in `commands/background_agents.rs`, and Zod schemas in `shared/tauri`.
- [ ] Re-export command handlers from `commands/mod.rs` and register them in `generate_handler!`.
- [ ] Add command tests in `apps/desktop/src-tauri/tests/commands/background_agents.rs` for each command and each invalid state.
- [ ] Add command tests proving `start_run` is the only public background start path.
- [ ] Add delete tests for archived and non-archived records.
- [ ] Add TanStack Query hooks for list/detail/mutations.
- [ ] Add UI tests for state matrix.
- [ ] Add route and router registration.
- [ ] Regenerate `apps/desktop/src/routeTree.gen.ts` through the existing TanStack Router generation path. Do not edit the generated file manually.

**Verification:**

```bash
cargo test -p jyowo-desktop-shell background_agent_commands
pnpm -C apps/desktop test -- commands.test.ts BackgroundAgentsPanel.test.tsx
pnpm check:desktop
pnpm check:rust
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 12: Background Supervisor and Full App Restart Recovery

**Files:**

- Create: `apps/desktop/src-tauri/src/agent_supervisor.rs`
- Create: `apps/desktop/src-tauri/src/bin/jyowo-agent-supervisor.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/build.rs`
- Create: `apps/desktop/src-tauri/binaries/README.md`
- Modify: `apps/desktop/src-tauri/capabilities/default.json`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/src/commands/background_agents.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/background_agents.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/background.rs`
- Modify: `crates/jyowo-harness-agent-runtime/tests/agent_orchestration_background.rs`
- Modify: `scripts/check-agent-orchestration-no-fakes.mjs`
- Modify: `scripts/check-agent-orchestration-no-fakes.test.mjs`
- Create: `scripts/build-agent-supervisor-sidecar.mjs`
- Modify: `package.json`

**Required behavior:**

- Desktop can start a local supervisor process for background agents.
- Supervisor runs the same backend policy stack through SDK code.
- Supervisor receives no raw provider keys from frontend.
- Supervisor access is workspace-scoped and authenticated with a short-lived local token stored outside prompt, journal, logs, traces, and UI state.
- If supervisor cannot start, `backgroundAgentsAvailable` is false with `BackgroundSupervisorUnavailable`.
- If app UI exits but supervisor continues, background agent execution continues.
- If supervisor exits unexpectedly, durable manager marks affected agents `interrupted`.
- Restarting the app reconnects to existing supervisor or recovers durable states.
- Tauri packaging must include the supervisor as a sidecar through `bundle.externalBin`.
- `tauri.conf.json` uses the base sidecar path `binaries/jyowo-agent-supervisor` in `bundle.externalBin`.
- The build pipeline creates the platform-specific sidecar filename required by Tauri under `apps/desktop/src-tauri/binaries/`, for example:
  - `jyowo-agent-supervisor-x86_64-apple-darwin`
  - `jyowo-agent-supervisor-aarch64-apple-darwin`
  - `jyowo-agent-supervisor-x86_64-pc-windows-msvc.exe`
  - `jyowo-agent-supervisor-x86_64-unknown-linux-gnu`
- `scripts/build-agent-supervisor-sidecar.mjs` builds the supervisor binary for the active target triple and copies it to the Tauri sidecar filename.
- The root package exposes `pnpm build:agent-supervisor-sidecar`.
- The sidecar build command must run before every verification or build command that triggers `apps/desktop/src-tauri/build.rs`, including `cargo test -p jyowo-desktop-shell`, `pnpm check:rust`, `pnpm check:desktop:full`, and Tauri bundle commands.
- `apps/desktop/src-tauri/build.rs` validates that the expected sidecar file exists for the active `TARGET` and fails the build with a clear message if it is missing.
- `apps/desktop/src-tauri/binaries/README.md` documents that files in this directory are generated by the sidecar build script and must not contain secrets.
- Rust startup must use the Tauri v2 sidecar API (`tauri_plugin_shell::ShellExt` and `app.shell().sidecar("jyowo-agent-supervisor")`) or an explicitly documented equivalent if the project chooses not to expose `tauri-plugin-shell`.
- If `tauri-plugin-shell` is added, shell capability must remain scoped to the supervisor sidecar and must not expose arbitrary shell execution to frontend code.
- Supervisor control transport is local-only. Acceptable transports are Unix domain socket on macOS/Linux and named pipe on Windows, or loopback TCP with random port, local bind, and token-bound requests.
- Supervisor writes a workspace-scoped PID/lock file under `.jyowo/runtime/agent-supervisor.lock`.
- Supervisor and app exchange heartbeats. Missing heartbeat marks attached running attempts `interrupted` unless the app can reconnect to a live supervisor with matching workspace id and token epoch.
- Supervisor stdout/stderr must be redacted before logs and must not contain provider keys, raw prompts, tool payloads, private path errors, or token values.
- After this task, the anti-fake gate fails production code that hardcodes `background_agents_available = false`.

**Steps:**

- [ ] Add supervisor binary entrypoint that opens workspace runtime, background registry, permission broker, event store, and SDK harness.
- [ ] Add local control channel with authenticated requests. Use local-only transport and reject non-local origin.
- [ ] Add supervisor lifecycle command in the desktop backend command modules, not frontend; keep process assembly in `commands/runtime.rs` and background-agent control in `commands/background_agents.rs`.
- [ ] Add Tauri sidecar packaging config in `tauri.conf.json` using `bundle.externalBin`, for example `binaries/jyowo-agent-supervisor`.
- [ ] Add `scripts/build-agent-supervisor-sidecar.mjs` and root `build:agent-supervisor-sidecar` package script so the supervisor sidecar is built and copied before desktop shell cargo tests, rust gates, desktop full checks, and Tauri bundling.
- [ ] Add `build.rs` validation for the expected `jyowo-agent-supervisor-$TARGET` sidecar filename, including `.exe` on Windows.
- [ ] Add tests or script dry-run coverage for target triple mapping and copied output path.
- [ ] Add sidecar launch code in Rust using `ShellExt::sidecar("jyowo-agent-supervisor")` or document and test the chosen equivalent.
- [ ] Add PID/lock file, token lifecycle, heartbeat, stale lock cleanup, and reconnect behavior.
- [ ] Add capability config so sidecar arguments are constrained if `tauri-plugin-shell` is used.
- [ ] Add recovery tests using temp workspace:
  - start background agent
  - simulate app restart
  - reconnect to supervisor
  - verify state continues or becomes recoverable
  - simulate supervisor crash
  - verify interrupted state
- [ ] Add packaging config so desktop build includes the supervisor sidecar.
- [ ] Extend the anti-fake gate and tests to forbid hardcoded background unavailable values in production code.

**Verification:**

```bash
pnpm build:agent-supervisor-sidecar
cargo test -p jyowo-desktop-shell background_supervisor
cargo test -p jyowo-harness-agent-runtime agent_orchestration_background --test agent_orchestration_background
pnpm check:agent-orchestration-no-fakes
pnpm check:rust
pnpm check:desktop:full
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 13: Settings > General Switches

**Files:**

- Modify: `apps/desktop/src/features/settings/ExecutionSettings.tsx`
- Modify: `apps/desktop/src/features/settings/ExecutionSettings.test.tsx`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`

**Required behavior:**

- General settings display switches for:
  - Subagents
  - Agent teams
  - Background agents
- Switches are disabled when backend says unavailable.
- Unavailable reasons are rendered from backend payload.
- Saving settings calls `setExecutionSettings`.
- A failed save restores backend truth from response/refetch and shows safe error.
- UI never hides backend unavailable reasons by hardcoded frontend assumptions.

**Steps:**

- [ ] Add switch UI using shared primitives.
- [ ] Add unavailable reason formatter.
- [ ] Add tests:
  - loading
  - available off
  - available on
  - unavailable disabled
  - save success
  - save failure
  - backend returns enabled false after attempted save
- [ ] Add Zod tests for all unavailable reason variants.

**Verification:**

```bash
pnpm -C apps/desktop test -- ExecutionSettings.test.tsx commands.test.ts
pnpm check:desktop
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 14: Permission, Redaction, Replay, and Support Bundle Safety

**Files:**

- Modify if source attribution needs broker support: `crates/jyowo-harness-permission/src/broker.rs`
- Modify if source attribution needs stream payload support: `crates/jyowo-harness-permission/src/stream.rs`
- Modify if source attribution needs public permission contract support: `crates/jyowo-harness-permission/src/lib.rs`
- Modify tests if permission crate changes: `crates/jyowo-harness-permission/tests/stream.rs`
- Modify tests if permission crate changes: `crates/jyowo-harness-permission/tests/contract.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/subagent.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/team.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/background_agent.rs`
- Modify: `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify if redactor coverage needs extension: `crates/jyowo-harness-observability/src/redactor.rs`
- Modify if replay safety needs extension: `crates/jyowo-harness-observability/src/replay.rs`
- Modify tests if observability crate changes: `crates/jyowo-harness-observability/tests/redactor.rs`
- Modify tests if observability crate changes: `crates/jyowo-harness-observability/tests/replay.rs`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify: `apps/desktop/src-tauri/src/commands/background_agents.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/runs_permissions.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/background_agents.rs`

**Required behavior:**

- Permission requests include actor source:
  - parent run
  - subagent id
  - team id and member id
  - background agent id
- Permission decisions are resolved by parent broker or foreground policy surface.
- Redactor runs before journal, replay, logs, traces, support bundle, and frontend state.
- MCP tool origin is preserved and validated for child agents.
- Secrets cannot enter:
  - prompt
  - event
  - log
  - trace
  - test snapshot
  - screenshot
  - frontend state
- Support bundle exports safe summaries and ids only.

**Steps:**

- [ ] Add source attribution where missing.
- [ ] Add negative tests for secret-bearing tool output in subagent/team/background events.
- [ ] Add replay tests proving unsafe fields are withheld.
- [ ] Add support bundle tests proving child agent internals are redacted.
- [ ] Add MCP origin negative tests for child agent tool exposure.

**Verification:**

```bash
cargo test -p jyowo-harness-subagent permission_bridge
cargo test -p jyowo-harness-team routing
cargo test -p jyowo-desktop-shell support_bundle_agent_redaction
pnpm check:rust
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 15: Anti-fake Gate Hardening and CI Integration

**Files:**

- Modify: `scripts/check-agent-orchestration-no-fakes.mjs`
- Modify: `scripts/check-agent-orchestration-no-fakes.test.mjs`
- Modify: `package.json`
- Modify: `docs/backend/backend-quality.md`
- Modify: `docs/frontend/frontend-quality.md`

**Required behavior:**

The Task 0 gate already exists. Harden it so it fails production code if it finds:

- any remaining hardcoded `subagents_available = false`, `agent_teams_available = false`, or `background_agents_available = false` in production code
- any remaining temporary allowlist for hardcoded agent capability availability fields
- command names for agent runtime that return fixed success without touching SDK runtime
- user-facing placeholder labels only when agent-related context appears near the match
- production fake background provider
- production fake agent runner
- frontend-only agent capability state not backed by command response

The final gate replaces the Task 0 initial path list with this final scoped agent-orchestration production path list:

```text
apps/desktop/src-tauri/src/commands/**
apps/desktop/src-tauri/src/lib.rs
apps/desktop/src-tauri/src/agent_supervisor.rs
apps/desktop/src-tauri/src/bin/jyowo-agent-supervisor.rs
apps/desktop/src-tauri/build.rs
apps/desktop/src-tauri/capabilities/default.json
apps/desktop/src-tauri/tauri.conf.json
apps/desktop/src/shared/tauri/commands.ts
apps/desktop/src/features/settings/ExecutionSettings.tsx
apps/desktop/src/features/conversation/Composer.tsx
apps/desktop/src/features/conversation/use-conversation.ts
apps/desktop/src/features/conversation/use-agent-profiles.ts
apps/desktop/src/features/conversation/ConversationWorkspace.tsx
apps/desktop/src/features/conversation/AgentActivitySegment.tsx
apps/desktop/src/features/conversation/timeline/conversation-timeline-selectors.ts
apps/desktop/src/features/background-agents/**
crates/jyowo-harness-contracts/**
crates/jyowo-harness-journal/src/conversation_worktree_projector.rs
crates/jyowo-harness-journal/src/conversation_read_model.rs
crates/jyowo-harness-agent-runtime/**
crates/jyowo-harness-sdk/**
crates/jyowo-harness-subagent/**
crates/jyowo-harness-team/**
scripts/build-agent-supervisor-sidecar.mjs
package.json
```

The final gate must not scan unrelated app placeholders. It must require agent-related context near generic terms such as `placeholder`, `fake`, `mock`, `noop`, `todo`, `coming soon`, and `experimental`.

The gate excludes:

```text
docs/**
**/*.test.ts
**/*.test.tsx
**/tests/**
target/**
node_modules/**
dist/**
storybook-static/**
```

**Steps:**

- [ ] Extend the existing script with the full final pattern list.
- [ ] Extend node tests that create temporary files and prove the script catches each forbidden pattern.
- [ ] Extend node tests proving unrelated placeholders, fixture mocks, and non-agent fake strings outside the scoped production path list do not fail.
- [ ] Verify the Task 3 subagent/team hardcoded false checks and Task 12 background hardcoded false checks are still active.
- [ ] Remove any temporary scanner allowlist created during intermediate tasks.
- [ ] Verify the package script still runs the test file before the scanner.
- [ ] Verify root `pnpm check` still runs this script after docs and before desktop/rust gates.
- [ ] Document the gate in frontend and backend quality docs.

**Verification:**

```bash
pnpm check:agent-orchestration-no-fakes
pnpm check:docs
pnpm check
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 16: End-to-end Runtime Scenarios

**Files:**

- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/agents.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/background_agents.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/runs_permissions.rs`
- Create: `apps/desktop/src-tauri/tests/agent_orchestration_e2e.rs`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`
- Modify: `apps/desktop/src/features/background-agents/BackgroundAgentsPanel.test.tsx`
- Modify: `apps/desktop/src/testing/command-client.ts`

**Required scenarios:**

- Real subagent spawn:
  - settings enable subagents
  - run request allows subagents
  - scripted model calls `agent`
  - journal has subagent events
  - conversation projection has subagent activity segment
- Real run-scoped team:
  - settings enable teams
  - run request allows team
  - team dispatch creates members
  - task list/mailbox persists
  - projection has team activity segment
- Real background agent:
  - settings enable background agents
  - run starts in background
  - list/detail commands return durable state
  - cancellation transitions state
  - restart recovery test marks or resumes correctly
- Negative:
  - disabled setting rejects per-run enable
  - unavailable runtime disables settings switch
  - write-capable agent without isolation fails closed
  - permission denied cancels unsafe child action

**Steps:**

- [ ] Add native backend tests for the three real scenarios.
- [ ] Add frontend command-client fixtures only after Rust contracts exist.
- [ ] Add frontend tests proving UI renders backend-projected states, not invented local state.
- [ ] Add restart recovery test using a temporary runtime directory.

**Verification:**

```bash
cargo test -p jyowo-desktop-shell agent_orchestration_e2e
pnpm -C apps/desktop test -- ConversationWorkspace.test.tsx BackgroundAgentsPanel.test.tsx
pnpm check:desktop:full
pnpm check:rust
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate.

## Task 17: Documentation Update and Final Release Gate

**Files:**

- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`
- Modify: `docs/frontend/frontend-engineering.md`
- Modify: `docs/frontend/frontend-quality.md`
- Modify: `docs/plans/2026-06-30-agent-orchestration-full-implementation.md` only to record implementation notes if the executing agent maintains notes in-place.

**Required behavior:**

- Backend docs describe:
  - agent orchestration domain ownership
  - capability resolver
  - background durable registry
  - supervisor process boundary
  - restart semantics
  - worktree isolation
  - permission source attribution
- Frontend docs describe:
  - settings switches
  - per-run controls
  - background agents panel
  - `AgentActivitySegment`
  - Zod schema requirements
- Quality docs list:
  - anti-fake gate
  - required subagent audit after every implementation task
  - required E2E scenarios

**Steps:**

- [ ] Update docs with only stable rules and final behavior.
- [ ] Run docs gate.
- [ ] Run full gate.
- [ ] Dispatch final read-only subagent audit for the entire plan implementation:

```text
Audit the completed implementation of docs/plans/2026-06-30-agent-orchestration-full-implementation.md.

Verify all tasks are complete, all task-level subagent audits passed, all gates ran successfully, and no production fake or placeholder remains.
Return PASS or FAIL with file and line evidence.
Do not modify files.
```

**Verification:**

```bash
pnpm check:docs
pnpm check
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** Required by the task close gate and final audit prompt above.

## Final Acceptance Checklist

The feature is complete only when every item is true.

- [ ] `Subagents`, `AgentTeams`, and `BackgroundAgents` are not described as experimental in product UI or docs.
- [ ] Settings > General has three backend-backed switches.
- [ ] `StartRunRequest` has validated `agentOptions`.
- [ ] `AgentRunOptions` includes validated `teamConfig` for run-scoped teams.
- [ ] Background user-facing start path is `start_run(agentOptions.background = background)` only.
- [ ] Desktop enables `agents-subagent` and `agents-team`.
- [ ] Subagent `agent` tool appears only when backend policy allows it.
- [ ] Run-scoped agent teams can be invoked from a run and persist task/mailbox state.
- [ ] Background agents have durable registry, commands, UI, supervisor, and restart recovery.
- [ ] Worktree/write isolation prevents same-checkout parallel writes.
- [ ] Permission prompts identify child agent source.
- [ ] Redaction protects child transcripts, mailbox payloads, support bundles, replay, logs, traces, and UI state.
- [ ] Conversation projection includes `AgentActivitySegment`.
- [ ] Background agent panel can list, inspect, pause, resume, cancel, send input, archive, and delete archived records.
- [ ] Contract schemas are generated from Rust and validated by Zod tests.
- [ ] Negative tests cover disabled settings, unavailable runtime, invalid payload, permission denial, no git worktree, duplicate write lease, restart interruption, and supervisor crash.
- [ ] `pnpm check:agent-orchestration-no-fakes` passes.
- [ ] `pnpm check:docs` passes.
- [ ] `pnpm check:desktop:full` passes.
- [ ] `pnpm check:rust` passes.
- [ ] `pnpm check` passes.
- [ ] Every task has a subagent audit result recorded as PASS.

## Commands for Final Verification

Run from the isolated implementation worktree:

```bash
git status --short
pnpm check:agent-orchestration-no-fakes
pnpm check:docs
pnpm check:desktop:full
pnpm check:rust
pnpm check
git diff --check
```

Expected:

```text
git status shows only intentional implementation files before commit
all checks exit 0
git diff --check exits 0
```

## Implementation Notes Template

The executing agent should append notes here or in a separate PR description. Do not mark a task complete without the subagent audit id.

```text
Task 0:
- Tests:
- Gate:
- Subagent audit:

Task 1:
- Tests:
- Gate:
- Subagent audit:

Task 2:
- Tests:
- Gate:
- Subagent audit:

Task 3:
- Tests:
- Gate:
- Subagent audit:

Task 4:
- Tests:
- Gate:
- Subagent audit:

Task 5:
- Tests:
- Gate:
- Subagent audit:

Task 6:
- Tests:
- Gate:
- Subagent audit:

Task 7:
- Tests:
- Gate:
- Subagent audit:

Task 8:
- Tests:
- Gate:
- Subagent audit:

Task 9:
- Tests:
- Gate:
- Subagent audit:

Task 10:
- Tests:
- Gate:
- Subagent audit:

Task 11:
- Tests:
- Gate:
- Subagent audit:

Task 12:
- Tests:
- Gate:
- Subagent audit:

Task 13:
- Tests:
- Gate:
- Subagent audit:

Task 14:
- Tests:
- Gate:
- Subagent audit:

Task 15:
- Tests:
- Gate:
- Subagent audit:

Task 16:
- Tests:
- Gate:
- Subagent audit:

Task 17:
- Tests:
- Gate:
- Subagent audit:
```
