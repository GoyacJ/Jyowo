# Conversation Turn Work Tree Timeline Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the conversation canvas from a flat RunEvent timeline with a user-facing conversation turn work tree that correctly nests assistant text, thinking summaries, tool attempts, permissions, retries, artifacts, and final answers.

**Architecture:** RunEvent remains the durable fact log. Rust owns a UI-safe projection from journal events into `ConversationTurn` work trees. React renders the projection and local UI state only; Activity, Details, Replay, and Raw JSON remain the execution transparency surfaces.

**Tech Stack:** Rust 1.96, serde, schemars, rusqlite, Tauri 2, React 19, TypeScript 6, Zod, TanStack Query, Vitest, Testing Library, Storybook.

---

## Design Baseline

The product surface is a conversation. Runs, events, tool calls, permissions, raw payloads, and replay cursors are execution details.

Current broken model:

```text
ConversationTimeline = RunEvent-derived blocks sorted by displaySequence
```

Target model:

```text
ConversationTimeline = ConversationTurn[]

ConversationTurn
├─ UserMessage
└─ AssistantWork
   ├─ ThinkingSummary
   ├─ TextSegment
   ├─ ToolGroup
   │  └─ ToolAttempt
   │     └─ PermissionState
   ├─ ArtifactSegment
   ├─ ReviewRequestSegment
   ├─ ClarificationRequestSegment
   ├─ NoticeSegment
   └─ ErrorSegment
```

Raw events still exist. They must be visible through Activity, Details, Replay, and Raw JSON, not through the main conversation canvas.

## Non-Negotiable Invariants

These are hard rules. Do not implement an alternative without updating this plan first.

- The conversation canvas must not render raw `RunEvent` objects.
- Top-level conversation items must be turns, not tools, permissions, thinking, or bare assistant messages.
- A single user message with one run must render one assistant work tree, even when the model performs multiple tool loops.
- `assistant.completed` must not directly create a top-level visible assistant message.
- Tool-call-only assistant messages must not produce empty assistant text.
- Thinking content must be summarized or withheld. Do not render full chain-of-thought.
- Permissions must be nested under the owning tool attempt.
- Tool execution status and permission decision status must be distinct.
- Snapshot hydration, gap recovery, and live updates must use the same Rust projection semantics.
- React may keep local expansion and optimistic state, but not product projection logic.
- Raw withheld placeholders such as `Tool error withheld from conversation timeline.` must not appear in the conversation canvas.
- Security decisions, redaction, permission finality, and safe failure summaries stay in Rust.
- Visible thinking text must never be derived from raw chain-of-thought deltas.
- Worktree paging must page complete turns. It must not project a partial raw-event page and then slice the result.
- `get_conversation.messages` must not drive the conversation canvas. It may remain for metadata, title, list, or compatibility surfaces only.
- Visible ordering must come from projected turn position, assistant segment order, and tool attempt order. React must not infer product order from raw event arrival time.

## Target Contract Shape

Add these UI-facing projection contracts in `crates/jyowo-harness-contracts/src/conversation.rs`.

Use serde `camelCase` for public payload fields. Use stable enum tags for segment kinds.
Every visible node has a stable id so React keys, scroll anchors, optimistic replacement, Details navigation, and visual regression tests are not tied to array indexes.
Every assistant segment and tool attempt must also carry an explicit order field inside its parent.

```rust
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationWorktreePage {
    pub turns: Vec<ConversationTurn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_cursor: Option<ConversationTurnCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_cursor: Option<ConversationCursor>,
    pub has_more_before: bool,
    pub has_more_after: bool,
    pub gap: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurnCursor {
    pub turn_id: String,
    pub position: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurn {
    pub id: String,
    pub conversation_id: String,
    pub position: u64,
    pub user: ConversationTurnUserMessage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant: Option<AssistantWork>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssistantWork {
    pub id: String,
    pub run_id: String,
    pub status: AssistantWorkStatus,
    pub segments: Vec<AssistantSegment>,
    pub event_refs: Vec<ConversationEventRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AssistantSegment {
    Thinking(ThinkingSegment),
    Text(TextSegment),
    ToolGroup(ToolGroupSegment),
    Artifact(ArtifactSegment),
    ReviewRequest(ReviewRequestSegment),
    ClarificationRequest(ClarificationRequestSegment),
    Notice(NoticeSegment),
    Error(ErrorSegment),
}
```

Stable id rules:

```text
ConversationTurn.id = turn:{userMessageId}
AssistantWork.id = assistant:{runId}
ThinkingSegment.id = segment:thinking:{runId}
TextSegment.id = segment:text:{messageId}
ToolGroupSegment.id = segment:tools:{firstToolUseId}
ToolAttempt.id = tool:{toolUseId}
ToolPermissionState.id = permission:{requestId}
ArtifactSegment.id = segment:artifact:{artifactId}
ReviewRequestSegment.id = segment:review:{requestId}
ClarificationRequestSegment.id = segment:clarification:{requestId}
NoticeSegment.id = segment:notice:{eventId}
ErrorSegment.id = segment:error:{eventId}
```

TypeScript mirrors the Rust contract in `apps/desktop/src/shared/tauri/commands.ts` with Zod validation. Feature components import typed projection models from the command client, not raw event schemas.

## Target UI Shape

Rendered example:

```text
你：
你可以帮我生成一张章鱼的图片吗

Jyowo
当然可以。我会先检查可用图像生成能力。

▸ 思考摘要
  正在检查可用图像生成工具

工具
  MiniMaxTextToImage      已完成
    权限：已批准

  MiniMaxModelRetrieve    失败
    权限：已批准
    图像模型接口当前不可用

非常抱歉，目前图片生成模型不可用...
```

Do not show these in the main canvas:

```text
Jyowo Complete
Jyowo Complete
Approved failed
Tool error withheld from conversation timeline.
```

## Implementation Strategy

Use a backend projection boundary.

```text
RunEvent Journal
  -> conversation read model raw timeline table
  -> materialized worktree projection tables
  -> ConversationWorktreePage
  -> Tauri command
  -> Zod parsed projection
  -> React render-only components
```

Keep raw event access for Activity and Details. The conversation canvas switches to `ConversationWorktreePage`.

## Paging And Materialization

Do not build a `ConversationWorktreePage` by reading only raw events after a cursor and projecting that partial set. A page can start in the middle of a turn. That loses the user message, run ownership, thinking state, tool attempts, permission ownership, and final answer ordering.

The read model must materialize the worktree projection during journal projection, in the same boundary that already projects redacted conversation timeline events.

Add materialized tables in `crates/jyowo-harness-journal`:

```text
conversation_worktree_turn
conversation_worktree_assistant_segment
conversation_worktree_tool_attempt
conversation_worktree_event_ref
```

Minimum ownership rules:

- `conversation_worktree_turn` stores one row per user turn with `turn_id`, `conversation_id`, `session_id`, `position`, user message fields, assistant run id, assistant status, and latest projected event cursor.
- `conversation_worktree_assistant_segment` stores ordered assistant segments keyed by stable segment id and turn id.
- `conversation_worktree_tool_attempt` stores ordered tool attempts keyed by `toolUseId`, including nested permission state fields when present.
- `conversation_worktree_event_ref` stores event refs for Details navigation without making raw payloads part of visible UI fields.

Paging rules:

- `limit` means number of complete turns.
- `pageCursor` is a turn cursor based on `turn_id` and `position`, not a raw event cursor.
- `eventCursor` is the latest raw event cursor consumed by the projection and is used for sync, invalidation, and Activity handoff.
- `hasMoreBefore` and `hasMoreAfter` describe turn paging, not raw event availability.
- A page must never split a turn.
- Gap recovery must rebuild or read the materialized projection, not render from partial raw events.

If materialized tables are rejected during implementation, the only acceptable alternative is replaying from session start into a complete in-memory projection before slicing by turn. That alternative must keep the same public contract and tests. It is expected to be slower and should not be the default.

## Thinking Display Policy

The conversation canvas may show model thinking state, not raw model thoughts.

Allowed sources:

- event-derived status text such as `正在检查工具结果` or `正在准备最终回复`
- an explicitly safe summary field if a future backend contract adds one
- a withheld placeholder such as `思考内容已折叠`

Forbidden sources:

- raw chain-of-thought deltas
- raw tool payloads
- raw model messages whose contract does not mark them as UI-safe

Tests must assert that raw thought text from journal events never appears in `ThinkingSegment.summary`.

## AI Drift Guard

Every implementation task must keep this checklist updated in its task notes:

```text
[ ] Top-level canvas model is ConversationTurn[]
[ ] No new top-level tool/permission/thinking block was added
[ ] No React component parses raw RunEvent for conversation product structure
[ ] Snapshot and live updates use the same projection source
[ ] ConversationCanvas reads page_conversation_worktree plus optimistic overlay only
[ ] get_conversation.messages does not drive ConversationCanvas
[ ] Empty assistant body does not render
[ ] Tool permission is nested under tool attempt
[ ] Thinking is status-derived, explicitly safe, or withheld
[ ] Tool failure has a safe user-facing summary
[ ] Activity/Raw JSON still expose redacted raw events
[ ] Tests were added before implementation
```

Add automated guards where possible:

```bash
pnpm -C apps/desktop test src/features/conversation/conversation-production-boundaries.test.ts
rg -n "permissionRequest|PermissionRequestBlock|Tool error withheld from conversation timeline" apps/desktop/src/features/conversation apps/desktop/src/shared
rg -n "RunEvent" apps/desktop/src/features/conversation/timeline
rg -n "get_conversation.messages|messages:" apps/desktop/src/features/conversation apps/desktop/src/shared/tauri
```

The boundary test must assert:

- conversation timeline render/model files do not import `RunEvent`
- old top-level block kinds are absent
- `ConversationCanvas` does not read `get_conversation.messages`

The first `rg` command must return no matches after migration.

The second command may only match Activity or explicit event-stream invalidation code, not render components or projection view models.

The third command may only match command-client definitions, command tests, or non-canvas compatibility code.

## Task 1: Add Projection Contract Types

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/conversation.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Test: `crates/jyowo-harness-contracts/tests/m1_contracts.rs`

**Step 1: Write failing contract tests**

Add tests covering:

- `ConversationWorktreePage` serializes with `turns`, `pageCursor`, `eventCursor`, `hasMoreBefore`, `hasMoreAfter`, and `gap`.
- `ConversationTurnCursor` serializes with `turnId` and `position`.
- `AssistantSegment` uses a stable tagged shape.
- every turn, assistant work, segment, tool attempt, and permission has a stable id.
- assistant segments and tool attempts carry explicit parent-local order fields.
- `ToolAttempt.permission` is nested.
- `ThinkingSegment` supports `running`, `complete`, and `withheld`.
- `ReviewRequest` and `ClarificationRequest` segment variants are schema-exported.
- `ConversationEventRef` keeps event id and cursor.

Run:

```bash
cargo test -p jyowo-harness-contracts conversation_worktree --test m1_contracts
```

Expected: fail because types do not exist.

**Step 2: Add minimal contract types**

Add enums:

```rust
pub enum AssistantWorkStatus {
    Running,
    Complete,
    Failed,
    Cancelled,
}

pub enum ThinkingSegmentStatus {
    Running,
    Complete,
    Withheld,
}

pub enum ToolAttemptStatus {
    Queued,
    WaitingPermission,
    Running,
    Completed,
    Failed,
    Denied,
}

pub enum ToolPermissionStatus {
    Pending,
    Submitting,
    Approved,
    Denied,
    Failed,
}
```

Add structs for:

- `ConversationWorktreePage`
- `ConversationTurnCursor`
- `ConversationTurn`
- `ConversationTurnUserMessage`
- `AssistantWork`
- `AssistantSegment`
- `ThinkingSegment`
- `ThinkingSummary`
- `TextSegment`
- `ToolGroupSegment`
- `ToolAttempt`
- `ToolPermissionState`
- `ArtifactSegment`
- `ReviewRequestSegment`
- `ClarificationRequestSegment`
- `NoticeSegment`
- `ErrorSegment`
- `ConversationEventRef`

Use `UiSafeText` for all user-visible text.
Add `id` to every visible node listed in the stable id rules above.

**Step 3: Export schemas**

Update `schema_export.rs` so the projection shape is part of the public schema export.

**Step 4: Verify**

Run:

```bash
cargo test -p jyowo-harness-contracts conversation_worktree --test m1_contracts
cargo test -p jyowo-harness-contracts schema_export --test m1_contracts
```

Expected: pass.

**Checkpoint:**

- Run the focused contract tests.
- Inspect the diff for only contract, schema export, and contract test changes.
- Do not commit unless the user explicitly asks. If committing later, exclude unrelated dirty files.

## Task 2: Build Pure Rust Worktree Projector

**Files:**

- Create: `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify: `crates/jyowo-harness-journal/src/lib.rs`
- Test: `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`

**Step 1: Write failing projector tests**

Create focused tests using redacted `ConversationTimelineEvent` inputs.

Required tests:

- user message creates one `ConversationTurn`
- same run text/tool/text/final answer remains one assistant work tree
- tool-call-only assistant completed event creates no empty text segment
- multiple `assistant.completed` events in one run append text segments under one assistant work tree
- `permission.requested` attaches to matching `toolUseId`
- `permission.resolved` updates nested permission status
- `tool.failed` stores safe `failureSummary`
- thinking events produce a single status-derived `ThinkingSegment`
- withheld thinking creates `ThinkingSegmentStatus::Withheld`
- raw chain-of-thought text from a thinking event never appears in `ThinkingSegment.summary`
- duplicated event ids are idempotent

Run:

```bash
cargo test -p jyowo-harness-journal --test conversation_worktree_projector
```

Expected: fail because projector does not exist.

**Step 2: Implement pure projector**

Expose a pure function:

```rust
pub fn project_conversation_worktree_snapshot(
    conversation_id: &str,
    events: impl IntoIterator<Item = ConversationTimelineEvent>,
) -> ConversationWorktreeProjection
```

`ConversationWorktreeProjection` is an internal journal type. It contains the complete projected turns for the supplied event set, the latest consumed event cursor, and event refs needed to write materialized rows. It is not the paged public API.

The implementation must:

- keep `ConversationTurn` keyed by user message id or run-associated user message
- attach run events to the active turn for that run
- attach tool events by `toolUseId`
- attach permission events by `requestId` and `toolUseId`
- merge thinking state into one thinking segment per assistant work unless a later design explicitly requires multiple summaries
- not emit text segments for empty assistant body
- preserve `eventRefs` for Details navigation
- assign deterministic segment and tool attempt order from projection semantics
- never expose raw JSON payload in user-visible fields
- never derive visible thinking summary text from raw chain-of-thought content

**Step 3: Add safe summary helpers**

Add helpers inside the projector module:

```rust
fn safe_tool_failure_summary(event: &ConversationTimelineEvent) -> UiSafeText
fn thinking_status_from_event(event: &ConversationTimelineEvent) -> ThinkingSegmentStatus
fn safe_thinking_display(
    status: ThinkingSegmentStatus,
    explicit_safe_summary: Option<UiSafeText>,
) -> ThinkingSummary
fn is_empty_assistant_body(value: &serde_json::Value) -> bool
```

Use only redacted payload fields and explicitly UI-safe summary fields. If a useful tool failure field is missing, return a product sentence such as:

```text
工具执行失败。详情可在 Activity 中查看。
```

Do not return:

```text
Tool error withheld from conversation timeline.
```

Do not inspect raw thought text for `safe_thinking_display`. The allowed outputs are event-derived status text, explicitly safe summary text, or a withheld placeholder.

**Step 4: Verify**

Run:

```bash
cargo test -p jyowo-harness-journal --test conversation_worktree_projector
```

Expected: pass.

**Checkpoint:**

- Run the focused projector tests.
- Inspect the diff for only projector module, module exports, and projector tests.
- Do not commit unless the user explicitly asks. If committing later, exclude unrelated dirty files.

## Task 3: Expose Worktree Paging From Journal And SDK

**Files:**

- Modify: `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Test: `crates/jyowo-harness-journal/tests/conversation_read_model.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Test: `crates/jyowo-harness-sdk/tests/conversation_read_model.rs`

**Step 1: Write failing read-model tests**

Add tests covering:

- `page_conversation_worktree` returns `ConversationWorktreePage`.
- materialized worktree rows are updated when raw conversation timeline events are projected.
- `limit = 1` returns one complete turn even when that turn contains many raw events.
- `pageCursor` points to the next turn page boundary, not the last raw event.
- `eventCursor` points to the latest raw event consumed by the worktree projection.
- `hasMoreBefore` and `hasMoreAfter` are based on turn availability.
- projection is not built from a partial raw event page.
- gap flag behaves the same as raw timeline paging.
- raw `page_conversation_timeline` still works for Activity/Replay if retained.

Run:

```bash
cargo test -p jyowo-harness-journal sqlite_conversation_read_model_projects_worktree
cargo test -p jyowo-harness-sdk conversation_worktree
```

Expected: fail because API does not exist.

**Step 2: Add materialized projection tables**

Add and populate these read-model tables:

```text
conversation_worktree_turn
conversation_worktree_assistant_segment
conversation_worktree_tool_attempt
conversation_worktree_event_ref
```

Implementation rules:

- update materialized worktree rows inside the read-model projection path
- keep projection idempotent by raw event id
- preserve stable node ids from the contract section
- persist segment order and tool attempt order
- keep redacted raw event refs available for Details navigation
- do not duplicate product projection logic in Tauri or React

**Step 3: Add journal API**

Add:

```rust
pub enum ConversationTurnPageDirection {
    Before,
    After,
}

pub async fn page_worktree(
    &self,
    tenant_id: TenantId,
    session_id: SessionId,
    page_cursor: Option<ConversationTurnCursor>,
    direction: ConversationTurnPageDirection,
    limit_turns: usize,
) -> Result<ConversationWorktreePage, JournalError>
```

Implementation:

- read complete turns from materialized worktree tables
- clamp `limit_turns`
- return `pageCursor` from the turn page boundary
- return `eventCursor` from the latest materialized projection event cursor
- set `hasMoreBefore` and `hasMoreAfter` from turn table queries
- never split a turn across pages
- do not re-project a partial raw event page

**Step 4: Add SDK facade**

Add:

```rust
pub async fn page_conversation_worktree(
    &self,
    conversation_id: &str,
    page_cursor: Option<ConversationTurnCursor>,
    direction: ConversationTurnPageDirection,
    limit_turns: usize,
) -> Result<ConversationWorktreePage, HarnessError>
```

Do not let Tauri reach directly into journal internals.

**Step 5: Verify**

Run:

```bash
cargo test -p jyowo-harness-journal conversation_worktree
cargo test -p jyowo-harness-sdk conversation_worktree
```

Expected: pass.

**Checkpoint:**

- Run the focused journal and SDK tests.
- Inspect the diff for read-model, SDK facade, and their tests.
- Do not commit unless the user explicitly asks. If committing later, exclude unrelated dirty files.

## Task 4: Add Tauri Command Boundary

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/frontend/frontend-engineering.md`

**Step 1: Write failing command tests**

Add tests covering:

- `page_conversation_worktree` returns `turns`.
- response shape is byte-for-byte compatible with `harness_contracts::ConversationWorktreePage` schema.
- tool-call-only assistant message does not create empty text segment.
- nested permission includes `requestId` and `toolUseId`.
- safe failure summary does not contain raw tool error or private path.
- malformed conversation id fails closed.

Run:

```bash
cargo test -p jyowo-desktop-shell --test commands page_conversation_worktree
```

Expected: fail because command does not exist.

**Step 2: Implement command**

Add Tauri command:

```rust
#[tauri::command(rename_all = "camelCase")]
pub async fn page_conversation_worktree(
    request: PageConversationWorktreeRequest,
    runtime_handle: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<harness_contracts::ConversationWorktreePage, CommandErrorPayload>
```

The command must:

- validate `conversationId`
- validate optional `pageCursor` shape
- validate `direction`
- call SDK facade
- serialize contract fields as camelCase
- not assemble ad hoc JSON strings
- not duplicate `ConversationWorktreePage` as a parallel Tauri-only payload type

If a wrapper is unavoidable because of existing command plumbing, add a parity test proving the wrapper serializes exactly like `harness_contracts::ConversationWorktreePage`.

Register it in `generate_handler!`.

**Step 3: Update docs**

Update command lists and payload examples in:

- `docs/backend/backend-engineering.md`
- `docs/frontend/frontend-engineering.md`

State that `page_conversation_worktree` is the conversation canvas data source. `page_conversation_timeline`, if retained, is a raw execution surface.
State that `get_conversation.messages` does not drive `ConversationCanvas`.

**Step 4: Verify**

Run:

```bash
cargo test -p jyowo-desktop-shell --test commands page_conversation_worktree
pnpm check:docs
```

Expected: pass.

**Checkpoint:**

- Run the focused desktop shell command tests and docs gate.
- Inspect the diff for command boundary, command tests, and docs.
- Do not commit unless the user explicitly asks. If committing later, exclude unrelated dirty files.

## Task 5: Add Frontend Zod Schema And Command Client API

**Files:**

- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/default-client.ts`
- Modify: `apps/desktop/src/shared/tauri/mock-client.ts`
- Test: `apps/desktop/src/shared/tauri/commands.test.ts`

**Step 1: Write failing schema tests**

Add tests covering:

- valid worktree page parses
- `pageCursor` and `eventCursor` parse as separate cursor types
- missing turn `position` fails
- missing stable node id fails
- missing user message fails
- missing tool `toolUseId` fails
- permission without `requestId` fails
- raw `RunEvent` shape is rejected by worktree schema
- user-visible text containing private absolute path fails or is already redacted

Run:

```bash
pnpm -C apps/desktop test src/shared/tauri/commands.test.ts
```

Expected: fail because schemas do not exist.

**Step 2: Add schemas and client method**

Add:

```ts
const conversationWorktreePageSchema = z.object({
  turns: z.array(conversationTurnSchema),
  pageCursor: conversationTurnCursorSchema.optional(),
  eventCursor: conversationCursorSchema.optional(),
  hasMoreBefore: z.boolean(),
  hasMoreAfter: z.boolean(),
  gap: z.boolean(),
}).strict()
```

Add `CommandClient.pageConversationWorktree`.

Use exact Tauri command name:

```ts
page_conversation_worktree
```

**Step 3: Update mocks**

Update `mock-client.ts` with realistic worktree page fixtures:

- one running assistant turn
- one completed assistant turn
- one failed tool attempt with safe summary
- one pending permission
- one withheld thinking segment

**Step 4: Verify**

Run:

```bash
pnpm -C apps/desktop test src/shared/tauri/commands.test.ts
```

Expected: pass.

**Checkpoint:**

- Run the focused command-client schema tests.
- Inspect the diff for shared Tauri client and tests.
- Do not commit unless the user explicitly asks. If committing later, exclude unrelated dirty files.

## Task 6: Replace Timeline Domain Model With Turn Work Tree

**Files:**

- Modify: `apps/desktop/src/features/conversation/timeline/conversation-blocks.ts`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline-selectors.ts`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-scroll-controller.ts`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline-selectors.test.ts`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-scroll-controller.test.ts`
- Test: `apps/desktop/src/features/conversation/conversation-production-boundaries.test.ts`

**Step 1: Write failing frontend model tests**

Add tests covering:

- selectors return `ConversationTurn[]`
- composer mode detects active assistant work from turn status
- pending permission selector reads nested tool permission
- scroll anchor uses turn id, not raw block id
- production boundary test fails if timeline render/model files import raw `RunEvent`
- production boundary test fails if old top-level block kinds remain exported

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline-selectors.test.ts src/features/conversation/timeline/conversation-scroll-controller.test.ts src/features/conversation/conversation-production-boundaries.test.ts
```

Expected: fail.

**Step 2: Replace block types**

Replace top-level block model with:

```ts
export type ConversationTimelineItem = ConversationTurn
```

Remove or deprecate these as top-level canvas models:

- `AssistantMessageBlock`
- `AssistantStreamingBlock`
- `ThinkingBlock`
- `ToolGroupBlock`
- `PermissionRequestBlock`

It is fine to keep local component props named `ToolGroupSegment` or `ThinkingSegment`. Do not keep them as top-level items.

**Step 3: Update selectors**

Selectors must operate on projected turns:

- `selectTurns`
- `selectComposerMode`
- `selectPendingPermissions`
- `selectShouldPollFallback`

Do not import `RunEvent` in selector code.

**Step 4: Verify**

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline-selectors.test.ts src/features/conversation/timeline/conversation-scroll-controller.test.ts src/features/conversation/conversation-production-boundaries.test.ts
```

Expected: pass.

**Checkpoint:**

- Run the focused selector, scroll, and boundary tests.
- Inspect the diff for conversation timeline model, selectors, scroll controller, and tests.
- Do not commit unless the user explicitly asks. If committing later, exclude unrelated dirty files.

## Task 7: Replace Event Reducer With Projection Store

**Files:**

- Modify: `apps/desktop/src/features/conversation/timeline/use-conversation-timeline.ts`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline-store.ts`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline-source.ts`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline-store.test.ts`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline-source.test.ts`

**Step 1: Write failing store tests**

Add tests covering:

- initial query loads worktree page
- `ConversationCanvas` source is `pageConversationWorktree` plus optimistic overlay only
- `get_conversation.messages` is not read by the canvas store
- optimistic local user turn is replaced by projected turn via `clientMessageId`
- live update invalidates/refetches projection instead of applying raw event shape to UI
- 100 streaming raw-event batches schedule no more than the allowed throttled worktree refetches
- terminal events trigger an immediate worktree refetch
- gap recovery refetches worktree page
- active run status is derived from assistant work status

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline-store.test.ts src/features/conversation/timeline/conversation-timeline-source.test.ts
```

Expected: fail.

**Step 2: Remove product use of raw reducer**

The conversation canvas should no longer depend on:

- `conversation-timeline-reducer.ts`
- `conversation-timeline-index.ts`
- `conversation-timeline-thinking.ts`

If these files remain temporarily, they must not be imported by the conversation canvas.

**Step 3: Implement projection query flow**

Use TanStack Query for `pageConversationWorktree`.

Use existing raw event subscription only as an invalidation signal if the new projected live stream is not implemented in this task.

Rules:

- raw event payload must not be parsed into UI structure
- during an active run, event batches may schedule a worktree refetch at most once per 500 ms
- `run.ended`, `engine.failed`, permission resolution, and tool terminal events may trigger immediate refetch
- 100 raw event batches must not produce 100 worktree IPC calls
- keep optimistic local turn in a separate local overlay
- `get_conversation.messages` must not be merged into the canvas timeline

Future phase:

- add a projected live update contract such as `ConversationWorktreePatch` or `TurnChanged { turnId, eventCursor }`
- keep the same `ConversationTurn` contract and stable node ids

**Step 4: Verify**

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline-store.test.ts src/features/conversation/timeline/conversation-timeline-source.test.ts
```

Expected: pass.

**Checkpoint:**

- Run the focused timeline store and source tests.
- Inspect the diff for timeline data source, store, and tests.
- Do not commit unless the user explicitly asks. If committing later, exclude unrelated dirty files.

## Task 8: Build Turn Work Tree Components

**Files:**

- Create: `apps/desktop/src/features/conversation/timeline/conversation-turn-view.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/assistant-work-view.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/thinking-panel.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/assistant-text-segment-view.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/tool-group-segment-view.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/tool-attempt-row.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/permission-inline-panel.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/artifact-segment-view.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/review-request-segment-view.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/clarification-request-segment-view.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-block-row.tsx`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`

**Step 1: Write failing component tests**

Add tests covering the screenshot failure:

- one user prompt with text/tool/text/final answer renders one `Jyowo` assistant turn
- no empty `Jyowo Complete` row exists
- tool group is inside assistant turn
- thinking is default collapsed
- failed tool displays safe summary
- permission status and tool result status are visually distinct
- review request segment renders inside assistant work
- clarification request segment renders inside assistant work
- Details callback receives `eventRef`

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.test.tsx
```

Expected: fail.

**Step 2: Implement components**

Render structure:

```tsx
<ConversationTurnView>
  <UserMessageView />
  <AssistantWorkView>
    <ThinkingPanel />
    <AssistantTextSegmentView />
    <ToolGroupSegmentView />
    <ArtifactSegmentView />
    <ReviewRequestSegmentView />
    <ClarificationRequestSegmentView />
  </AssistantWorkView>
</ConversationTurnView>
```

UI rules:

- Do not show machine statuses such as `Complete` as primary text.
- Use localized labels.
- Keep thinking low contrast and collapsed by default.
- Use a narrow, local status area for tool rows. Do not push status to the far right scroll edge.
- For `approved + failed`, show execution status first and permission as metadata.
- Failed tools should expand enough to show safe failure summary.

**Step 3: Remove old renderer paths**

Remove or stop importing:

- `assistant-message-block-view.tsx`
- `assistant-streaming-block-view.tsx`
- `thinking-block-view.tsx`
- `tool-group-block-view.tsx`
- `permission-request-block-view.tsx`
- `conversation-block-renderer.tsx`

Only delete files once imports and tests are updated.

**Step 4: Verify**

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.test.tsx
```

Expected: pass.

**Checkpoint:**

- Run the focused conversation timeline component tests.
- Inspect the diff for conversation timeline components and tests.
- Do not commit unless the user explicitly asks. If committing later, exclude unrelated dirty files.

## Task 9: Localize Conversation Work Tree UI

**Files:**

- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`

**Step 1: Write failing i18n assertions**

Test that Chinese UI does not show:

- `Tools`
- `Approved`
- `Complete`
- `completed`
- `failed`

when locale is `zh-CN`.

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.test.tsx
```

Expected: fail.

**Step 2: Add translation keys**

Required keys:

```text
assistant.status.running
assistant.status.complete
assistant.status.failed
thinking.collapsedRunning
thinking.collapsedComplete
thinking.withheld
tools.title
tools.attempt
tools.permission.approved
tools.permission.denied
tools.permission.pending
tools.result.completed
tools.result.failed
tools.result.denied
tools.failureFallback
details.viewRawEvents
```

**Step 3: Verify**

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.test.tsx
```

Expected: pass.

**Checkpoint:**

- Run the focused conversation timeline localization tests.
- Inspect the diff for locale files and timeline tests.
- Do not commit unless the user explicitly asks. If committing later, exclude unrelated dirty files.

## Task 10: Update Storybook State Matrix

**Files:**

- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx`
- Modify: `docs/frontend/frontend-quality.md`

**Step 1: Add stories**

Required stories:

- Empty conversation
- Simple completed turn
- Running turn with thinking
- Tool permission pending
- Tool approved and completed
- Tool approved and failed
- Multiple attempts for one tool
- Tool-call-only assistant message does not show empty text
- Withheld thinking
- Review request inside assistant work
- Clarification request inside assistant work
- Final answer after failed tool

**Step 2: Build Storybook**

Run:

```bash
pnpm -C apps/desktop build-storybook
```

Expected: pass.

**Step 3: Update quality docs**

Document that `ConversationTimeline` stories are worktree-based.

**Checkpoint:**

- Build Storybook.
- Inspect the diff for timeline stories and frontend quality docs.
- Do not commit unless the user explicitly asks. If committing later, exclude unrelated dirty files.

## Task 11: Remove Obsolete Event-Block Code

**Files:**

- Delete when unused:
  - `apps/desktop/src/features/conversation/timeline/conversation-timeline-reducer.ts`
  - `apps/desktop/src/features/conversation/timeline/conversation-timeline-index.ts`
  - `apps/desktop/src/features/conversation/timeline/conversation-timeline-thinking.ts`
  - `apps/desktop/src/features/conversation/timeline/conversation-block-renderer.tsx`
  - old block view files that no longer render
- Modify tests that reference old block types.

**Step 1: Run dependency checks before deleting**

Run:

```bash
rg -n "conversation-timeline-reducer|conversation-timeline-index|conversation-timeline-thinking|conversation-block-renderer" apps/desktop/src
rg -n "AssistantMessageBlock|ToolGroupBlock|ThinkingBlock|assistantStreaming|toolGroup" apps/desktop/src/features/conversation
```

Expected: only tests or files scheduled for deletion.

**Step 2: Delete obsolete files**

Use normal file deletion. Do not leave wrapper compatibility shims.

**Step 3: Run Knip and tests**

Run:

```bash
pnpm -C apps/desktop knip
pnpm -C apps/desktop test src/features/conversation/timeline
```

Expected: pass.

**Checkpoint:**

- Run Knip and focused timeline tests.
- Inspect the diff for removed obsolete timeline files and updated tests.
- Do not commit unless the user explicitly asks. If committing later, exclude unrelated dirty files.

## Task 12: Update Product And Engineering Docs

**Files:**

- Modify: `docs/frontend/frontend-product-ux.md`
- Modify: `docs/frontend/frontend-engineering.md`
- Modify: `docs/frontend/frontend-quality.md`
- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`

**Step 1: Update frontend docs**

Document:

- conversation canvas uses `ConversationTurn[]`
- `AssistantWork` owns thinking, tools, permissions, artifacts, and final answer
- review and clarification requests render as assistant work segments
- raw RunEvents belong to Activity/Details/Raw JSON
- React does not assemble product timeline from raw events
- `get_conversation.messages` does not drive `ConversationCanvas`
- thinking display is status-derived, explicitly safe, or withheld

**Step 2: Update backend docs**

Document:

- `ConversationWorktreePage` as UI-facing projection
- materialized worktree projection tables and turn-based paging semantics
- projector ownership in journal or SDK facade
- redaction and withheld semantics for thinking/tool failures
- command surface for worktree paging
- test requirements for projector invariants

**Step 3: Verify docs**

Run:

```bash
pnpm check:docs
```

Expected: pass.

**Checkpoint:**

- Run docs gate.
- Inspect the diff for frontend and backend documentation only.
- Do not commit unless the user explicitly asks. If committing later, exclude unrelated dirty files.

## Task 13: End-To-End Regression Tests

**Files:**

- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Test fixture updates as needed.

**Step 1: Add screenshot scenario as a test fixture**

Model this event flow:

```text
user.message.appended
assistant.delta "当然可以..."
assistant.completed "当然可以..."
tool.requested MiniMaxTextToImage
permission.requested for toolUseId A
permission.resolved approve
tool.completed A
tool.requested MiniMaxModelsList
tool.completed B
tool.requested MiniMaxTextToImage
tool.failed C
assistant.delta "让我用正确的参数..."
assistant.completed "让我用正确的参数..."
tool.requested MiniMaxModelRetrieve
tool.failed D
assistant.completed "非常抱歉..."
run.ended
```

Expected UI:

- one user turn
- one assistant work tree
- all visible nodes have stable ids
- no empty assistant row
- tools nested inside assistant work tree
- final answer after tools
- safe tool failure summary visible

**Step 2: Run focused regression tests**

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/ConversationWorkspace.test.tsx src/features/conversation/timeline/conversation-timeline.test.tsx
cargo test -p jyowo-desktop-shell --test commands page_conversation_worktree
```

Expected: pass.

**Checkpoint:**

- Run the focused frontend and Tauri regression tests.
- Inspect the diff for regression tests and fixtures.
- Do not commit unless the user explicitly asks. If committing later, exclude unrelated dirty files.

## Task 14: Full Gates

Run gates in this order:

```bash
pnpm check:docs
pnpm check:desktop
pnpm check:rust
pnpm check
```

For UI workflow changes also run:

```bash
pnpm -C apps/desktop build-storybook
pnpm -C apps/desktop test:e2e
pnpm -C apps/desktop test src/features/conversation/conversation-production-boundaries.test.ts
```

Expected:

- all commands exit 0
- no TypeScript errors
- no Rust compile or test failures
- no docs gate failures
- no Knip unused exports
- Storybook build succeeds
- Playwright smoke flow succeeds
- production boundary test succeeds

Run final drift checks:

```bash
rg -n "permissionRequest|PermissionRequestBlock|Tool error withheld from conversation timeline" apps/desktop/src/features/conversation apps/desktop/src/shared
rg -n "RunEvent" apps/desktop/src/features/conversation/timeline
rg -n "AssistantMessageBlock|ToolGroupBlock|ThinkingBlock|assistantStreaming|toolGroup" apps/desktop/src/features/conversation/timeline
rg -n "get_conversation.messages|messages:" apps/desktop/src/features/conversation apps/desktop/src/shared/tauri
```

Expected:

- first command returns no matches
- second command returns no render/product model matches
- third command returns no obsolete top-level block matches
- fourth command returns no canvas data-source matches

## Done Criteria

The refactor is complete only when all items are true:

```text
[ ] `page_conversation_worktree` exists and is documented
[ ] Rust projection contract is schema-exported and tested
[ ] `ConversationWorktreePage` has separate `pageCursor` and `eventCursor`
[ ] worktree paging is turn-based and never splits a turn
[ ] materialized worktree projection tables exist, or projection replays from session start before turn slicing
[ ] every visible node has a stable id
[ ] assistant segment order and tool attempt order are explicit and tested
[ ] Rust projector handles text, tools, permissions, thinking, artifacts, review requests, clarification requests, errors
[ ] tool-call-only assistant messages do not render empty text
[ ] multiple assistant completed events in one run stay under one assistant work
[ ] frontend conversation canvas renders `ConversationTurn[]`
[ ] raw RunEvent reducer is not used for product conversation rendering
[ ] `get_conversation.messages` does not drive `ConversationCanvas`
[ ] thinking is status-derived, explicitly safe, or withheld and defaults collapsed
[ ] raw thought text never appears in `ThinkingSegment.summary`
[ ] permissions are nested under tool attempts
[ ] tool failure displays safe summary, not raw withheld placeholder
[ ] live raw event invalidation is throttled and terminal events refetch immediately
[ ] production boundary test prevents old top-level block models from returning
[ ] Storybook covers required worktree states
[ ] docs explain product layer vs execution layer
[ ] `pnpm check` passes
[ ] Storybook build passes
[ ] Playwright smoke passes
```

## Post-Audit Completion Plan 2026-06-26

This section completes the unfinished work found by
`docs/plans/2026-06-25-conversation-turn-worktree-timeline-refactor-audit.md`.

The accepted current state is:

- The main canvas reads `ConversationTurn[]` from `page_conversation_worktree`.
- Timeline product rendering no longer imports `RunEvent`.
- `get_conversation.messages` is not the canvas data source.
- Tool permissions are nested under tool attempts.
- Safe tool failure summaries and withheld thinking are covered by focused tests.

The remaining work is not a rewrite. Keep the current worktree projection
architecture and close only the audited gaps.

Paging decision for this completion batch:

- Do not add materialized worktree tables in this batch.
- Treat complete timeline replay followed by turn slicing as the accepted
  fallback.
- Make that fallback explicit in docs and tests.
- Add materialized tables later only if a separate performance requirement
  proves the fallback is not acceptable.

Implementation rules:

- Write the failing test before each code change.
- Keep Rust as the projection and safety authority.
- Do not reintroduce the flat event-block reducer.
- Do not use broad `rg` drift checks as binary gates when they match raw
  schemas, tests, or valid segment kinds.
- Preserve existing passing projection boundaries.

### Task 15: Add Missing Segment Source Event Contracts And Type Mapping

**Why:** `ArtifactSegment`, `ReviewRequestSegment`,
`ClarificationRequestSegment`, and `NoticeSegment` exist in the worktree
contract, but review, clarification, and notice do not have durable Rust event
contracts that the read model can turn into timeline events.

Do not use dotted names as durable Rust event tags. Durable event tags come from
`Event` enum variants and `#[serde(rename_all = "snake_case")]`; the read model
maps those durable tags into dotted UI timeline event types.

Required mapping:

| Durable `Event` variant | Durable serde tag | Timeline `event_type` | Segment kind |
|---|---|---|---|
| `ArtifactCreated` | `artifact_created` | `artifact.created` | `artifact` |
| `ArtifactUpdated` | `artifact_updated` | `artifact.updated` | `artifact` |
| `AssistantReviewRequested` | `assistant_review_requested` | `assistant.review.requested` | `reviewRequest` |
| `AssistantClarificationRequested` | `assistant_clarification_requested` | `assistant.clarification.requested` | `clarificationRequest` |
| `AssistantNotice` | `assistant_notice` | `assistant.notice` | `notice` |

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/events/messages.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/mod.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `crates/jyowo-harness-contracts/tests/m1_contracts.rs`
- Modify: `apps/desktop/src/shared/events/run-event-schema.ts`
- Modify: `apps/desktop/src/shared/events/run-event-schema.test.ts`

**Steps:**

1. Add failing Rust contract tests for these durable event variants:
   - `Event::AssistantReviewRequested`
   - `Event::AssistantClarificationRequested`
   - `Event::AssistantNotice`
2. Assert the durable serde tags are snake_case:
   - `assistant_review_requested`
   - `assistant_clarification_requested`
   - `assistant_notice`
3. Follow the existing message-event contract shape in
   `crates/jyowo-harness-contracts/src/events/messages.rs`: include `run_id`,
   stable request or notice id, display text fields, and `at`. Do not add
   `session_id` to these message-scoped events unless the whole message event
   family is being migrated, because the current user and assistant message
   events get session scope from `EventEnvelope`.
4. Run:

   ```bash
   cargo test -p jyowo-harness-contracts assistant_review_requested --test m1_contracts
   ```

   Expected: fail because the event contracts do not exist.

5. Add minimal Rust event structs and `Event` enum variants.
6. Export their JSON schemas.
7. Add frontend raw event schema support for the dotted timeline event names:
   - `assistant.review.requested`
   - `assistant.clarification.requested`
   - `assistant.notice`
8. Add read-model mapping tests proving snake_case durable events become dotted
   timeline event types before projection.
9. Run:

   ```bash
   cargo test -p jyowo-harness-contracts assistant_review_requested --test m1_contracts
   cargo test -p jyowo-harness-journal --features sqlite --test conversation_read_model assistant_review_requested
   pnpm -C apps/desktop test src/shared/events/run-event-schema.test.ts
   ```

   Expected: all commands pass.

**Acceptance:**

- New durable event contracts are schema-exported with snake_case serde tags.
- Read-model tests prove durable event tags map to dotted timeline event types.
- Frontend raw event parsing accepts only the dotted timeline event names.
- The events contain only display-safe fields needed for projection.

### Task 16: Verify Or Add Real Segment Event Producers

**Why:** Contract and projector tests alone can pass with synthetic events while
the real runtime never emits those events. Each new segment type must have a
reachable producer, or the task must remain incomplete.

**Files:**

- Inspect: `crates/jyowo-harness-sdk/src/harness.rs`
- Inspect: `crates/jyowo-harness-tool/src/builtin/clarify.rs`
- Inspect: `crates/jyowo-harness-tool/src/orchestrator.rs`
- Inspect: `apps/desktop/src/features/conversation/ReviewRequest.tsx`
- Modify as needed: the smallest owner module that actually creates the user
  interaction or artifact event
- Test: the nearest existing unit or integration test for that producer

**Steps:**

1. Run producer discovery:

   ```bash
   rg -n "ArtifactCreated|ArtifactUpdated|ClarifyChannelCap|ReviewRequest|review|clarification|notice|AssistantReview|AssistantClarification|AssistantNotice" crates apps/desktop/src
   ```

2. Build a producer map before writing implementation code:

   ```text
   artifact.created / artifact.updated -> [producer file or "missing"]
   assistant_review_requested -> [producer file or "missing"]
   assistant_clarification_requested -> [producer file or "missing"]
   assistant_notice -> [producer file or "missing"]
   ```

3. For every producer marked `missing`, choose one of these outcomes before
   continuing:
   - add a narrowly scoped producer in the layer that owns that user-visible
     interaction
   - mark that segment type as contract/projector-only for this batch and keep
     its end-to-end done criterion unchecked
4. Add failing tests at the producer boundary for every producer that exists or
   is added. The tests must assert the durable `Event` variant is appended, not
   only that a frontend component can render a fixture.
5. Run the relevant producer tests. Use the nearest package test command; if the
   producer is in the tool layer, include:

   ```bash
   cargo test -p jyowo-harness-tool clarify
   ```

6. Only after producer reachability is documented or implemented, continue to
   projector work.

**Acceptance:**

- Every segment type has a documented producer decision.
- End-to-end completion is claimed only for segment types with reachable
  producers.
- No synthetic projector fixture is used as proof that the real runtime emits a
  segment.

**Post-audit producer decision 2026-06-26:**

```text
artifact.created / artifact.updated -> contract/projector-only in this batch.
assistant.review.requested -> contract/projector-only in this batch.
assistant.clarification.requested -> crates/jyowo-harness-tool/src/builtin/clarify.rs.
assistant.notice -> contract/projector-only in this batch.
```

Only `assistant.clarification.requested` has a reachable non-test producer in
this batch. The artifact, review, and notice segment types remain supported by
contracts, read-model projection, worktree projection, Tauri payload mapping,
frontend schema, and render fixtures, but their end-to-end producer criterion is
not checked.

### Task 17: Project Artifact, Review, Clarification, and Notice Segments

**Why:** The Rust projector currently handles text, thinking, tools,
permissions, run endings, and engine failures. It does not produce all segment
kinds that the UI and contract already support.

**Files:**

- Modify if structured artifact status is required:
  `crates/jyowo-harness-contracts/src/conversation.rs`
- Modify if structured artifact status is required:
  `apps/desktop/src/shared/tauri/commands.ts`
- Test if structured artifact status is required:
  `crates/jyowo-harness-contracts/tests/m1_contracts.rs`
- Test if structured artifact status is required:
  `apps/desktop/src/shared/tauri/commands.test.ts`
- Modify: `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Modify: `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify: `crates/jyowo-harness-journal/tests/conversation_read_model.rs`
- Modify: `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`

**Steps:**

1. Decide artifact status representation before writing projector code:
   - default: keep the current `ArtifactSegment` contract and use status only
     to derive safe summary text when needed
   - if product UI needs structured artifact status, first add a `status` field
     to `ArtifactSegment`, update schema export and frontend Zod, and add
     contract parity tests
2. Add failing projector tests for:
   - `artifact.created` creates or updates one `ArtifactSegment`.
   - `artifact.updated` updates title and summary without duplicating the
     segment.
   - `artifact.updated` updates structured status only if step 1 explicitly
     extends the contract.
   - `assistant.review.requested` creates one `ReviewRequestSegment`.
   - `assistant.clarification.requested` creates one
     `ClarificationRequestSegment`.
   - `assistant.notice` creates one `NoticeSegment`.
   - segment ids, order, and `eventRefs` are stable after renumbering.
3. Run:

   ```bash
   cargo test -p jyowo-harness-journal --test conversation_worktree_projector artifact
   cargo test -p jyowo-harness-journal --test conversation_worktree_projector review
   ```

   Expected: fail because the segments are not projected.

4. Add safe artifact fields to the read model payload. Allow `artifactId`,
   `title`, and a redacted preview/summary. Allow `status` only as a lifecycle
   enum or derived summary; do not expose it as `ArtifactSegment.status` unless
   the contract was extended in step 1. Do not include `blobRef`,
   `contentHash`, private paths, or raw file contents.
5. Map artifact lifecycle events into `ArtifactSegment`.
6. Map review, clarification, and notice events into their segment kinds.
7. Ensure these events attach to the current assistant work for the run. If no
   user turn exists yet, use the existing orphan/run fallback policy instead of
   inventing a frontend repair path.
8. Run:

   ```bash
   cargo test -p jyowo-harness-journal --test conversation_worktree_projector
   cargo test -p jyowo-harness-journal --features sqlite --test conversation_read_model
   ```

   Expected: pass.

**Acceptance:**

- Real journal events can produce every public `AssistantSegment` kind.
- Artifact projection never exposes blob refs, hashes, raw content, or private
  paths.
- Artifact status handling is explicit: either summary-derived under the
  existing contract, or structured with contract and Zod parity tests.
- Repeated artifact updates do not create duplicate artifact segments.

### Task 18: Lock Down Worktree Paging Fallback Semantics

**Why:** The current SQLite implementation does not have materialized worktree
tables. That is acceptable only if complete replay before turn slicing is
tested and documented.

**Files:**

- Modify: `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Modify: `crates/jyowo-harness-journal/tests/conversation_read_model.rs`
- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/backend/backend-engineering.md`

**Steps:**

1. Add a failing SQLite test with a session containing more turns than the page
   limit and events that affect an older turn after later turns exist.
2. Assert `page_worktree` reads from session start before slicing, so older
   turn state is complete even when the page starts near the end.
3. Assert `pageCursor` moves by turn position, not event sequence.
4. Lock the single-cursor semantics by direction:
   - for `direction = After`, returned `pageCursor` is the last turn in the
     selected page and the next `After` request must not repeat it
   - for `direction = Before`, returned `pageCursor` is the first turn in the
     selected page and the next `Before` request must not repeat it
   - returned turns stay in ascending conversation order for both directions
5. Assert `eventCursor` points at the latest consumed journal event from the
   complete replay, not just the last event referenced by the selected turn
   page.
6. Assert `gap` stays `false` for the complete replay fallback.
7. Run:

   ```bash
   cargo test -p jyowo-harness-journal --features sqlite --test conversation_read_model worktree
   ```

   Expected: fail until the semantics are covered.

8. Add the minimal implementation or test fixture corrections needed.
9. Document the fallback and cursor semantics in backend runtime and
   engineering docs.
10. Run the same command again.

   Expected: pass.

**Acceptance:**

- The repository documents that current paging is complete replay plus turn
  slicing.
- Tests prove turn pages are not built from partial raw timeline windows.
- Tests prove repeated `After` and repeated `Before` requests do not overlap at
  the cursor boundary.
- No plan or doc claims materialized worktree tables exist.

### Task 19: Add Tauri Boundary Regression Tests

**Why:** The command exists, but audit found missing malformed id and wire shape
parity coverage.

**Files:**

- Create: `crates/jyowo-harness-contracts/tests/fixtures/conversation_worktree_page.json`
- Modify: `crates/jyowo-harness-contracts/tests/m1_contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`

**Steps:**

1. Add a failing Tauri test for malformed `conversationId`.
2. Assert the command fails closed and does not call the SDK facade.
3. Add a shared wire-shape parity fixture at
   `crates/jyowo-harness-contracts/tests/fixtures/conversation_worktree_page.json`.
   The fixture must include all segment kinds, optional cursors, and
   `hasMoreBefore` / `hasMoreAfter`.
4. Add a Rust contract test that deserializes the shared fixture as
   `ConversationWorktreePage`, serializes it back to JSON, and asserts the
   stable camelCase wire keys.
5. Add a frontend command-client test that loads the same fixture from disk and
   validates it through `pageConversationWorktree` / the Zod response schema.
   Do not create a separate hand-written TypeScript-only fixture for parity.
6. Run:

   ```bash
   cargo test -p jyowo-harness-contracts conversation_worktree_fixture --test m1_contracts
   cargo test -p jyowo-desktop-shell --test commands page_conversation_worktree
   pnpm -C apps/desktop test src/shared/tauri/commands.test.ts
   ```

   Expected: fail until the tests and boundary are complete.

7. Add the minimal command/helper changes needed.
8. Run the same commands again.

   Expected: pass.

**Acceptance:**

- Empty and malformed conversation ids fail closed.
- Rust serde and frontend Zod validate the same fixture.
- The frontend schema still rejects raw event timeline shapes.

### Task 20: Remove Frontend Block Compatibility APIs

**Why:** The UI is already turn-based, but old names keep block semantics alive
and weaken drift checks.

**Files:**

- Rename: `apps/desktop/src/features/conversation/timeline/conversation-block-row.tsx`
  to `apps/desktop/src/features/conversation/timeline/conversation-turn-row.tsx`
- Rename: `apps/desktop/src/features/conversation/timeline/conversation-blocks.ts`
  to `apps/desktop/src/features/conversation/timeline/pending-tool-permission.ts`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/use-conversation-timeline.ts`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
- Modify: `apps/desktop/src/features/conversation/conversation-production-boundaries.test.ts`
- Modify tests and imports under
  `apps/desktop/src/features/conversation/timeline/`

**Steps:**

1. Add failing production boundary assertions for these exact names in
   production timeline code:
   - `ConversationBlockRow`
   - `blocks?: ConversationTurn[]`
   - `pendingPermissionBlocks`
2. Run:

   ```bash
   pnpm -C apps/desktop test src/features/conversation/conversation-production-boundaries.test.ts
   ```

   Expected: fail on current code.

3. Rename `ConversationBlockRow` to `ConversationTurnRow`.
4. Remove the `blocks` prop alias from `ConversationTimeline`.
5. Make `useConversationTimeline` return `turns` and
   `pendingToolPermissions`, not `blocks` or `pendingPermissionBlocks`.
6. Pass `turns={timeline.turns}` from `ConversationWorkspace`.
7. Rename the pending permission helper file and exported type if needed.
8. Keep test fixtures and raw event schema files out of this production naming
   guard.
9. Run:

   ```bash
   pnpm -C apps/desktop test src/features/conversation/conversation-production-boundaries.test.ts
   pnpm -C apps/desktop test src/features/conversation/ConversationWorkspace.test.tsx src/features/conversation/timeline/conversation-timeline.test.tsx
   ```

   Expected: pass.

**Acceptance:**

- Product conversation timeline APIs say `turn`, not `block`.
- No compatibility prop lets callers pass `blocks` to the timeline.
- Tests still prove the UI renders nested turns, assistant work, segments, and
  tool attempts.

### Task 21: Add Live Invalidation Pressure Test

**Why:** Raw event batches should request worktree refreshes, not rebuild UI
state. The audit found coalescing/throttle code but no IPC pressure test.

**Files:**

- Modify: `apps/desktop/src/features/conversation/timeline/use-conversation-timeline.test.ts`
- Modify: `apps/desktop/src/features/conversation/timeline/use-conversation-event-stream.test.ts`

**Steps:**

1. Add a failing test that emits 100 non-terminal raw event batches in one
   throttle window.
2. Assert `pageConversationWorktree` is not called 100 times.
3. Assert a terminal event still triggers an immediate refresh.
4. Use fake timers so the test is deterministic.
5. Run:

   ```bash
   pnpm -C apps/desktop test src/features/conversation/timeline/use-conversation-timeline.test.ts src/features/conversation/timeline/use-conversation-event-stream.test.ts
   ```

   Expected: fail until the IPC call count is covered.

6. Adjust the existing coalescing or throttle code only if the test exposes a
   real behavior gap.
7. Run the same command again.

   Expected: pass.

**Acceptance:**

- 100 streaming batches in one throttle window do not produce 100 worktree IPC
  calls.
- Terminal events still refetch immediately.
- Raw event payloads are never turned into render segments in frontend code.

### Task 22: Close Localization Coverage

**Why:** Localization exists, but the audit found incomplete negative coverage
for English leakage.

**Files:**

- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`

**Steps:**

1. Decide whether to keep the current `timeline.*` keys or rename them to the
   original plan keys. Prefer keeping current keys unless a rename removes real
   duplication.
2. Add failing Chinese locale tests that assert these English strings do not
   appear in rendered timeline UI:
   - `Tools`
   - `Approved`
   - `Complete`
   - `failed`
   - `View raw events`
3. Run:

   ```bash
   pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.test.tsx src/features/conversation/ConversationWorkspace.test.tsx
   ```

   Expected: fail if English leaks remain.

4. Add or correct only the missing locale entries and render call sites.
5. Run the same command again.

   Expected: pass.

**Acceptance:**

- Chinese conversation timeline states do not leak English fallback labels.
- Locale key naming is consistent and documented by tests.

### Task 23: Complete Storybook State Matrix

**Why:** Existing stories use worktree fixtures, but the audited matrix still
has missing states and no build evidence.

**Files:**

- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx`

**Steps:**

1. Add independent stories for:
   - `SimpleCompletedTurn`
   - `ToolApprovedCompleted`
   - `MultipleToolAttempts`
   - `ToolCallOnlyNoEmptyText`
   - `WithheldThinking`
   - `FinalAnswerAfterFailedTool`
2. Keep each story fixture small. Do not reuse a giant base turn when it hides
   the state under test.
3. Run:

   ```bash
   pnpm -C apps/desktop build-storybook
   ```

   Expected: pass.

4. If the build fails from unrelated existing Storybook issues, document the
   exact failure and add a targeted fix only if it is inside this feature area.

**Acceptance:**

- Storybook can inspect each required worktree state independently.
- Storybook build exits 0.

### Task 24: Repair Documentation and Scoped Drift Guards

**Why:** `docs/frontend/frontend-engineering.md` still describes the old
`ConversationBlock[]` reducer model. Existing drift commands also match legal
raw schemas, tests, and the valid `toolGroup` segment kind.

**Files:**

- Modify: `docs/frontend/frontend-engineering.md`
- Modify: `docs/frontend/frontend-quality.md`
- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `apps/desktop/src/features/conversation/conversation-production-boundaries.test.ts`

**Steps:**

1. Replace `ConversationBlock[]` canvas language with
   `ConversationTurn[]`.
2. State that `page_conversation_worktree` is the product conversation canvas
   source.
3. State that raw `RunEvent` and `page_conversation_timeline` are execution
   surfaces, not product render models.
4. Remove claims that live events, command results, artifacts, or local submits
   feed a frontend timeline reducer.
5. Document that the current backend paging implementation uses complete
   replay plus turn slicing, not materialized worktree tables.
6. Replace broad drift commands with scoped checks. Good scopes are production
   render/model files under:
   - `apps/desktop/src/features/conversation/timeline/`
   - `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
7. Do not fail on:
   - raw event schemas
   - tests
   - valid `AssistantSegment` kind `toolGroup`
   - legacy `get_conversation` schema used outside the canvas data source
8. Run:

   ```bash
   pnpm check:docs
   pnpm -C apps/desktop test src/features/conversation/conversation-production-boundaries.test.ts
   ```

   Expected: pass.

**Acceptance:**

- Frontend docs no longer contradict the implemented data source.
- Drift guards catch old product render models without flagging legal raw event
  contracts or tests.

### Task 25: Add Full MiniMax-Style Regression Fixture

**Why:** Focused tests cover many pieces, but audit found no full fixture for
the screenshot-style failure that motivated the refactor.

**Files:**

- Modify: `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`

**Fixture flow:**

1. User asks for an image or video generation task.
2. Assistant starts and produces safe thinking status.
3. Assistant requests a tool.
4. Permission is requested and approved.
5. Tool attempt completes or fails with a safe summary.
6. Assistant retries or continues with final text.
7. Artifact is created or updated.
8. Run ends.

**Steps:**

1. Add the fixture at the Rust projector layer first.
2. Assert it produces one user turn with one assistant work.
3. Assert tool permission is nested inside the tool attempt.
4. Assert failed tool output does not expose raw withheld text.
5. Assert artifact appears as an artifact segment.
6. Assert final assistant text remains in the same assistant work.
7. Reuse the same logical fixture through the Tauri command test.
8. Reuse the same logical fixture through frontend render tests.
9. Run:

   ```bash
   cargo test -p jyowo-harness-journal --test conversation_worktree_projector minimax
   cargo test -p jyowo-desktop-shell --test commands page_conversation_worktree
   pnpm -C apps/desktop test src/features/conversation/ConversationWorkspace.test.tsx src/features/conversation/timeline/conversation-timeline.test.tsx
   ```

   Expected: pass.

**Acceptance:**

- The original failure shape is covered end to end.
- The fixture proves the UI shows a coherent turn tree, not disconnected event
  blocks.

### Task 26: Review, Security Review, and Full Gates

**Why:** This completion batch touches contracts, IPC, redaction-sensitive
projection, frontend render state, docs, and tests.

**Steps:**

1. Run focused Rust gates:

   ```bash
   cargo fmt --all --check
   cargo test -p jyowo-harness-contracts assistant_review_requested --test m1_contracts
   cargo test -p jyowo-harness-contracts conversation_worktree_fixture --test m1_contracts
   cargo test -p jyowo-harness-contracts conversation_worktree --test m1_contracts
   cargo test -p jyowo-harness-contracts schema_export --test m1_contracts
   cargo test -p jyowo-harness-journal --test conversation_worktree_projector
   cargo test -p jyowo-harness-journal --features sqlite --test conversation_read_model worktree
   cargo test -p jyowo-harness-sdk --features testing conversation_read_model_facade_returns_worktree_page --test conversation_read_model
   cargo test -p jyowo-desktop-shell --test commands page_conversation_worktree
   ```

   Also run the concrete producer test command selected in Task 16. If a
   segment type is intentionally contract/projector-only in this batch, record
   that gap and keep the related end-to-end done criterion unchecked.

2. Run focused frontend gates:

   ```bash
   pnpm -C apps/desktop test src/shared/events/run-event-schema.test.ts
   pnpm -C apps/desktop test src/shared/tauri/commands.test.ts
   pnpm -C apps/desktop test src/features/conversation/conversation-production-boundaries.test.ts
   pnpm -C apps/desktop test src/features/conversation/ConversationWorkspace.test.tsx src/features/conversation/timeline/conversation-timeline.test.tsx
   pnpm -C apps/desktop test src/features/conversation/timeline/use-conversation-timeline.test.ts src/features/conversation/timeline/use-conversation-event-stream.test.ts
   pnpm -C apps/desktop build-storybook
   ```

3. Run repository gates:

   ```bash
   pnpm check:docs
   pnpm check:desktop
   pnpm check:rust
   pnpm check
   ```

4. Run code review after implementation:

   ```text
   /code-review-expert
   ```

5. Run security review before commit because the batch changes IPC, projection,
   redaction boundaries, and event contracts:

   ```text
   /security-review
   ```

**Acceptance:**

- All focused gates pass.
- All repository gates pass.
- Review findings are fixed or explicitly documented.
- No new secret, private path, raw thought text, blob ref, or content hash can
  enter the product conversation canvas.

### Post-Audit Done Criteria

The audit is closed only when all items are true:

```text
[x] Rust event contracts exist for review request, clarification request, and notice
[x] Durable snake_case event tags are mapped to dotted timeline event types in the read model
[x] Producer reachability is documented for artifact, review, clarification, and notice segments
[x] Rust projector emits artifact, reviewRequest, clarificationRequest, and notice segments
[x] Artifact projection includes only display-safe metadata
[x] Artifact status handling is explicitly contract-backed or summary-derived
[x] Worktree paging fallback is documented as complete replay plus turn slicing
[x] Worktree paging tests prove turn pages are not built from partial raw event pages
[x] Repeated `After` and repeated `Before` worktree page requests do not overlap at cursor boundaries
[x] Tauri malformed conversation id test fails closed
[x] Rust serde and frontend Zod validate the same shared worktree page fixture
[x] Frontend timeline production APIs no longer expose `blocks`
[x] `ConversationBlockRow` is renamed or removed from production timeline code
[x] `pendingPermissionBlocks` is renamed or removed from production timeline code
[x] 100 streaming raw-event batches do not create 100 worktree IPC calls
[x] Terminal events still trigger immediate worktree refetch
[x] Chinese timeline UI tests prevent English fallback leakage
[x] Storybook covers the audited missing matrix states
[x] `docs/frontend/frontend-engineering.md` no longer describes the old block reducer
[x] Drift guards are scoped and do not fail on legal raw schemas or tests
[x] MiniMax-style full fixture passes at projector, Tauri, and frontend layers
[x] `pnpm check:docs` passes
[x] `pnpm check:desktop` passes
[x] `pnpm check:rust` passes
[x] `pnpm check` passes
```

## Rollback Guidance

Do not rollback by re-enabling the flat event-block reducer.

If a task fails:

1. Keep the contract and projector tests.
2. Revert only the failing task's local edits.
3. Preserve already passing projection boundaries.
4. Fix the failing layer against the tests.

The old flat renderer is the source of the design problem. It must not be used as the fallback architecture.
