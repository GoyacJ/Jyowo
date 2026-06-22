# Conversation Timeline Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task.

**Goal:** Replace the current multi-source conversation rendering with a durable, event-driven conversation timeline that supports streaming assistant output, inline user interactions, artifact and diff review, recovery, and restart-safe replay.

**Architecture:** The conversation canvas renders only `ConversationBlock[]`. Backend snapshots and live events are inputs to a reducer, not direct UI sources. Rust remains the policy and persistence authority; React owns view projection, optimistic local state, interaction intent, and rendering.

**Tech Stack:** React 19, TypeScript 6, TanStack Query, TanStack Virtual, Zod, Testing Library, Storybook, Playwright, Tauri 2 events through `shared/tauri`, Rust 1.96, Tokio, serde, schemars, harness contracts, SDK, journal, and desktop Tauri commands.

---

## Required Reading

Read these before implementation:

- `AGENTS.md`
- `docs/frontend/agent-harness-frontend-development-guidelines.md`
- `docs/frontend/frontend-product-ux.md`
- `docs/frontend/frontend-engineering.md`
- `docs/frontend/frontend-quality.md`
- `docs/backend/agent-harness-backend-development-guidelines.md`
- `docs/backend/backend-runtime.md`
- `docs/backend/backend-engineering.md`
- `docs/backend/backend-quality.md`

Use these skills when executing:

- `superpowers:executing-plans`
- `superpowers:test-driven-development`
- `superpowers:systematic-debugging` for any failure or unexpected behavior
- `superpowers:verification-before-completion`

After each code-producing task, run code review. Security review is required before final delivery because this plan changes IPC, event visibility, permission UI behavior, and streaming runtime surfaces.

## Hard Architecture Rules

This plan chooses the long-term architecture. Do not implement a short-term compatibility layer as the final state.

Required final state:

- `ConversationBlock[]` is the only render source for the conversation canvas.
- `getConversation`, live events, artifacts, local optimistic submits, and command results feed a reducer.
- `ConversationWorkspace` does not independently merge `conversation.messages`, `localMessages`, `activity.events`, and `artifacts`.
- Assistant streaming text is rendered from event deltas, not by waiting for `getConversation`.
- Permission, clarification, review, tool progress, artifact, diff, and error states are first-class blocks.
- Activity rail remains secondary. It must not be the conversation body renderer.
- React never makes final policy decisions. It sends user intent and waits for Rust events or command responses.
- All external payloads crossing IPC or event boundaries are validated with Zod.
- No provider key, raw Secret, bearer token, unredacted absolute private path, or unredacted payload enters frontend state, screenshots, snapshots, or visible blocks. Frontend may store and render only backend-projected relative paths or redacted display paths that are explicitly marked safe for UI.
- Zustand remains UI-only.
- TanStack Query remains the owner of backend snapshots.
- Streaming buffers live in the conversation timeline reducer or the event adapter close to it.
- Use `@chenglou/pretext` only through `shared/text-layout` if virtual measurement needs it.

Removed final-state patterns:

- No `localMessages` array in `ConversationWorkspace`.
- No `toRuntimeState()` that combines unrelated sources inside the component.
- No artifact or review UI that blindly attaches to the latest assistant message.
- No raw `RunEvent` payload passed straight into conversation visual components.
- No one-off dashboard cards for timeline blocks.

Breaking refactors are allowed. Remove obsolete components, tests, and mocks after the replacement is complete.

## Product Design

Jyowo is a conversation-native local AI project workspace. The visible path is:

```text
Ask
Understand
Plan
Work
Review
Continue
```

The timeline should still feel like a calm chat surface. Internally it is a workflow timeline, not a simple message list.

The main canvas renders blocks:

```ts
type ConversationBlock =
  | UserMessageBlock
  | AssistantMessageBlock
  | AssistantStreamingBlock
  | ToolGroupBlock
  | PermissionRequestBlock
  | ClarificationRequestBlock
  | PlanTimelineBlock
  | ArtifactBlock
  | DiffReviewBlock
  | ReviewRequestBlock
  | ErrorBlock
  | CheckpointBlock
  | SystemNoticeBlock
```

Every block has stable identity:

```ts
type ConversationBlockBase = {
  id: string
  kind: ConversationBlockKind
  conversationId: string
  runId?: string
  turnId?: string
  conversationSequence: number
  runSequence?: number
  createdAt: string
  updatedAt?: string
  status?: string
}
```

Use `turnId` for a user-submitted unit of work. A turn can contain:

```text
UserMessageBlock
AssistantStreamingBlock
ToolGroupBlock
PermissionRequestBlock
ArtifactBlock
DiffReviewBlock
ReviewRequestBlock
AssistantMessageBlock
ErrorBlock
```

The UI must present this as one natural conversation flow, not an admin log.

## Frontend UX Rules

Conversation canvas:

- Primary visual surface.
- White document rhythm, warm neutral surfaces, quiet borders.
- No nested cards.
- No dashboard-style gray stacks.
- Message blocks use conversational spacing.
- Execution blocks use compact editorial rows, not log dumps.

Assistant streaming:

- Show one assistant bubble that grows as deltas arrive.
- Use a subtle streaming cursor or status affordance.
- Do not rerender Markdown and syntax highlight on every token.
- During streaming, render lightweight Markdown.
- After completion, render full Markdown and lazy code highlighting.
- Keep partial text visible if the run fails or is interrupted.

Tool progress:

- Group tool events into one `ToolGroupBlock` per turn or phase.
- Default collapsed when successful.
- Auto-expand failed, blocked, permission-related, or long-running tools.
- Show product language: "Reading files", "Running command", "Updating artifact".
- Raw JSON stays behind details.

Permission:

- Inline `PermissionRequestBlock` appears in the timeline.
- The block shows operation, reason, target, severity, decision scope, and allowed actions.
- Approve/Deny buttons enter `submitting`.
- Final state comes only from `permission.resolved` or command failure.
- The block remains visible after resolution.

Review:

- Artifact and diff review are independent blocks.
- Do not attach review controls to the latest assistant message.
- Review block supports continue, request changes, accept, and open artifact/diff where available.

Composer:

- Remains the main action entry.
- Supports ready, submitting, running-disabled, clarification-reply, review-comment, retry, and continue modes.
- Timeline blocks may have local inputs for clarification or review. These inputs still dispatch through the same command/client boundary.

Scrolling:

- Auto-follow only when the user is near the bottom.
- User scroll-up disables auto-follow.
- New block while scrolled up shows a "jump to latest" affordance.
- Permission and clarification blocks may subtly announce, but must not yank scroll.
- User submit always anchors to the submitted turn.
- Streaming deltas must not cause layout thrash.

Accessibility:

- Every interactive block has accessible names.
- Permission and review blocks support keyboard action.
- Streaming updates use restrained live-region behavior.
- Icon-only buttons use lucide icons with labels or tooltips.

## Data Flow

Final flow:

```text
getConversation snapshot
  -> hydrate timeline reducer

conversation event replay
  -> apply durable event history

live conversation event stream
  -> apply incremental events

local submit
  -> create clientMessageId and optimistic user block

command accepted
  -> bind optimistic turn to runId

command failed
  -> mark optimistic block failed and keep user draft recoverable

artifacts/query snapshots
  -> patch ArtifactBlock and DiffReviewBlock metadata
```

The reducer is the only projection layer that creates blocks.

## Event Semantics

The frontend event names currently include:

```text
run.started
run.ended
assistant.delta
assistant.completed
tool.requested
tool.approved
tool.denied
tool.completed
tool.failed
permission.requested
permission.resolved
artifact.created
artifact.updated
engine.failed
```

The long-term timeline also requires user-message confirmation. Add a public event projection for the existing backend `UserMessageAppended` event:

```text
user.message.appended
```

Do not fake user confirmation from `run.started`.

Every new user submit must have a frontend-generated `clientMessageId`.

Rules:

- `local.submit` creates `clientMessageId` before calling `start_run`.
- `start_run` accepts `client_message_id` / `clientMessageId` and passes it into the Rust conversation runtime.
- `user.message.appended` echoes the same `clientMessageId` for messages created through the new submit path.
- `clientMessageId` is required for optimistic confirmation. Do not confirm an optimistic block by body text alone.
- Historical or imported user messages without `clientMessageId` may render from snapshot or replay by `messageId`, but they must not be matched to a pending optimistic submit.
- If `user.message.appended` arrives before `commandAccepted`, the reducer must still confirm by `clientMessageId` and bind the later `runId` without duplicating the user block.

Required event handling:

| Event | Reducer behavior |
|---|---|
| `local.submit` | Create optimistic `UserMessageBlock` with `status: "sending"`. |
| `run.started` | Mark turn/run active. Create or update a compact run status if needed. |
| `user.message.appended` | Confirm or replace optimistic user block by `clientMessageId`; deduplicate persisted messages by `messageId`. Never deduplicate optimistic submits by body text. |
| `assistant.delta` | Create or append `AssistantStreamingBlock`. |
| `assistant.completed` | Finalize streaming block from redacted final `body`. If final body is missing, keep streamed content as partial and trigger snapshot reconciliation before marking the block final. |
| `tool.requested` | Create/update `ToolGroupBlock`; add queued tool item. |
| `tool.approved` | Mark tool item running. |
| `tool.denied` | Mark tool item blocked/denied. |
| `tool.completed` | Mark tool item completed and patch duration/output summary. |
| `tool.failed` | Mark tool item failed and expand group. |
| `permission.requested` | Create `PermissionRequestBlock` pending. |
| `permission.resolved` | Resolve the matching permission block. |
| `artifact.created` | Create or patch `ArtifactBlock`. |
| `artifact.updated` | Patch existing artifact block; create placeholder if missing. |
| `run.ended` | Mark run and turn completed; flush streaming block into final state. |
| `engine.failed` | Keep partial state, append `ErrorBlock`, mark turn failed. |

Visibility rules:

- `public`: render allowed payload fields.
- `redacted`: render safe summary only.
- `withheld`: render generic withheld notice; payload must be absent.

Ordering and cursor rules:

- Deduplicate by event id.
- Preserve backend conversation order as the canonical timeline order.
- Do not sort conversation blocks by `(runId, sequence)`. `sequence` is run-local validation data, not the global conversation ordering key.
- Each projected event must include an opaque `cursor` in batch metadata and a monotonic `conversationSequence` or equivalent order key from the backend journal/projection.
- The reducer applies events in backend-provided conversation order. If a live batch arrives out of cursor order, mark a gap and request replay instead of locally guessing an order.
- If a run-local sequence gap is detected, request replay from the last acknowledged conversation cursor.
- If replay cannot close the gap, refetch snapshot and render `SystemNoticeBlock`.
- `assistant.completed` can overwrite streaming content only when it provides redacted final `body` or snapshot reconciliation confirms final content.
- Snapshot reconciliation is an explicit source action. It must invalidate/refetch `getConversation`, compare by `messageId`, update the completed assistant block, and clear the pending reconciliation flag only after the final content is found or a safe error state is rendered.

## Backend Target

Add a durable event stream surface instead of relying on activity polling as the main conversation source.

Required backend behavior:

- Use existing `harness-contracts` event variants as source of truth.
- Add public projection payloads only where needed for the desktop UI.
- Preserve redaction before event persistence and event emission.
- Expose replay from an opaque conversation cursor.
- Expose live subscription for a selected conversation.
- Ensure subscription state is clearly documented as single-process.
- Ensure replay/snapshot state is restart-stable.
- Ensure live delivery is window-scoped and cleaned up when the window closes, the hook unmounts, or the selected conversation changes.
- Ensure replay is delivered before live batches for the same subscription.
- Ensure batching and backpressure are explicit. On overflow or unknown ordering, emit `gap: true` and require replay.

Proposed Tauri commands:

```text
subscribe_conversation_events(
  conversation_id: String,
  after_cursor: Option<String>
) -> Result<SubscribeConversationEventsResponse, CommandErrorPayload>

unsubscribe_conversation_events(
  subscription_id: String
) -> Result<UnsubscribeConversationEventsResponse, CommandErrorPayload>
```

Proposed emitted event name:

```text
conversation_event_batch
```

Proposed payload:

```rust
pub struct SubscribeConversationEventsResponse {
    pub subscription_id: String,
    pub conversation_id: String,
    pub replay_events: Vec<RunEventPayload>,
    pub cursor: Option<String>,
    pub gap: bool,
}

pub struct UnsubscribeConversationEventsResponse {
    pub subscription_id: String,
    pub status: ConversationSubscriptionStatus,
}

pub enum ConversationSubscriptionStatus {
    Unsubscribed,
    AlreadyClosed,
}

pub enum ConversationEventBatchPhase {
    Replay,
    Live,
}

pub struct ConversationEventBatchPayload {
    pub subscription_id: String,
    pub conversation_id: String,
    pub events: Vec<RunEventPayload>,
    pub cursor: Option<String>,
    pub gap: bool,
    pub phase: ConversationEventBatchPhase,
}
```

The subscribe command returns replay events in `SubscribeConversationEventsResponse`. The `shared/tauri` adapter must dispatch those replay events into the reducer before it accepts any emitted live batch for the same `subscription_id`. Emitted batches use `conversation_event_batch`; their `phase` is `live` unless the adapter explicitly documents a replay recovery path. Live batches must never overtake replay for the same `subscription_id`.

`RunEventPayload` must include enough safe data for timeline projection, including `conversationSequence` or an equivalent monotonic order key. It must not expose raw secrets. Permission display payloads must follow existing validation rules.

The frontend must not call Tauri `listen` directly from feature components or hooks. `shared/tauri` owns the Tauri event listener and exposes a typed subscription adapter to `features/conversation/timeline`.

`list_activity` can remain for Activity rail and details. It must not be the primary conversation renderer after this plan.

## Frontend Target File Layout

Create this feature-local timeline layer:

```text
apps/desktop/src/features/conversation/timeline/
  conversation-blocks.ts
  conversation-timeline-actions.ts
  conversation-timeline-reducer.ts
  conversation-timeline-reducer.test.ts
  conversation-timeline-selectors.ts
  conversation-timeline-source.ts
  conversation-timeline-source.test.ts
  use-conversation-event-stream.ts
  use-conversation-timeline.ts
  conversation-timeline.tsx
  conversation-timeline.test.tsx
  conversation-timeline.stories.tsx
  conversation-block-renderer.tsx
  user-message-block-view.tsx
  assistant-message-block-view.tsx
  assistant-streaming-block-view.tsx
  tool-group-block-view.tsx
  permission-request-block-view.tsx
  clarification-request-block-view.tsx
  artifact-block-view.tsx
  diff-review-block-view.tsx
  review-request-block-view.tsx
  error-block-view.tsx
  system-notice-block-view.tsx
```

File names must follow `docs/frontend/frontend-engineering.md`: components use `kebab-case.tsx`, hooks use `use-*.ts`, tests use `*.test.ts(x)`, and stories use `*.stories.tsx`. React component exports may still use PascalCase component names.

Modify existing files:

```text
apps/desktop/src/features/conversation/ConversationWorkspace.tsx
apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx
apps/desktop/src/features/conversation/ConversationWorkspace.stories.tsx
apps/desktop/src/features/conversation/Composer.tsx
apps/desktop/src/features/conversation/Composer.test.tsx
apps/desktop/src/features/conversation/conversation-models.ts
apps/desktop/src/features/conversation/conversation-production-boundaries.test.ts
apps/desktop/src/features/activity/use-activity.ts
apps/desktop/src/features/activity/ActivityRail.tsx
apps/desktop/src/shared/events/run-event-schema.ts
apps/desktop/src/shared/events/run-event-schema.test.ts
apps/desktop/src/shared/tauri/commands.ts
apps/desktop/src/shared/tauri/commands.test.ts
apps/desktop/src/shared/tauri/default-client.ts
apps/desktop/src/shared/tauri/mock-client.ts
apps/desktop/src/app/shell/AppShell.tsx
apps/desktop/src/app/shell/AppShell.test.tsx
```

Modify backend files:

```text
apps/desktop/src-tauri/src/commands.rs
apps/desktop/src-tauri/src/lib.rs
apps/desktop/src-tauri/capabilities/default.json
apps/desktop/src-tauri/tests/commands.rs
crates/jyowo-harness-contracts/src/events/mod.rs
crates/jyowo-harness-contracts/src/events/messages.rs
crates/jyowo-harness-contracts/tests/m1_contracts.rs
crates/jyowo-harness-sdk/src/harness.rs
crates/jyowo-harness-sdk/tests/runtime_assembly.rs
docs/frontend/frontend-engineering.md
docs/frontend/frontend-quality.md
docs/backend/backend-engineering.md
docs/backend/backend-quality.md
```

Remove or rewrite after migration:

```text
apps/desktop/src/features/conversation/ConversationMessage.tsx
apps/desktop/src/features/conversation/ProgressBlock.tsx
apps/desktop/src/features/conversation/ArtifactSummary.tsx
```

Only remove these if all imports are gone.

## Reducer Contract

Reducer input:

```ts
type ConversationTimelineAction =
  | { type: 'hydrateSnapshot'; snapshot: ConversationSnapshot }
  | { type: 'applyEvents'; events: RunEvent[]; cursor?: string | null }
  | { type: 'applyArtifacts'; artifacts: ArtifactView[] }
  | { type: 'localSubmit'; clientMessageId: string; draft: ComposerSubmitPayload; at: string }
  | { type: 'commandAccepted'; clientMessageId: string; runId: string }
  | { type: 'commandFailed'; clientMessageId: string; errorMessage: string }
  | { type: 'assistantFinalContentMissing'; runId: string; messageId: string }
  | { type: 'snapshotReconciled'; snapshot: ConversationSnapshot }
  | { type: 'permissionSubmitting'; requestId: string; decision: 'approve' | 'deny' }
  | { type: 'permissionSubmitFailed'; requestId: string; errorMessage: string }
  | { type: 'markGap'; runId?: string; afterCursor?: string }
```

Reducer state:

```ts
type ConversationTimelineState = {
  conversationId: string
  blocks: ConversationBlock[]
  eventsById: Record<string, true>
  cursor: string | null
  activeRunIds: string[]
  activeTurnByRunId: Record<string, string>
  clientMessageByRunId: Record<string, string>
  optimisticBlocksByClientMessageId: Record<string, string>
  streamingBlockByRunId: Record<string, string>
  toolGroupBlockByRunId: Record<string, string>
  artifactBlockByArtifactId: Record<string, string>
  permissionBlockByRequestId: Record<string, string>
  pendingAssistantReconcileByMessageId: Record<string, true>
  hasGap: boolean
}
```

Reducer invariants:

- Applying the same event twice produces the same state.
- Applying snapshot then replay produces the same final visible blocks as replay then snapshot reconciliation.
- `blocks` order is stable.
- `clientMessageId` is the only optimistic user-message confirmation key.
- Two identical user message bodies in the same conversation do not coalesce.
- Backend conversation order is preserved across multiple runs.
- Failed optimistic submits remain visible.
- Permission final state cannot be invented by frontend.
- Withheld payloads never expose hidden values.
- Artifact updates patch existing blocks.
- Tool groups do not become log walls.
- Unknown event types fail tests through exhaustive checks.

## Implementation Tasks

### Task 1: Lock the Event Contract

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/events/mod.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/messages.rs`
- Modify: `crates/jyowo-harness-contracts/tests/m1_contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src/shared/events/run-event-schema.ts`
- Modify: `apps/desktop/src/shared/events/run-event-schema.test.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`

**Step 1: Write failing Rust contract tests**

Add tests proving `UserMessageAppended` and `AssistantMessageCompleted` are projected as public timeline events with redacted display text and that event payload serialization does not expose secrets.

Run:

```bash
cargo test -p jyowo-harness-contracts m1_contracts -- --nocapture
```

Expected: fail until the projection exists.

**Step 2: Write failing frontend schema tests**

Add valid and invalid cases for:

- `user.message.appended`
- `assistant.completed` with final redacted `body`
- withheld payload rejection
- duplicate/monotonic sequence validation
- conversation cursor/order validation
- visible payload secret rejection
- two user messages with identical body but different `clientMessageId`

Run:

```bash
pnpm -C apps/desktop test -- run-event-schema.test.ts
```

Expected: fail until schema supports the new event.

**Step 3: Implement contract/schema changes**

Add only the fields needed by timeline projection.

For `user.message.appended`:

```ts
{
  messageId: string
  clientMessageId: string
  body: string
}
```

For `assistant.completed`:

```ts
{
  messageId: string
  body: string
}
```

If Rust already stores richer message content, project only redacted display text for frontend timeline payloads.

Update `start_run` request and tests to include `clientMessageId`. The frontend generates it once per submit and Rust echoes it in the projected `user.message.appended` event. Body-only optimistic matching is forbidden.

**Step 4: Verify**

Run:

```bash
cargo test -p jyowo-harness-contracts m1_contracts -- --nocapture
pnpm -C apps/desktop test -- run-event-schema.test.ts
```

Expected: pass.

### Task 2: Add Conversation Event Subscription IPC

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/capabilities/default.json`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Modify: `apps/desktop/src/shared/tauri/default-client.ts`
- Modify: `apps/desktop/src/shared/tauri/mock-client.ts`

**Step 1: Write failing backend command tests**

Cover:

- subscribe validates non-empty conversation id
- subscribe returns replay events after cursor
- subscribe returns replay before live for the same subscription
- subscribe returns no raw secret payload
- unsubscribe removes subscription
- subscription is scoped to the calling window and conversation id
- subscription cleanup runs on explicit unsubscribe and window close
- backpressure overflow emits `gap: true`
- deleted/missing conversation fails closed

Run:

```bash
cargo test -p jyowo-desktop-shell commands -- --nocapture
```

Expected: fail until commands exist.

**Step 2: Write failing frontend command tests**

Cover Zod parsing for:

- `SubscribeConversationEventsResponse`
- `UnsubscribeConversationEventsResponse`
- emitted batch payload schema
- replay response is dispatched before live emitted batches
- typed shared/tauri event listener cleanup
- stale `subscriptionId` and stale conversation batches are ignored

Run:

```bash
pnpm -C apps/desktop test -- commands.test.ts
```

Expected: fail until command client supports the new commands.

**Step 3: Implement backend commands**

Add:

```text
subscribe_conversation_events
unsubscribe_conversation_events
```

Handlers stay thin. Business logic belongs in runtime/SDK or a command-local adapter around the SDK facade.

Use the same projection rules as `list_activity`. If any event is withheld, emit no payload.

The command may accept the current Tauri window as a Rust command argument. Store subscriptions by `(window_label, subscription_id)` and emit `conversation_event_batch` only to that window.

The subscription must:

- return replay events from `after_cursor`
- start live delivery only after replay is complete
- batch live events on a documented interval or max batch size
- mark `gap: true` instead of dropping events silently
- remove its sender/task on unsubscribe, conversation switch, hook unmount, and window close

**Step 4: Register commands**

Register in `generate_handler!`.

Update Tauri capabilities.

**Step 5: Implement frontend command client**

Add typed methods to `CommandClient`, invoke client, default client, and mock client.

Add a typed event subscription adapter under `shared/tauri`. Feature code receives parsed `ConversationEventBatchPayload` values and an unsubscribe function; feature code must not import Tauri event APIs directly.

**Step 6: Verify**

Run:

```bash
cargo test -p jyowo-desktop-shell commands -- --nocapture
pnpm -C apps/desktop test -- commands.test.ts
```

Expected: pass.

### Task 3: Build the Timeline Model and Reducer

**Files:**

- Create: `apps/desktop/src/features/conversation/timeline/conversation-blocks.ts`
- Create: `apps/desktop/src/features/conversation/timeline/conversation-timeline-actions.ts`
- Create: `apps/desktop/src/features/conversation/timeline/conversation-timeline-reducer.ts`
- Create: `apps/desktop/src/features/conversation/timeline/conversation-timeline-selectors.ts`
- Create: `apps/desktop/src/features/conversation/timeline/conversation-timeline-reducer.test.ts`

**Step 1: Write reducer tests first**

Required tests:

- local submit creates sending `UserMessageBlock`
- `user.message.appended` confirms optimistic block by `clientMessageId`
- identical user bodies with different `clientMessageId` remain separate blocks
- `assistant.delta` creates and appends streaming block
- `assistant.completed` finalizes streaming block from final `body`
- `assistant.completed` without final `body` marks pending reconciliation and does not claim final content
- `run.ended` completes active turn
- `engine.failed` keeps partial output and creates `ErrorBlock`
- permission requested/resolved lifecycle
- tool events aggregate into one `ToolGroupBlock`
- artifact created/updated patches one `ArtifactBlock`
- duplicate events are ignored
- multiple runs preserve backend conversation order
- out-of-cursor-order events produce gap state
- withheld events render safe notice
- snapshot hydration plus replay produces deterministic blocks
- snapshot reconciliation replaces partial assistant text only by `messageId`

Run:

```bash
pnpm -C apps/desktop test -- conversation-timeline-reducer.test.ts
```

Expected: fail until reducer exists.

**Step 2: Define block types**

Use discriminated unions. No `any`.

Every renderer switch must be exhaustive.

**Step 3: Implement reducer**

Keep reducer pure.

No TanStack Query, Zustand, Tauri, DOM, i18n hooks, or timers inside reducer.

**Step 4: Implement selectors**

Selectors needed:

- `selectBlocks`
- `selectActiveRunIds`
- `selectComposerMode`
- `selectPendingPermissionBlocks`
- `selectShouldPollFallback`
- `selectLatestVisibleBlockId`

**Step 5: Verify**

Run:

```bash
pnpm -C apps/desktop test -- conversation-timeline-reducer.test.ts
```

Expected: pass.

### Task 4: Add Event Stream Source Hook

**Files:**

- Create: `apps/desktop/src/features/conversation/timeline/conversation-timeline-source.ts`
- Create: `apps/desktop/src/features/conversation/timeline/conversation-timeline-source.test.ts`
- Create: `apps/desktop/src/features/conversation/timeline/use-conversation-event-stream.ts`
- Create/modify tests beside the hook.

**Step 1: Write failing tests**

Cover:

- subscribes for selected conversation
- applies replayed events before live events
- unsubscribes on conversation change/unmount
- ignores stale events from old subscription id
- ignores events for a stale conversation id
- uses the shared/tauri listener adapter, not raw Tauri `listen`
- marks gap and requests replay when batch has `gap: true`
- marks gap when cursor order is not contiguous
- falls back to polling when subscription command fails

Run:

```bash
pnpm -C apps/desktop test -- conversation-timeline-source.test.ts
```

Expected: fail.

**Step 2: Implement source adapter**

The adapter normalizes:

```text
snapshot
replay
live batch
poll fallback
local submit
command result
```

into reducer actions.

**Step 3: Implement hook**

The hook may use the typed subscription adapter exposed by `shared/tauri`. Keep raw Tauri event listening out of feature code and visual components.

**Step 4: Verify**

Run:

```bash
pnpm -C apps/desktop test -- conversation-timeline-source.test.ts
```

Expected: pass.

### Task 5: Build `useConversationTimeline`

**Files:**

- Create: `apps/desktop/src/features/conversation/timeline/use-conversation-timeline.ts`
- Modify: `apps/desktop/src/features/conversation/use-conversation.ts`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`

**Step 1: Write failing integration hook/component tests**

Cover the original bug:

- `startRun` returns after `run.started`
- user message appears immediately as optimistic block
- later `user.message.appended` confirms it
- same-body repeated submits do not merge
- later `assistant.delta` streams visible text
- later `assistant.completed` finalizes assistant block
- `assistant.completed` final body replaces streamed draft by `messageId`
- no route switch is required to see content

Run:

```bash
pnpm -C apps/desktop test -- ConversationWorkspace.test.tsx
```

Expected: fail before hook integration.

**Step 2: Implement hook**

Responsibilities:

- load initial conversation snapshot through TanStack Query
- connect event stream source
- generate `clientMessageId` and dispatch local submit before `startRun`
- pass `clientMessageId` into `startRun`
- dispatch command accepted/failed after `startRun`
- trigger snapshot reconciliation when final assistant body is missing
- load artifact snapshots and patch artifact blocks
- expose blocks, composer mode, submit handlers, permission handlers, loading/error states

**Step 3: Keep boundaries**

The hook may use `CommandClient`.

Leaf block components must not import `CommandClient`.

**Step 4: Verify**

Run:

```bash
pnpm -C apps/desktop test -- ConversationWorkspace.test.tsx
```

Expected: pass.

### Task 6: Implement Timeline Rendering Components

**Files:**

- Create: `apps/desktop/src/features/conversation/timeline/conversation-timeline.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/conversation-block-renderer.tsx`
- Create all block view files listed in the target layout.
- Create: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx`

**Step 1: Write component tests**

Cover:

- empty timeline
- user message
- assistant streaming
- completed assistant message
- collapsed successful tool group
- expanded failed tool group
- pending permission with approve/deny
- resolved permission
- artifact ready/loading/failed
- diff review pending
- error block with retry
- withheld notice

Run:

```bash
pnpm -C apps/desktop test -- conversation-timeline.test.tsx
```

Expected: fail.

**Step 2: Implement visual components**

Use semantic tokens.

Use existing primitives from `shared/ui`.

Use lucide icons where buttons need icons.

Do not introduce new UI libraries.

**Step 3: Add Storybook states**

Storybook must include:

- loading
- empty
- streaming
- permission pending
- tool failed
- artifact ready
- diff review
- run failed
- long conversation

**Step 4: Verify**

Run:

```bash
pnpm -C apps/desktop test -- conversation-timeline.test.tsx
pnpm -C apps/desktop build-storybook
```

Expected: pass.

### Task 7: Refactor `ConversationWorkspace`

**Files:**

- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.stories.tsx`
- Modify/remove: `apps/desktop/src/features/conversation/ConversationMessage.tsx`
- Modify/remove: `apps/desktop/src/features/conversation/ProgressBlock.tsx`
- Modify/remove: `apps/desktop/src/features/conversation/ArtifactSummary.tsx`

**Step 1: Write failing workspace tests**

Cover:

- workspace renders `ConversationTimeline`
- composer remains bottom primary entry
- context panel behavior still works
- activity rail remains secondary
- no `localMessages` behavior remains
- artifact/review blocks are independent timeline blocks

Run:

```bash
pnpm -C apps/desktop test -- ConversationWorkspace.test.tsx
```

Expected: fail.

**Step 2: Refactor component**

`ConversationWorkspace` should become composition:

```text
useConversationTimeline
ConversationTimeline
Composer
```

It must not build message lists manually.

**Step 3: Delete obsolete code**

Remove old runtime projection code after tests pass:

- `OptimisticMessage`
- `toRuntimeState`
- `toActivityItems` inside `ConversationWorkspace`
- latest-assistant artifact attachment logic

**Step 4: Verify**

Run:

```bash
pnpm -C apps/desktop test -- ConversationWorkspace.test.tsx
pnpm -C apps/desktop test -- conversation-production-boundaries.test.ts
```

Expected: pass.

### Task 8: Upgrade Composer Modes

**Files:**

- Modify: `apps/desktop/src/features/conversation/Composer.tsx`
- Modify: `apps/desktop/src/features/conversation/Composer.test.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/use-conversation-timeline.ts`

**Step 1: Write failing tests**

Cover:

- ready send
- submitting
- running disabled with stop affordance if cancel is available
- clarification reply mode
- review comment mode
- retry failed turn
- continue completed turn
- input preserved on command failure

Run:

```bash
pnpm -C apps/desktop test -- Composer.test.tsx
```

Expected: fail.

**Step 2: Implement mode model**

Use an explicit prop:

```ts
type ComposerMode =
  | { kind: 'ready' }
  | { kind: 'submitting' }
  | { kind: 'running'; canCancel: boolean }
  | { kind: 'clarification'; blockId: string }
  | { kind: 'review'; blockId: string }
  | { kind: 'retry'; turnId: string }
```

Do not infer complex UX state from scattered booleans.

**Step 3: Verify**

Run:

```bash
pnpm -C apps/desktop test -- Composer.test.tsx
```

Expected: pass.

### Task 9: Wire Interaction Blocks

**Files:**

- Modify: `apps/desktop/src/features/conversation/timeline/permission-request-block-view.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/review-request-block-view.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/clarification-request-block-view.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/use-conversation-timeline.ts`
- Modify: `apps/desktop/src/features/activity/RunEventDetails.tsx` only if needed to keep details secondary.

**Step 1: Write failing tests**

Cover:

- approve permission enters submitting
- deny permission enters submitting
- `permission.resolved` finalizes UI
- permission command error restores pending or failed state
- review continue submits one request
- clarification answer creates local submit for the same turn context

Run:

```bash
pnpm -C apps/desktop test -- conversation-timeline.test.tsx ConversationWorkspace.test.tsx
```

Expected: fail.

**Step 2: Implement action callbacks**

Block views emit intent only.

The hook calls `CommandClient.resolvePermission`, `startRun`, or `cancelRun`.

**Step 3: Verify**

Run:

```bash
pnpm -C apps/desktop test -- conversation-timeline.test.tsx ConversationWorkspace.test.tsx
```

Expected: pass.

### Task 10: Refactor Activity Rail and App Shell Integration

**Files:**

- Modify: `apps/desktop/src/app/shell/AppShell.tsx`
- Modify: `apps/desktop/src/app/shell/AppShell.test.tsx`
- Modify: `apps/desktop/src/features/activity/use-activity.ts`
- Modify: `apps/desktop/src/features/activity/ActivityRail.tsx`

**Step 1: Write failing tests**

Cover:

- shell clears active run only after timeline terminal event is processed
- activity rail does not drive conversation body
- context panel still opens for active run
- permission details remain available but secondary

Run:

```bash
pnpm -C apps/desktop test -- AppShell.test.tsx ActivityRail.test.tsx use-activity.test.tsx
```

Expected: fail where old assumptions remain.

**Step 2: Implement integration**

Keep Activity rail compact.

Do not duplicate timeline block rendering in Activity rail.

**Step 3: Verify**

Run:

```bash
pnpm -C apps/desktop test -- AppShell.test.tsx ActivityRail.test.tsx use-activity.test.tsx
```

Expected: pass.

### Task 11: Add Virtualization and Scroll Anchoring

**Files:**

- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/use-conversation-scroll-anchor.ts`
- Create tests beside the hook/component.

**Step 1: Write failing tests**

Cover:

- auto-follow when near bottom
- no auto-follow after user scrolls up
- jump-to-latest appears when new block arrives offscreen
- user submit re-enables follow
- streaming deltas do not call `scrollIntoView` on every delta

Run:

```bash
pnpm -C apps/desktop test -- conversation-timeline.test.tsx
```

Expected: fail.

**Step 2: Implement TanStack Virtual**

Use `@tanstack/react-virtual`.

Do not create a custom virtualizer.

Use text measurement helpers through `shared/text-layout` only if needed.

**Step 3: Verify**

Run:

```bash
pnpm -C apps/desktop test -- conversation-timeline.test.tsx
```

Expected: pass.

### Task 12: Rebuild Stories and E2E Flow

**Files:**

- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.stories.tsx`
- Create/modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx`
- Modify relevant Playwright tests under `apps/desktop`.

**Step 1: Add Storybook state matrix**

Required stories:

- Empty
- Completed conversation
- Streaming assistant
- Permission pending
- Tool group failed
- Artifact review
- Diff review
- Clarification request
- Engine failed with partial output
- Long virtualized conversation

**Step 2: Add E2E smoke**

Use web mock runtime.

Flow:

```text
open /
send prompt
optimistic user block appears
assistant streaming block appears
permission block can be resolved
artifact block appears
review continue works
final assistant block remains
```

**Step 3: Verify**

Run:

```bash
pnpm -C apps/desktop build-storybook
pnpm -C apps/desktop test:e2e
```

Expected: pass.

### Task 13: Update Documentation

**Files:**

- Modify: `docs/frontend/frontend-engineering.md`
- Modify: `docs/frontend/frontend-quality.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`

**Step 1: Update frontend docs**

Document:

- `ConversationBlock[]` as conversation canvas render source
- timeline reducer ownership
- event stream hook boundary
- `clientMessageId` optimistic confirmation contract
- conversation cursor and ordering contract
- block renderer requirements
- streaming and scroll testing expectations

**Step 2: Update backend docs**

Document:

- new Tauri commands
- command payloads
- `start_run` `clientMessageId` correlation
- replay-before-live subscription behavior
- live subscription as single-process guarantee
- replay/snapshot as restart-stable guarantee
- critical tests

**Step 3: Verify docs**

Run:

```bash
pnpm check:docs
```

Expected: pass.

### Task 14: Remove Old Rendering Architecture

**Files:**

- Remove or rewrite old files with no remaining imports:
  - `apps/desktop/src/features/conversation/ConversationMessage.tsx`
  - `apps/desktop/src/features/conversation/ProgressBlock.tsx`
  - `apps/desktop/src/features/conversation/ArtifactSummary.tsx`
- Modify tests that referenced old latest-assistant attachment behavior.

**Step 1: Find old imports**

Run:

```bash
rg -n "ConversationMessage|ProgressBlock|ArtifactSummary|toRuntimeState|localMessages|OptimisticMessage" apps/desktop/src
```

Expected: no production references after deletion/refactor.

**Step 2: Delete obsolete code**

Use `apply_patch`. Do not leave dead wrappers.

**Step 3: Verify unused files/exports**

Run:

```bash
pnpm -C apps/desktop knip
```

Expected: pass.

### Task 15: Full Verification

**Files:** all changed files.

**Step 1: Frontend gate**

Run:

```bash
pnpm check:desktop
```

Expected: pass.

**Step 2: Rust gate**

Run:

```bash
pnpm check:rust
```

Expected: pass.

**Step 3: Docs gate**

Run:

```bash
pnpm check:docs
```

Expected: pass.

**Step 4: Full gate**

Run:

```bash
pnpm check
```

Expected: pass.

**Step 5: Manual product check**

Run the desktop app or web mock runtime.

Verify:

- send shows user block immediately
- assistant streams without route switch
- tool activity stays compact
- permission appears inline and resolves safely
- artifact and review blocks appear independently
- scroll behavior is stable
- route switch/reload restores final state
- no raw secret-like text appears in visible blocks

## Acceptance Criteria

Implementation is complete only when all are true:

- Conversation body renders from `ConversationBlock[]`.
- `ConversationWorkspace` no longer owns message merging logic.
- Streaming assistant output is visible before completion.
- User submit appears immediately and is later confirmed by backend event.
- User submit confirmation uses `clientMessageId`, never body matching.
- Conversation ordering uses backend conversation cursor/order, not `(runId, sequence)`.
- Assistant completion uses redacted final body or explicit snapshot reconciliation.
- Permission and review interactions are timeline blocks.
- Artifacts and diffs are independent blocks.
- Activity rail is secondary and not required for conversation rendering.
- Event subscription supports replay and live batches.
- Tauri event listening is wrapped by `shared/tauri`; feature code does not use raw Tauri listeners.
- Polling is fallback only.
- Reducer is deterministic and idempotent.
- Sequence gaps trigger replay or safe recovery.
- Withheld/redacted visibility is respected.
- Frontend does not expose or store secrets.
- Backend docs list new commands and guarantees.
- Frontend docs describe timeline ownership.
- Unit, component, Storybook, E2E, Rust, and docs gates pass.

## Explicit Non-Goals

Do not add these in this implementation:

- Multi-user collaboration.
- Cloud sync.
- Public sharing.
- Full IDE editor.
- Branch compare UI beyond stable ids and reducer-safe modeling.
- New styling system.
- New router, state library, form library, validation library, or icon library.
- Frontend-only permission decisions.
- Raw provider key display outside existing reveal flow.

## Final Notes for Implementers

When a local decision conflicts with this document, follow this document.

When this document conflicts with `AGENTS.md` or active frontend/backend specs, follow the specs and update this plan only with explicit user approval.

Do not preserve old rendering code for comfort. The goal is a clean timeline architecture with no parallel conversation body renderer.
