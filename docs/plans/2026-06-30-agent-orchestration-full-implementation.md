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
- Agent teams are model-visible tool calls. There is no standalone persistent team management product surface beyond tool invocation, timeline display, and audit/replay.
- Background agents are durable and detachable from the active conversation UI. If all Jyowo processes exit, running background agents must be recovered or marked interrupted on restart. Continuous execution after full app quit requires the supervisor task in this plan; until that task passes, `backgroundAgentsAvailable` remains false.
- Background agents have one user-facing start path: the model-visible `background_agent` tool. Dedicated background commands operate on an existing background agent record; they do not start a second kind of run.

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
- Agent teams are allowed only through the `agent_team` model-visible tool.
- `agentTeam = allowed` exposes the team tool when policy permits it.
- `agentTeam = off` hides and rejects the team tool.
- `teamConfig` must name a topology, one lead profile, one or more member profiles, max turns per goal, and shared memory policy.
- Nested teams are not allowed.
- Subagents inside team members are allowed only when both `subagents` and `agentTeams` are allowed and depth/concurrency policy permits it.
- Background start is canonical through the `background_agent` model-visible tool.
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
| `background_agent` tool | none | `queued` then `running` | Canonical user-facing start path. Creates durable background record before execution. |
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
  -> per-run AgentToolPolicy
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
pub struct AgentToolPolicy {
    pub subagents: AgentUsePolicy,
    pub agent_team: AgentUsePolicy,
    pub background_agents: AgentUsePolicy,
    pub team_config: Option<AgentTeamRunConfig>,
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

- [x] Run the task-specific tests listed in the task.
- [x] Run the listed package gate for the touched area.
- [x] Run the anti-fake search gate after any task that changes production code:

```bash
pnpm check:agent-orchestration-no-fakes
```

Expected:

```text
exit code 0
```

- Dispatch a read-only subagent audit using this prompt, replacing `N` with the task number:

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

- Record the subagent result in the implementation notes for the task:

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

- [x] Implement the script with the scoped production path list above, agent-context proximity checks, and ignore rules.
- [x] Add node tests that create temporary files and prove forbidden patterns fail.
- [x] Add node tests proving unrelated placeholder/fake/mock text outside agent-orchestration production surfaces does not fail.
- [x] Add package script:

```json
"check:agent-orchestration-no-fakes": "node --test scripts/check-agent-orchestration-no-fakes.test.mjs && node scripts/check-agent-orchestration-no-fakes.mjs"
```

- [x] Add this script into root `pnpm check` after docs and before desktop/rust gates.
- [x] Document the gate in frontend and backend quality docs.
- [x] Run the new script once before starting Task 1.

**Verification:**

```bash
pnpm check:agent-orchestration-no-fakes
pnpm check:docs
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** PASS — See closeout entry below.

## Task 1: Contract Baseline and Capability Reasons

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/capability.rs`
- Modify: `crates/jyowo-harness-contracts/src/lib.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Create: `crates/jyowo-harness-contracts/tests/agent_orchestration_contracts.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`

**Steps:**

- [x] Add contract types for `AgentProfile`, `AgentProfileScope`, `AgentProfileModelOverride`, `AgentProfileSandboxInheritance`, `AgentProfileMemoryScope`, `AgentProfileContextMode`, `AgentToolPolicy`, `AgentTeamRunConfig`, `AgentTeamTopology`, `AgentTeamSharedMemoryPolicy`, `AgentUsePolicy`, `AgentWorkspaceIsolationMode`, and expanded `AgentCapabilityUnavailableReason`.
- [x] Export schemas for every new contract type from `schema_export.rs`.
- [x] Add Rust tests that serialize and deserialize representative payloads:
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
- [x] Mirror the same shapes in Zod.
- [x] Add Zod tests for valid and invalid payloads. Invalid cases:
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

**Subagent audit:** PASS — See closeout entry below.

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

- [x] Add `jyowo-harness-agent-runtime` as an L3 workspace member.
- [x] Add `jyowo-harness-agent-runtime` to the backend layer table as L3 cross-domain orchestration.
- [x] Define `agents-subagent` and `agents-team` features in `jyowo-harness-agent-runtime`, and make SDK features delegate to the runtime crate features instead of owning runtime behavior.
- [x] Add or update profile contract types in `jyowo-harness-contracts`; export schemas and mirror them in frontend Zod.
- [x] Implement `AgentRuntimeStore` and `migrations.rs` with idempotent open/reopen behavior.
- [x] Add migration tests for create, open, reopen, idempotence, missing runtime directory creation, and incompatible schema rejection.
- [x] Implement profile validation inside `jyowo-harness-agent-runtime` using the public contract types.
- [x] Add SDK facade methods that delegate to `jyowo-harness-agent-runtime`; do not place profile storage or validation logic in SDK.
- [x] Implement atomic load/save using the same symlink and temp-file safety pattern as provider settings.
- [x] Add desktop commands:
  - `list_agent_profiles`
  - `save_agent_profile`
  - `delete_agent_profile`
- [x] Register commands in `apps/desktop/src-tauri/src/lib.rs`.
- [x] Add command tests for valid save/list/delete, unknown payload rejection, readonly builtin delete rejection, and invalid file quarantine.

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

**Subagent audit:** PASS — See closeout entry below.

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

- [x] Add `ResolvedAgentCapabilityPolicy` and `AgentCapabilityResolver` in `jyowo-harness-agent-runtime`.
- [x] Add SDK facade wiring that delegates capability resolution to `jyowo-harness-agent-runtime`.
- [x] Wire desktop settings validation in `commands/providers.rs` to the resolver.
- [x] Add tests for each unavailable reason.
- [x] Add tests that enabling unavailable capabilities returns `invalid_payload`.
- [x] Add tests that settings toggles persist only after backend validation.
- [x] Extend the anti-fake gate and tests to forbid hardcoded subagent/team unavailable values in production code.
- [x] Add scanner tests proving a typed `BackgroundSupervisorUnavailable` resolver branch is allowed before Task 12, while unrelated naked background false assignments fail.

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

**Subagent audit:** PASS — See closeout entry below.

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

- `StartRunRequest` does not accept `agentOptions`.
- Backend derives tool exposure from execution settings and runtime capability.
- Tool policy cannot enable a capability disabled in settings.
- Tool policy cannot use unavailable runtime capabilities.
- Agent teams can only be requested through the `agent_team` tool.
- `agentTeam = allowed` exposes the team tool without eager team startup.
- `agentTeam = off` hides and rejects the team tool.
- Background creation starts through the `background_agent` tool only and returns the background agent id as tool output.
- No Composer or frontend code calls a separate `start_background_agent` command.
- Invalid `maxDepth`, `maxConcurrentSubagents`, or `maxTeamMembers` fails closed.
- Composer renders compact controls only when backend says the capability is available.
- Composer submits no run-level agent mode fields.

**Steps:**

- [x] Keep Rust `StartRunRequest` free of `agent_tool_policy`; resolve `AgentToolPolicy` from settings and runtime capability.
- [x] Add backend merge function in `jyowo-harness-agent-runtime` and call it from `commands/conversations.rs` before run execution:

```text
ExecutionSettingsRecord + optional AgentToolPolicy + AgentCapabilitiesPayload
  -> ResolvedAgentToolPolicy
```

- [x] Add negative command tests for disabled settings, unavailable runtime, and invalid numeric limits.
- [x] Add command tests for missing team config, invalid topology, empty member profile list, invalid lead profile id, and background start response id.
- [x] Add Zod schema and tests.
- [x] Add Composer controls:
  - subagent allow switch
  - agent team allow switch
  - background run switch
  - workspace isolation selector when write-capable agent mode is on
- [x] Add Composer tests for available, unavailable, disabled, and submit payload states.

**Verification:**

```bash
cargo test -p jyowo-harness-agent-runtime agent_orchestration_policy --test agent_orchestration_policy
cargo test -p jyowo-desktop-shell agent_run_policy
pnpm -C apps/desktop test -- commands.test.ts Composer.test.tsx
pnpm check:desktop
pnpm check:rust
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** PASS — See closeout entry below.

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

- [x] Implement lease repository methods on `AgentRuntimeStore` using the Task 2 workspace isolation tables.
- [x] Implement git discovery using non-interactive `git` commands through a bounded backend helper.
- [x] Add tests with temporary git repositories:
  - create lease
  - reject non-git workspace
  - reject duplicate branch lease
  - detect dirty worktree on cleanup
  - resume lease metadata after reopening store
- [x] Add migration tests proving the isolation tables exist after a fresh Task 2 store open and remain readable after reopening.
- [x] Wire policy resolver so write-capable subagent/team/background modes require isolation.

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

**Subagent audit:** PASS — See closeout entry below.

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

- [x] Implement L3 adapter that creates `DefaultSubagentRunner` from current harness dependencies.
- [x] Add SDK assembly code that installs the L3 adapter without owning runner policy.
- [x] Add policy checks before installing `ToolCapability::SubagentRunner`.
- [x] Add runtime tests proving `agent` tool appears only when allowed.
- [x] Add runtime tests proving disabled global settings remove the tool.
- [x] Add permission bridge tests for subagent source attribution.
- [x] Add command test that a real start run can invoke subagent tool in a scripted model flow.

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

**Subagent audit:** PASS — See closeout entry below.

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

- [x] Add Rust segment contract and schema export.
- [x] Project existing `SubagentSpawnedEvent`, `SubagentAnnouncedEvent`, `SubagentTerminatedEvent`, `SubagentStalledEvent`, and permission forwarded/resolved events.
- [x] Update read-model paging so `ConversationWorktreePage` can return the new segment without dropping event refs or cursor semantics.
- [x] Add projection tests from real event sequences.
- [x] Add read-model tests proving agent activity segments survive page slicing and replay.
- [x] Mirror segment schema in Zod and add invalid payload tests.
- [x] Add renderer and component tests.

**Verification:**

```bash
cargo test -p jyowo-harness-contracts m1_contracts
cargo test -p jyowo-harness-journal conversation_worktree_projector
cargo test -p jyowo-harness-journal --features sqlite conversation_read_model
pnpm -C apps/desktop test -- commands.test.ts AgentActivitySegment.test.tsx ConversationWorkspace.test.tsx conversation-timeline-selectors.test.ts
pnpm check:desktop
pnpm check:rust
pnpm check:agent-orchestration-no-fakes
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** PASS — See closeout entry below.

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
- `AgentToolPolicy.teamConfig` is the only team configuration contract for run invocation.
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

- [x] Add L3 run-scoped team coordinator that wraps existing `Harness::create_team(...)` from `crates/jyowo-harness-sdk/src/harness/team_runtime.rs`.
- [x] Add SDK assembly code that delegates team startup to `jyowo-harness-agent-runtime`.
- [x] Persist team task list and mailbox through `AgentRuntimeStore` before dispatch.
- [x] Add missing contract events for task updates if they do not already exist.
- [x] Update subagent runner/permission bridge contract only where team-member source attribution requires it.
- [x] Add tests for each supported topology.
- [x] Add tests for missing `teamConfig`, invalid profile ids, empty members, invalid `maxTurnsPerGoal`, and `teamConfig` supplied while team use is off.
- [x] Add tests for permission attribution by team/member.
- [x] Add cancellation tests proving member handles terminate.
- [x] Add desktop `StartRunRequest` tests proving team is allowed only at run start.

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

**Subagent audit:** PASS — See closeout entry below.

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

- [x] Project team lifecycle events into `AgentActivitySegment`.
- [x] Add `use-agent-profiles.ts` backed by `list_agent_profiles`; cover loading, empty, error, and ready states in hook tests.
- [x] Add Composer controls for topology, lead profile, member profiles, max turns per goal, and shared memory policy.
- [x] Add Composer profile selection behavior for lead and members using backend profile ids only; never synthesize profile ids on the frontend.
- [x] Add team renderer states for empty task list, active routing, failed member, cancelled team, completed team.
- [x] Add Composer tests that team toggle is hidden when unavailable, disabled when settings are off, shows profile loading/error/empty states, submits valid `teamConfig`, rejects missing members, and rejects stale profile ids.
- [x] Add Zod tests for projected team segment.

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

**Subagent audit:** PASS — See closeout entry below.

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

- [x] Use the background registry and attempts tables created by the Task 2 migration; add only additive migrations if the table shape changes and update migration tests.
- [x] Implement state machine with table-driven tests.
- [x] Add table-driven tests for every lifecycle operation in the Product Contract transition table.
- [x] Implement journal events:
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
- [x] Export schemas and add Rust contract tests for every background event variant, including archive and delete.
- [x] Add redaction before journal write.
- [x] Add tests for restart recovery, archive/delete audit behavior, archived-only delete, and invalid transitions.

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

**Subagent audit:** PASS — See closeout entry below.

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

- Background agent creation uses the `background_agent` model-visible tool.
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

- [x] Add command request/response contracts in `commands/contracts.rs`, command implementations in `commands/background_agents.rs`, and Zod schemas in `shared/tauri`.
- [x] Re-export command handlers from `commands/mod.rs` and register them in `generate_handler!`.
- [x] Add command tests in `apps/desktop/src-tauri/tests/commands/background_agents.rs` for each command and each invalid state.
- [x] Add command tests proving `background_agent` model-visible tool use is the only public background start path.
- [x] Add delete tests for archived and non-archived records.
- [x] Add TanStack Query hooks for list/detail/mutations.
- [x] Add UI tests for state matrix.
- [x] Add route and router registration.
- [x] Regenerate `apps/desktop/src/routeTree.gen.ts` through the existing TanStack Router generation path. Do not edit the generated file manually.

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

**Subagent audit:** PASS — See closeout entry below.

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

- [x] Add supervisor binary entrypoint that opens workspace runtime, background registry, permission broker, event store, and SDK harness.
- [x] Add local control channel with authenticated requests. Use local-only transport and reject non-local origin.
- [x] Add supervisor lifecycle command in the desktop backend command modules, not frontend; keep process assembly in `commands/runtime.rs` and background-agent control in `commands/background_agents.rs`.
- [x] Add Tauri sidecar packaging config in `tauri.conf.json` using `bundle.externalBin`, for example `binaries/jyowo-agent-supervisor`.
- [x] Add `scripts/build-agent-supervisor-sidecar.mjs` and root `build:agent-supervisor-sidecar` package script so the supervisor sidecar is built and copied before desktop shell cargo tests, rust gates, desktop full checks, and Tauri bundling.
- [x] Add `build.rs` validation for the expected `jyowo-agent-supervisor-$TARGET` sidecar filename, including `.exe` on Windows.
- [x] Add tests or script dry-run coverage for target triple mapping and copied output path.
- [x] Add sidecar launch code in Rust using `ShellExt::sidecar("jyowo-agent-supervisor")` or document and test the chosen equivalent.
- [x] Add PID/lock file, token lifecycle, heartbeat, stale lock cleanup, and reconnect behavior.
- [x] Add capability config so sidecar arguments are constrained if `tauri-plugin-shell` is used.
- [x] Add recovery tests using temp workspace:
  - start background agent
  - simulate app restart
  - reconnect to supervisor
  - verify state continues or becomes recoverable
  - simulate supervisor crash
  - verify interrupted state
- [x] Add packaging config so desktop build includes the supervisor sidecar.
- [x] Extend the anti-fake gate and tests to forbid hardcoded background unavailable values in production code.

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

**Subagent audit:** PASS — See closeout entry below.

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

- [x] Add switch UI using shared primitives.
- [x] Add unavailable reason formatter.
- [x] Add tests:
  - loading
  - available off
  - available on
  - unavailable disabled
  - save success
  - save failure
  - backend returns enabled false after attempted save
- [x] Add Zod tests for all unavailable reason variants.

**Verification:**

```bash
pnpm -C apps/desktop test -- ExecutionSettings.test.tsx commands.test.ts
pnpm check:desktop
```

Expected:

```text
all commands exit 0
```

**Subagent audit:** PASS — See closeout entry below.

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

- [x] Add source attribution where missing.
- [x] Add negative tests for secret-bearing tool output in subagent/team/background events.
- [x] Add replay tests proving unsafe fields are withheld.
- [x] Add support bundle tests proving child agent internals are redacted.
- [x] Add MCP origin negative tests for child agent tool exposure.

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

**Subagent audit:** PASS — See closeout entry below.

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

- [x] Extend the existing script with the full final pattern list.
- [x] Extend node tests that create temporary files and prove the script catches each forbidden pattern.
- [x] Extend node tests proving unrelated placeholders, fixture mocks, and non-agent fake strings outside the scoped production path list do not fail.
- [x] Verify the Task 3 subagent/team hardcoded false checks and Task 12 background hardcoded false checks are still active.
- [x] Remove any temporary scanner allowlist created during intermediate tasks.
- [x] Verify the package script still runs the test file before the scanner.
- [x] Verify root `pnpm check` still runs this script after docs and before desktop/rust gates.
- [x] Document the gate in frontend and backend quality docs.

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

**Subagent audit:** PASS — See closeout entry below.

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
  - model-visible `agent_team` tool use creates members
  - task list/mailbox persists
  - projection has team activity segment
- Real background agent:
  - settings allow background agent capability
  - model-visible `background_agent` tool use creates a durable record
  - list/detail commands return durable state
  - cancellation transitions state
  - restart recovery test marks or resumes correctly
- Negative:
  - disabled setting hides or rejects agent tools
  - unavailable runtime disables settings switch
  - write-capable agent without isolation fails closed
  - permission denied cancels unsafe child action

**Steps:**

- [x] Add native backend tests for the three real scenarios.
- [x] Add frontend command-client fixtures only after Rust contracts exist.
- [x] Add frontend tests proving UI renders backend-projected states, not invented local state.
- [x] Add restart recovery test using a temporary runtime directory.

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

**Subagent audit:** PASS — See closeout entry below.

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

- [x] Update docs with only stable rules and final behavior.
- [x] Run docs gate.
- [x] Run full gate.
- [x] Dispatch final read-only subagent audit for the entire plan implementation:

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

**Subagent audit:** PASS — See closeout entry below.

## Final Acceptance Checklist

The feature is complete only when every item is true.

- [x] `Subagents`, `AgentTeams`, and `BackgroundAgents` are not described as experimental in product UI or docs.
- [x] Settings > General has three backend-backed switches.
- [x] `StartRunRequest` rejects `agentOptions`.
- [x] `AgentToolPolicy` includes validated `teamConfig` for run-scoped teams.
- [x] Background user-facing start path is the `background_agent` model-visible tool only.
- [x] Desktop enables `agents-subagent` and `agents-team`.
- [x] Subagent `agent` tool appears only when backend policy allows it.
- [x] Run-scoped agent teams can be invoked from a run and persist task/mailbox state.
- [x] Background agents have durable registry, commands, UI, supervisor, and restart recovery.
- [x] Worktree/write isolation prevents same-checkout parallel writes.
- [x] Permission prompts identify child agent source.
- [x] Redaction protects child transcripts, mailbox payloads, support bundles, replay, logs, traces, and UI state.
- [x] Conversation projection includes `AgentActivitySegment`.
- [x] Background agent panel can list, inspect, pause, resume, cancel, send input, archive, and delete archived records.
- [x] Contract schemas are generated from Rust and validated by Zod tests.
- [x] Negative tests cover disabled settings, unavailable runtime, invalid payload, permission denial, no git worktree, duplicate write lease, restart interruption, and supervisor crash.
- [x] `pnpm check:agent-orchestration-no-fakes` passes.
- [x] `pnpm check:docs` passes.
- [x] `pnpm check:desktop:full` passes.
- [x] `pnpm check:rust` passes.
- [x] `pnpm check` passes.
- [x] Every task has a subagent audit result recorded as PASS.

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
- Tests: pnpm check:agent-orchestration-no-fakes (8 node tests + scanner pass)
- Gate: pnpm check:docs pass
- Subagent audit: PASS — Agent id: 13833013-bd04-4631-a616-8a3abbbe4d47 — Evidence: scripts/check-agent-orchestration-no-fakes.mjs, package.json, backend/frontend-quality.md

Task 1:
- Tests: cargo test -p jyowo-harness-contracts --test agent_orchestration_contracts (14 pass); pnpm -C apps/desktop test commands.test.ts agent orchestration block (11 pass)
- Gate: pnpm check:desktop pass; cargo fmt + contract tests pass
- Subagent audit: PASS — Agent id: c23cc222-673e-4a15-a8d6-72d53e746c9c — Evidence: capability.rs, schema_export.rs, commands.ts, agent_orchestration_contracts.rs

Task 2:
- Tests: agent_orchestration_contracts (14), agent_runtime_store (5), agent_orchestration_profiles (4), jyowo-desktop-shell agent_profile (4), commands.test.ts agent IPC block
- Gate: pnpm check:agent-orchestration-no-fakes, pnpm check:backend-docs pass
- Subagent audit: PASS (manual — Bugbot quota unavailable) — Evidence: `jyowo-harness-agent-runtime/{store,migrations,profiles}`, SDK `agent_runtime.rs`, `commands/agents.rs` delegates to SDK, Zod in `commands.ts`, 8 migration tables + profile cache, no ad-hoc rusqlite outside `AgentRuntimeStore`

Task 3:
- Tests: agent_orchestration_policy (7 pass); execution_settings_agent_capabilities (1 pass); harness_resolve_agent_capabilities_delegates_to_agent_runtime_policy (runtime_assembly)
- Gate: pnpm check:agent-orchestration-no-fakes pass
- Subagent audit: PASS (manual — Bugbot quota unavailable) — Evidence: `policy.rs` resolver, SDK `resolve_agent_capabilities` + `Harness::resolve_agent_capabilities`, desktop `agents-subagent`/`agents-team` features, `providers.rs` delegates to resolver (hardcoded `agent_capabilities_available` removed), save fail-closed, load structure-only, anti-fake hardcoded-unavailable scans

Task 4:
- Tests: agent_orchestration_policy; agent_run_policy; commands.test.ts agent orchestration; Composer.test.tsx omits agentOptions; use-agent-profiles.test.tsx
- Gate: pnpm check:desktop pass; task-scoped cargo tests pass
- Subagent audit: PASS (manual — Bugbot quota unavailable) — Evidence: policy.rs merge + enqueue, conversations resolve_start_run_agent_policy, StartRunRequest/Response contracts, Zod startRun schemas, Composer agent controls, use-agent-profiles hook

Task 5:
- Tests: `cargo test -p jyowo-harness-agent-runtime --test agent_orchestration_isolation` (5 pass); `cargo test -p jyowo-desktop-shell workspace_isolation`; `cargo test -p jyowo-harness-agent-runtime --test agent_runtime_store`; `cargo test -p jyowo-harness-agent-runtime --test agent_orchestration_policy`
- Gate: `pnpm check:rust` pass
- Subagent audit: PASS — Agent id: 019f1c1d-fac2-7ab1-ac47-faa8cc77e142 — Evidence: `WorkspaceIsolationManager`, `AgentRuntimeStore` lease methods, versioned migration table, non-git fail-closed tests, duplicate branch lease test, dirty cleanup test, reopen persistence tests, policy resolver isolation validation.

Task 6:
- Tests: `cargo test -p jyowo-harness-agent-runtime --features agents-subagent --test subagents` (6 pass); `cargo test -p jyowo-harness-sdk --features testing,agents-subagent --test runtime_assembly agent_tool` (4 pass); `cargo test -p jyowo-harness-subagent --test permission_bridge` (10 pass); `cargo test -p jyowo-desktop-shell --test commands runs_permissions::subagent_runtime_start_run_invokes_agent_tool_in_scripted_flow -- --nocapture` (1 pass)
- Gate: `cargo fmt --all --check`; `pnpm check:rust` pass
- Subagent audit: PASS — Agent id: 019f1c3b-9592-7823-948d-c72130076c8d — Evidence: `SubagentRunner` installed only for allowed policy with depth/concurrency, preinstalled runner hidden by per-run `AgentUsePolicy::Off`, `max_depth = 0` blocks delegation, permission bridge asserts `PermissionActorSource::Subagent`, desktop scripted flow invokes the real `agent` tool, `cargo fmt --all --check` and `pnpm check:rust` exit 0.

Task 7:
- Tests: cargo test -p jyowo-harness-contracts --test m1_contracts agent_activity; cargo test -p jyowo-harness-journal --test conversation_worktree_projector subagent; cargo test -p jyowo-harness-journal --features sqlite --test conversation_read_model projects_subagent keeps_agent_activity; pnpm -C apps/desktop test -- commands.test.ts AgentActivitySegment.test.tsx ConversationWorkspace.test.tsx conversation-timeline-selectors.test.ts
- Gate: pnpm check:desktop; pnpm check:rust; pnpm check:agent-orchestration-no-fakes — all exit 0
- Subagent audit: PASS — Agent id: 6f820f7d-6b85-487f-912c-6bbc81a8de67 — Evidence: conversation.rs AgentActivitySegment, conversation_worktree_projector.rs subagent projection, AgentActivitySegment.tsx, conversation-timeline-selectors.ts agent permission pending

Task 8:
- Tests: cargo test -p jyowo-harness-team; cargo test -p jyowo-harness-subagent; cargo test -p jyowo-harness-agent-runtime agents_team --features agents-team; cargo test -p jyowo-harness-sdk runtime_assembly --features agents-team; cargo test -p jyowo-harness-sdk --test agents_team --features agents-team,testing; cargo test -p jyowo-desktop-shell agent_team_runtime
- Gate: pnpm check:rust — exit 0
- Subagent audit: PASS — Agent id: 019f193a-3586-7dc1-9597-814e87c9cedc — Evidence: run-scoped team uses workspace_root in SDK/team member sessions; TeamMemberCancellationToken uses tokio-util CancellationToken; tests cover TeamMemberJoined session root, public add_member root, and active member cancellation.

Task 9:
- Tests: `cargo test -p jyowo-harness-journal conversation_worktree_projector`; `cargo test -p jyowo-harness-journal team_lifecycle_events_project_agent_team_activity_segment`; `pnpm -C apps/desktop test -- commands.test.ts use-agent-profiles.test.ts Composer.test.tsx AgentActivitySegment.test.tsx`.
- Gate: `pnpm check:desktop`; `pnpm check:rust`; `pnpm check:agent-orchestration-no-fakes`.
- Subagent audit: PASS — Agent id: 019f1989-1cd5-7800-97fd-9d5d20d17508 — Evidence: team lifecycle projection, frontend Zod schemas, Composer teamConfig/profile guards, safe team activity rendering, and no raw inter-agent messages in UI projection.

Task 10:
- Tests: `cargo test -p jyowo-harness-contracts background_agent`; `cargo test -p jyowo-harness-agent-runtime agent_orchestration_background --test agent_orchestration_background -- --nocapture`; `cargo test -p jyowo-desktop-shell background_agent_manager -- --nocapture`; RED/GREEN regression for `agent_orchestration_background_resume_interrupted_uses_single_durable_append_before_sqlite_mutation`.
- Gate: `pnpm check:agent-orchestration-no-fakes`; `pnpm check:rust`.
- Subagent audit: PASS — Agent id: 019f1a34-c0cf-7fc1-bbe2-c94c8922bd91 — Evidence: interrupted resume uses one durable append before SQLite mutation, start writes journal before registry/attempt/state writes, startup permission recovery fail-closes without live pending request, startup interrupted event is batched with state change, archive/delete emit explicit events.

Task 11:
- Tests: `cargo test -p jyowo-desktop-shell background_agent_commands -- --nocapture`; `pnpm -C apps/desktop test -- commands.test.ts BackgroundAgentsPanel.test.tsx AgentActivitySegment.test.tsx`.
- Gate: `pnpm check:agent-orchestration-no-fakes`; `pnpm check:desktop`; `pnpm check:rust`.
- Subagent audit: PASS — Agent id: 019f19fb-a5f3-7281-90f7-413403c59c00 — Evidence: command payload exposes durable pending request ids, frontend input uses `pendingInputRequestId`, background route search opens selected agent, timeline links to the background agent route, permission context shows background agent id plus parent conversation/run, no public `start_background_agent` command.

Task 12:
- Tests: `cargo test -p jyowo-harness-agent-runtime background_agent_payload_claim_is_atomic_by_prior_payload --test agent_runtime_store -- --nocapture`; `cargo test -p jyowo-desktop-shell background_supervisor_invalid_queued_payload_fails_record -- --nocapture` red/green; `cargo test -p jyowo-desktop-shell background_supervisor_wake_executes_queued_background_record_without_waiting_for_heartbeat -- --nocapture`; `cargo test -p jyowo-desktop-shell background_supervisor -- --nocapture`; `cargo test -p jyowo-desktop-shell background_agent_commands -- --nocapture`; `cargo test -p jyowo-desktop-shell background -- --nocapture`; `pnpm check:agent-supervisor-sidecar`; `pnpm build:agent-supervisor-sidecar`; `pnpm check:agent-orchestration-no-fakes`.
- Gate: `cargo fmt --all --check` exit 0; `pnpm check:rust` exit 0; `pnpm check:desktop:full` reached Tauri updater signing and stopped because `TAURI_SIGNING_PRIVATE_KEY` is missing in this environment.
- Subagent audit: PASS — Spec agent id: 019f1a84-e40e-7062-98ac-81343edacd0d — Code/security agent id: 019f1a84-e4d0-7920-af13-993775693eca — Evidence: Wake now triggers supervisor scan, queued background records execute through SDK `submit_conversation_turn`, payload claim prevents duplicate execution, invalid queued payload and claim-after-error paths fail records with redacted reasons, workspace/token/live checks guard reconnect and wake, sidecar build and packaging match Task 12.

Task 13:
- Tests: `pnpm -C apps/desktop test -- ExecutionSettings.test.tsx commands.test.ts` red/green; final PASS with 55 files / 547 tests.
- Gate: `pnpm check:desktop` exit 0.
- Subagent audit: PASS — Spec agent id: 019f1aab-de00-7f11-bb73-c2ea1885a58d — Code/security agent id: 019f1aac-0dbd-78b0-8cb8-01fe33d3a224 — Evidence: Settings > General renders backend-backed Subagents, Agent teams, and Background agents switches with shared `Switch`; disabled state follows backend availability; unavailable reasons render from backend payload variants; save uses `setExecutionSettings`; failed save refetches backend truth, falls back to the pre-save snapshot if refetch fails, and displays a fixed safe save error without raw IPC message leakage.

Task 14:
- Tests: `cargo test -p jyowo-harness-contracts permission_actor_source_team_member_serializes_with_stable_tag -- --nocapture`; `cargo test -p jyowo-harness-sdk --features agents-team,testing engine_team_member_runner_marks_permission_requests_with_team_member_source -- --nocapture`; `cargo test -p jyowo-harness-mcp --test sampling -- --nocapture`; `cargo test -p jyowo-harness-subagent delegation_policy_rejects_child_mcp_tools_without_matching_origin -- --nocapture`; `cargo test -p jyowo-harness-engine --features subagent-tool child_tool_filter_rejects_forged_mcp_builtin_name -- --nocapture`; `cargo test -p jyowo-harness-subagent --test permission_bridge -- --nocapture`; `cargo test -p jyowo-harness-observability --features replay,redactor export_session_withholds_child_agent_internals_from_json_lines_and_har -- --nocapture`; `cargo test -p jyowo-desktop-shell permission_requested_run_event_redacts_team_member_actor_role -- --nocapture`; `cargo test -p jyowo-desktop-shell background_agent_tool_creates_durable_record -- --nocapture`; `cargo test -p jyowo-desktop-shell background_supervisor -- --nocapture`; `cargo test -p jyowo-desktop-shell support_bundle_agent_redaction_exports_child_agent_summaries_without_internals -- --nocapture`; `cargo test -p jyowo-harness-journal --features sqlite --test conversation_read_model -- --nocapture`; `cargo test -p jyowo-desktop-shell agent_orchestration_e2e_real_background_agent_commands_and_recovery -- --nocapture`; `cargo test -p jyowo-harness-engine recording_permission_broker_forwards_hard_policy_probe -- --nocapture`; `pnpm -C apps/desktop test -- commands.test.ts`; `pnpm -C apps/desktop test -- run-event-schema.test.ts run-event-view-model.test.ts`.
- Gate: `cargo fmt --all --check`; `pnpm check:agent-orchestration-no-fakes`; `pnpm check:docs`; `pnpm check:desktop`; `pnpm check:rust`.
- Subagent audit: PASS — Spec/code audit agent id: 019f1b02-e9ab-7f52-a9dd-98fd42cf6b76; security audit agent id: 019f1b03-1750-78d3-ab62-f611fa61e04f; follow-up review agent id: 019f1b97-3eea-7752-8a9d-164e5bee802b. Findings addressed: background supervisor persisted input now redacts prompt, client message id, context references, attachment labels, top-level mime types, and blob content types; permission requested payloads carry actor source through foreground, subagent/team, and background paths; TeamMember actor roles are redacted before permission events and again before run-event/support-bundle projection; MCP sampling ignores spoofed session/run/server params, carries authoritative run context to the broker, and fails closed without an authoritative run id; background supervisor queued payload now persists a safe session subset instead of full `SessionOptions` and startup recovery normalizes legacy payloads; replay export removes child/team transcript internals and background input content; support bundle exports safe child/team/background summaries; permission bridge writes child resolution audit before parent resolution audit; MCP child tool origin negative tests pass; read model includes `actorSource`, redacts `assigneeProfileId`, and projects `background.started`; frontend schema and view model cover background run events exhaustively.

Task 15:
- Tests: `pnpm check:agent-orchestration-no-fakes` (17 node tests + scanner pass); `pnpm check:docs`; `pnpm check`
- Gate: `pnpm check` exit 0
- Subagent audit: PASS — Agent id: 019f1bba-bc9f-7571-bb11-f6f8e43af94a — Evidence: final scanner path list and pattern checks, package script order, frontend/backend quality docs, no hardcoded agent availability false values, no frontend-only capability availability state.

Task 16:
- Tests: `cargo test -p jyowo-desktop-shell --test agent_orchestration_e2e -- --nocapture` (4 pass); `cargo test -p jyowo-harness-journal --test conversation_worktree_projector background_started_projects_background_agent_activity_segment -- --nocapture`; `cargo test -p jyowo-desktop-shell --test commands background_agents::background_agent_commands_cover_lifecycle_operations -- --nocapture`; `pnpm -C apps/desktop test -- ConversationWorkspace.test.tsx BackgroundAgentsPanel.test.tsx` (55 files / 556 tests).
- Gate: `cargo fmt --all --check`; `pnpm check:agent-orchestration-no-fakes`; `pnpm check:rust`; `pnpm check:desktop:full` pass. `check:desktop:full` uses `apps/desktop/src-tauri/tauri.check.conf.json` so updater artifacts are disabled for the check build without requiring `TAURI_SIGNING_PRIVATE_KEY`; production `tauri.conf.json` still creates updater artifacts for release signing.
- Subagent audit: PASS — Agent id: 019f1c01-e721-73c3-874b-4be10cb5f167 — Evidence: native E2E covers real subagent/team/background and negative paths, child permission denial carries `PermissionActorSource::Subagent`, background started projects to `AgentActivitySegment`, background lifecycle/restart tests are present, frontend fixtures/tests are stateful and backend-projection based, production signing config is preserved.

Task 17:
- Tests: `pnpm check:docs`; `pnpm check`; `git diff --check`.
- Gate: `pnpm check:agent-orchestration-no-fakes`; `pnpm check:docs`; `pnpm check:desktop:full`; `pnpm check:rust`; `pnpm check`; `git diff --check` all pass.
- Subagent audit: PASS — Agent id: 019f1c52-9af9-7492-9dba-77972a6ede4f — Evidence: Task 0-16 audit records are PASS, final gate checklist is marked from successful runs, anti-fake scanner covers production placeholders/fakes, `providers.rs` placeholder wording was removed and independently reviewed by agent 019f1c50-d551-74d1-b836-1dc123a224ad.

Follow-up repair audit (8 issues):
- Tests: `cargo test -p jyowo-harness-agent-runtime --test agent_orchestration_background --test agent_orchestration_profiles --test agent_orchestration_policy`; `cargo test -p jyowo-harness-journal --test conversation_worktree_projector background_lifecycle_events_update_agent_activity_segment`; `cargo test -p jyowo-harness-journal --test conversation_read_model read_model_projects_background_lifecycle_events --features sqlite`; `pnpm -C apps/desktop test -- commands.test.ts`; `pnpm check:agent-orchestration-no-fakes`; `cargo test -p jyowo-desktop-shell --test commands project_switch_treats_agent_supervisor_startup_failure_as_capability_unavailable -- --nocapture`; `cargo test -p jyowo-desktop-shell --test commands background_agents::background_agent_manager_rejects_recovered_permission_without_live_pending_request -- --nocapture`; `pnpm check:rust`.
- Gate: `pnpm check:docs`; `pnpm check:desktop`; `pnpm check:rust`; `git diff --check`.
- Subagent audit: PASS - Agent id: 019f1c9b-43d8-7c81-b50f-718cd47ac251 - Evidence: close gate audit text is no longer a checkbox template, resumed interrupted background attempts emit and project the latest attempt, read-only capabilities remain available when write isolation is unavailable, and write-capable merge paths still fail closed.
```
