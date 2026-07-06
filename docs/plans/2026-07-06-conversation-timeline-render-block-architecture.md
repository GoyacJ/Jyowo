# Conversation Timeline Render Block Architecture Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> The implementation session MUST use the `chatgpt5.5 xhigh` model if that model is selectable. If the executor cannot select that model, stop before editing and report the blocker.

**Goal:** Build an extensible conversation timeline rendering architecture that displays assistant text, file edits, reads/searches, tool work, command executions, artifacts, notices, and errors as stable, collapsible, Codex-style work blocks without rendering raw `RunEvent` data in the main conversation canvas.

**Architecture:** `RunEvent` remains the durable fact log. Rust remains the projection authority and emits UI-safe `ConversationTurn[]` / `AssistantWork` data. React converts projected assistant segments into frontend-only `TimelineRenderBlock[]`, then renders them through a small renderer registry and a shared `EvidenceDisclosure` shell.

**Tech Stack:** Rust, serde, schemars, rusqlite, Tauri 2, React 19, TypeScript 6, Zod, TanStack Query, TanStack Virtual, Zustand, Tailwind CSS v4, shiki, lucide-react, Vitest, Testing Library, Storybook.

---

## Branch And Worktree Rules

This plan document is generated on `main`.

Before implementation starts, this exact plan file MUST already exist in `main` as a Git-tracked file:

```bash
git branch --show-current
test -f docs/plans/2026-07-06-conversation-timeline-render-block-architecture.md
git ls-files --error-unmatch docs/plans/2026-07-06-conversation-timeline-render-block-architecture.md
```

Expected:

```text
main
plan file exists
plan file is tracked by git
```

If any command fails, stop. Do not create the implementation worktree from a branch that cannot see this plan.

Implementation MUST NOT happen directly in `main`.

Start implementation in an isolated worktree:

```bash
git status --short
git branch --show-current
git worktree add ../Jyowo-timeline-render-blocks -b goya/timeline-render-blocks main
cd ../Jyowo-timeline-render-blocks
git status --short
test -f docs/plans/2026-07-06-conversation-timeline-render-block-architecture.md
git ls-files --error-unmatch docs/plans/2026-07-06-conversation-timeline-render-block-architecture.md
```

Expected:

```text
main
clean status before worktree creation
new branch: goya/timeline-render-blocks
clean status inside isolated worktree
plan file exists inside isolated worktree
plan file is tracked inside isolated worktree
```

If `main` is dirty, stop. Do not stash, reset, or checkout over user changes.

## Non-Negotiable Rules

- The main conversation canvas MUST render `ConversationTurn[]`, never raw `RunEvent[]`.
- Rust owns durable facts, redaction, policy, permission finality, and UI-safe projection.
- React owns only display composition, local disclosure state, and inspector selection.
- Do not parse stdout, markdown, raw event JSON, or tool output to infer product UI.
- Do not add provider-specific rendering branches in React.
- Do not render raw chain-of-thought. Only projected safe summaries or withheld states may appear.
- Do not use production mocks, fake runtimes, placeholder command responses, or hard-coded success paths.
- Do not use mock data as acceptance evidence. Component tests may construct minimal typed values, but runtime acceptance must be covered by Rust projection tests and frontend contract/schema tests.
- No compatibility wrappers whose only purpose is to preserve old internal component APIs.
- Destructive refactor is allowed when it simplifies ownership and removes debt. It must be justified in the task completion notes.
- Large command output, full diff patches, and artifact content stay behind evidence refs and inspector fetches.
- Feature code uses semantic tokens and existing `shared/ui` primitives.
- No raw Tauri `invoke` from feature components.
- No frontend-only security or permission decisions.

## Current Code Anchors

Read these before implementation:

- `AGENTS.md`
- `docs/testing/testing-strategy.md`
- `docs/frontend/agent-harness-frontend-development-guidelines.md`
- `docs/frontend/frontend-product-ux.md`
- `docs/frontend/frontend-engineering.md`
- `docs/frontend/frontend-quality.md`
- `docs/backend/agent-harness-backend-development-guidelines.md`
- `docs/backend/backend-runtime.md`
- `docs/backend/backend-engineering.md`
- `docs/backend/backend-quality.md`
- `crates/jyowo-harness-contracts/src/conversation.rs`
- `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- `apps/desktop/src/shared/tauri/commands.ts`
- `apps/desktop/src/features/conversation/timeline/assistant-work-view.tsx`
- `apps/desktop/src/features/conversation/timeline/process-panel.tsx`
- `apps/desktop/src/features/conversation/timeline/process-step-row.tsx`
- `apps/desktop/src/features/conversation/timeline/command-evidence-block.tsx`
- `apps/desktop/src/features/conversation/timeline/diff-evidence-block.tsx`
- `apps/desktop/src/features/conversation/evidence/CommandExecutionView.tsx`
- `apps/desktop/src/features/conversation/evidence/DiffPane.tsx`
- `apps/desktop/src/features/workbench/WorkbenchInspector.tsx`
- `apps/desktop/src/shared/state/workbench-selection.ts`
- `apps/desktop/src/shared/state/ui-store.ts`

## Target Design

The target pipeline is:

```text
RunEvent
  -> Rust ConversationWorktreeProjection
  -> ConversationTurn[] / AssistantWork / AssistantSegment
  -> frontend buildTimelineRenderBlocks(assistant)
  -> TimelineBlockRenderer registry
  -> EvidenceDisclosure + specialized block renderer
```

The frontend render model is internal. It must not enter the Tauri contract.

Use these exact discriminants. Do not add a catch-all raw block.

```ts
type FileEditRenderFile = {
  changeSetId: string
  path: string
  oldPath?: string
  status: ChangeSetFile['status']
  addedLines: number
  removedLines: number
  preview?: string
  fullPatchRef?: string
  riskFlags: ChangeSetFile['riskFlags']
}

type ActivityRenderItem = {
  id: string
  kind: 'file' | 'search' | 'tool' | 'command'
  label: string
  detail?: string
}

type CommandRenderItem = {
  id: string
  stepId: string
  status: ProcessStep['status']
  command: CommandExecution
}

type TimelineRenderBlock =
  | {
      kind: 'assistantText'
      id: string
      order: number
      segment: TextSegment
    }
  | {
      kind: 'fileEdit'
      id: string
      order: number
      processSegmentId: string
      steps: ProcessStep[]
      files: FileEditRenderFile[]
      defaultOpen: boolean
      forcedOpen: boolean
    }
  | {
      kind: 'activity'
      id: string
      order: number
      processSegmentId: string
      steps: ProcessStep[]
      title: string
      itemCount?: number
      items: ActivityRenderItem[]
      defaultOpen: boolean
      forcedOpen: boolean
    }
  | {
      kind: 'commandGroup'
      id: string
      order: number
      processSegmentId: string
      steps: ProcessStep[]
      commands: CommandRenderItem[]
      defaultOpen: boolean
      forcedOpen: boolean
    }
  | {
      kind: 'toolGroup'
      id: string
      order: number
      segment: ToolGroupSegment
      defaultOpen: boolean
      forcedOpen: boolean
    }
  | { kind: 'artifact'; id: string; order: number; segment: ArtifactSegment }
  | { kind: 'reviewRequest'; id: string; order: number; segment: ReviewRequestSegment }
  | {
      kind: 'clarificationRequest'
      id: string
      order: number
      segment: ClarificationRequestSegment
    }
  | { kind: 'notice'; id: string; order: number; segment: NoticeSegment }
  | { kind: 'error'; id: string; order: number; segment: ErrorSegment }
  | { kind: 'agentActivity'; id: string; order: number; segment: AgentActivitySegment }
```

The contract extension point remains projected typed data:

```text
AssistantSegment.kind
ProcessStep.kind
ProcessStepDetail.type
```

Future display types should be added by projecting safe typed data and adding a renderer. Do not add raw payload renderers.

## Required Contract Additions

Add optional fields only where needed.

### AssistantWork Runtime Metadata

Add optional timing metadata to `AssistantWork`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub started_at: Option<DateTime<Utc>>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub ended_at: Option<DateTime<Utc>>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub duration_ms: Option<u64>,
```

Frontend names:

```ts
startedAt?: string
endedAt?: string
durationMs?: number
```

Display rule: show duration only when `durationMs` is present. Never invent duration in React.

### Activity Items

Extend `ProcessStepDetail::Activity` with optional typed items:

```rust
Activity {
    summary: UiSafeText,
    #[serde(rename = "itemCount", default, skip_serializing_if = "Option::is_none")]
    item_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    items: Vec<ProcessActivityItem>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProcessActivityItem {
    pub kind: ProcessActivityItemKind,
    pub label: UiSafeText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<UiSafeText>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProcessActivityItemKind {
    File,
    Search,
    Tool,
    Command,
}
```

Projection rule: if a safe label cannot be derived, keep `items` empty and show count-only UI. Do not pass unsafe raw paths or raw tool input.

## UI-Safe Projection Source Policy

Activity item labels are UI contract data, not raw event data.

Allowed sources:

- values returned by existing safety helpers such as `tool_affected_targets`, `safe_relative_path`, `ui_text`, and `bounded_ui_preview`
- existing safe summary fields already handled by `safe_summary_field` / `safeSummary`
- redacted, bounded labels that reject `[REDACTED]`, private absolute paths, unsafe URLs, and obvious secrets
- evidence refs such as `fullOutputRef` and `fullPatchRef` as opaque ids only

Forbidden sources:

- direct `string_field(payload, "path")`, `string_field(payload, "target")`, `string_field(payload, "query")`, or equivalent raw reads used as UI labels without the safety helper gate
- raw command stdout, stderr, full output, or full patch
- raw unredacted tool input, arguments JSON, provider payload JSON, or MCP payload JSON
- private absolute paths, home-relative paths, workspace-private `.jyowo` paths, URLs, or secret-like values

Required helper shape:

```rust
fn safe_activity_label(value: &str) -> Option<UiSafeText> {
    let bounded = bounded_ui_preview(value, 240);
    let text = ui_text(bounded);
    let display = text.as_str();
    if display.trim().is_empty() || display.contains("[REDACTED]") {
        return None;
    }
    Some(text)
}
```

If a candidate label cannot pass this helper or an existing stricter helper, return no item and preserve count-only UI.

## Render Block Semantics

### Text

`AssistantSegment.Text` renders as normal assistant markdown.

Do not place normal assistant text inside evidence cards.

### File Edit

Sources:

- `ProcessStepKind.FileEdit`
- `ProcessStepKind.Diff`
- `ProcessStepDetail.Diff(ChangeSet)`

Collapsed UI:

```text
已编辑 1 个文件
已编辑 worker_service_test.go +67 -0
```

Expanded UI:

```text
已编辑的文件
worker_service_test.go +67 -0
diff preview
```

Rules:

- Group related `fileEdit` and `diff` steps into one `FileEditRenderBlock` when they belong to the same process segment and adjacent work.
- Use `ChangeSet.files[]` for file names and counts.
- Use project-relative or projected safe labels only.
- Full patch opens in inspector via `fullPatchRef` / change set selection.

### Read And Search Activity

Sources:

- `ProcessStepKind.FileRead`
- `ProcessStepKind.FileSearch`
- `ProcessStepDetail.Activity`
- optional `ProcessActivityItem[]`

Collapsed UI:

```text
已读取 4 个文件和已搜索代码
```

Expanded UI:

```text
Searched for AssistantDeltaProduced|...
Read turn.rs
Read conversation_worktree_projector.rs
```

Rules:

- Default collapsed after completion.
- Running, failed, or permission-related activity stays visible.
- Do not show file contents in the main timeline.
- If no `items` exist, show summary and count only.

### Command Execution

Sources:

- `ProcessStepKind.Command`
- `ProcessStepDetail.Command(CommandExecution)`

Collapsed UI:

```text
已运行 3 条命令
已运行 df -h /System/Volumes/Data
已运行 git status --short
```

Expanded UI:

```text
Shell
$ command
output preview
exit status / duration
```

Rules:

- Failed, running, and non-zero commands default open.
- Successful historical commands default collapsed.
- The most recent successful command may default open when there is no failed/running command in the same group.
- Timeline shows preview only.
- Full output fetch remains inspector-owned unless explicitly approved in this plan later.

### Tool Group

Sources:

- `ToolGroupSegment`
- `ToolAttempt`

Rules:

- Completed low-signal attempts collapse behind a summary.
- Failed, denied, running, and permission-pending attempts remain visible.
- Permission UI stays nested under the owning tool attempt.
- Tool execution status and permission status stay separate.

### Artifacts, Review, Clarification, Notice, Error, Agent Activity

These keep their existing segment semantics. They should render through the same registry so adding new block types does not add branches back into `AssistantWorkView`.

## Disclosure State

Use existing `evidenceDisclosureOpen` in `apps/desktop/src/shared/state/ui-store.ts`.

Stable key format:

```text
conversation:{conversationId}:run:{runId}:block:{kind}:{blockId}
```

Rules:

- Keys must not depend on array indexes.
- Refresh, pagination, and live projection updates must preserve user-opened state.
- Forced-open states override stored collapsed state.
- UI-only state stays in Zustand; do not persist it to Rust.

Stable block id rules:

- `assistantText`, `artifact`, `reviewRequest`, `clarificationRequest`, `notice`, `error`, and `agentActivity` use the projected segment id.
- `toolGroup` uses the projected tool group segment id.
- `fileEdit` uses `process:{processSegment.id}:file-edit:{firstEditOrDiffStep.id}`. Adding an adjacent diff/file row to the same block must not change this id.
- `activity` uses `process:{processSegment.id}:activity:{firstActivityStep.id}`. Updating item count or appending adjacent read/search items must not change this id.
- `commandGroup` uses `process:{processSegment.id}:commands:{firstCommandStep.id}`. Appending later commands to that same group must not change this id.
- React list keys and disclosure keys must not use array indexes.

Required stability tests:

- user-opened file edit block remains open after a diff preview appears.
- user-opened activity block remains open after item count increases.
- user-opened command group remains open after a new command is appended.
- separate later groups get distinct ids.

## Required Task Protocol

Create a local execution log in the implementation worktree:

```bash
mkdir -p .codex/task-log
touch .codex/task-log/timeline-render-blocks.md
```

Never stage or commit `.codex/task-log/timeline-render-blocks.md`.

Every task must start by appending this analysis to that log:

```text
Task intent:
- What behavior this task must add/change:
- Files expected to change:
- Tests expected to fail first:
- Invariants that must remain true:
- What this task must not change:
```

Before ending every task, write this completion analysis:

```text
Task completion check:
- Implemented behavior:
- Tests run and result:
- Diff reviewed:
- Unrelated changes found:
- Compatibility/debt left behind:
- Reason destructive refactor was or was not necessary:
```

Then run a read-only subagent audit for the task.

Required audit prompt:

```text
You are a read-only code review subagent. Audit Task N of
docs/plans/2026-07-06-conversation-timeline-render-block-architecture.md.

Check only this task's scope.
Read `.codex/task-log/timeline-render-blocks.md` before judging completion.
Return PASS or FAIL.
Verify:
- task intent was implemented
- no raw RunEvent rendering was introduced
- Rust remains projection authority
- no production mocks/fakes/placeholders were added
- no unsafe raw paths, secrets, or chain-of-thought can enter frontend state
- tests match the required layer
- code follows existing project boundaries
- no orphan wrappers or unused code remain

Do not edit files.
```

Task is not complete until the audit returns PASS. If FAIL, fix and re-audit.

## Per-Task Gate Policy

Focused tests are not enough for task completion.

Before any task commit:

- Rust contract or Rust projection changes require the focused Rust tests and `pnpm check:rust`.
- Frontend TypeScript, React, Zustand, Storybook, or Zod changes that are already integrated into the production import graph require focused frontend tests and `pnpm check:desktop`.
- Pre-integration frontend tasks that create production files before they are imported by `AssistantWorkView` must run focused tests, `pnpm -C apps/desktop typecheck`, and `pnpm -C apps/desktop lint`. Do not run `pnpm -C apps/desktop knip` or `pnpm check:desktop` for those tasks, because those gates intentionally fail on unreferenced production files.
- Task 6 is the first integration task. It MUST run focused tests, `pnpm -C apps/desktop knip`, and `pnpm check:desktop` after wiring the new files into the production import graph.
- Cross frontend/backend changes require both focused layer tests and `pnpm check`.
- Docs changes require `pnpm check:docs`.
- If a gate is impossible because of an environmental blocker, stop and record the exact command, exit reason, and why continuing would be unsafe.

## Task 0: Implementation Worktree And Baseline Audit

**Files:**

- Read: `AGENTS.md`
- Read: frontend, backend, and testing docs listed above
- No source edits

**Step 1: Task intent analysis**

Write the required task intent block.

**Step 2: Create isolated worktree**

Run:

```bash
git status --short
git branch --show-current
git worktree add ../Jyowo-timeline-render-blocks -b goya/timeline-render-blocks main
cd ../Jyowo-timeline-render-blocks
git status --short
test -f docs/plans/2026-07-06-conversation-timeline-render-block-architecture.md
git ls-files --error-unmatch docs/plans/2026-07-06-conversation-timeline-render-block-architecture.md
```

Expected:

```text
current branch before creation: main
worktree branch: goya/timeline-render-blocks
status clean
plan file exists inside isolated worktree
plan file is tracked inside isolated worktree
```

**Step 3: Read required docs**

Run:

```bash
cat AGENTS.md
cat docs/testing/testing-strategy.md
cat docs/frontend/agent-harness-frontend-development-guidelines.md
cat docs/frontend/frontend-product-ux.md
cat docs/frontend/frontend-engineering.md
cat docs/frontend/frontend-quality.md
cat docs/backend/agent-harness-backend-development-guidelines.md
cat docs/backend/backend-runtime.md
cat docs/backend/backend-engineering.md
cat docs/backend/backend-quality.md
```

If terminal output is truncated, continue reading the same file in ranges until EOF. Do not rely on summaries or first-page excerpts.

Expected: executor has read each file completely and can state the timeline/projection boundary, Rust policy authority, IPC boundary, frontend state boundary, and test gates.

**Step 4: Create local execution log**

Run:

```bash
mkdir -p .codex/task-log
touch .codex/task-log/timeline-render-blocks.md
git status --short
```

Expected: `.codex/task-log/timeline-render-blocks.md` may appear as untracked. It must not be staged or committed.

**Step 5: Baseline tests**

Run:

```bash
pnpm check:quick
```

Expected: PASS. If baseline fails, stop and report. Do not start feature edits on a failing baseline.

**Step 6: Completion analysis**

Write the required task completion block.

**Step 7: Subagent audit**

Run the required read-only subagent audit for Task 0.

Expected: PASS.

**Step 8: Commit**

No commit if no files changed.

## Task 1: Contract Tests For Timeline Projection Extensions

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/conversation.rs`
- Modify: `crates/jyowo-harness-contracts/tests/core_contracts.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`

**Step 1: Task intent analysis**

Write the required task intent block.

**Step 2: Write failing Rust contract tests**

Add tests that prove:

- `AssistantWork` accepts optional `startedAt`, `endedAt`, and `durationMs`.
- `ProcessStepDetail::Activity` accepts optional `items`.
- `ProcessActivityItem.label` and `detail` use `UiSafeText`.
- JSON schema export includes new fields.

Expected failing commands:

```bash
CARGO_TARGET_DIR=target cargo test -p jyowo-harness-contracts activity_items --test core_contracts
CARGO_TARGET_DIR=target cargo test -p jyowo-harness-contracts assistant_work_runtime_metadata --test core_contracts
```

Expected: FAIL because fields/types do not exist.

**Step 3: Write failing frontend Zod tests**

In `apps/desktop/src/shared/tauri/commands.test.ts`, add tests that parse a `pageConversationWorktree` response containing:

- assistant runtime metadata
- activity items
- existing segment kinds unchanged

Also add negative tests proving:

- activity items reject unknown extra fields.
- activity item `label` and `detail` reject private absolute paths, unsafe URLs, `.jyowo` paths, and obvious secrets.
- displayed `ToolAttempt` string fields reject unsafe display data before entering frontend state.

Run:

```bash
pnpm -C apps/desktop test src/shared/tauri/commands.test.ts -- pageConversationWorktree
```

Expected: FAIL because schemas do not include the new fields.

**Step 4: Implement Rust contract**

Update `AssistantWork`, `ProcessStepDetail::Activity`, and add `ProcessActivityItem` / `ProcessActivityItemKind`.

Keep serde names camelCase.

Do not remove existing fields.

**Step 5: Implement Zod schema**

Update `commands.ts` with:

```ts
const processActivityItemSchema = z
  .object({
    kind: z.enum(['file', 'search', 'tool', 'command']),
    label: conversationDisplayTextSchema,
    detail: conversationDisplayTextSchema.optional(),
  })
  .strict()
```

Add `items: z.array(processActivityItemSchema).optional()` to activity detail.

Add optional `startedAt`, `endedAt`, `durationMs` to `assistantWorkSchema`.

Tighten displayed tool attempt strings:

```ts
const toolDisplayTextSchema = conversationDisplayTextSchema

toolName: toolDisplayTextSchema
argumentsPreview: toolDisplayTextSchema.optional()
outputSummary: toolDisplayTextSchema.optional()
affectedTargets: z.array(toolDisplayTextSchema).optional()
failureSummary: toolDisplayTextSchema.optional()
```

Keep `toolUseId`, raw ids, and evidence refs as opaque ids. Do not apply display text schemas to ids.

**Step 6: Run tests**

Run:

```bash
CARGO_TARGET_DIR=target cargo test -p jyowo-harness-contracts --test core_contracts
pnpm -C apps/desktop test src/shared/tauri/commands.test.ts -- pageConversationWorktree
pnpm check
```

Expected: PASS.

**Step 7: Completion analysis**

Write the required completion block.

**Step 8: Subagent audit**

Run the required read-only subagent audit for Task 1.

Expected: PASS.

**Step 9: Commit**

```bash
git add crates/jyowo-harness-contracts/src/conversation.rs crates/jyowo-harness-contracts/tests/core_contracts.rs apps/desktop/src/shared/tauri/commands.ts apps/desktop/src/shared/tauri/commands.test.ts
git commit -m "feat: extend conversation worktree evidence contracts"
```

## Task 2: Rust Worktree Projection For Metadata And Activity Items

**Files:**

- Modify: `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify: `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
- Modify only if needed: `crates/jyowo-harness-contracts/src/conversation.rs`

**Step 1: Task intent analysis**

Write the required task intent block.

**Step 2: Write failing projection tests**

Add tests proving:

- `run.started` sets `assistant.startedAt`.
- `run.ended` sets `assistant.endedAt` and `assistant.durationMs`.
- file read/search completed events project safe activity items when safe target labels exist.
- if no safe labels exist, projection keeps count-only activity and does not synthesize unsafe labels.
- file edit with diff still projects `ChangeSet` and does not duplicate raw patch content into text.
- private absolute paths and secrets are not projected as activity item labels.
- raw `path`, `target`, `query`, and raw tool input fields are not used directly as item labels.
- `ToolAttempt.toolName`, `argumentsPreview`, `outputSummary`, `affectedTargets`, and `failureSummary` are redacted/bounded before projection.

Run:

```bash
CARGO_TARGET_DIR=target cargo test -p jyowo-harness-journal --test conversation_worktree_projector assistant_runtime_metadata
CARGO_TARGET_DIR=target cargo test -p jyowo-harness-journal --test conversation_worktree_projector activity_items
```

Expected: FAIL.

**Step 3: Implement runtime metadata projection**

In `project_run_started`, set `started_at`.

In `project_run_ended`, set `ended_at` and `duration_ms` when both timestamps are known.

Use saturating duration conversion. If timestamps are invalid or missing, omit duration.

**Step 4: Implement activity item extraction**

Add a small helper near existing tool projection helpers:

```rust
fn process_activity_items_from_payload(
    event: &ConversationTimelineEvent,
    kind: ProcessStepKind,
) -> Vec<ProcessActivityItem>
```

Allowed sources:

- `tool_affected_targets(&event.payload)`, because it already rejects private absolute paths, URLs, home-relative paths, and unsafe path traversal
- labels returned by `safe_activity_label`
- safe summaries from `safe_summary_field`

Forbidden sources:

- direct raw `string_field` values used as labels without `safe_activity_label`
- raw command output
- raw unredacted tool input
- private absolute paths
- URLs
- secret-like strings

If uncertain, return an empty vector.

**Step 5: Wire items into activity detail**

Update:

- `merged_activity_detail`
- `process_step_detail_for_tool`
- aggregate file read/search projection

Preserve count behavior.

**Step 6: Run tests**

Run:

```bash
CARGO_TARGET_DIR=target cargo test -p jyowo-harness-journal --test conversation_worktree_projector
CARGO_TARGET_DIR=target cargo test -p jyowo-harness-contracts --test core_contracts
pnpm check:rust
```

Expected: PASS.

**Step 7: Completion analysis**

Write the required completion block.

**Step 8: Subagent audit**

Run the required read-only subagent audit for Task 2.

Expected: PASS.

**Step 9: Commit**

```bash
git add crates/jyowo-harness-journal/src/conversation_worktree_projector.rs crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs crates/jyowo-harness-contracts/src/conversation.rs
git commit -m "feat: project timeline activity metadata"
```

Do not include `crates/jyowo-harness-contracts/src/conversation.rs` if it was not modified.

## Task 3: Frontend Timeline Render Block Adapter

**Files:**

- Create: `apps/desktop/src/features/conversation/timeline/timeline-render-blocks.ts`
- Create: `apps/desktop/src/features/conversation/timeline/timeline-render-blocks.test.ts`
- Modify only if needed: `apps/desktop/src/features/conversation/timeline/conversation-timeline-test-utils.tsx`

**Step 1: Task intent analysis**

Write the required task intent block.

**Step 2: Write failing adapter tests**

Tests must cover:

- text segments stay text blocks.
- adjacent file edit and diff process steps become one `fileEdit` block.
- read/search activity steps become one collapsed `activity` block with items.
- command process steps become one `commandGroup` block.
- failed/non-zero commands are marked forced-open.
- artifacts, review, clarification, notice, error, and agent activity preserve order.
- unknown data cannot become a raw event block.
- stable block ids do not change when file edit, activity, or command groups receive appended adjacent work.
- separate later groups get distinct ids.

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/timeline-render-blocks.test.ts
```

Expected: FAIL because file does not exist.

**Step 3: Implement render block types**

Define frontend-only types. Minimum shape:

```ts
export type TimelineRenderBlock =
  | { kind: 'assistantText'; id: string; order: number; segment: TextSegment }
  | {
      kind: 'fileEdit'
      id: string
      order: number
      processSegmentId: string
      steps: ProcessStep[]
      files: FileEditRenderFile[]
      defaultOpen: boolean
      forcedOpen: boolean
    }
  | {
      kind: 'activity'
      id: string
      order: number
      processSegmentId: string
      steps: ProcessStep[]
      title: string
      itemCount?: number
      items: ActivityRenderItem[]
      defaultOpen: boolean
      forcedOpen: boolean
    }
  | {
      kind: 'commandGroup'
      id: string
      order: number
      processSegmentId: string
      steps: ProcessStep[]
      commands: CommandRenderItem[]
      defaultOpen: boolean
      forcedOpen: boolean
    }
  | {
      kind: 'toolGroup'
      id: string
      order: number
      segment: ToolGroupSegment
      defaultOpen: boolean
      forcedOpen: boolean
    }
  | { kind: 'artifact'; id: string; order: number; segment: ArtifactSegment }
  | { kind: 'reviewRequest'; id: string; order: number; segment: ReviewRequestSegment }
  | { kind: 'clarificationRequest'; id: string; order: number; segment: ClarificationRequestSegment }
  | { kind: 'notice'; id: string; order: number; segment: NoticeSegment }
  | { kind: 'error'; id: string; order: number; segment: ErrorSegment }
  | { kind: 'agentActivity'; id: string; order: number; segment: AgentActivitySegment }
```

Keep all types local to timeline. Do not export them from `shared/tauri`.

**Step 4: Implement `buildTimelineRenderBlocks`**

Signature:

```ts
export function buildTimelineRenderBlocks(assistant: AssistantWork): TimelineRenderBlock[]
```

Rules:

- Sort by segment order.
- Inside process segments, sort steps by step order.
- Group only within the same process segment.
- Preserve visible sequence.
- Do not drop unsupported known segment kinds.
- Use stable ids from projected ids.

**Step 5: Implement disclosure policy**

Add:

```ts
export function getDefaultRenderBlockDisclosure(block: TimelineRenderBlock): {
  defaultOpen: boolean
  forcedOpen: boolean
}
```

Rules match this plan's disclosure section.

**Step 6: Run tests**

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/timeline-render-blocks.test.ts
pnpm -C apps/desktop typecheck
pnpm -C apps/desktop lint
```

Expected: PASS.

**Step 7: Completion analysis**

Write the required completion block.

**Step 8: Subagent audit**

Run the required read-only subagent audit for Task 3.

Expected: PASS.

**Step 9: Commit**

```bash
git add apps/desktop/src/features/conversation/timeline/timeline-render-blocks.ts apps/desktop/src/features/conversation/timeline/timeline-render-blocks.test.ts apps/desktop/src/features/conversation/timeline/conversation-timeline-test-utils.tsx
git commit -m "feat: add timeline render block adapter"
```

Do not include the test utils file if it was not modified.

## Task 4: Shared Timeline EvidenceDisclosure Component

**Files:**

- Create: `apps/desktop/src/features/conversation/timeline/evidence-disclosure.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/evidence-disclosure.test.tsx`
- Modify only if needed: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify only if needed: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`

**Step 1: Task intent analysis**

Write the required task intent block.

**Step 2: Write failing component tests**

Cover:

- renders icon, title, metadata, chevron, and body.
- toggles with `aria-expanded`.
- forced-open blocks cannot collapse.
- action slot renders copy/open buttons.
- long text truncates without layout shift.
- keyboard activation works through native button behavior.

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/evidence-disclosure.test.tsx
```

Expected: FAIL.

**Step 3: Implement component**

Use a button header, not `<details>`.

Required props:

```ts
type EvidenceDisclosureProps = {
  id: string
  icon: LucideIcon
  title: ReactNode
  meta?: ReactNode
  open: boolean
  forcedOpen?: boolean
  onOpenChange?: (open: boolean) => void
  actions?: ReactNode
  children: ReactNode
}
```

Styling:

- compact rounded surface
- no nested card shell
- semantic tokens only
- stable body width
- no viewport-scaled font sizes
- no text overlap

**Step 4: Run tests**

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/evidence-disclosure.test.tsx
pnpm -C apps/desktop typecheck
pnpm -C apps/desktop lint
```

Expected: PASS.

**Step 5: Completion analysis**

Write the required completion block.

**Step 6: Subagent audit**

Run the required read-only subagent audit for Task 4.

Expected: PASS.

**Step 7: Commit**

```bash
git add apps/desktop/src/features/conversation/timeline/evidence-disclosure.tsx apps/desktop/src/features/conversation/timeline/evidence-disclosure.test.tsx apps/desktop/src/shared/i18n/locales/en-US.ts apps/desktop/src/shared/i18n/locales/zh-CN.ts
git commit -m "feat: add timeline evidence disclosure shell"
```

Do not include locale files if they were not modified.

## Task 5: Specialized Timeline Block Renderers

**Files:**

- Create: `apps/desktop/src/features/conversation/timeline/timeline-block-renderer.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/file-edit-render-block.tsx`
- Create: `apps/desktop/src/features/conversation/timeline/activity-render-block.tsx`
- Create or modify: `apps/desktop/src/features/conversation/timeline/command-render-block.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/diff-evidence-block.tsx`
- Modify: `apps/desktop/src/features/conversation/evidence/CommandExecutionView.tsx`
- Delete if made obsolete: `apps/desktop/src/features/conversation/timeline/command-evidence-block.tsx`
- Test: `apps/desktop/src/features/conversation/timeline/conversation-timeline.large-output.test.tsx`
- Test: `apps/desktop/src/features/conversation/timeline/command-evidence-block.test.tsx` or replacement test

**Step 1: Task intent analysis**

Write the required task intent block.

**Step 2: Write failing renderer tests**

Add/modify tests for:

- collapsed file edit summary matches `已编辑 1 个文件`.
- expanded file edit shows file rows and diff preview.
- file edit file rows show `filename +N -M`.
- read/search activity collapsed summary shows counts.
- expanded read/search activity shows item labels.
- command group collapsed summary shows command list.
- expanded command group shows shell blocks.
- command failure/non-zero defaults open.
- full output is not fetched in main timeline.
- inspector/full-output path remains available.

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.large-output.test.tsx
pnpm -C apps/desktop test src/features/conversation/timeline/command-evidence-block.test.tsx
```

Expected: FAIL.

**Step 3: Refactor command rendering**

Unify command rendering around `features/conversation/evidence/CommandExecutionView.tsx`.

Target API:

```ts
type CommandExecutionViewProps = {
  command: CommandExecution
  conversationId: string
  density?: 'timeline' | 'inspector'
  allowFullOutputFetch?: boolean
}
```

Rules:

- Timeline passes `allowFullOutputFetch={false}`.
- Inspector may pass `allowFullOutputFetch={true}`.
- No fake fetch button appears when disabled.
- Copy command/output still works for visible text.

Remove old duplicate command evidence component if it no longer owns behavior.

**Step 4: Implement file edit renderer**

Use `EvidenceDisclosure`.

Render:

- header summary
- collapsed file rows
- expanded "edited files" label
- `DiffEvidenceBlock` for previews
- inspector action for full diff when refs exist

**Step 5: Implement activity renderer**

Use `EvidenceDisclosure`.

Render:

- compact summary
- item list when open
- fallback summary when no items exist

Do not display raw file contents.

**Step 6: Implement command group renderer**

Use `EvidenceDisclosure`.

Render:

- command rows in collapsed state
- `CommandExecutionView` blocks in expanded state
- status and duration from `CommandExecution`

**Step 7: Implement renderer registry**

`timeline-block-renderer.tsx` exports:

```ts
export function TimelineBlockRenderer(props: {
  block: TimelineRenderBlock
  conversationId: string
  runId: string
  turnId: string
  onOpenDetails?: (eventRef: ConversationEventRef) => void
  onPermissionResolve?: (request: ResolvePermissionRequest) => void
  onReviewContinue?: (prompt: string) => void
  artifactRevisionIdsByArtifactId?: Record<string, string>
  processImageArtifactIds?: Set<string>
})
```

Use exhaustive switch. If adding `assertNever`, place it in local utility or existing shared utility.

**Step 8: Run tests**

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.large-output.test.tsx
pnpm -C apps/desktop test src/features/conversation/timeline/command-evidence-block.test.tsx
pnpm -C apps/desktop test src/features/conversation/evidence/CommandExecutionView.test.tsx
pnpm -C apps/desktop typecheck
pnpm -C apps/desktop lint
```

Expected: PASS.

**Step 9: Completion analysis**

Write the required completion block.

**Step 10: Subagent audit**

Run the required read-only subagent audit for Task 5.

Expected: PASS.

**Step 11: Commit**

```bash
git add apps/desktop/src/features/conversation/timeline apps/desktop/src/features/conversation/evidence/CommandExecutionView.tsx apps/desktop/src/features/conversation/evidence/CommandExecutionView.test.tsx
git commit -m "feat: render timeline evidence blocks"
```

Review staged files before commit. Do not stage unrelated timeline files.

## Task 6: Integrate Render Blocks Into AssistantWorkView

**Files:**

- Modify: `apps/desktop/src/features/conversation/timeline/assistant-work-view.tsx`
- Modify or delete if obsolete: `apps/desktop/src/features/conversation/timeline/process-panel.tsx`
- Modify or delete if obsolete: `apps/desktop/src/features/conversation/timeline/process-step-row.tsx`
- Modify or delete if obsolete: `apps/desktop/src/features/conversation/timeline/process-status-row.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.render.test.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.large-output.test.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx`

**Step 1: Task intent analysis**

Write the required task intent block.

**Step 2: Write failing integration tests**

Tests must prove:

- `AssistantWorkView` uses `buildTimelineRenderBlocks`.
- segment order is preserved after grouping.
- assistant duration row appears only when `durationMs` exists.
- text blocks remain normal markdown.
- evidence blocks are collapsible.
- failed/running blocks are forced visible.
- no raw process rows leak after refactor.
- process image artifact preview still suppresses duplicate ready image artifact segment rendering.
- completed tool preparation steps already covered by a `ToolGroupSegment` do not render as duplicate timeline work.
- permission UI remains nested under the owning tool attempt and is not moved into generic process rows.

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.render.test.tsx
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.large-output.test.tsx
```

Expected: FAIL.

**Step 3: Integrate adapter**

In `AssistantWorkView`:

- call `buildTimelineRenderBlocks(assistant)`
- render each block through `TimelineBlockRenderer`
- keep model label and assistant status header
- add duration display if `assistant.durationMs` exists
- preserve `getProcessImageArtifactIds` behavior or move it into the adapter with identical tests
- preserve covered tool preparation suppression or move it into the adapter with identical tests

Do not keep the old direct `segment.kind` switch unless it only delegates to block rendering for unsupported transition. Remove obsolete direct rendering paths when tests pass.

**Step 4: Remove orphan components**

If `ProcessPanel`, `ProcessStepRow`, or `ProcessStatusRow` become unused, delete them and update tests/stories.

If a component remains used, document why in completion analysis.

Run:

```bash
pnpm -C apps/desktop knip
```

Expected: no new unused exports/files.

**Step 5: Update Storybook**

Ensure stories cover:

- collapsed file edit
- expanded file edit
- collapsed read/search
- expanded read/search
- collapsed successful command group
- expanded failed command
- permission-pending tool attempt
- completed run with failed evidence

Use contract-shaped fixture values only. Do not create fake runtime behavior.

**Step 6: Run tests**

Run:

```bash
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.render.test.tsx
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.large-output.test.tsx
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.permission.test.tsx
pnpm -C apps/desktop knip
pnpm check:desktop
```

Expected: PASS.

**Step 7: Completion analysis**

Write the required completion block.

**Step 8: Subagent audit**

Run the required read-only subagent audit for Task 6.

Expected: PASS.

**Step 9: Commit**

```bash
git add apps/desktop/src/features/conversation/timeline
git commit -m "refactor: route assistant work through render blocks"
```

## Task 7: Inspector And Evidence Ref Boundaries

**Files:**

- Modify: `apps/desktop/src/features/workbench/WorkbenchInspector.tsx`
- Modify: `apps/desktop/src/features/conversation/evidence/DiffPane.tsx`
- Modify: `apps/desktop/src/features/conversation/evidence/CommandExecutionView.tsx`
- Modify: `apps/desktop/src/shared/state/workbench-selection.ts`
- Test: `apps/desktop/src/features/workbench/WorkbenchInspector.test.tsx`
- Test: `apps/desktop/src/features/conversation/evidence/DiffPane.test.tsx`
- Test: `apps/desktop/src/features/conversation/evidence/CommandExecutionView.test.tsx`

**Step 1: Task intent analysis**

Write the required task intent block.

**Step 2: Write failing boundary tests**

Tests must prove:

- timeline command preview does not fetch full output.
- inspector command view can fetch full output by `fullOutputRef`.
- timeline diff preview does not fetch full patch.
- inspector diff view can fetch full patch by `fullPatchRef` / change set id.
- redaction withheld state renders without unsafe output.
- clicking open inspector sets `WorkbenchSelection` with opaque refs only.

Run:

```bash
pnpm -C apps/desktop test src/features/workbench/WorkbenchInspector.test.tsx
pnpm -C apps/desktop test src/features/conversation/evidence/CommandExecutionView.test.tsx
pnpm -C apps/desktop test src/features/conversation/evidence/DiffPane.test.tsx
```

Expected: FAIL if boundaries are not explicit.

**Step 3: Implement boundary fixes**

Rules:

- Full output fetch only in inspector or explicit evidence view.
- Full patch fetch only in inspector or explicit diff pane.
- Timeline passes opaque selection values into `setWorkbenchSelection`.
- No production fake handlers.

Required API boundary:

```ts
type DiffPaneProps = {
  conversationId: string
  files: ChangeSetFile[]
  allowFullPatchFetch?: boolean
}
```

Rules:

- Timeline never renders `DiffPane`.
- Inspector passes `allowFullPatchFetch={true}`.
- If `allowFullPatchFetch` is false or omitted, fetch/copy-full-patch controls must not render and no `getConversationDiffPatch` call can occur.
- `CommandExecutionView` follows the same rule with `allowFullOutputFetch`.

**Step 4: Run tests**

Run:

```bash
pnpm -C apps/desktop test src/features/workbench/WorkbenchInspector.test.tsx
pnpm -C apps/desktop test src/features/conversation/evidence/CommandExecutionView.test.tsx
pnpm -C apps/desktop test src/features/conversation/evidence/DiffPane.test.tsx
pnpm check:desktop
```

Expected: PASS.

**Step 5: Completion analysis**

Write the required completion block.

**Step 6: Subagent audit**

Run the required read-only subagent audit for Task 7.

Expected: PASS.

**Step 7: Commit**

```bash
git add apps/desktop/src/features/workbench apps/desktop/src/features/conversation/evidence apps/desktop/src/shared/state/workbench-selection.ts
git commit -m "fix: preserve timeline evidence ref boundaries"
```

## Task 8: End-To-End Regression Matrix For Projection And Rendering

**Files:**

- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.large-output.test.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx`
- Modify: `apps/desktop/src/testing/conversation-evidence-fixtures.ts`
- Modify: `crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs`
- Modify if needed: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify if needed: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`

**Step 1: Task intent analysis**

Write the required task intent block.

**Step 2: Add regression scenarios**

Coverage must include:

- file edit collapsed/expanded
- multiple file edit rows
- read/search collapsed/expanded with activity items
- read/search count-only fallback
- successful command collapsed
- failed command expanded
- running command expanded
- large command output preview
- large diff preview
- withheld command output
- completed run with failed evidence
- assistant duration present
- assistant duration absent
- zh-CN and en-US labels

Do not add fake runtime behavior. Use minimal typed fixtures for component rendering and Rust event/projection tests for runtime semantics.

**Step 3: Run focused tests**

Run:

```bash
CARGO_TARGET_DIR=target cargo test -p jyowo-harness-journal --test conversation_worktree_projector
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.large-output.test.tsx
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.render.test.tsx
pnpm -C apps/desktop build-storybook
pnpm check
```

Expected: PASS.

**Step 4: Completion analysis**

Write the required completion block.

**Step 5: Subagent audit**

Run the required read-only subagent audit for Task 8.

Expected: PASS.

**Step 6: Commit**

```bash
git add apps/desktop/src/features/conversation/timeline apps/desktop/src/testing/conversation-evidence-fixtures.ts crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs apps/desktop/src/shared/i18n/locales/en-US.ts apps/desktop/src/shared/i18n/locales/zh-CN.ts
git commit -m "test: cover timeline render block regressions"
```

Do not include locale files if they were not modified.

## Task 9: Documentation And Architecture Guardrails

**Files:**

- Modify: `docs/frontend/frontend-product-ux.md`
- Modify: `docs/frontend/frontend-engineering.md`
- Modify: `docs/frontend/frontend-quality.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`
- Modify if needed: `docs/testing/testing-strategy.md`
- Modify if needed: `docs/backend/backend-runtime.md`

**Step 1: Task intent analysis**

Write the required task intent block.

**Step 2: Update frontend docs**

Document:

- main canvas still renders `ConversationTurn[]`.
- React converts `AssistantWork` to frontend-only `TimelineRenderBlock[]`.
- renderer registry is the extension point.
- new display types require typed Rust projection, Zod schema, renderer, tests, and Storybook.
- raw `RunEvent`, chain-of-thought, raw output, and full patches do not drive main canvas rendering.
- evidence refs and inspector boundaries.

Do not turn docs into temporary progress notes.

**Step 3: Update quality docs**

Add required coverage:

- render block adapter tests
- evidence disclosure tests
- file edit/read-search/command group collapsed and expanded states
- projection tests for activity metadata
- no raw event renderer regressions

**Step 4: Update backend docs**

Document:

- Rust projection remains the authority for timeline UI-safe typed data.
- `ProcessStepDetail`, `AssistantWork`, and other public conversation contract changes require serde shape tests, schema/Zod updates, projection tests, and redaction coverage.
- raw payloads, raw event JSON, command output, full patches, and chain-of-thought must not drive frontend rendering.
- evidence refs are opaque UI-safe references; full output and full patch access stays behind inspector or explicit evidence fetch boundaries.
- `docs/backend/backend-runtime.md` must be updated only if implementation changes runtime ownership, replay, journal, redaction, or projection boundaries.

Do not add backend docs if the final implementation did not affect backend contract, projection, runtime, or safety boundaries. If backend docs are not modified, record the reason in the task completion analysis.

**Step 5: Run docs gate**

Run:

```bash
pnpm check:docs
pnpm check:frontend-docs
pnpm check:backend-docs
pnpm check:testing-docs
```

Expected: PASS.

**Step 6: Completion analysis**

Write the required completion block.

**Step 7: Subagent audit**

Run the required read-only subagent audit for Task 9.

Expected: PASS.

**Step 8: Commit**

```bash
git add docs/frontend/frontend-product-ux.md docs/frontend/frontend-engineering.md docs/frontend/frontend-quality.md docs/backend/backend-engineering.md docs/backend/backend-quality.md docs/testing/testing-strategy.md docs/backend/backend-runtime.md
git commit -m "docs: define timeline render block architecture"
```

Do not include `docs/testing/testing-strategy.md` if it was not modified.
Do not include `docs/backend/backend-runtime.md` if it was not modified.

## Task 10: Full Gates And Final Audit

**Files:**

- No planned source edits

**Step 1: Task intent analysis**

Write the required task intent block.

**Step 2: Run required focused gates**

Run:

```bash
CARGO_TARGET_DIR=target cargo test -p jyowo-harness-contracts --test core_contracts
CARGO_TARGET_DIR=target cargo test -p jyowo-harness-journal --test conversation_worktree_projector
pnpm -C apps/desktop test src/shared/tauri/commands.test.ts -- pageConversationWorktree
pnpm -C apps/desktop test src/features/conversation/timeline/timeline-render-blocks.test.ts
pnpm -C apps/desktop test src/features/conversation/timeline/evidence-disclosure.test.tsx
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.large-output.test.tsx
pnpm -C apps/desktop test src/features/conversation/timeline/conversation-timeline.render.test.tsx
pnpm -C apps/desktop test src/features/conversation/evidence/CommandExecutionView.test.tsx
pnpm -C apps/desktop test src/features/workbench/WorkbenchInspector.test.tsx
```

Expected: PASS.

**Step 3: Run project gates**

Run:

```bash
pnpm check:desktop
pnpm check:rust
pnpm check:docs
pnpm check:test-architecture
pnpm audit:tests
pnpm check:quick
```

Expected: PASS.

If time allows and machine resources are available, run:

```bash
pnpm check
```

Expected: PASS.

If `pnpm check` cannot run, record why and include all focused gate results.

**Step 4: Review diff**

Run:

```bash
git status --short
git diff --stat main...HEAD
git diff --check
```

Expected:

- no whitespace errors
- no unrelated files
- no generated noise unless expected by `pnpm audit:tests`

**Step 5: Final read-only subagent audit**

Run a full read-only subagent audit:

```text
Audit the complete implementation of
docs/plans/2026-07-06-conversation-timeline-render-block-architecture.md.

Return PASS or FAIL.
Check:
- every task was completed
- `.codex/task-log/timeline-render-blocks.md` contains every task's intent, completion notes, and subagent PASS
- `.codex/task-log/timeline-render-blocks.md` was not staged or committed
- main canvas still renders ConversationTurn[]
- no raw RunEvent renderer was introduced
- Rust projection owns all durable facts and safety decisions
- frontend render blocks are internal only
- evidence disclosure state is stable and UI-only
- command/diff full content stays behind evidence refs and inspector
- no production mocks/fakes/placeholders
- tests and docs gates passed
- no orphan compatibility wrappers remain
```

Expected: PASS.

**Step 6: Completion analysis**

Write the required completion block.

**Step 7: Final commit if needed**

If `pnpm audit:tests` or docs updates changed files:

```bash
git add <changed-files>
git commit -m "chore: finalize timeline render block gates"
```

Otherwise no commit.

## Pull Request Requirements

PR description must include:

- summary of contract changes
- summary of Rust projection changes
- summary of frontend render block architecture
- screenshots or Storybook references for collapsed/expanded states
- exact gate commands and results
- list of subagent audits, one per task
- task log summary copied from `.codex/task-log/timeline-render-blocks.md`
- explicit statement that no production mocks/fakes/placeholders were added
- explicit statement that main canvas still renders `ConversationTurn[]`

## Acceptance Checklist

- [ ] Implementation was done in isolated worktree, not `main`.
- [ ] Executor used `chatgpt5.5 xhigh`, or stopped and reported inability.
- [ ] Every task started with task intent analysis.
- [ ] Every task ended with completion analysis.
- [ ] Every task had read-only subagent audit PASS.
- [ ] `.codex/task-log/timeline-render-blocks.md` was used for execution notes and was not committed.
- [ ] Rust contract and projection tests pass.
- [ ] Frontend Zod schema tests pass.
- [ ] Render block adapter tests pass.
- [ ] Evidence disclosure tests pass.
- [ ] Conversation timeline render tests pass.
- [ ] Command and diff inspector boundary tests pass.
- [ ] Storybook builds.
- [ ] `pnpm check:desktop` passes.
- [ ] `pnpm check:rust` passes.
- [ ] `pnpm check:docs` passes.
- [ ] `pnpm check:test-architecture` passes.
- [ ] `pnpm audit:tests` completed and intentional changes were committed.
- [ ] `pnpm check:quick` passes.
- [ ] No raw `RunEvent` rendering in the main conversation canvas.
- [ ] No raw chain-of-thought enters frontend state.
- [ ] No raw secret, private absolute path, or unsafe tool payload enters frontend state.
- [ ] No production mock data, fake implementation, placeholder runtime, or hard-coded success path.
- [ ] No orphan compatibility wrappers or unused components remain.
