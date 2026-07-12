# Task Timeline Semantic Projection Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make daemon engine output appear as a correct, live, grouped task conversation while keeping internal engine events in Audit and preventing incompatible workbench selections.

**Architecture:** The journal owns the canonical typed event-to-timeline projection. A pure TypeScript reducer mirrors that semantic mapping for committed live envelopes after the snapshot boundary and derives the visible task state, queue, and timeline. Rendering groups assistant chunks by message identity; raw envelopes remain untouched for Audit.

**Tech Stack:** Rust, serde/schemars, SQLite journal projections, TypeScript 6, React 19, Zustand, Vitest, React Testing Library, Playwright, i18next.

---

### Task 1: Add semantic grouping to the daemon contract

**Files:**
- Modify: `crates/jyowo-harness-contracts/src/daemon.rs`
- Modify: `crates/jyowo-harness-journal/src/task_projection.rs`
- Modify: `crates/jyowo-harness-journal/src/task_projection_tests.rs`
- Generate: `apps/desktop/src/generated/daemon-protocol.schema.json`
- Generate: `apps/desktop/src/generated/daemon-protocol.ts`

**Step 1: Write the failing contract/projection test**

Add an assertion that an assistant timeline item serializes `semanticGroupId` while ordinary timeline items omit it. Update existing fixture constructors to initialize the new optional field.

**Step 2: Run the focused Rust test and verify RED**

Run:

```bash
cargo test -p jyowo-harness-contracts -p jyowo-harness-journal -p jyowo-harness-daemon -p jyowo-desktop-shell task_projection
```

Expected: compilation or assertion failure because `TimelineItemProjection` has no semantic group field.

**Step 3: Implement the contract**

Add this field to `TimelineItemProjection`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub semantic_group_id: Option<String>,
```

Initialize it to `None` in existing projection paths.

**Step 4: Regenerate protocol types**

Run:

```bash
pnpm generate:daemon-protocol
```

Expected: generated schema and TypeScript include `semanticGroupId?: string`.

**Step 5: Run focused tests and verify GREEN**

Run the Rust command from Step 2 plus:

```bash
pnpm check:daemon-protocol
```

**Step 6: Commit**

```bash
git add crates/jyowo-harness-contracts/src/daemon.rs crates/jyowo-harness-journal/src/task_projection.rs crates/jyowo-harness-journal/src/task_projection_tests.rs apps/desktop/src/generated/daemon-protocol.schema.json apps/desktop/src/generated/daemon-protocol.ts
git commit -m "feat: add timeline semantic group identity"
```

### Task 2: Project typed engine assistant events in Rust

**Files:**
- Modify: `crates/jyowo-harness-journal/src/task_projection.rs`
- Modify: `crates/jyowo-harness-journal/src/task_projection_tests.rs`

**Step 1: Write failing projection tests**

Create real `TaskEvent::Engine` envelopes containing:

- two `Event::AssistantDeltaProduced` text chunks for one message;
- `Event::AssistantMessageCompleted` for the same message;
- completion without prior text chunks;
- lifecycle/accounting events such as `RunStarted`, `RunEnded`, and usage-bearing completion metadata.

Assert that text chunks become `assistant_text` with the mapped task run segment and message semantic group; completion does not append duplicate text; completion-only creates one final row; internal events create no primary timeline rows; rebuild output matches incremental output.

**Step 2: Run the focused tests and verify RED**

Run the combined Rust projection command from Task 1. Expected: engine events are still projected as generic notices.

**Step 3: Implement typed semantic projection**

Replace the generic `TaskEvent::Engine { event_type, .. }` arm with a helper matching `payload.event` by typed `Event` variant. Extract visible text only from `DeltaChunk::Text` and final `MessageContent` text. Resolve engine run IDs to task run segment IDs from committed projection state. Return `None` for lifecycle, duplicate user-message, accounting, heartbeat, and unsupported internal events.

For assistant completion, query existing timeline projection rows for the same semantic group. If text chunks exist, mark the last chunk complete without appending the final body. If none exist, append the final body once.

**Step 4: Run focused tests and verify GREEN**

Run the combined Rust projection command from Task 1.

**Step 5: Commit**

```bash
git add crates/jyowo-harness-journal/src/task_projection.rs crates/jyowo-harness-journal/src/task_projection_tests.rs
git commit -m "fix: project engine assistant output semantically"
```

### Task 3: Build a pure live task projection reducer

**Files:**
- Create: `apps/desktop/src/features/tasks/task-live-projection.ts`
- Create: `apps/desktop/src/features/tasks/task-live-projection.test.ts`
- Modify: `apps/desktop/src/features/tasks/TaskWorkspace.tsx`
- Modify: `apps/desktop/src/features/tasks/TaskWorkspace.test.tsx`

**Step 1: Write failing reducer tests**

Define a pure `deriveLiveTaskSnapshot(snapshot, events)` API. Feed it real daemon envelope JSON shapes for task/run/queue events and engine assistant delta/completion events. Assert:

- only contiguous events after `snapshotOffset` are applied;
- replayed/boundary events are ignored;
- task state, current run, queue, permission and subagents update from live envelopes;
- assistant text has run/message identities;
- completion coalesces without duplication;
- completion-only remains visible;
- internal/unknown engine events stay absent from the visible timeline.

**Step 2: Run reducer tests and verify RED**

Run:

```bash
pnpm -C apps/desktop test src/features/tasks/task-live-projection.test.ts
```

Expected: module/API missing.

**Step 3: Implement the minimal pure reducer**

Move queue and timeline event mapping out of `TaskWorkspace.tsx`. Clone the snapshot projection, apply ordered contiguous envelopes, and return one derived snapshot/view. Mirror the Rust assistant mapping and semantic grouping. Never create a fallback row for unknown `engine.*` events.

**Step 4: Run reducer tests and verify GREEN**

Run the command from Step 2.

**Step 5: Integrate TaskWorkspace**

Replace component-local `timelineItems`, `queueItems`, and direct stale snapshot reads with the derived live view. Pass raw envelopes unchanged to Audit.

**Step 6: Run workspace tests**

```bash
pnpm -C apps/desktop test src/features/tasks/task-live-projection.test.ts src/features/tasks/TaskWorkspace.test.tsx src/features/tasks/task-store.test.ts
```

Expected: all pass and no console warnings.

**Step 7: Commit**

```bash
git add apps/desktop/src/features/tasks/task-live-projection.ts apps/desktop/src/features/tasks/task-live-projection.test.ts apps/desktop/src/features/tasks/TaskWorkspace.tsx apps/desktop/src/features/tasks/TaskWorkspace.test.tsx
git commit -m "fix: derive live task state from committed events"
```

### Task 4: Preserve assistant message boundaries in rendering

**Files:**
- Modify: `apps/desktop/src/features/tasks/timeline/RunSegment.tsx`
- Modify: `apps/desktop/src/features/tasks/timeline/TaskTimeline.test.tsx`

**Step 1: Write failing rendering tests**

Render adjacent assistant chunks from two semantic group IDs and assert two narrative containers. Render multiple chunks from one group and assert one narrative container with concatenated visible text. Verify incomplete state is present only while the group is streaming.

**Step 2: Run timeline tests and verify RED**

```bash
pnpm -C apps/desktop test src/features/tasks/timeline/TaskTimeline.test.tsx
```

Expected: adjacent messages collapse into one narrative.

**Step 3: Implement group-aware rendering**

Group consecutive assistant rows only while both `runSegmentId` and `semanticGroupId` match. Preserve the existing item identities for selection and scroll updates.

**Step 4: Run timeline tests and verify GREEN**

Run the command from Step 2.

**Step 5: Commit**

```bash
git add apps/desktop/src/features/tasks/timeline/RunSegment.tsx apps/desktop/src/features/tasks/timeline/TaskTimeline.test.tsx
git commit -m "fix: preserve streamed assistant message boundaries"
```

### Task 5: Enforce workbench panel compatibility

**Files:**
- Modify: `apps/desktop/src/features/tasks/workbench/TaskWorkbench.tsx`
- Modify: `apps/desktop/src/features/tasks/workbench/TaskWorkbench.test.tsx`

**Step 1: Write failing selection tests**

Open a command event, switch to Changes, Sources, and Audit, and assert incompatible `eventId`, `segmentId`, and `blobId` values are cleared. Assert a compatible selection remains when reopening the same panel.

**Step 2: Run workbench tests and verify RED**

```bash
pnpm -C apps/desktop test src/features/tasks/workbench/TaskWorkbench.test.tsx
```

Expected: unrelated event identity remains selected.

**Step 3: Implement panel-scoped selection**

Centralize compatibility rules by panel. On tab changes, retain only fields valid for the destination panel. Use task identity only as the stable fallback; never substitute it as an event ID.

**Step 4: Run workbench tests and verify GREEN**

Run the command from Step 2.

**Step 5: Commit**

```bash
git add apps/desktop/src/features/tasks/workbench/TaskWorkbench.tsx apps/desktop/src/features/tasks/workbench/TaskWorkbench.test.tsx
git commit -m "fix: scope workbench selection to active panel"
```

### Task 6: Localize remaining task chrome

**Files:**
- Modify: `apps/desktop/src/features/tasks/TaskWorkspace.tsx`
- Modify: `apps/desktop/src/features/tasks/RunStatusBar.tsx`
- Modify: `apps/desktop/src/features/tasks/timeline/RunSegment.tsx`
- Modify: `apps/desktop/src/features/tasks/timeline/TimelineEvent.tsx`
- Modify: `apps/desktop/src/features/tasks/workbench/TaskWorkbench.tsx`
- Modify: `apps/desktop/src/locales/en/tasks.json`
- Modify: `apps/desktop/src/locales/zh-CN/tasks.json`
- Modify: relevant component tests

**Step 1: Write failing Chinese locale tests**

Render the task workspace and workbench under `zh-CN`. Assert visible connection, run, timeline, workbench, empty-state, button, and tab labels contain the expected Chinese resources and no known hard-coded English chrome.

**Step 2: Run task component tests and verify RED**

```bash
pnpm -C apps/desktop test src/features/tasks/TaskWorkspace.test.tsx src/features/tasks/RunStatusBar.test.tsx src/features/tasks/timeline/TaskTimeline.test.tsx src/features/tasks/workbench/TaskWorkbench.test.tsx
```

**Step 3: Add resources and wire translations**

Use the existing `tasks` namespace. Keep protocol event types and IDs untranslated inside Audit.

**Step 4: Run task component tests and verify GREEN**

Run the command from Step 2.

**Step 5: Commit**

```bash
git add apps/desktop/src/features/tasks apps/desktop/src/locales/en/tasks.json apps/desktop/src/locales/zh-CN/tasks.json
git commit -m "fix: localize task timeline and workbench chrome"
```

### Task 7: Add real daemon-to-UI regression coverage

**Files:**
- Modify: `apps/desktop/e2e/task-daemon-recovery.spec.ts` or create a focused sibling spec
- Modify: existing daemon fixture/harness files used by the spec

**Step 1: Write the failing browser regression**

Start the real daemon fixture, create/open a task, send a message, and wait for engine-produced assistant output. Assert the final response text is visible once, the task reaches its terminal state, internal engine lifecycle notices are absent from the primary timeline but visible in Audit, and no page/console errors occur.

**Step 2: Run the focused Playwright test and verify RED**

```bash
pnpm -C apps/desktop test:e2e --grep "projects engine response into task timeline"
```

Expected: response text is absent and generic engine notices appear.

**Step 3: Add only the fixture support required by the test**

Keep the test on the production protocol path. Do not hand-author an `assistant_text` timeline snapshot.

**Step 4: Run the focused Playwright test and verify GREEN**

Run the command from Step 2.

**Step 5: Commit**

```bash
git add apps/desktop/e2e
git commit -m "test: cover engine task response projection"
```

### Task 8: Full verification, review, and local integration

**Files:**
- Review all changed files

**Step 1: Run formatters**

```bash
cargo fmt --all
pnpm -C apps/desktop format
```

**Step 2: Run generated contract, frontend, Rust, and browser checks**

```bash
pnpm check:daemon-protocol
pnpm check:frontend:fast
cargo test -p jyowo-harness-contracts -p jyowo-harness-journal -p jyowo-harness-daemon -p jyowo-desktop-shell task_projection
pnpm -C apps/desktop test:e2e --grep "projects engine response into task timeline"
```

**Step 3: Inspect the diff for technical debt**

Verify there is one Rust semantic mapper, one frontend live mapper, no generic engine-notice fallback, no stale direct snapshot state in the workspace, no incompatible workbench retention, no debug logging, and no unrelated edits.

**Step 4: Commit any verified cleanup**

```bash
git add -u
git commit -m "chore: finalize task timeline projection"
```

Skip the commit if the tree is clean.

**Step 5: Merge and clean up as authorized**

From the main worktree, merge `goya/fix-task-timeline-projection` into `main`, rerun the relevant verification on the merged tree, remove `.worktrees/fix-task-timeline-projection`, and delete the feature branch.
