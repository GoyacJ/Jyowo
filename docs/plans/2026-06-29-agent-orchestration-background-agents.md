# Agent Orchestration And Background Agents Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task.
>
> **For subagent-driven execution:** Use `superpowers:subagent-driven-development`. Every task must end with an independent subagent verification pass before the task checkbox is marked complete.

**Goal:** 将 Jyowo 的 subagent、agent team、background agent 做成稳定可用能力，接入设置、主聊天、持久运行模型、可审计事件和 UI 控制面。

**Architecture:** Rust 仍是 Policy authority。React 只展示 capability 状态并发起用户意图。Subagent 和 agent team 是主聊天可用的 runtime tools；background agent 是持久 job/supervisor，不是前端状态、普通 async task 或临时线程。

**Tech Stack:** Rust 1.96, Tauri 2, Tokio, serde, schemars, rusqlite, refinery, React 19, TypeScript 6, Zod, TanStack Query, Zustand for UI-only state, Tailwind CSS v4, Vitest, Testing Library, Storybook, Playwright.

---

## Product References

Verified against official docs on 2026-06-29.

Use these references as product and safety inputs, not as branding targets:

- Codex subagents: subagents are stable, explicitly triggered, inherit sandbox/approval controls, and return consolidated summaries instead of raw worker noise. Source: `https://developers.openai.com/codex/subagents`.
- Codex app automations/background work: recurring or background tasks run either in local project mode or in isolated worktrees, with sandbox policy applied and results surfaced for triage/review. Source: `https://developers.openai.com/codex/app/automations`.
- Codex app worktrees/threads: worktrees isolate parallel work from the active checkout. Source: `https://developers.openai.com/codex/app/features`.
- Claude Code subagents: subagents are configured agents with separate context and tool permissions. Source: `https://code.claude.com/docs/en/sub-agents`.
- Claude Code agent team / agent view: long-running work needs explicit ownership, status, and inspectable agent activity. Sources: `https://code.claude.com/docs/en/agent-teams`, `https://code.claude.com/docs/en/agent-view`.

The Jyowo design must keep the repo rules:

- Conversation is the main product object.
- Runs, raw events, Replay, Activity and Raw JSON remain support surfaces.
- Rust owns final policy, permission, sandbox, tool, journal, replay and redaction decisions.
- Public contracts live in `crates/jyowo-harness-contracts`.
- Tauri commands stay thin.

## Current Implementation Facts

These facts are from the current `main` branch and must be rechecked before implementation:

- `jyowo-harness-sdk` has Cargo features `agents-subagent` and `agents-team`, but they are not in the default SDK feature set.
- `apps/desktop/src-tauri/Cargo.toml` currently enables many SDK features explicitly, but not `agents-subagent` or `agents-team`.
- `crates/jyowo-harness-engine/src/engine.rs` has `with_subagent_tool()` behind the `subagent-tool` feature. When enabled, it appends `harness_subagent::AgentTool`.
- `crates/jyowo-harness-subagent/src/lib.rs` defines `AgentTool` with tool name `agent`, input `{ role, task, prompt_template? }`, and `ToolCapability::SubagentRunner`.
- `crates/jyowo-harness-contracts/src/events/subagent.rs` already has subagent events for spawned, announced, terminated, stalled, spawn paused, permission forwarded, and permission resolved.
- `crates/jyowo-harness-team/src/lib.rs` already has team topology, message bus, shared memory, pause/resume, member runners, lifecycle, quotas, and journaled team/member/message events.
- `crates/jyowo-harness-team` does not yet provide a persistent shared task list with claim/dependency semantics.
- Existing team mailbox behavior is represented by `AgentMessageSentEvent` and `AgentMessageRoutedEvent`, but there is no desktop-facing persistent mailbox projection or read API.
- `apps/desktop/src/features/settings/ExecutionSettings.tsx` currently manages permission mode only.
- `apps/desktop/src-tauri/src/commands.rs` persists execution settings at `.jyowo/runtime/execution-settings.json` with only `permission_mode`.
- `apps/desktop/src/shared/tauri/commands.ts` has Zod schemas for `get_execution_settings` and `set_execution_settings`, also permission-mode only.
- No dedicated persistent background agent job/supervisor model exists.

## Design Decisions

### D1. Stable Capability Names

Stop using product wording like experimental for these capabilities.

Use:

```text
subagents
agentTeams
backgroundAgents
```

Do not use:

```text
experimentalSubagents
experimentalAgentTeams
background thread as a synonym for background agent
```

Compile-time feature gates may remain for optional crate compilation. User-facing naming and runtime settings must not call these capabilities experimental.

### D2. Runtime Toggles Are Policy Inputs

Settings - General gets three workspace-level policy switches:

```text
Allow subagents in chat
Allow agent teams in chat
Allow background agents
```

Default values:

```text
subagentsEnabled: false
agentTeamsEnabled: false
backgroundAgentsEnabled: false
```

Reason:

- The features are stable, but they can increase token usage, run tools, spawn multiple model/tool loops, run while the user is not focused on the conversation, and edit files from a background job.
- Users should opt in per workspace.
- The toggles are not frontend-only preferences. Rust must enforce them when building runtime tools and accepting background job commands.

If a feature is not compiled into the desktop build, the UI shows it unavailable and disabled. It must not save an enabled state for an unavailable backend feature.

### D3. Main Chat Uses Explicit Runtime Options And Tools

Subagents and agent teams become available to the main chat by exposing backend-owned tools to the model:

```text
agent
agent_team
```

The model may call them only when:

- The SDK was compiled with the required feature.
- The workspace execution setting is enabled.
- The runtime has the required capability and policy dependencies.

If disabled, the tool descriptor must not be exposed. If the model somehow calls a disabled tool name, the tool dispatch path must fail closed with a safe error.

Workspace settings answer whether a capability may be used. Each chat run also carries explicit runtime options so the user understands what the run is allowed to do.

Required per-run options:

```text
delegationMode: none | auto | prefer_subagents | prefer_agent_team
allowedCapabilities: subagents, agent_teams
```

Rules:

- Normal send defaults to `delegationMode: auto` only for capabilities enabled in Settings.
- `allowedCapabilities` is only for foreground delegation tools. It may contain `subagents` and `agent_teams`; it must not contain `background_agents`.
- The composer must expose a delegation menu whenever either subagents or agent teams are enabled.
- The user can force `none` for a run even when workspace settings allow delegation.
- `prefer_subagents` and `prefer_agent_team` bias runtime tool availability and system instruction, but they do not bypass backend policy.
- Background execution is a distinct composer action and must require `backgroundAgentsEnabled = true`.
- Background execution must not be implemented through `start_run`, a hidden conversation run, or a prompt-only flag. The composer background action must call `prepare_background_agent` and then `start_background_agent`.
- `start_run` must reject legacy or malformed fields that attempt to request background execution, including `backgroundStartMode`.
- No runtime option may be implemented as prompt text only. Foreground delegation must be part of the backend run request or SDK session options. Background execution must be part of `StartBackgroundAgentRequest`.

### D4. Agent Team Is A Coordination Runtime

Agent team is not only parallel subagent fan-out.

It must own:

```text
team spec
member registry
shared task list
task claim protocol
dependency graph
mailbox projection
shared memory write policy
member lifecycle
quota and backpressure
termination
replayable events
```

`crates/jyowo-harness-team` remains the L3 owner. Public event and DTO types go to `crates/jyowo-harness-contracts`.

### D5. Background Agent Is A Persistent Job

Background agent is not:

```text
tokio::spawn without a durable job row
frontend local state
hidden conversation run
automation only
subagent with no parent UI
```

Background agent is:

```text
BackgroundAgentJob
BackgroundAgentSupervisor
BackgroundAgentRunner
BackgroundAgentStore
BackgroundAgentEvent stream
Conversation or project-linked result surface
```

It must survive app restart at the state level:

- queued, paused, waiting-for-permission, succeeded, failed, cancelled jobs remain listable
- running jobs are reconciled after restart
- unfinished running jobs become `interrupted` or `needs_resume`, never silently disappear

### D6. Worktree Isolation Is Required For Git Repos

For Git repositories, background agents must support worktree mode.

Default mode:

```text
Git repo: worktree
Non-Git directory: local
```

Local mode is allowed but must be explicit when a Git repo is detected and the job may edit files. Worktree create/delete/list behavior should be owned by a backend domain, not by React.

Background agent start must include a backend preflight:

```text
detected repository kind
recommended mode
whether the job may edit files
whether local mode needs explicit acknowledgement
worktree branch/display path preview
unsupported reason if worktree cannot be created
```

For Git repositories:

- Worktree mode is the default for jobs that may edit files.
- Local mode requires an explicit `localModeAcknowledged` input and a backend audit event.
- The UI must show the mode before starting the job.
- The detail view must include open worktree, compare changes, merge/review handoff, and request cleanup actions when a worktree exists.

### D7. Permission Waiting Is A First-Class Background State

Background agents must reuse the existing permission broker and `resolve_permission` command path. If the current command shape cannot safely identify a background job permission, extend that command contract instead of adding a separate resolver.

When `backgroundAgentsEnabled = false`, starting or resuming background execution must be blocked. Recovery and safety actions for already-created jobs must remain available:

```text
list
get
pause
cancel
deny pending permission
request worktree cleanup
```

Approving a pending background permission is equivalent to allowing execution to continue and must be rejected while background agents are disabled.

Required behavior:

- Background permission requests must carry source identity: `background_agent_id`, `project_id`, optional `conversation_id`, optional `run_id`, and optional `team_id`/`agent_id`.
- Project-only background jobs must be resolvable by `background_agent_id` plus `project_id` even when no conversation exists.
- If `conversation_id` or `run_id` is present, the permission resolver must validate that it belongs to the same background job before applying the decision.
- `WaitingForPermission` jobs remain visible in the background agent list and detail view.
- The detail view shows the pending permission card with approve/deny actions backed by `resolve_permission`.
- Approving a permission resumes the waiting job from the supervisor state.
- Denying a permission records a durable failed/denied step and either fails the job or lets the runner continue if the underlying tool can recover.
- Restart reconciliation must preserve waiting permission state or mark it `needs_resume` with a durable reason.

### D8. Redaction And Journal Rules Do Not Change

Every new persisted event, job record, mailbox item, task item, transcript summary, support bundle, Replay output and frontend payload must pass through the existing redaction/visibility boundary.

No raw Secret, private absolute path, provider-native payload, raw tool argument, raw thought text, bearer token, cookie, or API key may enter:

```text
prompt
event
job record
mailbox
task list
log
trace
frontend state
test snapshot
screenshot
support bundle
```

## Target Architecture

```text
apps/desktop/src
  features/settings
    ExecutionSettings.tsx
  features/conversation
    main chat renders tool/team/background evidence from ConversationTurn[]
  features/agents
    background agent list/detail/control surface
  shared/tauri
    Zod validated IPC schemas

apps/desktop/src-tauri
  commands.rs
    thin IPC handlers

crates/jyowo-harness-contracts
  public capability settings
  subagent/team/background DTOs
  team task/mailbox/background events
  JsonSchema export

crates/jyowo-harness-team
  shared task list
  claim/dependency protocol
  mailbox projection
  team tool

crates/jyowo-harness-worktree
  Git worktree create/list/cleanup policy

crates/jyowo-harness-background-agent
  job store
  supervisor
  runner
  restart reconciliation

crates/jyowo-harness-sdk
  facade methods
  runtime assembly
  test adapters
```

New crates must be added only if the implementation confirms no existing crate owns the domain. If a crate is added, update `docs/backend/backend-engineering.md` and the workspace member list in the same task.

Dependency layer requirements:

```text
L0 contracts: jyowo-harness-contracts
L1 primitives/policy: jyowo-harness-permission, jyowo-harness-observability
L2 domains: jyowo-harness-worktree, jyowo-harness-background-agent
L3 runtimes: jyowo-harness-engine, jyowo-harness-team
L4 facade: jyowo-harness-sdk
Tauri shell: apps/desktop/src-tauri
```

Rules:

- `jyowo-harness-worktree` may depend on contracts and low-level filesystem/git helpers only. It must not depend on SDK, Tauri, React, or desktop commands.
- `jyowo-harness-background-agent` owns durable job state, supervisor state transitions, event emission contracts, and runner traits. It must not depend on `jyowo-harness-sdk` or Tauri.
- SDK wires `jyowo-harness-background-agent` to real session/run execution by implementing/injecting a runner trait from the background-agent crate.
- Tauri calls SDK facade methods only.
- If implementation finds an existing crate already owns worktree or background job storage, this layer table must be updated before any code is written.

## Public Contracts

Add contract types in `crates/jyowo-harness-contracts`.

### Agent Capability Settings

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AgentCapabilitySettings {
    #[serde(default)]
    pub subagents_enabled: bool,
    #[serde(default)]
    pub agent_teams_enabled: bool,
    #[serde(default)]
    pub background_agents_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AgentCapabilityAvailability {
    pub subagents_available: bool,
    pub agent_teams_available: bool,
    pub background_agents_available: bool,
    pub unavailable_reasons: Vec<AgentCapabilityUnavailableReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum AgentCapabilityUnavailableReason {
    NotCompiled { capability: AgentCapabilityKind },
    MissingRuntimeDependency { capability: AgentCapabilityKind, dependency: UiSafeText },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentCapabilityKind {
    Subagents,
    AgentTeams,
    BackgroundAgents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentDelegationCapabilityKind {
    Subagents,
    AgentTeams,
}
```

Wire shape is stable snake_case in Rust serde:

```json
{
  "reason": "missing_runtime_dependency",
  "capability": "agent_teams",
  "dependency": "team runtime unavailable"
}
```

Frontend shape uses camelCase through `shared/tauri`:

```ts
{
  permissionMode: 'default' | 'auto' | 'bypass_permissions'
  autoModeAvailable: boolean
  agentCapabilities: {
    subagentsEnabled: boolean
    agentTeamsEnabled: boolean
    backgroundAgentsEnabled: boolean
    subagentsAvailable: boolean
    agentTeamsAvailable: boolean
    backgroundAgentsAvailable: boolean
    unavailableReasons: Array<{
      capability: 'subagents' | 'agent_teams' | 'background_agents'
      reason: 'not_compiled' | 'missing_runtime_dependency'
      dependency?: string
    }>
  }
}
```

### Run Agent Options

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentOptions {
    pub delegation_mode: AgentDelegationMode,
    pub allowed_capabilities: Vec<AgentDelegationCapabilityKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentDelegationMode {
    None,
    Auto,
    PreferSubagents,
    PreferAgentTeam,
}
```

Rules:

- `allowed_capabilities` is intersected with workspace settings in Rust.
- `allowed_capabilities` must reject `background_agents`; background execution is controlled only by `StartBackgroundAgentRequest` and `background_agents_enabled`.
- Unknown capability names must be rejected by frontend Zod and Rust serde.
- `RunAgentOptions` and the outer `start_run` request must deny unknown fields so legacy `backgroundStartMode` cannot be silently ignored.

### Team Task Model

```rust
pub struct TeamTaskId(...);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamTask {
    pub task_id: TeamTaskId,
    pub team_id: TeamId,
    pub title: UiSafeText,
    pub body: UiSafeText,
    pub status: TeamTaskStatus,
    pub claim: Option<TeamTaskClaim>,
    pub dependencies: Vec<TeamTaskId>,
    pub created_by: AgentId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamTaskStatus {
    Open,
    Blocked,
    Claimed,
    InProgress,
    Review,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamTaskClaim {
    pub agent_id: AgentId,
    pub claimed_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}
```

Events:

```text
team_task_created
team_task_updated
team_task_claimed
team_task_claim_released
team_task_dependency_added
team_task_dependency_removed
team_task_completed
team_task_cancelled
```

Rules:

- Claim uses version compare-and-set.
- Claim fails if the task is already claimed by another active member.
- Claim fails if dependencies are not done.
- Dependency graph rejects cycles.
- All task text must use `UiSafeText`, created through redaction before persistence.

### Team Mailbox Model

`AgentMessageSentEvent` and `AgentMessageRoutedEvent` are the durable mailbox facts. Current events do not expose a thread/correlation field, so the implementation must explicitly add one before the desktop projection relies on it.

Required event compatibility change:

```rust
pub struct AgentMessageSentEvent {
    pub team_id: TeamId,
    pub from: AgentId,
    pub to: Recipient,
    pub payload: MessagePayload,
    pub message_id: MessageId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<CorrelationId>,
    pub at: DateTime<Utc>,
}

pub struct AgentMessageRoutedEvent {
    pub team_id: TeamId,
    pub message_id: MessageId,
    pub resolved_recipients: Vec<AgentId>,
    pub routing_policy: RoutingPolicyKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<CorrelationId>,
    pub at: DateTime<Utc>,
}
```

Old events with no `correlation_id` must deserialize and project into one-message synthetic threads keyed by `message_id`.

Correlation rules:

- A team goal dispatch creates a new `CorrelationId`.
- All messages produced while handling that dispatch inherit the active `CorrelationId`.
- Direct replies inherit the `correlation_id` of the message being answered.
- Handoff messages inherit the active `CorrelationId`.
- Standalone member messages with no active dispatch create a new `CorrelationId`.
- The team runtime must store the active correlation in member run context, not infer it from message order.
- Tests must prove correlation survives replay and restart.

Add projection DTOs:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamMailboxThreadId {
    Correlation { correlation_id: CorrelationId },
    Message { message_id: MessageId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamMailboxThread {
    pub team_id: TeamId,
    pub thread_id: TeamMailboxThreadId,
    pub messages: Vec<TeamMailboxMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamMailboxMessage {
    pub message_id: MessageId,
    pub from: AgentId,
    pub to: Recipient,
    pub payload_preview: UiSafeText,
    pub payload_kind: TeamMailboxPayloadKind,
    pub sent_at: DateTime<Utc>,
    pub routed_to: Vec<AgentId>,
    pub visibility: ContextVisibility,
}
```

Do not expose raw `MessagePayload` through desktop mailbox read APIs.

### Background Agent Model

```rust
pub struct BackgroundAgentId(...);
pub struct BackgroundWorktreeId(...);
pub struct SubscriptionId(...);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    Unsubscribed,
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BackgroundAgentEventCursor {
    pub event_id: EventId,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BackgroundWorktreeRef {
    pub worktree_id: BackgroundWorktreeId,
    pub branch_name: UiSafeText,
    pub display_path: UiSafeText,
    pub base_ref: Option<UiSafeText>,
    pub cleanup_status: BackgroundWorktreeCleanupStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BackgroundWorktreePreview {
    pub branch_name: UiSafeText,
    pub display_path: UiSafeText,
    pub base_ref: Option<UiSafeText>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundWorktreeCleanupStatus {
    NotRequested,
    Requested,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BackgroundPermissionRef {
    pub request_id: RequestId,
    pub project_id: WorkspaceId,
    pub conversation_id: Option<SessionId>,
    pub run_id: Option<RunId>,
    pub source: BackgroundPermissionSource,
    pub subject_preview: UiSafeText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BackgroundPermissionSource {
    pub background_agent_id: BackgroundAgentId,
    pub team_id: Option<TeamId>,
    pub agent_id: Option<AgentId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BackgroundAgentJob {
    pub id: BackgroundAgentId,
    pub conversation_id: Option<SessionId>,
    pub project_id: WorkspaceId,
    pub title: UiSafeText,
    pub prompt_ref: BlobRef,
    pub prompt_preview: UiSafeText,
    pub mode: BackgroundAgentMode,
    pub status: BackgroundAgentStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub run_id: Option<RunId>,
    pub worktree: Option<BackgroundWorktreeRef>,
    pub result_ref: Option<BlobRef>,
    pub result_preview: Option<UiSafeText>,
    pub error_summary: Option<UiSafeText>,
    pub pending_permission: Option<BackgroundPermissionRef>,
    pub permission_mode: PermissionMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundAgentMode {
    Local,
    Worktree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundRepositoryKind {
    Git,
    NonGit,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum BackgroundPreflightUnavailableReason {
    BackgroundAgentsDisabled,
    WorktreeUnsupported { summary: UiSafeText },
    WorktreeCreateWouldEscapeWorkspace,
    GitUnavailable { summary: UiSafeText },
    ProjectScopeInvalid { summary: UiSafeText },
    MissingRuntimeDependency { dependency: UiSafeText },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundAgentStatus {
    Queued,
    Running,
    WaitingForPermission,
    Paused,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
    NeedsResume,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PrepareBackgroundAgentRequest {
    pub conversation_id: Option<SessionId>,
    pub project_id: WorkspaceId,
    pub may_edit_files: bool,
    pub requested_mode: Option<BackgroundAgentMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PrepareBackgroundAgentResponse {
    pub repository_kind: BackgroundRepositoryKind,
    pub may_edit_files: bool,
    pub recommended_mode: BackgroundAgentMode,
    pub local_mode_requires_acknowledgement: bool,
    pub worktree_available: bool,
    pub worktree_preview: Option<BackgroundWorktreePreview>,
    pub unavailable_reasons: Vec<BackgroundPreflightUnavailableReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct StartBackgroundAgentRequest {
    pub conversation_id: Option<SessionId>,
    pub project_id: WorkspaceId,
    pub title: Option<String>,
    pub prompt: String,
    pub mode: BackgroundAgentMode,
    pub may_edit_files: bool,
    pub local_mode_acknowledged: bool,
    pub permission_mode: PermissionMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct StartBackgroundAgentResponse {
    pub job: BackgroundAgentJob,
    pub cursor: BackgroundAgentEventCursor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ListBackgroundAgentsRequest {
    pub project_id: Option<WorkspaceId>,
    pub conversation_id: Option<SessionId>,
    pub statuses: Vec<BackgroundAgentStatus>,
    pub cursor: Option<BackgroundAgentEventCursor>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ListBackgroundAgentsResponse {
    pub jobs: Vec<BackgroundAgentJob>,
    pub next_cursor: Option<BackgroundAgentEventCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct GetBackgroundAgentRequest {
    pub background_agent_id: BackgroundAgentId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct GetBackgroundAgentResponse {
    pub job: BackgroundAgentJob,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BackgroundAgentControlRequest {
    pub background_agent_id: BackgroundAgentId,
    pub expected_status: Option<BackgroundAgentStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BackgroundAgentControlResponse {
    pub job: BackgroundAgentJob,
    pub cursor: BackgroundAgentEventCursor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SubscribeBackgroundAgentEventsRequest {
    pub project_id: Option<WorkspaceId>,
    pub background_agent_id: Option<BackgroundAgentId>,
    pub after_cursor: Option<BackgroundAgentEventCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SubscribeBackgroundAgentEventsResponse {
    pub subscription_id: SubscriptionId,
    pub replayed_events: Vec<BackgroundAgentEvent>,
    pub cursor: Option<BackgroundAgentEventCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct UnsubscribeBackgroundAgentEventsRequest {
    pub subscription_id: SubscriptionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct UnsubscribeBackgroundAgentEventsResponse {
    pub subscription_id: SubscriptionId,
    pub status: SubscriptionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RequestBackgroundWorktreeCleanupRequest {
    pub worktree_id: BackgroundWorktreeId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RequestBackgroundWorktreeCleanupResponse {
    pub worktree: BackgroundWorktreeRef,
    pub cursor: BackgroundAgentEventCursor,
}
```

`StartBackgroundAgentRequest.title` and `StartBackgroundAgentRequest.prompt` are request-only. The backend must redact and offload them into `title`, `prompt_ref` and `prompt_preview` before any event, job row, log, trace, frontend state update or support bundle.

Preflight rules:

- `prepare_background_agent` is side-effect-free.
- It must not create a `BackgroundAgentJob`.
- It must not create a Git worktree, branch, worktree metadata row, `BackgroundWorktreeRef`, or background event.
- It may calculate a deterministic `BackgroundWorktreePreview`, but the real `BackgroundWorktreeRef` is created only by `start_background_agent`.
- Tests must prove repeated preflight calls do not change the job store, worktree store, event stream or filesystem.

Events:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BackgroundAgentEvent {
    pub background_agent_id: BackgroundAgentId,
    pub cursor: BackgroundAgentEventCursor,
    pub kind: BackgroundAgentEventKind,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackgroundAgentEventKind {
    Created { job: BackgroundAgentJob },
    Queued,
    Started { run_id: RunId },
    Progress { summary: UiSafeText },
    WaitingForPermission { permission: BackgroundPermissionRef },
    PermissionResolved { request_id: RequestId, decision: Decision },
    Paused,
    Resumed,
    Cancelling,
    Finished { result_ref: Option<BlobRef>, result_preview: Option<UiSafeText> },
    Failed { error_summary: UiSafeText },
    Interrupted { reason: UiSafeText },
    NeedsResume { reason: UiSafeText },
    WorktreeCreated { worktree: BackgroundWorktreeRef },
    WorktreeCleanupRequested { worktree_id: BackgroundWorktreeId },
    WorktreeCleanupCompleted { worktree_id: BackgroundWorktreeId },
}
```

The job store must be durable and restart-readable. Running-process handles are single-process only.

## IPC Commands

All new commands must be thin Tauri boundaries.

Execution settings:

```text
get_execution_settings() -> GetExecutionSettingsResponse
set_execution_settings(
  permission_mode: PermissionMode,
  subagents_enabled: bool,
  agent_teams_enabled: bool,
  background_agents_enabled: bool
) -> SetExecutionSettingsResponse
```

Background agents:

```text
prepare_background_agent(request) -> PrepareBackgroundAgentResponse
start_background_agent(request) -> StartBackgroundAgentResponse
list_background_agents(request) -> ListBackgroundAgentsResponse
get_background_agent(request: GetBackgroundAgentRequest) -> GetBackgroundAgentResponse
pause_background_agent(request: BackgroundAgentControlRequest) -> BackgroundAgentControlResponse
resume_background_agent(request: BackgroundAgentControlRequest) -> BackgroundAgentControlResponse
cancel_background_agent(request: BackgroundAgentControlRequest) -> BackgroundAgentControlResponse
subscribe_background_agent_events(request) -> SubscribeBackgroundAgentEventsResponse
unsubscribe_background_agent_events(request: UnsubscribeBackgroundAgentEventsRequest) -> UnsubscribeBackgroundAgentEventsResponse
request_background_worktree_cleanup(request) -> RequestBackgroundWorktreeCleanupResponse
```

Background permission approval/denial must use the existing `resolve_permission` command. If the current command cannot safely address background jobs, extend that command contract instead of adding a parallel permission resolver.

Agent teams:

```text
list_agent_team_runs(conversation_id?) -> ListAgentTeamRunsResponse
get_agent_team_run(team_id) -> GetAgentTeamRunResponse
list_agent_team_tasks(team_id) -> ListAgentTeamTasksResponse
list_agent_team_mailbox(team_id, thread_id?) -> ListAgentTeamMailboxResponse
```

These read commands must use SDK facade methods. Tauri must not reach into team internals.

Required DTO coverage:

- `ListAgentTeamRunsRequest/Response`
- `GetAgentTeamRunRequest/Response`
- `ListAgentTeamTasksRequest/Response`
- `ListAgentTeamMailboxRequest/Response`
- frontend Zod schemas for every request and response
- Rust command tests proving every command is registered in `generate_handler!`

## Frontend UX

### Settings - General

Add an "Agent capabilities" section near current execution settings.

States:

```text
loading
ready available
ready unavailable
saving
error
saved
```

Controls:

- `Switch` for subagents.
- `Switch` for agent teams.
- `Switch` for background agents.
- Disabled switch with backend-provided unavailable reason when not compiled or missing dependencies.

Do not store these toggles in Zustand or `@tauri-apps/plugin-store`.

### Main Chat

No separate "admin dashboard" posture.

When a run uses subagents or teams, the conversation projection should show:

```text
subagent started
subagent completed/failed
team created
team task claimed/completed
team mailbox summary
team completed/failed
background agent started from this conversation
background result ready
```

The main canvas still renders `ConversationTurn[]`. Raw team/background events remain in Activity/Details.

Composer requirements:

- Show a delegation menu when subagents or agent teams are enabled.
- Menu states: no delegation, automatic delegation, prefer subagents, prefer agent team.
- Show background run as a separate action from normal send.
- Background action is hidden or disabled when `backgroundAgentsEnabled = false`.
- The composer must call backend run options; it must not encode delegation intent only in user-visible prompt text.
- A run summary/evidence row must show which mode was used after the run starts.

### Background Agents Surface

Add `features/agents` and a route/sidebar entry if the current shell has no implementation. Product IA name should be "Agents" only if it contains both background jobs and team run details; otherwise use "Background Agents" for the navigation label and keep team details reachable from conversation evidence.

Required screens:

- Background agent list grouped by active, waiting for permission, paused, needs resume, succeeded, failed, cancelled, interrupted.
- Create background agent flow with prompt, project, may-edit-files intent, preflight result, mode selection and explicit local-mode acknowledgement when required.
- Detail view: prompt summary, project/worktree, status, timeline, pending permission, result summary, diff/worktree link if available.
- Controls: start, pause, resume, cancel, approve/deny pending permission, open conversation, open worktree, compare changes, request worktree cleanup.
- Empty state action: create background agent.

Required states:

```text
loading
empty
ready
error
permission waiting
interrupted after restart
needs resume after restart
cancel pending
cleanup pending
```

## Implementation Protocol For AI Workers

Every task must follow this order:

1. Read the files listed in the task.
2. Write or update the failing tests first unless the task is docs-only.
3. Run the focused failing test and capture the expected failure.
4. Implement the task.
5. Run the focused tests.
6. Run the required package gate.
7. Run the task's independent subagent verification.
8. Fix every valid finding from the verification.
9. Mark the task checkbox only after verification passes.

Completion evidence required for every task:

```text
changed files
tests added or updated
focused commands run
new test names observed in command output
non-zero focused test count where the runner reports counts
package/root gates run
subagent verification result
known residual risk
```

No task may be marked complete if:

- It only changes UI without backend enforcement.
- It only changes backend without frontend Zod schema updates for changed IPC.
- It returns untyped `serde_json::Value` across stable IPC.
- It adds public contract shape without serde/schema tests.
- It adds Tauri commands without registering them in `generate_handler!`.
- It exposes raw Secret, absolute private path, raw tool argument, or raw thought text.
- It adds TODO/stub/mock-only implementation for production paths.
- It skips the independent subagent verification.
- It relies on a filtered test command without proving the intended tests actually ran.

Use this verification prompt after every implementation task, replacing `[TASK]` and `[FILES]`:

```text
Review Task [TASK] from docs/plans/2026-06-29-agent-orchestration-background-agents.md.
Check whether the implementation fully satisfies the task, including backend policy enforcement,
frontend schemas, tests, docs, and gates. Inspect [FILES] and any related call sites.
Do not assume success from commit messages or comments. Report PASS only if code and tests prove it.
If incomplete, list exact missing requirements with file references.
```

## Task Plan

### Task 0: Reconfirm Baseline

**Files:**

- Read: `AGENTS.md`
- Read: `docs/frontend/agent-harness-frontend-development-guidelines.md`
- Read: `docs/frontend/frontend-product-ux.md`
- Read: `docs/frontend/frontend-engineering.md`
- Read: `docs/frontend/frontend-quality.md`
- Read: `docs/backend/agent-harness-backend-development-guidelines.md`
- Read: `docs/backend/backend-runtime.md`
- Read: `docs/backend/backend-engineering.md`
- Read: `docs/backend/backend-quality.md`
- Read: `apps/desktop/src-tauri/Cargo.toml`
- Read: `crates/jyowo-harness-sdk/Cargo.toml`
- Read: `crates/jyowo-harness-engine/src/engine.rs`
- Read: `crates/jyowo-harness-subagent/src/lib.rs`
- Read: `crates/jyowo-harness-team/src/lib.rs`
- Read: `apps/desktop/src/features/settings/ExecutionSettings.tsx`
- Read: `apps/desktop/src/shared/tauri/commands.ts`
- Read: `apps/desktop/src-tauri/src/commands.rs`

**Steps:**

- [ ] Run `git branch --show-current` and verify the branch intended for implementation.
- [ ] Run `git status --short` and record unrelated user changes without reverting them.
- [ ] Run `rg -n "agents-subagent|agents-team|with_subagent_tool|AgentTool|ExecutionSettingsRecord|background_agent|TeamTask|mailbox" crates apps docs -g '!target/**' -g '!node_modules/**'`.
- [ ] Update this plan only if the codebase has moved since this document was written.

**Verification:**

- [ ] No code changes unless the plan is stale.
- [ ] Subagent verification: use the global verification prompt and ask it to confirm the baseline inventory is accurate.

### Task 1: Rename Capability Semantics Away From Experimental

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/*` if experimental capability names exist there.
- Modify: `crates/jyowo-harness-sdk/src/*` if experimental feature flag types exist there.
- Modify: `crates/jyowo-harness-engine/src/*` if runtime names expose experimental wording.
- Modify: `apps/desktop/src/shared/tauri/commands.ts` if frontend schemas expose experimental wording.
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`
- Test: contract/schema tests in `crates/jyowo-harness-contracts/tests/*`
- Test: frontend command schema tests in `apps/desktop/src/shared/tauri/commands.test.ts`

**Steps:**

- [ ] Write a contract/frontend test that fails if any public serialized setting uses `experimental_subagents` or `experimental_agent_teams`.
- [ ] Replace user-facing and IPC/runtime setting names with stable names.
- [ ] Keep Cargo feature names if changing them would create avoidable build churn; document that Cargo feature gates are compile-time packaging, not product maturity.
- [ ] Run `rg -n "experimental_subagents|experimental_agent_teams|experimental subagent|experimental agent team|experimentalSubagents|experimentalAgentTeams" crates apps docs -g '!target/**' -g '!node_modules/**'`.

**Focused commands:**

```bash
cargo test -p jyowo-harness-contracts
pnpm -C apps/desktop test -- commands.test.ts
```

**Gate:**

```bash
pnpm check:rust
pnpm check:desktop
```

**Subagent verification:**

- [ ] Ask the verifier to prove no public/user-facing experimental naming remains except documented Cargo feature labels.

### Task 2: Add Agent Capability Settings Contract

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/*`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Test: `crates/jyowo-harness-contracts/tests/*`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Test: `apps/desktop/src/shared/tauri/commands.test.ts`

**Steps:**

- [ ] Add `AgentCapabilitySettings`, `AgentCapabilityAvailability`, `AgentCapabilityKind`, and unavailable reason contracts including `background_agents_enabled`.
- [ ] Add explicit `serde(rename_all)` / `serde(tag)` attributes matching the Public Contracts section.
- [ ] Export JsonSchema for new stable contracts.
- [ ] Add serde shape tests for snake_case Rust payloads.
- [ ] Add schema export snapshot/update test coverage.
- [ ] Add Zod schemas with camelCase frontend shape.
- [ ] Add invalid payload tests for unknown capability, missing booleans, and malformed unavailable reasons.

**Focused commands:**

```bash
cargo test -p jyowo-harness-contracts
pnpm -C apps/desktop test -- commands.test.ts
```

**Gate:**

```bash
pnpm check:rust
pnpm check:desktop
```

**Subagent verification:**

- [ ] Ask the verifier to compare Rust serde shapes, schema exports, and TypeScript Zod schemas for drift.

### Task 3: Persist Capability Toggles In Rust Execution Settings

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `docs/backend/backend-engineering.md` only if command surface docs need updates.
- Modify: `docs/frontend/frontend-engineering.md` only if frontend command docs need updates.

**Steps:**

- [ ] Extend `ExecutionSettingsRecord` with defaulted `subagents_enabled: bool`, `agent_teams_enabled: bool`, and `background_agents_enabled: bool`.
- [ ] Extend `GetExecutionSettingsResponse` and `SetExecutionSettingsRequest/Response`.
- [ ] Preserve backward compatibility for existing `.jyowo/runtime/execution-settings.json` files containing only `permission_mode`.
- [ ] Reject saving `subagents_enabled = true` when subagents are not compiled into the desktop build.
- [ ] Reject saving `agent_teams_enabled = true` when agent teams are not compiled into the desktop build.
- [ ] Reject saving `background_agents_enabled = true` when background agents are not compiled into the desktop build.
- [ ] Ensure invalid settings files are handled with the same fail-closed/reset behavior as current permission mode parsing.
- [ ] Update command docs if command signature documentation is enforced by docs gates.

**Focused commands:**

```bash
cargo test -p jyowo-desktop-shell execution_settings
```

**Gate:**

```bash
pnpm check:rust
pnpm check:backend-docs
```

**Subagent verification:**

- [ ] Ask the verifier to inspect old-settings migration, unavailable feature rejection for all three toggles, and Rust-side enforcement.

### Task 4: Add Settings UI Switches

**Files:**

- Modify: `apps/desktop/src/features/settings/ExecutionSettings.tsx`
- Modify: `apps/desktop/src/features/settings/ExecutionSettings.test.tsx`
- Modify: `apps/desktop/src/shared/tauri/mock-client.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`
- Storybook optional if settings page stories exist.

**Steps:**

- [ ] Add failing component tests for rendering subagent, agent team and background agent switches.
- [ ] Add tests for unavailable backend capability disabling the switch.
- [ ] Add tests that saving sends `permissionMode`, `subagentsEnabled`, `agentTeamsEnabled`, and `backgroundAgentsEnabled` together.
- [ ] Implement UI with `shared/ui` `Switch` if available; otherwise add or extend the shared primitive first.
- [ ] Preserve loading, error, saving and saved states.
- [ ] Do not store these toggles in Zustand or local plugin store.

**Focused commands:**

```bash
pnpm -C apps/desktop test -- ExecutionSettings.test.tsx
```

**Gate:**

```bash
pnpm check:desktop
```

**Subagent verification:**

- [ ] Ask the verifier to inspect that the UI is backed by Rust settings and that disabled/unavailable states cannot save invalid values for all three switches.

### Task 4A: Add Per-Run Agent Options To Main Chat

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/*`
- Modify: `crates/jyowo-harness-sdk/src/options.rs` or current session/run option owner.
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/mock-client.ts`
- Modify: `apps/desktop/src/features/conversation/Composer.tsx`
- Modify: `apps/desktop/src/features/conversation/Composer.test.tsx`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
- Test: `crates/jyowo-harness-contracts/tests/*`
- Test: `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`
- Test: `apps/desktop/src/shared/tauri/commands.test.ts`

**Steps:**

- [ ] Add `RunAgentOptions`, `AgentDelegationMode`, and `AgentDelegationCapabilityKind` to stable Rust contracts and schema export.
- [ ] Add Zod schemas for the exact camelCase wire shape: `delegationMode`, `allowedCapabilities`.
- [ ] Extend the desktop `start_run` request/response path to accept run agent options.
- [ ] Extend SDK session/run options so runtime assembly receives the options as typed data.
- [ ] Add Rust tests proving options are intersected with workspace settings before any tool descriptors are built.
- [ ] Add Rust tests proving `delegationMode = none` removes `agent` and `agent_team` even when workspace settings enable them.
- [ ] Add Rust and frontend tests proving `background_agents` is rejected inside `allowedCapabilities`.
- [ ] Add Rust and frontend tests proving `start_run` rejects `backgroundStartMode` or any other background execution request field.
- [ ] Add Composer UI for no delegation, automatic delegation, prefer subagents, prefer agent team.
- [ ] Add component tests proving Composer sends the typed run options and does not encode delegation intent only in prompt text.
- [ ] Add invalid payload tests for unknown delegation mode, unknown capability, and unknown agent option fields.

**Focused commands:**

```bash
cargo test -p jyowo-harness-contracts run_agent_options
cargo test -p jyowo-harness-sdk runtime_assembly --features agents-subagent,agents-team,testing
cargo test -p jyowo-desktop-shell start_run
pnpm -C apps/desktop test -- commands.test.ts Composer.test.tsx
```

**Gate:**

```bash
pnpm check
```

**Subagent verification:**

- [ ] Ask the verifier to inspect IPC DTOs, Zod schemas, SDK run options, runtime tool gating, Composer controls and tests for drift.

### Task 5: Enforce Subagent Toggle In Runtime Assembly Before Desktop Feature Enablement

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Modify: `crates/jyowo-harness-engine/src/engine.rs` only if the SDK cannot enforce cleanly.
- Test: `apps/desktop/src-tauri/tests/commands.rs`
- Test: `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`
- Test: `crates/jyowo-harness-engine/tests/subagent_tool_feature.rs`

**Steps:**

- [ ] Add a failing test: when `subagents_enabled = false`, a conversation run does not expose `agent` even if the crate feature is compiled.
- [ ] Add a failing test: when `subagents_enabled = true`, a conversation run exposes `agent` and can execute a bounded subagent.
- [ ] Ensure the tool descriptor is absent when disabled, not merely hidden in UI.
- [ ] Ensure tool dispatch fails closed if a disabled `agent` call reaches runtime.
- [ ] Preserve subagent depth, sandbox inheritance, permission mode, context report, and transcript behavior.
- [ ] Do not enable desktop shell `agents-subagent` or `agents-team` before this task passes.

**Focused commands:**

```bash
cargo test -p jyowo-harness-sdk subagent --features agents-subagent,testing
cargo test -p jyowo-harness-engine --features subagent-tool subagent_tool_feature
cargo test -p jyowo-desktop-shell start_run
```

**Gate:**

```bash
pnpm check:rust
```

**Subagent verification:**

- [ ] Ask the verifier to inspect model tool exposure, fail-closed behavior, and no frontend-only gating.

### Task 6: Compile Desktop With Stable Agent Features After Runtime Enforcement

**Files:**

- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `crates/jyowo-harness-sdk/Cargo.toml` only if feature composition needs adjustment.
- Test: `apps/desktop/src-tauri/tests/commands.rs`
- Test: `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`

**Steps:**

- [ ] Enable `agents-subagent` and `agents-team` in the desktop shell SDK dependency only after Task 5 passes.
- [ ] Keep feature availability surfaced through backend response rather than frontend guessing.
- [ ] Add a shell/runtime test proving `get_execution_settings` reports available features in a compiled build.
- [ ] Add a negative test path if cfg allows unavailable features in a narrower build.
- [ ] Add a regression test proving the default disabled setting still keeps `agent` absent in desktop runs after features are compiled.

**Focused commands:**

```bash
cargo test -p jyowo-desktop-shell get_execution_settings
cargo test -p jyowo-harness-sdk default_session_installs_agent_tool_when_subagent_runner_is_configured --features agents-subagent,testing
cargo test -p jyowo-desktop-shell start_run
```

**Gate:**

```bash
pnpm check:rust
```

**Subagent verification:**

- [ ] Ask the verifier to check that compile-time features and runtime settings are separate, correctly reported, and still fail-closed by default.

### Task 7: Render Subagent Evidence In Conversation Projection

**Files:**

- Modify: `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify: `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
- Modify: `apps/desktop/src/shared/events/run-event-schema.ts` if event schema is exposed to Activity.
- Modify: `apps/desktop/src/features/conversation/timeline/*`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`
- Story: `apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx`

**Steps:**

- [ ] Add projection tests for `SubagentSpawned`, `SubagentAnnounced`, `SubagentTerminated`, and `SubagentStalled`.
- [ ] Project subagent activity as UI-safe `ProcessStep` or equivalent evidence, not raw event JSON.
- [ ] Render role, safe summary, status, usage preview, and transcript availability without raw transcript content.
- [ ] Handle failed/cancelled/stalled states.
- [ ] Add Storybook states for running, completed, failed, stalled, and transcript-ref available.

**Focused commands:**

```bash
cargo test -p jyowo-harness-journal conversation_worktree_projector
pnpm -C apps/desktop test -- conversation-timeline.test.tsx
pnpm -C apps/desktop build-storybook
```

**Gate:**

```bash
pnpm check
```

**Subagent verification:**

- [ ] Ask the verifier to confirm the main canvas still uses `ConversationTurn[]` and does not render raw subagent event payloads.

### Task 8: Add Team Task List Contracts And Events

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/events/team.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/mod.rs`
- Modify: `crates/jyowo-harness-contracts/src/ids.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Test: `crates/jyowo-harness-contracts/tests/*`

**Steps:**

- [ ] Add `TeamTaskId`.
- [ ] Add task DTOs and status/claim types.
- [ ] Add team task event variants.
- [ ] Add optional `correlation_id` to `AgentMessageSentEvent` and `AgentMessageRoutedEvent` with backward-compatible serde defaults.
- [ ] Add tests proving goal dispatch creates a correlation, replies/handoffs inherit it, and standalone messages create a new correlation.
- [ ] Add serde and JsonSchema tests.
- [ ] Add tests for event tag stability.
- [ ] Add compatibility tests proving old message events without `correlation_id` still deserialize and project.
- [ ] Ensure task body/title fields use safe display text rules in downstream projection tests.

**Focused commands:**

```bash
cargo test -p jyowo-harness-contracts team
```

**Gate:**

```bash
pnpm check:rust
```

**Subagent verification:**

- [ ] Ask the verifier to inspect event compatibility, message correlation source, schema export, and whether all task lifecycle events required by this plan exist.

### Task 9: Implement Team Shared Task List, Claim, And Dependency Protocol

**Files:**

- Modify: `crates/jyowo-harness-team/src/lib.rs` or split into `crates/jyowo-harness-team/src/task_list.rs` if the file is too large.
- Test: `crates/jyowo-harness-team/tests/task_list.rs`
- Test: `crates/jyowo-harness-team/tests/team_e2e.rs`

**Steps:**

- [ ] Add failing tests for creating tasks.
- [ ] Add failing tests for claiming a task by version.
- [ ] Add failing tests for claim conflict.
- [ ] Add failing tests for releasing a claim.
- [ ] Add failing tests for dependencies blocking claim.
- [ ] Add failing tests for cycle rejection.
- [ ] Add failing tests for completing a task and unblocking dependents.
- [ ] Implement task list state by replaying durable team task events.
- [ ] Ensure all mutations append journal events before returning success.
- [ ] Ensure invalid mutation attempts do not append success events.
- [ ] Ensure member pause/termination prevents new claims by that member.

**Focused commands:**

```bash
cargo test -p jyowo-harness-team task_list
cargo test -p jyowo-harness-team team_e2e
```

**Gate:**

```bash
pnpm check:rust
```

**Subagent verification:**

- [ ] Ask the verifier to check claim atomicity, dependency DAG invariants, event persistence, and restart replay behavior.

### Task 10: Implement Team Mailbox Projection

**Files:**

- Modify: `crates/jyowo-harness-team/src/lib.rs` or add `crates/jyowo-harness-team/src/mailbox.rs`
- Modify: `crates/jyowo-harness-sdk/src/team.rs`
- Test: `crates/jyowo-harness-team/tests/bus.rs`
- Test: `crates/jyowo-harness-team/tests/replay_classifier.rs`
- Test: `crates/jyowo-harness-sdk/tests/agents_team.rs`

**Steps:**

- [ ] Add projection API that returns mailbox threads by `team_id` and optional `thread_id`.
- [ ] Build the projection from journaled `AgentMessageSentEvent` and `AgentMessageRoutedEvent`.
- [ ] Group messages with `correlation_id` by correlation thread; group legacy messages without it into synthetic message threads.
- [ ] Include routed recipients, safe payload preview, payload kind, sender, recipient, and timestamps.
- [ ] Do not expose raw payload if it can contain secrets or long tool content.
- [ ] Add SDK facade method for mailbox reads.
- [ ] Add tests for replay after restart using the durable event store.

**Focused commands:**

```bash
cargo test -p jyowo-harness-team mailbox
cargo test -p jyowo-harness-sdk agents_team --features agents-team,testing
```

**Gate:**

```bash
pnpm check:rust
```

**Subagent verification:**

- [ ] Ask the verifier to confirm mailbox state is durable and sanitized, not an in-memory broadcast receiver dump.

### Task 11: Add Agent Team Runtime Tool For Main Chat

**Files:**

- Modify: `crates/jyowo-harness-team/src/lib.rs`
- Modify: `crates/jyowo-harness-engine/src/engine.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Modify: `crates/jyowo-harness-sdk/src/team.rs`
- Test: `crates/jyowo-harness-team/tests/api.rs`
- Test: `crates/jyowo-harness-team/tests/team_e2e.rs`
- Test: `crates/jyowo-harness-sdk/tests/agents_team.rs`
- Test: `crates/jyowo-harness-engine/tests/*`

**Steps:**

- [ ] Define a built-in tool named `agent_team`.
- [ ] Tool input must support one-shot team execution with explicit goal, topology, members, and optional task list seed.
- [ ] Tool input must reject unbounded member count, unbounded turns, recursive teams, unknown topology, unsafe toolset, and missing goal.
- [ ] Tool execution must create a team, dispatch the goal, persist lifecycle/task/mailbox events, and return a safe summary.
- [ ] Tool output must include `team_id`, status, usage, participating agents, task summary, mailbox summary, and optional transcript ref.
- [ ] The tool must be absent unless `agentTeamsEnabled` is true and required runtime dependencies exist.
- [ ] The tool must fail closed if invoked while disabled.
- [ ] Coordinator control tools must remain scoped to team members and must not leak into the parent main chat.

**Focused commands:**

```bash
cargo test -p jyowo-harness-team
cargo test -p jyowo-harness-sdk agents_team --features agents-team,testing
cargo test -p jyowo-harness-engine
```

**Gate:**

```bash
pnpm check:rust
```

**Subagent verification:**

- [ ] Ask the verifier to inspect tool schema, runtime gating, team event persistence, and bounded execution limits.

### Task 12: Render Agent Team Evidence In Conversation

**Files:**

- Modify: `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify: `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
- Modify: `apps/desktop/src/features/conversation/timeline/*`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-evidence-fixtures.ts`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`
- Story: `apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx`

**Steps:**

- [ ] Project team created, member joined/left, task claimed/completed, mailbox summary, team completed/failed events into conversation evidence.
- [ ] Keep detailed mailbox and task history in Activity/Details, not the main canvas.
- [ ] Main canvas shows compact team status and a summarized task list.
- [ ] Add component tests for running, blocked dependency, completed, failed, and cancelled team states.
- [ ] Add Storybook states for team execution.

**Focused commands:**

```bash
cargo test -p jyowo-harness-journal conversation_worktree_projector
pnpm -C apps/desktop test -- conversation-timeline.test.tsx
pnpm -C apps/desktop build-storybook
```

**Gate:**

```bash
pnpm check
```

**Subagent verification:**

- [ ] Ask the verifier to confirm team projection is UI-safe, compact, and derived from durable events.

### Task 12A: Expose Agent Team Read APIs

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/events/team.rs` or create `crates/jyowo-harness-contracts/src/team.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `crates/jyowo-harness-sdk/src/team.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/capabilities/*` if command permissions are enumerated there.
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/mock-client.ts`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/frontend/frontend-engineering.md`
- Test: `crates/jyowo-harness-contracts/tests/*`
- Test: `crates/jyowo-harness-sdk/tests/agents_team.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`
- Test: `apps/desktop/src/shared/tauri/commands.test.ts`

**Steps:**

- [ ] Add DTOs for `ListAgentTeamRunsRequest/Response`, `GetAgentTeamRunRequest/Response`, `ListAgentTeamTasksRequest/Response`, and `ListAgentTeamMailboxRequest/Response`.
- [ ] Add serde shape tests and JsonSchema exports for every team read DTO.
- [ ] Add SDK facade methods for listing team runs, reading one team run, listing team tasks, and listing team mailbox threads.
- [ ] Add thin Tauri command wrappers for `list_agent_team_runs`, `get_agent_team_run`, `list_agent_team_tasks`, and `list_agent_team_mailbox`.
- [ ] Register every command in `generate_handler!`.
- [ ] Add Zod request/response schemas and invalid payload tests.
- [ ] Ensure read payloads expose only safe task text, safe mailbox previews, status, timestamps, IDs, and transcript refs.
- [ ] Add command tests for valid reads, unknown team, unauthorized conversation scope, malformed thread id, and command registration.
- [ ] Update backend command docs and frontend command docs in the same task.

**Focused commands:**

```bash
cargo test -p jyowo-harness-contracts team
cargo test -p jyowo-harness-sdk agents_team --features agents-team,testing
cargo test -p jyowo-desktop-shell agent_team
pnpm -C apps/desktop test -- commands.test.ts
pnpm check:backend-docs
pnpm check:frontend-docs
```

**Gate:**

```bash
pnpm check
```

**Subagent verification:**

- [ ] Ask the verifier to inspect command registration, SDK facade ownership, safe DTOs, frontend Zod schemas, and absence of raw mailbox payloads.

### Task 13: Add Worktree Backend Domain

**Files:**

- Create: `crates/jyowo-harness-worktree/Cargo.toml`
- Create: `crates/jyowo-harness-worktree/src/lib.rs`
- Modify: `Cargo.toml`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md` if tests are added to the critical list.
- Test: `crates/jyowo-harness-worktree/tests/*`

**Steps:**

- [ ] Add a worktree domain only if no existing backend crate already owns this.
- [ ] Implement Git repo detection.
- [ ] Implement safe worktree path allocation under `.jyowo/runtime/worktrees` or a documented workspace-owned location.
- [ ] Implement a preview-only allocation path for `prepare_background_agent` that returns `BackgroundWorktreePreview` without creating directories, branches, metadata rows or worktree refs.
- [ ] Implement create/list/cleanup metadata.
- [ ] Reject symlink traversal, non-workspace paths, branch name injection, and paths outside the allowed worktree root.
- [ ] Use `tokio::process` for non-interactive git commands.
- [ ] Keep destructive cleanup behind explicit command/policy.
- [ ] Add tests for Git repo, non-Git directory, path traversal rejection, existing branch collision, preview side-effect-free behavior, and cleanup idempotence.

**Focused commands:**

```bash
cargo test -p jyowo-harness-worktree
pnpm check:backend-docs
```

**Gate:**

```bash
pnpm check:rust
pnpm check:docs
```

**Subagent verification:**

- [ ] Ask the verifier to inspect path safety, git command construction, docs layer table, and tests.

### Task 14: Add Background Agent Contracts

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/*`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Test: `crates/jyowo-harness-contracts/tests/*`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Test: `apps/desktop/src/shared/tauri/commands.test.ts`

**Steps:**

- [ ] Add IDs, job DTOs, status enum, mode enum, `BackgroundWorktreeRef`, `BackgroundWorktreePreview`, permission ref, command request/response DTOs, event cursor and event DTOs exactly as defined in Public Contracts.
- [ ] Add JsonSchema exports.
- [ ] Add serde shape tests.
- [ ] Add frontend Zod schemas and invalid payload tests.
- [ ] Ensure all UI-visible fields use `UiSafeText` in Rust and safe display string validation in frontend schemas.
- [ ] Ensure `StartBackgroundAgentRequest.title` and `StartBackgroundAgentRequest.prompt` are request-only and no response/event schema returns raw title/prompt text.
- [ ] Add serde/Zod tests proving `BackgroundPermissionRef` accepts project-only jobs with `project_id` and optional `conversation_id`/`run_id`.
- [ ] Add serde/Zod tests proving `BackgroundWorktreePreview` does not contain `worktree_id` or cleanup state.

**Focused commands:**

```bash
cargo test -p jyowo-harness-contracts background
pnpm -C apps/desktop test -- commands.test.ts
```

**Gate:**

```bash
pnpm check:rust
pnpm check:desktop
```

**Subagent verification:**

- [ ] Ask the verifier to compare Rust contracts, schema exports, and TypeScript schemas for background agent payloads.

### Task 15A: Implement Background Agent Store And Migrations

**Files:**

- Create: `crates/jyowo-harness-background-agent/Cargo.toml`
- Create: `crates/jyowo-harness-background-agent/src/lib.rs`
- Create: `crates/jyowo-harness-background-agent/src/store.rs`
- Create: `crates/jyowo-harness-background-agent/tests/store.rs`
- Create: `crates/jyowo-harness-background-agent/tests/migrations.rs`
- Modify: `Cargo.toml`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md` if critical tests are listed.

**Steps:**

- [ ] Add the crate as an L2 domain crate with dependencies limited to contracts, observability/redaction, storage helpers and low-level async/runtime crates.
- [ ] Add a dependency test or docs policy assertion proving the crate does not depend on `jyowo-harness-sdk`, Tauri or desktop code.
- [ ] Implement durable job store with schema migration tests.
- [ ] Implement create/list/get/update status.
- [ ] Implement restart reconciliation: previously running jobs become `needs_resume` or `interrupted` with a durable reason.
- [ ] Store prompt and result bodies as redacted/offloaded blobs where needed.
- [ ] Never store raw provider credentials, raw tool payloads, or raw model reasoning.
- [ ] Add tests for migrations, old row compatibility, create/list/get/update, restart reconciliation, redaction before persistence and blob offload.

**Focused commands:**

```bash
cargo test -p jyowo-harness-background-agent store
cargo test -p jyowo-harness-background-agent migrations
pnpm check:backend-docs
```

**Gate:**

```bash
pnpm check:rust
pnpm check:docs
```

**Subagent verification:**

- [ ] Ask the verifier to inspect durability, schema migrations, restart behavior, layer dependencies, and redaction before persistence.

### Task 15B: Implement Background Agent Supervisor And Event Stream

**Files:**

- Modify: `crates/jyowo-harness-background-agent/src/lib.rs`
- Create: `crates/jyowo-harness-background-agent/src/supervisor.rs`
- Create: `crates/jyowo-harness-background-agent/src/events.rs`
- Test: `crates/jyowo-harness-background-agent/tests/supervisor.rs`
- Test: `crates/jyowo-harness-background-agent/tests/events.rs`

**Steps:**

- [ ] Define a `BackgroundAgentRunner` trait in this crate; the trait accepts sanitized job context and returns progress/result events.
- [ ] Implement supervisor with bounded concurrent job execution.
- [ ] Implement pause/resume/cancel state transitions.
- [ ] Implement cancellation before model call and during runner/tool wait using cooperative cancellation tokens.
- [ ] Implement event emission with monotonic `BackgroundAgentEventCursor`.
- [ ] Persist every state-changing event before reporting it to subscribers.
- [ ] Implement subscribe replay-before-live semantics from `after_cursor`.
- [ ] Add tests for all status transitions, event order, replay-before-live, cancellation, concurrency limits and waiting permission state.

**Focused commands:**

```bash
cargo test -p jyowo-harness-background-agent supervisor
cargo test -p jyowo-harness-background-agent events
```

**Gate:**

```bash
pnpm check:rust
```

**Subagent verification:**

- [ ] Ask the verifier to inspect supervisor state transitions, event cursor monotonicity, replay-before-live semantics, cancellation and no SDK/Tauri dependency.

### Task 15C: Implement Background Agent Runner Boundary

**Files:**

- Create: `crates/jyowo-harness-background-agent/src/runner.rs`
- Test: `crates/jyowo-harness-background-agent/tests/runner.rs`
- Modify: `crates/jyowo-harness-sdk/src/*`
- Test: `crates/jyowo-harness-sdk/tests/background_agent.rs`

**Steps:**

- [ ] Keep `BackgroundAgentRunner` trait ownership in `jyowo-harness-background-agent`.
- [ ] Implement a test runner in the background-agent crate without importing SDK.
- [ ] Implement the SDK adapter that starts a backend session/run with selected mode and permission policy.
- [ ] Ensure runner context includes `background_agent_id`, `project_id`, optional `conversation_id`, optional `run_id`, worktree/local mode, permission mode and source identity for permissions.
- [ ] Ensure runner output stores result bodies as redacted/offloaded blobs and returns `UiSafeText` previews.
- [ ] Add tests proving the SDK adapter cannot start when `backgroundAgentsEnabled = false`.

**Focused commands:**

```bash
cargo test -p jyowo-harness-background-agent runner
cargo test -p jyowo-harness-sdk background_agent --features testing
```

**Gate:**

```bash
pnpm check:rust
```

**Subagent verification:**

- [ ] Ask the verifier to inspect runner trait ownership, SDK adapter direction, permission source identity and no raw prompt/result persistence.

### Task 16: Expose Background Agent SDK Facade And Tauri Commands

**Files:**

- Modify: `crates/jyowo-harness-sdk/src/*`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/capabilities/*` if command permissions are enumerated there.
- Test: `crates/jyowo-harness-sdk/tests/*`
- Test: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/frontend/frontend-engineering.md`

**Steps:**

- [ ] Add SDK methods for prepare/start/list/get/pause/resume/cancel/subscribe/unsubscribe background agents.
- [ ] Add SDK method for worktree cleanup request.
- [ ] Add thin Tauri command wrappers.
- [ ] Register commands in `generate_handler!`.
- [ ] Ensure start and resume reject when `backgroundAgentsEnabled = false`.
- [ ] Ensure list/get, pause, cancel, deny pending permission, and worktree cleanup stay available for existing jobs when `backgroundAgentsEnabled = false`.
- [ ] Ensure approving a pending background permission is rejected when `backgroundAgentsEnabled = false`.
- [ ] Reuse `resolve_permission` for background job permissions or extend that command contract if the current conversation-scoped shape cannot safely address background jobs.
- [ ] Ensure project-only background jobs can resolve permissions through `background_agent_id` and `project_id` without requiring `conversation_id`.
- [ ] Ensure `prepare_background_agent` does not create a job row, worktree ref, worktree directory, branch or background event.
- [ ] Add command tests for valid lifecycle.
- [ ] Add command tests for invalid ID, missing project, unauthorized scope, and unsupported worktree mode.
- [ ] Add subscription tests for replay-before-live semantics and window scoping if event streams are exposed.
- [ ] Add permission waiting tests: pending permission appears on background detail, approve resumes, deny records durable denial/failure, and project-only permission requests work without a conversation id.
- [ ] Add prepare/preflight tests proving repeated calls leave the job store, worktree store, event stream and filesystem unchanged.
- [ ] Update backend/frontend command docs.

**Focused commands:**

```bash
cargo test -p jyowo-harness-sdk background
cargo test -p jyowo-desktop-shell background_agent
pnpm check:backend-docs
pnpm check:frontend-docs
```

**Gate:**

```bash
pnpm check
```

**Subagent verification:**

- [ ] Ask the verifier to inspect IPC thinness, command registration, scope checks, and docs sync.

### Task 17: Add Background Agents Frontend API And UI

**Files:**

- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/mock-client.ts`
- Modify: `apps/desktop/src/shared/tauri/default-client.ts`
- Create: `apps/desktop/src/features/agents/*`
- Modify: `apps/desktop/src/app/shell/*` or current sidebar owner.
- Modify: `apps/desktop/src/routes/*`
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`
- Test: `apps/desktop/src/features/agents/*.test.tsx`
- Story: `apps/desktop/src/features/agents/*.stories.tsx`

**Steps:**

- [ ] Add Zod-validated frontend functions for all background commands.
- [ ] Add list/detail hooks using TanStack Query.
- [ ] Add background agent list route/surface with grouped active/waiting/paused/needs-resume/succeeded/failed/cancelled/interrupted sections.
- [ ] Add create background agent flow with prompt, project, may-edit-files intent, prepare/preflight call, mode selection and explicit local-mode acknowledgement when required.
- [ ] Add detail view with status, prompt summary, worktree, timeline, result summary and controls.
- [ ] Add pause/resume/cancel/approve permission/deny permission/request cleanup actions with pending and error states.
- [ ] Add empty, loading, ready, error, interrupted, needs-resume, waiting-for-permission, cleanup-pending states.
- [ ] Keep Zustand limited to UI panel state if needed.
- [ ] Use `shared/ui` primitives and lucide icons.
- [ ] Add Storybook states.

**Focused commands:**

```bash
pnpm -C apps/desktop test -- agents
pnpm -C apps/desktop build-storybook
```

**Gate:**

```bash
pnpm check:desktop
```

**Subagent verification:**

- [ ] Ask the verifier to inspect that frontend state is server-derived through TanStack Query, create/preflight is implemented, and controls call validated IPC.

### Task 18: Add Main Chat Background Agent Entry

**Files:**

- Modify: `apps/desktop/src/features/conversation/Composer.tsx`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
- Modify: `apps/desktop/src/features/conversation/Composer.test.tsx`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/*`
- Modify: `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Test: `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`

**Steps:**

- [ ] Add a composer action for starting the current prompt as a background agent.
- [ ] Keep normal send as the primary action.
- [ ] Hide or disable the action when `backgroundAgentsEnabled = false` or backend reports unavailable.
- [ ] Run prepare/preflight before start and show Git worktree/local mode before creating the job.
- [ ] Require explicit local-mode acknowledgement when Git repo + may-edit-files + local mode.
- [ ] Ensure the composer background action calls `prepare_background_agent` then `start_background_agent`, never `start_run`.
- [ ] Add tests proving successful composer background start creates a durable background job row and created event.
- [ ] Add tests proving preflight alone does not create a job, worktree ref, branch, directory or background event.
- [ ] Show background run creation in the conversation as a compact evidence/status row.
- [ ] Link to background agent detail.
- [ ] Add tests for starting background mode, disabled setting, unavailable backend, preflight local acknowledgement, pending state, command error, and success state.
- [ ] Add projection tests for background job events appearing in the related conversation.

**Focused commands:**

```bash
pnpm -C apps/desktop test -- Composer.test.tsx ConversationWorkspace.test.tsx
cargo test -p jyowo-harness-journal conversation_worktree_projector
```

**Gate:**

```bash
pnpm check
```

**Subagent verification:**

- [ ] Ask the verifier to inspect that background entry does not bypass normal prompt validation, permissions, or redaction.

### Task 19: Add Agent Team Read Surfaces

**Files:**

- Create: `apps/desktop/src/features/agents/team-*` or integrate under `features/agents`.
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/mock-client.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`
- Test: `apps/desktop/src/features/agents/*.test.tsx`
- Story: `apps/desktop/src/features/agents/*.stories.tsx`

**Steps:**

- [ ] Add UI for team run detail: members, task list, claims, blocked dependencies, mailbox thread summary, final report.
- [ ] Keep it as a detail surface reachable from conversation evidence or Agents page.
- [ ] If team run detail is placed under `features/agents`, define navigation labels so "Background Agents" and "Agent Team Runs" are separate tabs or sections.
- [ ] Add tests for empty task list, claimed task, blocked dependency, mailbox messages, completed team, and failed team.
- [ ] Ensure no raw mailbox payload or transcript body is shown without explicit safe projection.

**Focused commands:**

```bash
pnpm -C apps/desktop test -- agents
pnpm -C apps/desktop build-storybook
```

**Gate:**

```bash
pnpm check:desktop
```

**Subagent verification:**

- [ ] Ask the verifier to inspect UI state coverage and absence of raw event rendering.

### Task 20: Security Review And Redaction Coverage

**Files:**

- Modify tests across:
  - `crates/jyowo-harness-observability/tests/*`
  - `crates/jyowo-harness-journal/tests/*`
  - `crates/jyowo-harness-team/tests/*`
  - `crates/jyowo-harness-background-agent/tests/*`
  - `apps/desktop/src-tauri/tests/commands.rs`
  - `apps/desktop/src/shared/tauri/commands.test.ts`

**Steps:**

- [ ] Add tests proving team task text is redacted before durable read.
- [ ] Add tests proving mailbox preview is redacted.
- [ ] Add tests proving background prompt/result summaries are redacted/offloaded.
- [ ] Add tests proving support bundle and Replay do not expose new raw payloads.
- [ ] Add tests proving permission requests from subagents/background jobs surface with source identity.
- [ ] Add tests proving project-only background permission requests resolve through `background_agent_id` and `project_id`.
- [ ] Add tests proving `backgroundAgentsEnabled = false` blocks start/resume and approving pending background permissions.
- [ ] Add tests proving `backgroundAgentsEnabled = false` still allows list/get, pause, cancel, deny pending permission and worktree cleanup for existing jobs.
- [ ] Add tests proving Git local mode for editing jobs requires explicit acknowledgement.
- [ ] Add tests proving background jobs cannot run destructive actions without policy path approval.
- [ ] Run a manual review for prompt injection surfaces in background internet/tool flows.

**Focused commands:**

```bash
cargo test --workspace redactor
cargo test --workspace replay
cargo test --workspace permission
pnpm -C apps/desktop test -- commands.test.ts
```

**Gate:**

```bash
pnpm check
```

**Subagent verification:**

- [ ] Ask a security-focused subagent to review permission, secret, redaction, replay, support bundle, and background unattended execution paths.

### Task 21: Full Documentation Update

**Files:**

- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/backend/backend-quality.md`
- Modify: `docs/frontend/frontend-engineering.md`
- Modify: `docs/frontend/frontend-product-ux.md`
- Modify: `docs/frontend/frontend-quality.md`
- Modify: `AGENTS.md` only if agent execution rules need permanent updates.

**Steps:**

- [ ] Document new crates and dependency layer ownership.
- [ ] Document new Tauri commands and payload shapes.
- [ ] Document new frontend feature ownership under `features/agents`.
- [ ] Document new required tests and gates.
- [ ] Document that subagent/team/background settings are runtime policy inputs.
- [ ] Document background agent durability, restart semantics, preflight, worktree/local mode and permission waiting flow.
- [ ] Document navigation IA for Background Agents and Agent Team Runs if both live under `features/agents`.
- [ ] Ensure docs gate accepts the changed active docs.

**Focused commands:**

```bash
pnpm check:frontend-docs
pnpm check:backend-docs
pnpm check:docs
```

**Gate:**

```bash
pnpm check:docs
```

**Subagent verification:**

- [ ] Ask the verifier to compare docs against actual command names, workspace crate members, frontend directories, and quality gates.

### Task 22: End-To-End Acceptance

**Files:**

- Test: `apps/desktop/src/features/settings/ExecutionSettings.test.tsx`
- Test: `apps/desktop/src/features/conversation/*.test.tsx`
- Test: `apps/desktop/src/features/agents/*.test.tsx`
- Test: `apps/desktop/src-tauri/tests/commands.rs`
- Test: `crates/jyowo-harness-sdk/tests/*`
- Test: `crates/jyowo-harness-team/tests/*`
- Test: `crates/jyowo-harness-background-agent/tests/*`
- Test: Playwright E2E under current app test structure.

**Steps:**

- [ ] Add an E2E or integration flow: enable subagents, start chat that delegates to `agent`, see subagent evidence.
- [ ] Add an E2E or integration flow: enable agent teams, start chat that uses `agent_team`, see team evidence and task/mailbox detail.
- [ ] Add an E2E or integration flow: start background agent from composer, see it in Agents list, pause/resume/cancel or finish it, open detail.
- [ ] Add an E2E or integration flow: create background agent from Agents page empty/list state through prepare/preflight.
- [ ] Add an E2E or integration assertion proving background start creates a durable `BackgroundAgentJob` and `Created` event, not a hidden `start_run` conversation run.
- [ ] Add an E2E or integration flow: background agent waits for permission, approve/deny resolves through the existing permission command, detail state updates.
- [ ] Add restart/reopen integration coverage for background job list and status reconciliation.
- [ ] Verify settings toggles survive app/runtime reload.
- [ ] Verify disabled subagent/team toggles remove tools from main chat.
- [ ] Verify disabled background toggle blocks background start/resume/permission approval while still allowing recovery actions on existing jobs.

**Focused commands:**

```bash
pnpm -C apps/desktop test
pnpm -C apps/desktop test:e2e
cargo test --workspace
```

**Gate:**

```bash
pnpm check
pnpm -C apps/desktop check:full
```

**Subagent verification:**

- [ ] Ask the verifier to run a requirements matrix against this whole plan and mark any unmet task before final delivery.

## Requirements Matrix

The final verifier must fill this matrix with file references, test names, command outputs and subagent verification links. Any row without evidence blocks completion.

| Requirement | Required Evidence |
|---|---|
| Stable naming | `rg` proof that user-facing/public names do not use experimental wording except documented Cargo feature labels |
| Settings policy | Rust and frontend tests for `subagentsEnabled`, `agentTeamsEnabled`, `backgroundAgentsEnabled`, migration from old settings, unavailable feature rejection |
| Main chat delegation | SDK/runtime tests proving `agent` and `agent_team` descriptors appear only when settings and per-run options allow them; conversation UI tests proving mode evidence is rendered |
| Per-run user control | Composer tests for none/auto/prefer-subagents/prefer-agent-team and backend run option payloads; serde/Zod tests reject `background_agents` in `allowedCapabilities`; `start_run` rejects `backgroundStartMode` |
| Team task model | Contract/schema tests, team replay tests, claim conflict tests, dependency cycle tests, dependency unblock tests |
| Team mailbox | Contract compatibility tests for message events with/without `correlation_id`, durable projection tests, frontend safe preview tests |
| Team read APIs | SDK, Tauri, frontend Zod and command registration tests for `list_agent_team_runs`, `get_agent_team_run`, `list_agent_team_tasks`, `list_agent_team_mailbox` |
| Background contracts | Serde/schema/Zod tests for every request/response/event/cursor/worktree/permission DTO, including project-only permission refs and preview-only worktree DTOs |
| Background store | Migration tests, restart reconciliation tests, redaction/blob offload tests, no SDK/Tauri dependency proof |
| Background supervisor | State transition tests, event cursor tests, replay-before-live tests, cancellation/concurrency tests |
| Background runner | SDK adapter tests, permission source identity tests, no raw prompt/result persistence tests |
| Background UX | Agents page create/list/detail tests, composer background preflight tests, permission waiting approve/deny tests, worktree/local acknowledgement tests, and proof composer background start uses `start_background_agent` rather than `start_run` |
| Worktree safety | Path traversal, branch injection, symlink, preview side-effect-free behavior, cleanup idempotence and explicit local mode acknowledgement tests |
| Security/redaction | Redactor, Replay, support bundle, project-only permission source, destructive action policy and frontend unsafe display rejection tests |
| Documentation | `pnpm check:docs`, `pnpm check:frontend-docs`, `pnpm check:backend-docs`; docs match actual command and crate names |
| Full gates | `pnpm check`, `pnpm check:desktop`, `pnpm check:rust`, `cargo test --workspace`, UI E2E/Storybook commands with non-zero relevant tests |

## Final Release Gate

Run all commands before merge:

```bash
pnpm check
pnpm check:docs
pnpm check:agent-docs
pnpm check:frontend-docs
pnpm check:backend-docs
pnpm check:desktop
pnpm check:rust
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
```

For UI changes also run:

```bash
pnpm -C apps/desktop build-storybook
pnpm -C apps/desktop test:e2e
```

For security-sensitive changes, run an independent security review after normal code review.

Final completion requires:

- [ ] All task checkboxes complete.
- [ ] Every task has a recorded subagent verification pass.
- [ ] Every valid verifier finding is fixed or explicitly documented as rejected with code evidence.
- [ ] `pnpm check` passes.
- [ ] No public contract drift without tests.
- [ ] No new unchecked Tauri command.
- [ ] No frontend-only policy gate.
- [ ] No raw Secret or private path in events, logs, traces, snapshots, UI state or support bundle.
- [ ] Background agent restart behavior is tested.
- [ ] Agent team task claim/dependency/mailbox behavior is tested.
- [ ] Subagent, agent team and background agent settings are enforced by Rust.
