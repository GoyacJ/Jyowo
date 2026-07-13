# Workspace Lease Release Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Release foreground workspace leases when a task no longer needs them and show concrete daemon rejection messages.

**Architecture:** The task actor already owns serialized run lifecycle transitions. It will ask the workspace-bound run factory to release task leases only after terminal persistence confirms there is no queued work, and after task removal succeeds. The daemon protocol will carry an optional rejection message consumed by the desktop client.

**Tech Stack:** Rust, Tokio, SQLite journal projections, Serde/JSON Schema, TypeScript, React, Vitest.

---

### Task 1: Terminal lease regression

**Files:**
- Modify: `crates/jyowo-harness-daemon/tests/task_actor/task_actor_cases.rs`

1. Add a test that creates two tasks in one workspace, completes the first run, and asserts its lease is released and the second submit is accepted.
2. Run `cargo test -p jyowo-harness-daemon --test task_actor completed_run_releases_workspace_for_next_task -- --exact` and confirm it fails because the first lease remains active.

### Task 2: Release task leases

**Files:**
- Modify: `crates/jyowo-harness-daemon/src/run_coordinator.rs`
- Modify: `crates/jyowo-harness-daemon/src/task_actor.rs`

1. Expose a narrow `release_task_leases` operation through the workspace-bound factory.
2. After a terminal run is committed, inspect the current queue and release the task leases only when no queued item remains.
3. Release leases after an accepted remove mutation.
4. Run the focused daemon test and the full `task_actor` integration test.

### Task 3: Rejection message regression

**Files:**
- Modify: `crates/jyowo-harness-daemon/src/ipc/server.rs`
- Modify: `apps/desktop/src/shared/daemon/task-command.ts`
- Modify: relevant Rust and TypeScript tests

1. Add failing tests proving `InvalidCommand { message }` survives IPC conversion and `requireAcceptedCommand` throws that message.
2. Add `message: Option<String>` to `CommandRejected` and populate it for invalid commands.
3. Regenerate `apps/desktop/src/generated/daemon-protocol.ts` and schema with `pnpm generate:daemon-protocol`.
4. Prefer `frame.message.message` in the desktop error.
5. Run focused Rust and Vitest tests.

### Task 4: Development data reset and verification

**Files:**
- Delete runtime data: `~/Library/Application Support/com.goyacj.jyowo/daemon`

1. Stop the Jyowo daemon process if it is running.
2. Remove the development daemon directory.
3. Run formatting, daemon tests, desktop tests, and protocol consistency checks.
4. Review `git diff` and ensure unrelated user changes remain untouched.
