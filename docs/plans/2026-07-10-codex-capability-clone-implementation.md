# Codex Capability Clone Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace Jyowo's current conversation runtime with a Codex-style local task daemon, durable event timeline, steerable message queue, isolated subagents, and matching desktop workspace in light and dark themes.

**Architecture:** Rust contract types define one versioned daemon protocol and export JSON Schema. A single-user local daemon owns supervised task actors and writes every task event, command outcome, projection, checkpoint, workspace lease, and blob reference to one SQLite WAL database. Tauri is a thin UDS/Named Pipe bridge; React consumes projections plus globally ordered events and renders the approved Codex timeline, queue, composer, and workbench.

**Tech Stack:** Rust 2021, Tokio, SQLite/rusqlite, serde/schemars, Tauri 2, React 19, TypeScript 6, Ajv, TanStack Query/Virtual, Zustand, Vitest, Storybook, Playwright, pnpm 11.

---

## Execution rules

- Work only in `/Users/goya/Repo/Git/Jyowo/.worktrees/codex-capability-clone` on branch `goya/codex-capability-clone`.
- Read and apply `@superpowers:test-driven-development` before every implementation task.
- Read and apply `@frontend-design` before Tasks 14–18. Preserve the approved Codex visual hierarchy; do not introduce a second visual direction.
- After every task that changes code, run `/code-review-expert` and resolve its actionable findings before committing.
- Run `/security-review` before committing Tasks 3, 4, 10, 11, 12, and 13 because they handle user input, IPC, permissions, blobs, or filesystem authority.
- Use global event offsets only for client synchronization. Keep per-task stream sequence for optimistic concurrency.
- Treat SQLite WAL as the only task truth. Do not add another task database to the daemon, Tauri layer, agent runtime, or renderer.
- Do not migrate or read old sessions. The final cutover deletes the old conversation and TCP supervisor paths after the new path passes all gates.
- Run `@superpowers:verification-before-completion` before the final completion claim.
- Treat every numbered assertion, ordered list item, and test case inside a step as its own 2–5 minute action. Run the named focused test after each action; do not batch an entire paragraph into one edit.

## Verification baseline

Run these before Task 1 and record existing failures without fixing unrelated code:

```bash
git status --short
cargo test -p jyowo-harness-contracts -p jyowo-harness-journal -p jyowo-harness-engine
pnpm -C apps/desktop test
```

Expected: the worktree is clean and the selected baseline suites pass. If a baseline fails, save the exact command and output before proceeding.

### Task 1: Define the daemon contract and generated TypeScript surface

**Files:**

- Create: `crates/jyowo-harness-contracts/src/daemon.rs`
- Create: `crates/jyowo-harness-contracts/examples/export_daemon_schema.rs`
- Create: `crates/jyowo-harness-contracts/tests/daemon_contract.rs`
- Create: `scripts/generate-daemon-protocol.mjs`
- Create: `apps/desktop/src/generated/daemon-protocol.schema.json`
- Create: `apps/desktop/src/generated/daemon-protocol.ts`
- Create: `apps/desktop/src/shared/daemon/protocol.ts`
- Create: `apps/desktop/src/shared/daemon/protocol.test.ts`
- Modify: `crates/jyowo-harness-contracts/src/lib.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `apps/desktop/package.json`
- Modify: `package.json`
- Modify: `pnpm-lock.yaml`

**Step 1: Write the failing Rust contract test**

Add a test that requires a single tagged protocol envelope and the initial command/event surface:

```rust
use harness_contracts::{daemon_protocol_schema, ClientFrame, ClientRequest, PROTOCOL_VERSION};

#[test]
fn daemon_protocol_exports_one_versioned_schema() {
    assert_eq!(PROTOCOL_VERSION, 1);
    let value = serde_json::to_value(daemon_protocol_schema()).unwrap();
    let text = serde_json::to_string(&value).unwrap();
    for required in [
        "handshake",
        "submit_message",
        "edit_queued_message",
        "delete_queued_message",
        "promote_queued_message",
        "resolve_permission",
        "subscribe_events",
        "read_blob",
    ] {
        assert!(text.contains(required), "missing {required}");
    }

    let frame = ClientFrame {
        request_id: "req-1".into(),
        protocol_version: PROTOCOL_VERSION,
        request: ClientRequest::SubscribeEvents { after_offset: 42 },
    };
    assert_eq!(serde_json::to_value(frame).unwrap()["request"]["type"], "subscribe_events");
}
```

**Step 2: Run the contract test to verify it fails**

Run: `cargo test -p jyowo-harness-contracts --test daemon_contract`

Expected: FAIL because `daemon_protocol_schema`, `ClientFrame`, and `ClientRequest` do not exist.

**Step 3: Implement the Rust protocol types and schema root**

Define newtypes for task, run-segment, queue-item, command, client, actor, workspace-lease, checkpoint, and blob IDs. Add `ClientFrame`, `ServerFrame`, `ClientRequest`, `CommandAccepted`, `CommandRejected`, `TaskEventEnvelope`, projection DTOs, `Handshake`, and `ProtocolError`. Use internally tagged snake-case enums:

```rust
pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClientFrame {
    pub request_id: String,
    pub protocol_version: u16,
    pub request: ClientRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientRequest {
    Handshake(HandshakeRequest),
    CreateTask(CreateTaskCommand),
    SubmitMessage(SubmitMessageCommand),
    EditQueuedMessage(EditQueuedMessageCommand),
    DeleteQueuedMessage(DeleteQueuedMessageCommand),
    PromoteQueuedMessage(PromoteQueuedMessageCommand),
    StopRun(StopRunCommand),
    ContinueTask(ContinueTaskCommand),
    ResolvePermission(ResolvePermissionCommand),
    SubscribeEvents { after_offset: u64 },
    LoadTask { task_id: TaskId },
    ListTasks,
    ReadBlob { blob_id: BlobId },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DaemonProtocol {
    pub client: ClientFrame,
    pub server: ServerFrame,
}

pub fn daemon_protocol_schema() -> schemars::Schema {
    schemars::schema_for!(DaemonProtocol)
}
```

The schema must describe queue states `queued | promoting | consumed | deleted`, task states, run terminal reasons, timeline event kinds, workspace modes, permission routing, and blob responses. Reject arbitrary blob paths at the type level: `ReadBlob` accepts only `BlobId`.

**Step 4: Export one deterministic JSON Schema file**

Implement `export_daemon_schema.rs` to pretty-print `daemon_protocol_schema()` to stdout. Register only the root as `daemon_protocol` in `export_all_schemas()`. Verify deterministic output:

Run: `cargo run -q -p jyowo-harness-contracts --example export_daemon_schema > /tmp/daemon-schema-a.json && cargo run -q -p jyowo-harness-contracts --example export_daemon_schema > /tmp/daemon-schema-b.json && cmp /tmp/daemon-schema-a.json /tmp/daemon-schema-b.json`

Expected: exit 0 and no `cmp` output.

**Step 5: Add the TypeScript generation script and runtime validator**

Add `ajv` to desktop dependencies and `json-schema-to-typescript` to dev dependencies. `scripts/generate-daemon-protocol.mjs` must:

1. run the Rust schema example;
2. write `apps/desktop/src/generated/daemon-protocol.schema.json`;
3. compile the root schema to `daemon-protocol.ts` with a stable banner;
4. fail if a second run changes either generated file.

Implement the validator without handwritten protocol DTOs:

```ts
import Ajv from 'ajv'
import schema from '@/generated/daemon-protocol.schema.json'
import type { ServerFrame } from '@/generated/daemon-protocol'

const validate = new Ajv({ allErrors: true, strict: true }).compile(schema.$defs?.ServerFrame)

export function parseServerFrame(value: unknown): ServerFrame {
  if (!validate(value)) {
    throw new Error(`Invalid daemon frame: ${JSON.stringify(validate.errors)}`)
  }
  return value as ServerFrame
}
```

**Step 6: Write and run the frontend protocol test**

Test one valid event frame, an unknown event kind, a missing protocol version, and a `read_blob` request containing a raw path. The last three must be rejected.

Run: `pnpm generate:daemon-protocol && pnpm -C apps/desktop test src/shared/daemon/protocol.test.ts`

Expected: PASS. A second `pnpm generate:daemon-protocol` leaves `git diff --exit-code -- apps/desktop/src/generated` clean.

**Step 7: Run review and commit**

Run `/code-review-expert`, fix findings, then:

```bash
git add Cargo.toml Cargo.lock package.json pnpm-lock.yaml scripts/generate-daemon-protocol.mjs crates/jyowo-harness-contracts apps/desktop/package.json apps/desktop/src/generated apps/desktop/src/shared/daemon
git commit -m "feat: define daemon protocol contracts"
```

### Task 2: Add the unified TaskStore schema and global event offsets

**Files:**

- Create: `crates/jyowo-harness-journal/src/task_store.rs`
- Create: `crates/jyowo-harness-journal/src/task_schema.rs`
- Create: `crates/jyowo-harness-journal/tests/task_store.rs`
- Modify: `crates/jyowo-harness-journal/src/lib.rs`
- Modify: `crates/jyowo-harness-journal/Cargo.toml`

**Step 1: Write the failing ordering and optimistic-concurrency tests**

The test opens one temporary database, appends events to two tasks, and asserts:

```rust
assert_eq!(offsets, vec![1, 2, 3]);
assert_eq!(store.stream_version(task_a).unwrap(), 2);
assert!(matches!(
    store.append(task_a, 0, source(), vec![event("stale")]),
    Err(TaskStoreError::WrongExpectedVersion { expected: 0, actual: 2 })
));
```

Also reopen the database and assert the next committed event receives offset 4.

**Step 2: Run the store test to verify it fails**

Run: `cargo test -p jyowo-harness-journal --features sqlite,blob-file --test task_store`

Expected: FAIL because `TaskStore` is missing.

**Step 3: Create the strict SQLite schema**

Create tables in one WAL database:

```sql
CREATE TABLE event_log (
  global_offset INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id TEXT NOT NULL,
  stream_sequence INTEGER NOT NULL,
  event_id TEXT NOT NULL UNIQUE,
  event_type TEXT NOT NULL,
  schema_version INTEGER NOT NULL,
  recorded_at TEXT NOT NULL,
  source_json TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  UNIQUE(task_id, stream_sequence)
) STRICT;

CREATE TABLE command_inbox (
  command_id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  idempotency_key TEXT NOT NULL UNIQUE,
  expected_stream_version INTEGER NOT NULL,
  status TEXT NOT NULL,
  accepted_at TEXT NOT NULL,
  completed_at TEXT,
  outcome_json TEXT
) STRICT;
```

Add projection, checkpoint, blob metadata, and lease tables now, but do not add behavior that is not tested until later tasks. Enable `WAL`, `synchronous=NORMAL`, foreign keys, and `busy_timeout=5000` on every connection.

**Step 4: Implement transactional append and reads**

`TaskStore::append` begins `IMMEDIATE`, checks `MAX(stream_sequence)`, inserts all events, applies synchronous projectors through a callback, commits, then returns committed envelopes. Never broadcast before commit. Provide:

```rust
pub fn append(
    &self,
    task_id: TaskId,
    expected_version: u64,
    source: EventSource,
    events: Vec<NewTaskEvent>,
) -> Result<Vec<TaskEventEnvelope>, TaskStoreError>;

pub fn events_after(
    &self,
    after_global_offset: u64,
    limit: usize,
) -> Result<Vec<TaskEventEnvelope>, TaskStoreError>;
```

Clamp `limit` to `1..=1000` and order strictly by `global_offset ASC`.

**Step 5: Run focused and regression tests**

Run:

```bash
cargo test -p jyowo-harness-journal --features sqlite,blob-file --test task_store
cargo test -p jyowo-harness-journal --features sqlite,blob-file
```

Expected: PASS; existing session-local `SqliteEventStore` tests remain unchanged.

**Step 6: Run review and commit**

Run `/code-review-expert`, then:

```bash
git add crates/jyowo-harness-journal
git commit -m "feat: add unified task event store"
```

### Task 3: Make command acceptance idempotent and atomic with projections

**Files:**

- Create: `crates/jyowo-harness-journal/src/task_projection.rs`
- Create: `crates/jyowo-harness-journal/tests/task_commands.rs`
- Create: `crates/jyowo-harness-journal/tests/task_projection.rs`
- Modify: `crates/jyowo-harness-journal/src/task_store.rs`
- Modify: `crates/jyowo-harness-journal/src/lib.rs`

**Step 1: Write failing idempotency and rollback tests**

Cover these cases:

- the same idempotency key returns the stored outcome and appends no event;
- the same command ID with a different body is rejected;
- an expected-version mismatch records a rejected outcome but changes no projection;
- a projector failure rolls back the event, inbox outcome, and projection together;
- reopening the database returns the original accepted outcome.

Use a deliberately failing projector and assert `latest_global_offset() == 0` after rollback.

**Step 2: Run tests to verify failure**

Run: `cargo test -p jyowo-harness-journal --features sqlite,blob-file --test task_commands --test task_projection`

Expected: FAIL because command transactions and projections are not implemented.

**Step 3: Implement command transactions and synchronous projectors**

Add:

```rust
pub fn transact_command<F>(
    &self,
    command: AcceptedCommand,
    decide: F,
) -> Result<CommandOutcome, TaskStoreError>
where
    F: FnOnce(&TaskProjection) -> Result<Vec<NewTaskEvent>, CommandRejection>;
```

Inside one transaction: load or reserve inbox row, check idempotency and expected stream version, call the pure decision function, append events, apply task/run/queue/permission/subagent/workspace/timeline projectors, store the outcome, and commit. Persist a hash of the canonical command payload so an idempotency key cannot be reused for another payload.

**Step 4: Add projection rebuild**

Implement `rebuild_projections()` by truncating only projection tables and replaying `event_log` in global order. It must not touch `event_log`, `command_inbox`, checkpoints, or blobs. Add an invariant check that rebuilt rows equal rows captured before truncation.

**Step 5: Run focused tests, security review, and commit**

Run:

```bash
cargo test -p jyowo-harness-journal --features sqlite,blob-file --test task_commands --test task_projection
cargo test -p jyowo-harness-journal --features sqlite,blob-file
```

Expected: PASS. Run `/code-review-expert` and `/security-review`, then:

```bash
git add crates/jyowo-harness-journal
git commit -m "feat: make task commands transactional"
```

### Task 4: Store task blobs and adapt engine events into the unified log

**Files:**

- Create: `crates/jyowo-harness-journal/src/task_blob.rs`
- Create: `crates/jyowo-harness-journal/src/task_event_adapter.rs`
- Create: `crates/jyowo-harness-journal/tests/task_blob.rs`
- Create: `crates/jyowo-harness-journal/tests/task_event_adapter.rs`
- Modify: `crates/jyowo-harness-journal/src/task_store.rs`
- Modify: `crates/jyowo-harness-journal/src/lib.rs`

**Step 1: Write failing blob and adapter tests**

Require content-addressed deduplication, reference validation, missing-artifact state, and engine event ordering. The adapter test injects three `harness_contracts::Event` values through `EventStore::append` and asserts they appear in `event_log` with one task ID, ascending global offsets, and original run metadata.

**Step 2: Run tests to verify failure**

Run: `cargo test -p jyowo-harness-journal --features sqlite,blob-file --test task_blob --test task_event_adapter`

Expected: FAIL because `TaskBlobStore` and `TaskEventStoreAdapter` are missing.

**Step 3: Implement controlled blob storage**

Use BLAKE3 IDs and an application-owned blob directory. Write to a same-directory temporary file, `sync_all`, then atomically rename. Metadata insertion and event reference creation occur in the command transaction. Expose only:

```rust
pub fn put(&self, media_type: &str, bytes: &[u8]) -> Result<BlobRef, TaskStoreError>;
pub fn read(&self, blob_id: &BlobId) -> Result<BlobRead, TaskStoreError>;
```

Never accept a client path. Validate hash, size, media type, and ownership on read. Return `BlobRead::Missing` if metadata exists but the file is absent.

**Step 4: Implement the TaskStore-backed EventStore adapter**

Map engine `SessionId` to a daemon `TaskId` supplied when constructing the adapter. Convert existing engine events to versioned `engine.*` task events and append them through the same TaskStore transaction path. Do not open `SqliteEventStore`, `JsonlEventStore`, or the conversation read model in daemon code.

**Step 5: Run regression tests, reviews, and commit**

Run:

```bash
cargo test -p jyowo-harness-journal --features sqlite,blob-file --test task_blob --test task_event_adapter
cargo test -p jyowo-harness-sdk --test sdk_session_flow
```

Expected: PASS. Run `/code-review-expert` and `/security-review`, then:

```bash
git add crates/jyowo-harness-journal
git commit -m "feat: unify task artifacts and engine events"
```

### Task 5: Create the daemon supervisor and one-foreground-run TaskActor

**Files:**

- Create: `crates/jyowo-harness-daemon/Cargo.toml`
- Create: `crates/jyowo-harness-daemon/src/lib.rs`
- Create: `crates/jyowo-harness-daemon/src/supervisor.rs`
- Create: `crates/jyowo-harness-daemon/src/task_actor.rs`
- Create: `crates/jyowo-harness-daemon/src/run_coordinator.rs`
- Create: `crates/jyowo-harness-daemon/tests/task_actor.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

**Step 1: Write the failing actor lifecycle test**

Use a fake run coordinator controlled by channels. Send two commands to the same actor and commands to a second actor. Assert:

- task A never has two foreground coordinators;
- task B may run while task A runs;
- actor panic is reported to the supervisor and task A becomes `failed` without stopping task B;
- a completed task accepts another message and opens a new segment.

**Step 2: Run the actor test to verify failure**

Run: `cargo test -p jyowo-harness-daemon --test task_actor`

Expected: FAIL because the daemon crate does not exist.

**Step 3: Add the crate and actor messages**

Add the workspace member and dependencies on contracts, journal, engine, SDK, agent runtime, Tokio, serde, tracing, and thiserror. Define:

```rust
pub enum TaskActorMessage {
    Command(ValidatedTaskCommand, oneshot::Sender<CommandOutcome>),
    RunEvent(RunCoordinatorEvent),
    Shutdown,
}

pub trait RunCoordinatorFactory: Send + Sync {
    fn spawn(&self, request: StartSegmentRequest) -> RunningSegment;
}
```

`TaskActor` loads its projection before processing the mailbox. Every transition is decided from the projection and committed through `transact_command` before side effects start.

**Step 4: Implement supervision and quotas**

`Supervisor` owns a `JoinSet`, task-id-to-mailbox map, actor restart policy, and semaphores for global foreground runs and subagents. Recreate a crashed actor from TaskStore projection; never reconstruct state from in-memory maps. Persist `task.actor_failed` before exposing the failure.

**Step 5: Run tests and commit**

Run:

```bash
cargo test -p jyowo-harness-daemon --test task_actor
cargo test -p jyowo-harness-daemon
```

Expected: PASS. Run `/code-review-expert`, then:

```bash
git add Cargo.toml Cargo.lock crates/jyowo-harness-daemon
git commit -m "feat: supervise task actors in local daemon"
```

### Task 6: Implement the persistent queued-message state machine

**Files:**

- Create: `crates/jyowo-harness-daemon/src/queue.rs`
- Create: `crates/jyowo-harness-daemon/tests/queue_state_machine.rs`
- Modify: `crates/jyowo-harness-daemon/src/task_actor.rs`
- Modify: `crates/jyowo-harness-journal/src/task_projection.rs`
- Modify: `crates/jyowo-harness-contracts/src/daemon.rs`

**Step 1: Write the failing state-machine table test**

Test every allowed transition and reject all others:

| Current | Command | Result |
|---|---|---|
| none | submit while running | `queued` revision 1 |
| `queued` | edit expected revision | `queued` revision + 1 |
| `queued` | delete | `deleted` tombstone |
| `queued` | promote | `promoting`, frozen content |
| `promoting` | consume | `consumed`, bound segment ID |
| `promoting` after recovery | recover | `queued`, revision unchanged |

Add a stale-edit test that expects `CommandRejected::StaleQueueRevision` with the latest DTO.

Add a `proptest` sequence generator for submit/edit/delete/promote/consume/recover. After every generated command assert one foreground segment, monotonic revisions, FIFO normal consumption, and exactly one terminal queue state.

**Step 2: Run the queue test to verify failure**

Run: `cargo test -p jyowo-harness-daemon --test queue_state_machine`

Expected: FAIL because queue decision logic is missing.

**Step 3: Implement pure queue decisions**

Keep the state machine free of I/O:

```rust
pub fn decide_queue(
    projection: &QueueItemProjection,
    command: QueueCommand,
) -> Result<Vec<NewTaskEvent>, CommandRejection>;
```

Persist attachment blob IDs and context references with the queued message. Editing replaces content atomically and increments revision. Promotion freezes the exact revision. Deletion keeps an event and projection tombstone but excludes the item from normal queue queries.

**Step 4: Bind normal consumption atomically to a run segment**

When the task is idle, consume the oldest queued item by `(created_global_offset, queue_item_id)`. In one append, write `message.consumed` and `run.started` with the same new segment ID. A crash cannot leave a consumed item without its segment.

**Step 5: Run focused tests and commit**

Run:

```bash
cargo test -p jyowo-harness-daemon --test queue_state_machine
cargo test -p jyowo-harness-journal --features sqlite,blob-file --test task_projection
```

Expected: PASS. Run `/code-review-expert`, then:

```bash
git add crates/jyowo-harness-contracts crates/jyowo-harness-daemon crates/jyowo-harness-journal
git commit -m "feat: persist steerable task message queue"
```

### Task 7: Add engine safe points and force-stop steering

**Files:**

- Create: `crates/jyowo-harness-engine/src/safe_point.rs`
- Create: `crates/jyowo-harness-engine/tests/safe_point.rs`
- Create: `crates/jyowo-harness-daemon/tests/steering.rs`
- Modify: `crates/jyowo-harness-engine/src/lib.rs`
- Modify: `crates/jyowo-harness-engine/src/turn.rs`
- Modify: `crates/jyowo-harness-engine/src/interrupt.rs`
- Modify: `crates/jyowo-harness-daemon/src/run_coordinator.rs`
- Modify: `crates/jyowo-harness-daemon/src/task_actor.rs`

**Step 1: Write the failing engine safe-point tests**

Use a blocking atomic tool and a second counted tool. Start a turn, request yield while the first tool runs, release it, and assert:

```rust
assert_eq!(first_tool.calls(), 1);
assert_eq!(second_tool.calls(), 0);
assert_eq!(outcome, TurnOutcome::YieldedAtSafePoint);
```

Add tests for yield during model streaming and force-stop during a cancellable child process. Partial assistant text must remain in emitted events and be marked incomplete.

**Step 2: Run tests to verify failure**

Run: `cargo test -p jyowo-harness-engine --test safe_point`

Expected: FAIL because the engine has cancellation but no safe-point control.

**Step 3: Implement the engine control API**

Add a watch-based control shared with the turn:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunControl { Continue, YieldAfterAtomicOperation, ForceStop }

pub enum SafePointDecision { Continue, Yield, ForceStop }
```

Check control before a provider request, after each stream chunk boundary, before each tool starts, and after each atomic tool terminal event. For the daemon execution path, dispatch tool calls one at a time so a yield cannot start another tool after being requested. Preserve the current parallel orchestrator behavior outside the daemon path.

**Step 4: Wire actor promotion semantics**

Safe promotion commits `message.promoted` and `run.yield_requested`, sends `YieldAfterAtomicOperation`, waits for `run.safe_point_reached`, then commits in one transaction:

1. old segment `superseded`;
2. promoted message `consumed`;
3. new segment `started`.

Force promotion sends `ForceStop`, cancels the model stream, attempts to terminate cancellable processes, records non-revertible side effects, closes the old segment as `forced_interruption`, and starts the promoted segment. It must not claim rollback.

**Step 5: Run engine and daemon steering tests**

Run:

```bash
cargo test -p jyowo-harness-engine --test safe_point --test interrupt
cargo test -p jyowo-harness-daemon --test steering
```

Expected: PASS with no second tool invocation after a yield request.

**Step 6: Run review and commit**

Run `/code-review-expert`, then:

```bash
git add crates/jyowo-harness-engine crates/jyowo-harness-daemon
git commit -m "feat: steer runs at safe execution points"
```

### Task 8: Persist checkpoints and recover interrupted runs safely

**Files:**

- Create: `crates/jyowo-harness-daemon/src/checkpoint.rs`
- Create: `crates/jyowo-harness-daemon/src/recovery.rs`
- Create: `crates/jyowo-harness-daemon/tests/recovery.rs`
- Create: `crates/jyowo-harness-daemon/tests/context_compaction.rs`
- Modify: `crates/jyowo-harness-daemon/src/supervisor.rs`
- Modify: `crates/jyowo-harness-daemon/src/run_coordinator.rs`
- Modify: `crates/jyowo-harness-journal/src/task_store.rs`
- Modify: `crates/jyowo-harness-journal/src/task_projection.rs`

**Step 1: Write the failing crash matrix**

Parameterize daemon restart after:

- message consumption;
- tool start before terminal event;
- completed tool;
- permission decision;
- subagent state change;
- `yielding` before consumption;
- terminal run event.

Assert running segments become `interrupted_by_restart`, unresolved `promoting` items return to `queued`, terminal tools retain their recorded result, and a started tool without a terminal event becomes `indeterminate`.

Add compaction tests proving canonical timeline events remain queryable, a valid summary records its source offset range and blob ID, and a failed replacement leaves the previous valid summary active.

**Step 2: Run recovery tests to verify failure**

Run: `cargo test -p jyowo-harness-daemon --test recovery --test context_compaction`

Expected: FAIL because checkpoint and recovery services are missing.

**Step 3: Implement safe checkpoints**

Persist a checkpoint after the approved boundaries. The row contains task ID, segment ID, committed global offset, context cursor, queue revision, workspace baseline, incomplete tool IDs, and child actor references. Store large context/compaction artifacts as blob IDs. Never serialize hidden reasoning, a provider connection, process handles, or actor mailboxes.

**Step 4: Implement startup recovery**

On daemon startup:

1. load nonterminal task projections;
2. mark active segments `interrupted_by_restart`;
3. convert unmatched `tool.started` records to `indeterminate`;
4. return unconsumed `promoting` messages to `queued`;
5. invalidate unresolved runtime permission requests;
6. spawn idle actors without starting providers or tools.

`ContinueTask` always creates a new segment. It reuses completed tool results, asks the user to resolve each indeterminate tool, and never auto-replays it.

**Step 5: Run recovery and journal tests**

Run:

```bash
cargo test -p jyowo-harness-daemon --test recovery
cargo test -p jyowo-harness-daemon --test context_compaction
cargo test -p jyowo-harness-journal --features sqlite,blob-file
```

Expected: PASS; no fake clock or temporary process remains alive after tests.

**Step 6: Run review and commit**

Run `/code-review-expert`, then:

```bash
git add crates/jyowo-harness-daemon crates/jyowo-harness-journal
git commit -m "feat: recover task actors from safe checkpoints"
```

### Task 9: Move workspace leases and managed worktrees into TaskStore

**Files:**

- Create: `crates/jyowo-harness-daemon/src/workspace.rs`
- Create: `crates/jyowo-harness-daemon/tests/workspace_coordinator.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/isolation.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/store.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/lib.rs`
- Modify: `crates/jyowo-harness-daemon/src/run_coordinator.rs`
- Modify: `crates/jyowo-harness-journal/src/task_projection.rs`

**Step 1: Write failing lease and worktree tests**

Cover:

- two read-only current-workspace tasks may coexist;
- one writer excludes another writer and a read-to-write upgrade;
- waiting writers acquire in FIFO order;
- lease owner crash releases or expires the lease visibly;
- a background task and parallel child default to managed worktrees;
- non-Git managed-worktree requests are rejected, while current mode works;
- dirty managed worktrees produce a retained patch and `cleanup_blocked` event.

**Step 2: Run tests to verify failure**

Run: `cargo test -p jyowo-harness-daemon --test workspace_coordinator`

Expected: FAIL because daemon workspace coordination does not exist.

**Step 3: Introduce a TaskStore-backed runtime repository**

Replace direct `AgentRuntimeStore` use in workspace isolation with a trait:

```rust
pub trait WorkspaceLeaseRepository: Send + Sync {
    fn acquire(&self, request: LeaseRequest) -> Result<LeaseOutcome, WorkspaceError>;
    fn release(&self, lease_id: &WorkspaceLeaseId) -> Result<(), WorkspaceError>;
    fn active_for_root(&self, root: &CanonicalWorkspaceRoot) -> Result<Vec<Lease>, WorkspaceError>;
}
```

Implement it in TaskStore so lease state and events commit together. Keep Git discovery and worktree filesystem operations in `jyowo-harness-agent-runtime`; remove its authority to open `agent-runtime.sqlite` for daemon work.

**Step 4: Implement workspace mutation gates**

Classify tool actions as read or write before dispatch. Require an exclusive current-workspace lease for writes. The daemon must canonicalize roots, reject symlink escapes, record the workspace baseline commit/status, and emit lease wait/acquire/release events. Explicit override must be a separate auditable command.

**Step 5: Run tests and commit**

Run:

```bash
cargo test -p jyowo-harness-daemon --test workspace_coordinator
cargo test -p jyowo-harness-agent-runtime --test agent_orchestration_isolation
```

Expected: PASS. Run `/code-review-expert`, then:

```bash
git add crates/jyowo-harness-daemon crates/jyowo-harness-agent-runtime crates/jyowo-harness-journal
git commit -m "feat: coordinate task workspace isolation"
```

### Task 10: Give every subagent an actor, event stream, and workspace

**Files:**

- Create: `crates/jyowo-harness-daemon/src/subagent.rs`
- Create: `crates/jyowo-harness-daemon/tests/subagent_actor.rs`
- Modify: `crates/jyowo-harness-daemon/src/supervisor.rs`
- Modify: `crates/jyowo-harness-daemon/src/task_actor.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/subagents.rs`
- Modify: `crates/jyowo-harness-journal/src/task_projection.rs`
- Modify: `crates/jyowo-harness-contracts/src/daemon.rs`

**Step 1: Write failing parent-child topology tests**

Assert a spawned child receives a distinct actor ID, segment ID, event stream, context cursor, and managed-worktree lease. The parent timeline receives only child lifecycle references and bounded summaries. Add tests for depth/global quotas, parent safe-stop propagation, explicit `continue_in_background`, and child crash isolation.

**Step 2: Run tests to verify failure**

Run: `cargo test -p jyowo-harness-daemon --test subagent_actor`

Expected: FAIL because child runs still use the existing runner abstraction without daemon actors.

**Step 3: Implement the subagent actor bridge**

Adapt `SubagentRunner` requests into supervisor commands. Create child task aggregates linked by `parent_task_id`, `parent_segment_id`, and `delegation_id`. The child uses its own TaskStore adapter and workspace. Persist `subagent.spawned`, state changes, summary updates, background detachment, and terminal outcome.

**Step 4: Implement cancellation and summary flow**

Parent safe-stop asks active children to yield. Parent force-stop requests child cancellation but does not erase committed child effects. `continue_in_background` removes cancellation coupling and keeps the child visible in task projections. Parent context receives a size-bounded, redacted summary and child reference, never the full child event stream.

**Step 5: Run tests, reviews, and commit**

Run:

```bash
cargo test -p jyowo-harness-daemon --test subagent_actor
cargo test -p jyowo-harness-agent-runtime --test subagents
```

Expected: PASS. Run `/code-review-expert` and `/security-review`, then:

```bash
git add crates/jyowo-harness-contracts crates/jyowo-harness-daemon crates/jyowo-harness-agent-runtime crates/jyowo-harness-journal
git commit -m "feat: supervise subagents as task actors"
```

### Task 11: Centralize permission routing and invalidate stale decisions

**Files:**

- Create: `crates/jyowo-harness-daemon/src/permission_broker.rs`
- Create: `crates/jyowo-harness-daemon/tests/permission_routing.rs`
- Modify: `crates/jyowo-harness-daemon/src/run_coordinator.rs`
- Modify: `crates/jyowo-harness-daemon/src/task_actor.rs`
- Modify: `crates/jyowo-harness-journal/src/task_projection.rs`
- Modify: `crates/jyowo-harness-contracts/src/daemon.rs`

**Step 1: Write failing permission authority tests**

Cover command, filesystem, network, MCP, and automation requests. Assert:

- only the daemon can emit `permission.requested/resolved`;
- a UI decision requires the current request ID, revision, option ID, and expected task version;
- safe or force promotion invalidates an unresolved request;
- a late decision for an invalidated request is rejected;
- child requests route to the owning foreground task unless a saved daemon policy resolves them;
- previews and persisted payloads redact secrets.
- queue edit/delete/promote commands remain available while the task waits for permission.

**Step 2: Run tests to verify failure**

Run: `cargo test -p jyowo-harness-daemon --test permission_routing`

Expected: FAIL because permission authority is not centralized in the daemon.

**Step 3: Implement PermissionBroker decisions**

The broker owns request creation, saved-policy evaluation, expiry, invalidation, and resolution. UI frames carry decisions only. Revalidate action plan hash, sandbox policy hash, workspace, subject, allowed options, actor source, and expiration immediately before committing the decision.

**Step 4: Connect steering and recovery**

When a task leaves `waiting_permission` due to steering, append `permission.invalidated` before segment interruption. Recovery closes unresolved runtime requests as `expired_by_restart`. Continuation generates a new request if the action is still required.

**Step 5: Run tests, reviews, and commit**

Run:

```bash
cargo test -p jyowo-harness-daemon --test permission_routing
cargo test -p jyowo-harness-engine --test permission --test permission_hooks
```

Expected: PASS. Run `/code-review-expert` and `/security-review`, then:

```bash
git add crates/jyowo-harness-contracts crates/jyowo-harness-daemon crates/jyowo-harness-journal
git commit -m "feat: centralize daemon permission authority"
```

### Task 12: Implement versioned UDS and Named Pipe IPC with multiple clients

**Files:**

- Create: `crates/jyowo-harness-daemon/src/ipc/mod.rs`
- Create: `crates/jyowo-harness-daemon/src/ipc/codec.rs`
- Create: `crates/jyowo-harness-daemon/src/ipc/server.rs`
- Create: `crates/jyowo-harness-daemon/src/ipc/transport_unix.rs`
- Create: `crates/jyowo-harness-daemon/src/ipc/transport_windows.rs`
- Create: `crates/jyowo-harness-daemon/src/lifecycle.rs`
- Create: `crates/jyowo-harness-daemon/src/bin/jyowo-harness-daemon.rs`
- Create: `crates/jyowo-harness-daemon/tests/ipc.rs`
- Modify: `crates/jyowo-harness-daemon/src/lib.rs`
- Modify: `crates/jyowo-harness-daemon/Cargo.toml`

**Step 1: Write failing codec and multi-client tests**

Test fragmented frames, coalesced frames, zero/oversize lengths, invalid JSON, protocol mismatch, invalid token, slow subscribers, duplicate command delivery, and two clients observing the same global offsets. On Unix, assert no TCP listener exists and socket permissions are owner-only. Keep the Windows transport behind `cfg(windows)` and compile-check it.

**Step 2: Run IPC tests to verify failure**

Run: `cargo test -p jyowo-harness-daemon --test ipc`

Expected: FAIL because IPC transport is missing.

**Step 3: Implement bounded length-prefixed framing**

Use a 4-byte big-endian length followed by UTF-8 JSON. Reject frames larger than 8 MiB before allocation. Handshake must verify protocol version, client/daemon version compatibility, user-instance ID, connection token, and last acknowledged offset. Do not log raw frames or tokens.

**Step 4: Implement local transports and subscriptions**

Use Unix Domain Socket on macOS/Linux and Tokio Named Pipe on Windows. Create the endpoint under an application-owned runtime directory, remove stale endpoints only after checking owner/lock identity, and set owner-only access. Each client gets bounded response and event queues. On lag, send a gap response containing the last committed offset; the client must resnapshot and resume.

**Step 5: Implement daemon lifecycle**

Acquire one per-user lock, write an ephemeral connection token with owner-only permissions, recover TaskStore, start transports, and publish readiness. Exit after five minutes only when there are no clients, active tasks, or background processes. A client disconnect never stops work.

**Step 6: Run cross-platform checks and reviews**

Run:

```bash
cargo test -p jyowo-harness-daemon --test ipc
cargo test -p jyowo-harness-daemon
cargo check -p jyowo-harness-daemon --target x86_64-pc-windows-msvc
```

Expected: native tests pass; Windows check passes when the target is installed. If the target is unavailable, record that fact and let CI perform the Windows gate. Run `/code-review-expert` and `/security-review`.

**Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/jyowo-harness-daemon
git commit -m "feat: expose daemon over secure local ipc"
```

### Task 13: Replace the Tauri supervisor with a thin daemon sidecar bridge

**Files:**

- Create: `apps/desktop/src-tauri/src/daemon_client.rs`
- Create: `apps/desktop/src-tauri/src/commands/daemon.rs`
- Create: `apps/desktop/src-tauri/tests/daemon_bridge.rs`
- Create: `scripts/daemon-sidecar-utils.mjs`
- Create: `scripts/build-daemon-sidecar.mjs`
- Create: `scripts/build-daemon-sidecar.test.mjs`
- Create: `scripts/check-daemon-sidecar.mjs`
- Create: `scripts/check-daemon-sidecar.test.mjs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/build.rs`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`
- Modify: `apps/desktop/src-tauri/binaries/README.md`
- Modify: `apps/desktop/package.json`
- Modify: `package.json`

**Step 1: Write failing bridge and packaging-policy tests**

Require Tauri commands for `daemon_connect`, `daemon_request`, `daemon_subscribe`, `daemon_unsubscribe`, and `daemon_read_blob`. Assert the bridge forwards validated frames, reconnects after daemon restart, never accepts a blob path, and does not contain task transition logic. Update script policy tests to require `binaries/jyowo-harness-daemon` in `bundle.externalBin`.

**Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p jyowo-desktop-shell --test daemon_bridge
node --test scripts/build-daemon-sidecar.test.mjs scripts/check-daemon-sidecar.test.mjs
```

Expected: FAIL because the bridge and scripts do not exist.

**Step 3: Implement the thin client and Tauri commands**

The Rust client discovers/starts the sidecar, reads the token file, performs handshake, multiplexes request IDs, and forwards server event batches through a single Tauri event name. Tauri must not open TaskStore, run Harness, decide queue transitions, resolve permissions, or project timeline state.

**Step 4: Replace build and bundle wiring**

Build `jyowo-harness-daemon` for the active target triple, copy it to Tauri's required sidecar filename, validate it in `build.rs`, and update root/desktop scripts to run `build:daemon-sidecar`. Keep the old supervisor files temporarily so old UI tests can run until Task 19; remove all references from the active Tauri handler now.

**Step 5: Run integration tests and reviews**

Run:

```bash
pnpm build:daemon-sidecar
pnpm check:daemon-sidecar
cargo test -p jyowo-desktop-shell --test daemon_bridge
cargo test -p jyowo-desktop-shell
```

Expected: PASS and no listener uses loopback TCP. Run `/code-review-expert` and `/security-review`.

**Step 6: Commit**

```bash
git add package.json scripts/daemon-sidecar-utils.mjs scripts/build-daemon-sidecar.mjs scripts/build-daemon-sidecar.test.mjs scripts/check-daemon-sidecar.mjs scripts/check-daemon-sidecar.test.mjs apps/desktop/package.json apps/desktop/src-tauri
git commit -m "feat: bridge desktop to task daemon"
```

### Task 14: Consume generated contracts and repair global-offset gaps in React

**Files:**

- Create: `apps/desktop/src/shared/daemon/client.ts`
- Create: `apps/desktop/src/shared/daemon/client.test.ts`
- Create: `apps/desktop/src/features/tasks/task-store.ts`
- Create: `apps/desktop/src/features/tasks/task-store.test.ts`
- Create: `apps/desktop/src/features/tasks/use-task.ts`
- Create: `apps/desktop/src/features/tasks/use-task-events.ts`
- Create: `apps/desktop/src/features/tasks/use-task-events.test.tsx`
- Modify: `apps/desktop/src/shared/tauri/default-client.ts`
- Modify: `apps/desktop/src/shared/tauri/react.tsx`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/events/run-event-schema.ts`

**Step 1: Write failing client synchronization tests**

Start from snapshot offset 40, deliver offsets `41, 42, 42, 44`, and assert:

- 41 and 42 apply once;
- duplicate 42 is ignored;
- 44 is held and triggers one resnapshot request after offset 42;
- snapshot offset 50 replaces projection state;
- subscription resumes at 50;
- an invalid generated frame puts the connection in `protocol_error`, not partial state.

Also simulate two browser windows receiving the same events in different batch boundaries and assert equal final state.

**Step 2: Run tests to verify failure**

Run: `pnpm -C apps/desktop test src/shared/daemon/client.test.ts src/features/tasks/task-store.test.ts src/features/tasks/use-task-events.test.tsx`

Expected: FAIL because the daemon client and task store are missing.

**Step 3: Implement the typed daemon client**

Expose methods generated from `ClientRequest`; parse every server frame with Ajv before dispatch. Keep the raw Tauri bridge private. Blob reads take `BlobId` and return bytes/media metadata. Do not duplicate Rust enum definitions with Zod or handwritten unions.

**Step 4: Implement projection plus offset state**

Use a per-task store containing `snapshot`, `lastAppliedOffset`, `connectionState`, and a bounded pending batch. Reduce events only when `offset === lastAppliedOffset + 1`. On gap or server lag signal, fetch an authoritative snapshot, replace state, and resubscribe. Batch token-chunk rendering on animation frames without changing committed offset order.

**Step 5: Remove task protocol definitions from handwritten validators**

Delete only conversation/run/task DTOs superseded by generated daemon contracts from `commands.ts` and `run-event-schema.ts`. Keep unrelated settings, plugins, memory, and provider command schemas. Add a policy test that fails if a daemon event type is declared outside `src/generated` or `shared/daemon`.

**Step 6: Run tests and commit**

Run:

```bash
pnpm generate:daemon-protocol
pnpm -C apps/desktop test src/shared/daemon src/features/tasks
pnpm -C apps/desktop typecheck
```

Expected: PASS. Run `/code-review-expert`, then:

```bash
git add apps/desktop/src/generated apps/desktop/src/shared/daemon apps/desktop/src/shared/tauri apps/desktop/src/shared/events apps/desktop/src/features/tasks
git commit -m "feat: synchronize desktop from daemon offsets"
```

### Task 15: Render the Codex single-column task timeline

**Files:**

- Create: `apps/desktop/src/features/tasks/TaskWorkspace.tsx`
- Create: `apps/desktop/src/features/tasks/TaskWorkspace.test.tsx`
- Create: `apps/desktop/src/features/tasks/timeline/TaskTimeline.tsx`
- Create: `apps/desktop/src/features/tasks/timeline/TaskTimeline.test.tsx`
- Create: `apps/desktop/src/features/tasks/timeline/RunSegment.tsx`
- Create: `apps/desktop/src/features/tasks/timeline/UserMessage.tsx`
- Create: `apps/desktop/src/features/tasks/timeline/TimelineEvent.tsx`
- Create: `apps/desktop/src/features/tasks/timeline/ArtifactContainer.tsx`
- Create: `apps/desktop/src/features/tasks/timeline/task-timeline.stories.tsx`
- Modify: `apps/desktop/src/features/conversation/timeline/conversation-scroll-controller.ts`
- Modify: `apps/desktop/src/features/conversation/timeline/use-conversation-scroll-anchor.ts`
- Modify: `apps/desktop/src/routes/index.tsx`
- Modify: `apps/desktop/src/routes/index.lazy.tsx`
- Modify: `apps/desktop/src/app/shell/AppShell.tsx`

**Step 1: Read `@frontend-design` and write failing component tests**

Build one fixture containing two run segments, narrative text, a lightweight tool row, command output, diff, image, permission, compaction notice, subagent event, partial assistant text, and a forced interruption. Assert DOM order exactly matches global offsets. Assert user messages are right-aligned bubbles and assistant narrative has no enclosing card/bubble.

**Step 2: Run timeline tests to verify failure**

Run: `pnpm -C apps/desktop test src/features/tasks/timeline src/features/tasks/TaskWorkspace.test.tsx`

Expected: FAIL because task timeline components are missing.

**Step 3: Implement the timeline hierarchy**

Render one centered reading column with a 760–840 px desktop width:

```text
UserMessage (right-aligned bubble)
RunSegment
  status + elapsed time
  divider
  ordered narrative and event rows
  terminal status
```

Use rows for reads, searches, short tool results, compaction, and subagent status. Use `ArtifactContainer` only for command sessions, diffs, images, permission decisions, long output, and errors requiring action. Keep timestamps and IDs out of the primary visual hierarchy but accessible in details.

**Step 4: Implement event ordering and large-history behavior**

Derive render blocks only from committed snapshot/events. Coalesce adjacent narrative chunks from the same segment without crossing another event. Preserve incomplete output. Reuse the current virtual/scroll-anchor logic so prepending older history does not move the viewport and new output autoscrolls only when the user is near the bottom.

**Step 5: Switch the route to task IDs**

Change `/` search from `conversationId` to `taskId`, render `TaskWorkspace`, and update shell active-run selection to generated task projections. Do not keep a fallback that opens old conversations.

**Step 6: Run focused tests and Storybook**

Run:

```bash
pnpm -C apps/desktop test src/features/tasks src/routes src/app/shell/AppShell.test.tsx
pnpm -C apps/desktop build-storybook
```

Expected: PASS; the Codex fixture story renders without console or accessibility errors.

**Step 7: Run review and commit**

Run `/code-review-expert`, then:

```bash
git add apps/desktop/src/features/tasks apps/desktop/src/features/conversation/timeline/conversation-scroll-controller.ts apps/desktop/src/features/conversation/timeline/use-conversation-scroll-anchor.ts apps/desktop/src/routes apps/desktop/src/app/shell
git commit -m "feat: render Codex task timeline"
```

### Task 16: Add the editable queue above an always-available Composer

**Files:**

- Create: `apps/desktop/src/features/tasks/queue/QueuedMessages.tsx`
- Create: `apps/desktop/src/features/tasks/queue/QueuedMessageRow.tsx`
- Create: `apps/desktop/src/features/tasks/queue/QueuedMessageEditor.tsx`
- Create: `apps/desktop/src/features/tasks/queue/QueuedMessages.test.tsx`
- Create: `apps/desktop/src/features/tasks/TaskComposer.tsx`
- Create: `apps/desktop/src/features/tasks/TaskComposer.test.tsx`
- Modify: `apps/desktop/src/features/tasks/TaskWorkspace.tsx`
- Modify: `apps/desktop/src/features/conversation/Composer.tsx`
- Modify: `apps/desktop/src/features/conversation/composer/composer-draft-store.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`

**Step 1: Write failing queue interaction tests**

While a run is active, submit two messages and assert both appear above the composer as queued rather than in the timeline. Test edit, delete, safe promotion, and `Stop now and run`. Add a stale-revision response that replaces the local row with the latest server value and announces the conflict. Verify `promoting` disables edit/delete, `consumed` disappears from the queue and appears in timeline, and `deleted` is omitted.

**Step 2: Run tests to verify failure**

Run: `pnpm -C apps/desktop test src/features/tasks/queue src/features/tasks/TaskComposer.test.tsx`

Expected: FAIL because queue UI is missing and the current composer disables during runs.

**Step 3: Make the composer state-aware but always send-capable**

Reuse the editor, references, attachment picker, model selector, permission mode, and draft persistence. Remove `running-disabled` from the active task path. Button semantics are:

- idle: `Send` and start a segment;
- running/waiting/yielding: `Queue`;
- submitting: disable only duplicate submission;
- disconnected: preserve the draft and show retryable connection state.

The server decides whether submission starts immediately or queues; the client does not infer the transition.

**Step 4: Implement queue controls**

Place a compact queue region directly above the composer. Each row shows order, one-line content/attachment summary, state text, and actions. Editing expands inline inside that row so the main composer remains available; do not open a modal. Safe promotion is primary. Force promotion requires a confirmation that states running processes are terminated when possible and committed side effects are not rolled back. Use generated command DTOs with idempotency keys and expected revisions.

**Step 5: Run interaction and workspace tests**

Run:

```bash
pnpm -C apps/desktop test src/features/tasks/queue src/features/tasks/TaskComposer.test.tsx src/features/tasks/TaskWorkspace.test.tsx
pnpm -C apps/desktop typecheck
```

Expected: PASS; sending during a run never disables the editor or inserts an optimistic timeline turn.

**Step 6: Run review and commit**

Run `/code-review-expert`, then:

```bash
git add apps/desktop/src/features/tasks apps/desktop/src/features/conversation/Composer.tsx apps/desktop/src/features/conversation/composer/composer-draft-store.ts apps/desktop/src/shared/i18n/locales
git commit -m "feat: add steerable task queue composer"
```

### Task 17: Build the Codex workbench and task navigation projections

**Files:**

- Create: `apps/desktop/src/features/tasks/workbench/TaskWorkbench.tsx`
- Create: `apps/desktop/src/features/tasks/workbench/TaskWorkbench.test.tsx`
- Create: `apps/desktop/src/features/tasks/workbench/DiffPanel.tsx`
- Create: `apps/desktop/src/features/tasks/workbench/CommandPanel.tsx`
- Create: `apps/desktop/src/features/tasks/workbench/SubagentsPanel.tsx`
- Create: `apps/desktop/src/features/tasks/workbench/EnvironmentPanel.tsx`
- Create: `apps/desktop/src/features/tasks/workbench/SourcesPanel.tsx`
- Create: `apps/desktop/src/features/tasks/workbench/AuditPanel.tsx`
- Create: `apps/desktop/src/features/tasks/RunStatusBar.tsx`
- Create: `apps/desktop/src/features/tasks/RunStatusBar.test.tsx`
- Create: `apps/desktop/src/features/tasks/TaskList.tsx`
- Create: `apps/desktop/src/features/tasks/TaskList.test.tsx`
- Modify: `apps/desktop/src/features/tasks/TaskWorkspace.tsx`
- Modify: `apps/desktop/src/features/workbench/WorkbenchInspector.tsx`
- Modify: `apps/desktop/src/features/workspace/SidebarNav.tsx`
- Modify: `apps/desktop/src/shared/state/ui-store.ts`
- Modify: `apps/desktop/src/shared/state/workbench-selection.ts`

**Step 1: Write failing selection and projection tests**

Select a diff, command, subagent, environment, source, and audit event from the timeline and assert the corresponding right panel opens with the same task/segment/event IDs. Switching tasks must clear stale selection. Test task navigation status text for running, queued, waiting permission, interrupted, failed, and completed states. Test the bottom status surface with current step, elapsed time, queue count, and file-change summary.

**Step 2: Run tests to verify failure**

Run: `pnpm -C apps/desktop test src/features/tasks/workbench src/features/tasks/TaskList.test.tsx`

Expected: FAIL because the task workbench projection UI is missing.

**Step 3: Implement projection-driven panels**

Reuse existing diff, command output, artifact preview, and disclosure primitives. Load full artifacts by blob ID only when a panel opens. The tabs are `Changes`, `Commands`, `Agents`, `Environment`, `Sources`, and `Audit`. Show missing blobs explicitly. Keep the timeline summary usable when the right panel is closed.

**Step 4: Add workbench width modes and the bottom run status surface**

Support closed, 360–440 px inspector, and 45–50% collaboration modes. Persist only the user's mode, not an obsolete event selection. On narrow layouts, close or overlay the inspector before reducing the timeline below its readable width. Pin `RunStatusBar` below the workspace while a segment is active; its state comes from the current run and change-set projections.

**Step 5: Replace conversation navigation with task navigation**

Render TaskStore list projections in the sidebar, grouped by active/recent/archived if the projection supplies those states. Display status icon plus text/accessible label; color is secondary. Creating a task calls the daemon and navigates to `?taskId=...`. Do not query the old conversation read model.

**Step 6: Run tests and commit**

Run:

```bash
pnpm -C apps/desktop test src/features/tasks src/features/workbench src/features/workspace/SidebarNav.test.tsx src/shared/state/ui-store.test.ts
pnpm -C apps/desktop typecheck
```

Expected: PASS. Run `/code-review-expert`, then:

```bash
git add apps/desktop/src/features/tasks apps/desktop/src/features/workbench/WorkbenchInspector.tsx apps/desktop/src/features/workspace/SidebarNav.tsx apps/desktop/src/shared/state/ui-store.ts apps/desktop/src/shared/state/workbench-selection.ts
git commit -m "feat: add task workbench and navigation"
```

### Task 18: Finalize semantic themes, accessibility, and real visual regression

**Files:**

- Create: `apps/desktop/e2e/task-workspace-visual.spec.ts`
- Create: `apps/desktop/e2e/task-workspace-accessibility.spec.ts`
- Modify: `apps/desktop/src/shared/styles/global.css`
- Modify: `apps/desktop/src/shared/local-store/ui-preferences-store.ts`
- Modify: `apps/desktop/src/shared/state/ui-store.ts`
- Modify: `apps/desktop/src/app/providers.tsx`
- Modify: `apps/desktop/.storybook/preview.tsx`
- Modify: `apps/desktop/src/features/tasks/timeline/task-timeline.stories.tsx`
- Modify: `apps/desktop/e2e/conversation-evidence-storybook.spec.ts`
- Modify: `apps/desktop/playwright.storybook.config.ts`

**Step 1: Write failing semantic-token and accessibility tests**

Require `light`, `dark`, and `system`; system must react to `prefers-color-scheme` changes without restart. Test keyboard access for queue actions, composer, timeline artifact disclosures, workbench tabs, permission decisions, and task list. Assert focus is visible, status has text, live regions announce new queued messages/permission requests, and motion is reduced when requested.

**Step 2: Replace screenshot-size checks with pixel baselines**

For 1280×900 and 900×760 viewports, add `expect(page).toHaveScreenshot(...)` baselines for:

- idle task;
- active streaming segment with two queued messages;
- permission waiting;
- failed command and large diff;
- interrupted recovery with an indeterminate tool;
- open workbench;
- light and dark modes.

Mask elapsed time, cursors, and other nondeterministic regions. Do not use `screenshot.length` as a visual assertion.

**Step 3: Run tests to verify failure**

Run:

```bash
pnpm -C apps/desktop test src/shared/local-store/ui-preferences-store.test.ts src/shared/state/ui-store.test.ts
pnpm -C apps/desktop test:e2e:storybook -- task-workspace-visual.spec.ts task-workspace-accessibility.spec.ts
```

Expected: FAIL until tokens, behavior, and baselines are implemented.

**Step 4: Implement semantic theme tokens**

Define surface, raised surface, border, primary/secondary text, muted row, user bubble, artifact container, focus ring, selection, destructive, and every task/queue/run state token in both themes. Components consume semantic variables only. Set default preference to `system`, apply the effective class before first paint, and preserve the explicit preference in `data-theme`.

**Step 5: Implement accessible behavior and responsive layout**

Use proper buttons, headings, lists, tabs, dialogs, and live regions. Add non-color state labels. Keep composer and queue reachable at narrow widths; collapse the workbench below the reading column rather than shrinking timeline content below its minimum. Respect `prefers-reduced-motion`.

**Step 6: Generate and inspect baselines**

Run: `pnpm -C apps/desktop test:e2e:storybook -- task-workspace-visual.spec.ts --update-snapshots`

Expected: baseline PNGs are created. Inspect every image for alignment, clipping, theme contrast, queue placement, reading-column width, and accidental card styling before accepting them.

**Step 7: Run the complete frontend gate and commit**

Run:

```bash
pnpm check:design-tokens
pnpm -C apps/desktop check
pnpm -C apps/desktop build-storybook
pnpm -C apps/desktop test:e2e:storybook
```

Expected: PASS. Run `/code-review-expert`, then:

```bash
git add apps/desktop
git commit -m "feat: polish Codex workspace themes and accessibility"
```

### Task 19: Prove recovery and scale, then remove the old runtime path

**Files:**

- Create: `crates/jyowo-harness-daemon/tests/fault_injection.rs`
- Create: `crates/jyowo-harness-daemon/tests/concurrency.rs`
- Create: `crates/jyowo-harness-daemon/tests/performance.rs`
- Create: `apps/desktop/e2e/task-daemon-recovery.spec.ts`
- Modify: `scripts/check-agent-orchestration-no-fakes.mjs`
- Modify: `scripts/check-rust-deps.mjs`
- Modify: `package.json`
- Modify: `apps/desktop/package.json`
- Delete: `apps/desktop/src-tauri/src/agent_supervisor.rs`
- Delete: `apps/desktop/src-tauri/src/bin/jyowo-agent-supervisor.rs`
- Delete: `scripts/agent-supervisor-sidecar-utils.mjs`
- Delete: `scripts/build-agent-supervisor-sidecar.mjs`
- Delete: `scripts/build-agent-supervisor-sidecar.test.mjs`
- Delete: `scripts/check-agent-supervisor-sidecar.mjs`
- Delete: `scripts/check-agent-supervisor-sidecar.test.mjs`
- Delete: `apps/desktop/src/features/conversation/timeline/conversation-timeline-source.ts`
- Delete: `apps/desktop/src/features/conversation/timeline/conversation-timeline-store.ts`
- Delete: `apps/desktop/src/features/conversation/timeline/use-conversation-event-stream.ts`
- Delete: corresponding old conversation timeline source/store/stream tests
- Modify or delete after `rg` confirms no consumers: `apps/desktop/src-tauri/src/commands/conversations.rs`
- Modify or delete after `rg` confirms no consumers: `crates/jyowo-harness-journal/src/conversation_read_model.rs`
- Modify or delete after `rg` confirms no consumers: `crates/jyowo-harness-journal/src/conversation_worktree_projector.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `crates/jyowo-harness-journal/src/lib.rs`

**Step 1: Write the final failure and concurrency matrix**

Inject process death before and after each SQLite commit boundary, projection failure, blob rename failure, provider disconnect, unresponsive tool process, client disconnect, slow subscriber, actor panic, and workspace cleanup failure. Run concurrent commands from two clients with duplicate idempotency keys and stale expected versions. Exercise 20 concurrent task actors and 8 concurrent subagents in one task. Assert quota enforcement, event/projection consistency, and no automatic indeterminate-tool replay. For provider retry, assert only the model request count increases; completed or indeterminate tool call counts do not.

**Step 2: Write the 100,000-event performance test**

In release mode, seed 100 tasks and 100,000 mixed timeline events. Measure:

- append plus synchronous projection under 30 seconds on the local test machine;
- projection rebuild under 30 seconds;
- `events_after` for 1,000 events under 250 ms;
- loading one task snapshot plus first timeline page under 500 ms.

Mark the test ignored in debug and run it explicitly in CI/release verification. Log database size and timings so regressions are diagnosable; do not weaken correctness assertions to meet timing.

**Step 3: Run new tests to verify any missing behavior**

Run:

```bash
cargo test -p jyowo-harness-daemon --test fault_injection --test concurrency
cargo test -p jyowo-harness-daemon --release --test performance -- --ignored --nocapture
pnpm -C apps/desktop test:e2e -- task-daemon-recovery.spec.ts
```

Expected: PASS after fixing only defects exposed by these tests. Commit any focused fixes separately with messages describing the invariant repaired.

**Step 4: Prove the old path is unreachable before deleting it**

Run:

```bash
rg -n "AgentSupervisor|jyowo-agent-supervisor|subscribeConversationEvents|pageConversationTimeline|ConversationTimelineSource|ConversationReadModel|conversation_worktree_projector" apps crates scripts package.json
rg -n "TcpListener|TcpStream|control_addr" apps/desktop/src-tauri crates/jyowo-harness-daemon
```

Expected: matches remain only in files listed for deletion or historical docs/tests. `TcpListener`, `TcpStream`, and `control_addr` have no active runtime match.

**Step 5: Delete the old protocol, storage, and supervisor code**

Remove the listed files, old command registrations, old sidecar scripts, old package scripts, obsolete Cargo features/dependencies, and old generated route/test fixtures. For the three conditional files, delete them only if `rg` shows no unrelated consumer; otherwise remove only the obsolete task/conversation projection surface. Do not delete reusable visual primitives, settings APIs, memory, provider configuration, or artifact viewers.

**Step 6: Add architecture guards**

Update policy scripts to fail when:

- Tauri imports Harness, TaskStore, TaskActor, or RunCoordinator;
- daemon code opens `agent-runtime.sqlite`, old conversation read models, JSONL task journals, loopback TCP, or arbitrary client blob paths;
- frontend declares daemon event unions outside generated output;
- more than one task SQLite path is constructed.

**Step 7: Run full verification with required skill**

Apply `@superpowers:verification-before-completion`, then run:

```bash
git status --short
pnpm generate:daemon-protocol
git diff --exit-code -- apps/desktop/src/generated
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
pnpm check:quick
pnpm check:desktop:full
pnpm check:tool-network-broker-boundary
pnpm check:rust
```

Expected: every command passes. `git status --short` contains only the intended final-task changes before commit. Record any platform-only Windows sidecar/Named Pipe gate that CI must execute.

**Step 8: Run final reviews and commit**

Run `/code-review-expert` across the complete branch and `/security-review` across daemon IPC, command validation, permissions, blobs, and workspace boundaries. Resolve all actionable findings, rerun affected gates, then:

```bash
git add -A
git commit -m "refactor: remove legacy conversation runtime"
git status --short
```

Expected: commit succeeds and final status is clean.

## Completion evidence

Before marking the implementation complete, attach or record:

- the full verification command outputs;
- Windows Named Pipe and sidecar CI result;
- 100,000-event timing output and database size;
- light/dark visual baseline diff report;
- fault-injection matrix result;
- `rg` output proving the old supervisor, TCP control plane, conversation event source, and second task store are absent;
- the final `/code-review-expert` and `/security-review` findings with resolutions.
