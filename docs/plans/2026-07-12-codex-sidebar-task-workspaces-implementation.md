# Codex-style Task Workspaces Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix message execution by acquiring a durable foreground workspace lease, then ship a Codex-style sidebar with default-workspace conversations, project conversations, pinning, folding, renaming, archiving, and removal.

**Architecture:** Keep daemon task projections as the source of truth for conversations. Extend the event-sourced task projection with pinned and removed metadata, add task metadata commands to the daemon protocol, and acquire a current-workspace lease in the supervisor before the first run is committed. The desktop joins task workspace roots to the existing project registry and keeps only view expansion state in UI preferences.

**Tech Stack:** Rust, Tokio, SQLite event projections, Schemars/Serde protocol generation, Tauri 2, React 19, TanStack Query/Router, Zustand, Vitest, Testing Library.

---

### Task 1: Reproduce and fix missing foreground workspace leases

**Files:**
- Modify: `crates/jyowo-harness-daemon/src/supervisor.rs`
- Modify: `crates/jyowo-harness-daemon/src/task_actor.rs`
- Test: `crates/jyowo-harness-daemon/tests/task_actor/task_actor_cases.rs`
- Test: `crates/jyowo-harness-daemon/tests/ipc/ipc_cases.rs`

**Step 1: Write the failing integration test**

Add a production-component test that creates a task with a real temporary workspace, submits its first message, and asserts before the run factory starts:

```rust
let projection = store.task_projection(task_id).unwrap().unwrap();
let lease_id = store
    .active_workspace_lease(task_id)
    .unwrap()
    .expect("first submit acquires a workspace lease")
    .lease_id;
assert_eq!(starts[0].input.workspace_lease_id, Some(lease_id));
assert_eq!(projection.state, TaskState::Running);
```

Also add a failure case using a missing workspace root. Assert `CommandOutcome::Rejected`, no `run.started` event, no `message.queued` event, and the original task remains idle.

**Step 2: Run the test and verify it fails**

Run:

```bash
cargo test -p jyowo-harness-daemon first_submit_acquires_foreground_workspace -- --nocapture
cargo test -p jyowo-harness-daemon missing_workspace_rejects_submit_before_run_start -- --nocapture
```

Expected: the first test observes `workspace_lease_id == None`; the second currently accepts the command and records a failed run.

**Step 3: Add supervisor lease preparation**

Store `Arc<TaskStore>` and `Arc<WorkspaceCoordinator>` on `Supervisor`. Before routing a command that can start a segment, call a helper with this behavior:

```rust
fn ensure_foreground_workspace(
    store: &TaskStore,
    workspace: &WorkspaceCoordinator,
    task_id: TaskId,
) -> Result<(), SupervisorError> {
    if store.active_workspace_lease(task_id)?.is_some() {
        return Ok(());
    }
    let projection = store
        .task_projection(task_id)?
        .ok_or(SupervisorError::TaskNotFound)?;
    let selection = projection
        .workspace
        .ok_or(SupervisorError::WorkspaceSelectionMissing)?;
    let actor_id = projection.actor_id.unwrap_or_else(|| task_actor_id(task_id));
    match workspace.acquire(WorkspaceLeaseRequest {
        task_id,
        actor_id,
        root: selection.root.into(),
        mode: Some(selection.mode),
        access: WorkspaceAccess::Write,
        execution_kind: WorkspaceExecutionKind::Foreground,
        expires_at: None,
    })? {
        WorkspaceAcquireOutcome::Acquired(_) => Ok(()),
        WorkspaceAcquireOutcome::Waiting(_) => Err(SupervisorError::WorkspaceBusy),
    }
}
```

Invoke it for `SubmitMessage` only when the task has no active run, and for `StartSegment`/`ContinueTask`. Convert errors into `command.rejected(error.to_string())` before sending anything to the task actor. Do not acquire a second lease for queue-only submissions.

**Step 4: Run focused tests**

Run:

```bash
cargo test -p jyowo-harness-daemon first_submit_acquires_foreground_workspace missing_workspace_rejects_submit_before_run_start
```

Expected: both pass; failed acquisition produces no run events.

**Step 5: Run daemon regression tests**

Run:

```bash
cargo test -p jyowo-harness-daemon
```

Expected: all tests pass.

**Step 6: Commit**

```bash
git add crates/jyowo-harness-daemon/src/supervisor.rs crates/jyowo-harness-daemon/src/task_actor.rs crates/jyowo-harness-daemon/tests
git commit -m "fix: acquire task workspace before runs"
```

### Task 2: Add durable pin and remove projection state

**Files:**
- Modify: `crates/jyowo-harness-contracts/src/daemon.rs`
- Modify: `crates/jyowo-harness-journal/src/task_event.rs`
- Modify: `crates/jyowo-harness-journal/src/task_projection.rs`
- Test: `crates/jyowo-harness-contracts/tests/daemon_contract.rs`
- Test: `crates/jyowo-harness-journal/src/task_projection_tests.rs`

**Step 1: Write failing projection tests**

Add events to a task stream and verify they survive projection rebuild:

```rust
NewTaskEvent::task_pinned(true),
NewTaskEvent::task_title_changed("Renamed"),
NewTaskEvent::task_archived(true),
NewTaskEvent::task_removed(true),
```

Assert `projection.pinned`, `projection.archived`, `projection.removed`, and the renamed title. Add invalid-transition tests proving metadata events cannot precede `task.created`.

**Step 2: Run the projection tests and verify failure**

Run:

```bash
cargo test -p jyowo-harness-journal task_metadata_projects_and_rebuilds
```

Expected: compile failure because pinned/removed events and fields do not exist.

**Step 3: Implement event and projection fields**

Add `TaskPinned { pinned: bool }` and `TaskRemoved { removed: bool }` events. Extend `TaskProjection` with backwards-compatible defaults:

```rust
#[serde(default)]
pub pinned: bool,
#[serde(default)]
pub removed: bool,
```

Project both events, validate that the task exists, and emit bounded notice rows. Keep existing title and archived events unchanged.

**Step 4: Add protocol serialization coverage**

Extend `daemon_contract.rs` to assert `pinned` and `removed` are camel-case boolean fields and older stored projections deserialize with both false.

**Step 5: Run tests**

Run:

```bash
cargo test -p jyowo-harness-journal task_metadata
cargo test -p jyowo-harness-contracts daemon_contract
```

Expected: pass.

**Step 6: Commit**

```bash
git add crates/jyowo-harness-contracts crates/jyowo-harness-journal
git commit -m "feat: project task pin and removal state"
```

### Task 3: Expose task metadata commands through the daemon protocol

**Files:**
- Modify: `crates/jyowo-harness-contracts/src/daemon.rs`
- Modify: `crates/jyowo-harness-daemon/src/ipc/server.rs`
- Modify: `crates/jyowo-harness-daemon/src/task_actor.rs`
- Test: `crates/jyowo-harness-contracts/tests/daemon_contract.rs`
- Test: `crates/jyowo-harness-daemon/tests/ipc/ipc_cases.rs`
- Regenerate: `apps/desktop/src/generated/daemon-protocol.ts`
- Regenerate: `apps/desktop/src/generated/daemon-protocol.schema.json`

**Step 1: Write failing IPC tests**

For rename, pin, archive, and remove, send a command with current stream version and assert a `command_accepted` response plus the updated projection. Add stale-version tests and reject removal while the task has a running segment.

**Step 2: Run and verify failure**

Run:

```bash
cargo test -p jyowo-harness-daemon task_metadata_commands_update_projection -- --nocapture
```

Expected: compile failure because request variants are absent.

**Step 3: Add request contracts**

Add these `ClientRequest` variants and request structs:

```rust
RenameTask { metadata, task_id, title }
SetTaskPinned { metadata, task_id, pinned }
SetTaskArchived { metadata, task_id, archived }
RemoveTask { metadata, task_id }
```

Titles are trimmed, non-empty, and rely on journal title limits. All commands use task-scoped optimistic concurrency and existing idempotency behavior.

**Step 4: Route mutations through the task actor**

Add a `ValidatedTaskCommand::Metadata` variant holding one `TaskMetadataMutation`. In the actor, transact exactly one corresponding event. Reject remove when a run or queued message is active. A removed task remains in the audit log but is excluded from `ListTasks`; `LoadTask` returns not found for removed tasks.

**Step 5: Regenerate protocol bindings**

Run:

```bash
pnpm generate:daemon-protocol
pnpm check:daemon-protocol
```

Expected: generated TypeScript contains all four request variants and `TaskProjection.pinned/removed`.

**Step 6: Run tests**

Run:

```bash
cargo test -p jyowo-harness-contracts daemon_contract
cargo test -p jyowo-harness-daemon ipc
```

Expected: pass.

**Step 7: Commit**

```bash
git add crates/jyowo-harness-contracts crates/jyowo-harness-daemon apps/desktop/src/generated scripts/generate-daemon-protocol.mjs
git commit -m "feat: add daemon task metadata commands"
```

### Task 4: Add default workspace and project display-name operations

**Files:**
- Modify: `apps/desktop/src-tauri/src/project_registry.rs`
- Modify: `apps/desktop/src-tauri/src/commands/projects.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Test: `apps/desktop/src-tauri/src/project_registry.rs`
- Test: `apps/desktop/src/shared/tauri/commands.test.ts`

**Step 1: Write failing registry tests**

Test that `default_workspace_root()` returns `$HOME/.jyowo/workspaces/default`, creates it with owner-only permissions, and rejects a symlink. Test that renaming a project changes only its stored display name and survives reload.

**Step 2: Run and verify failure**

Run:

```bash
cargo test -p jyowo-desktop-shell default_workspace_root rename_project
```

Expected: compile failure because both operations are absent.

**Step 3: Implement safe default-root resolution**

Add a Tauri command returning:

```json
{ "path": "/home/user/.jyowo/workspaces/default" }
```

Use existing no-symlink and owner-only directory helpers. Do not expose the full home directory as the default workspace.

**Step 4: Implement project renaming**

Add `ProjectRegistry::rename(path, name)`. Trim the name, reject empty names, cap it at 120 characters, preserve `path` and `lastOpenedAt`, and persist atomically. Expose `rename_project(path, name)` through Tauri and `CommandClient`.

**Step 5: Run Rust and TypeScript boundary tests**

Run:

```bash
cargo test -p jyowo-desktop-shell project_registry
pnpm -C apps/desktop test -- src/shared/tauri/commands.test.ts
```

Expected: pass.

**Step 6: Commit**

```bash
git add apps/desktop/src-tauri apps/desktop/src/shared/tauri
git commit -m "feat: add default workspace and project labels"
```

### Task 5: Persist sidebar section expansion state

**Files:**
- Modify: `apps/desktop/src/shared/local-store/ui-preferences-store.ts`
- Modify: `apps/desktop/src/shared/local-store/ui-preferences-store.test.ts`
- Modify: `apps/desktop/src/shared/state/ui-store.ts`
- Modify: `apps/desktop/src/shared/state/ui-store.test.ts`

**Step 1: Write failing store tests**

Assert defaults and round-trip persistence for:

```ts
sidebarSections: {
  pinned: true,
  projects: true,
  conversations: true,
},
expandedProjects: Record<string, boolean>
```

**Step 2: Run and verify failure**

Run:

```bash
pnpm -C apps/desktop test -- src/shared/local-store/ui-preferences-store.test.ts src/shared/state/ui-store.test.ts
```

Expected: missing fields/actions.

**Step 3: Implement state and persistence**

Add `setSidebarSectionExpanded(section, expanded)` and `setProjectExpanded(path, expanded)`. Preserve defaults when old preference files omit the new fields. Persist changes through the existing preference writer in the app provider binding.

**Step 4: Run tests**

Run the same command. Expected: pass.

**Step 5: Commit**

```bash
git add apps/desktop/src/shared/local-store apps/desktop/src/shared/state
git commit -m "feat: persist sidebar expansion state"
```

### Task 6: Build the grouped task-list view with complete menus

**Files:**
- Replace: `apps/desktop/src/features/tasks/TaskList.tsx`
- Modify: `apps/desktop/src/features/workspace/ProjectSelector.tsx`
- Use: `apps/desktop/src/shared/ui/dropdown-menu.tsx`
- Use: `apps/desktop/src/shared/ui/dialog.tsx`
- Test: `apps/desktop/src/features/tasks/TaskList.test.tsx`

**Step 1: Write failing component tests**

Cover these visible behaviors:

- labels `Pinned`, `Projects`, `Conversations`;
- each section toggles independently and has the correct `aria-expanded` value;
- pinned tasks appear only in Pinned;
- default-root tasks appear in Conversations;
- project-root tasks appear inside their project;
- unknown roots fall back to Conversations;
- a project row expands/collapses and creates a conversation in that project;
- task menu invokes pin/unpin, rename, archive, and remove callbacks;
- project menu invokes rename, move, and remove callbacks;
- all empty and loading states remain navigable.

**Step 2: Run and verify failure**

Run:

```bash
pnpm -C apps/desktop test -- src/features/tasks/TaskList.test.tsx
```

Expected: current state-based Active/Recent/Archived UI fails the new assertions.

**Step 3: Implement a presentational grouped model**

Export a pure `groupSidebarTasks(tasks, projects, defaultRoot)` helper. Canonical comparison uses the exact canonical paths returned by the backend; do not normalize paths again in the browser. Order pinned and child task lists by descending `lastGlobalOffset`; preserve project registry order.

Render accessible section buttons with `ChevronRight/ChevronDown`, project rows with `Folder`, task rows with status icons, and `MoreHorizontal` menus. Use confirmation dialogs for archive/remove actions and a small validated dialog for rename.

**Step 4: Run component tests**

Run the same command. Expected: pass.

**Step 5: Commit**

```bash
git add apps/desktop/src/features/tasks/TaskList.tsx apps/desktop/src/features/tasks/TaskList.test.tsx apps/desktop/src/features/workspace/ProjectSelector.tsx
git commit -m "feat: add grouped Codex task list"
```

### Task 7: Wire sidebar queries and mutations

**Files:**
- Modify: `apps/desktop/src/features/workspace/SidebarNav.tsx`
- Modify: `apps/desktop/src/features/workspace/SidebarNav.test.tsx`
- Modify: `apps/desktop/src/shared/daemon/client.ts`
- Modify: `apps/desktop/src/shared/daemon/client.test.ts`
- Modify: `apps/desktop/src/testing/daemon-client.ts`

**Step 1: Write failing sidebar integration tests**

Test these command payloads:

```ts
// Global action
workspace: { mode: 'current', root: '/home/me/.jyowo/workspaces/default' }

// Project action
workspace: { mode: 'current', root: '/repo/alpha' }
```

Assert the title is `New conversation`. Add tests for adding a folder through `pickProjectDirectory`, project rename/order/remove, and task pin/rename/archive/remove. Verify accepted task mutations invalidate `['daemon-tasks']` and removed active tasks navigate to `/` without a `taskId`.

**Step 2: Run and verify failure**

Run:

```bash
pnpm -C apps/desktop test -- src/features/workspace/SidebarNav.test.tsx
```

Expected: the current sidebar requires an active project and sends title `New task`.

**Step 3: Extend the daemon client**

Add typed helpers for the four generated metadata commands. Reuse `createTaskCommandMetadata(taskId, streamVersion, operation)` and `requireAcceptedCommand` so retries keep the same command identity.

**Step 4: Wire project and task mutations**

Load `listProjects`, `getDefaultWorkspace`, and `listTasks` concurrently. The primary action always targets the default root. Project-level actions pass the project root. Centralize accepted-command validation and show errors in the relevant section without clearing current data.

**Step 5: Run integration tests**

Run:

```bash
pnpm -C apps/desktop test -- src/features/workspace/SidebarNav.test.tsx src/shared/daemon/client.test.ts
```

Expected: pass.

**Step 6: Commit**

```bash
git add apps/desktop/src/features/workspace apps/desktop/src/shared/daemon apps/desktop/src/testing/daemon-client.ts
git commit -m "feat: wire workspace task sidebar"
```

### Task 8: Localize task terminology and empty workspace

**Files:**
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`
- Modify: `apps/desktop/src/routes/index.lazy.tsx`
- Modify: `apps/desktop/src/features/tasks/TaskWorkspace.tsx`
- Test: `apps/desktop/src/shared/i18n/i18n.test.ts`
- Test: `apps/desktop/src/app/App.test.tsx`

**Step 1: Write failing localization tests**

Assert Chinese UI strings include `新建会话`, `置顶`, `项目`, and `会话`; assert no rendered primary surface contains `New task` or `New Task` in either locale.

**Step 2: Run and verify failure**

Run:

```bash
pnpm -C apps/desktop test -- src/shared/i18n/i18n.test.ts src/app/App.test.tsx
```

Expected: current task title and empty route copy fail.

**Step 3: Replace user-facing task terminology**

Use conversation wording in the sidebar, empty state, workspace header defaults, aria labels, status text, dialogs, and command palette. Keep daemon protocol names such as `create_task` unchanged.

**Step 4: Run tests**

Run the same command. Expected: pass.

**Step 5: Commit**

```bash
git add apps/desktop/src/shared/i18n apps/desktop/src/routes/index.lazy.tsx apps/desktop/src/features/tasks/TaskWorkspace.tsx apps/desktop/src/app/App.test.tsx
git commit -m "feat: localize conversation task surfaces"
```

### Task 9: Verify complete behavior

**Files:**
- Modify if needed: files changed in Tasks 1-8 only

**Step 1: Run formatting and generated-contract checks**

Run:

```bash
cargo fmt --all --check
pnpm check:daemon-protocol
pnpm -C apps/desktop lint
pnpm -C apps/desktop typecheck
```

Expected: all pass.

**Step 2: Run frontend suite**

Run:

```bash
pnpm -C apps/desktop test
```

Expected: all tests pass.

**Step 3: Run relevant Rust suites**

Run:

```bash
cargo test -p jyowo-harness-contracts -p jyowo-harness-journal -p jyowo-harness-daemon -p jyowo-desktop-shell
```

Expected: all tests pass.

**Step 4: Build release boundaries**

Run:

```bash
pnpm build:daemon-sidecar
pnpm -C apps/desktop build
```

Expected: both complete successfully.

**Step 5: Perform the desktop smoke test**

Run the Tauri app with an isolated HOME/runtime fixture. Verify:

1. Primary `New conversation` creates a task rooted at `~/.jyowo/workspaces/default`.
2. Adding a project and its row action creates a task rooted at that project.
3. Sending a message no longer produces an immediate `Run failed`; the workspace lease is active before provider execution.
4. Pinned, Projects, and Conversations fold independently and keep their state after reload.
5. Rename, pin, archive, remove, project rename/order/remove all update the sidebar without restart.

**Step 6: Review the final diff**

Run:

```bash
git status --short
git diff --check
git log --oneline --decorate -10
```

Expected: no unintended files, no whitespace errors, and one focused commit per task.

