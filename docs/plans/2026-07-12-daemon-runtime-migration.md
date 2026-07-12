# Daemon Runtime Migration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Complete the daemon migration so task execution, tools, memory, automation, and child-agent management use one daemon authority, while obsolete conversation runtime code is removed.

**Architecture:** Tauri keeps non-task settings persistence. The daemon reads global and workspace configuration directly, resolves an immutable runtime snapshot per run, owns memory and automation execution, and exposes those operations through the versioned daemon protocol. React uses daemon task projections and no longer calls deleted conversation or background-agent commands.

**Tech Stack:** Rust 2021, Tokio, rusqlite, serde/schemars, Tauri 2, React 19, TypeScript 6, TanStack Query, Vitest, Node test runner, pnpm 11.

---

### Task 1: Extend the daemon contract for detached children, memory, tools, and automation

**Files:**
- Modify: `crates/jyowo-harness-contracts/src/daemon.rs`
- Modify: `crates/jyowo-harness-contracts/src/task_projection.rs`
- Modify: `crates/jyowo-harness-contracts/src/automation.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `crates/jyowo-harness-contracts/tests/daemon_contract.rs`
- Create: `crates/jyowo-harness-contracts/tests/daemon_runtime_contract.rs`
- Regenerate: `apps/desktop/src/generated/daemon-protocol.schema.json`
- Regenerate: `apps/desktop/src/generated/daemon-protocol.ts`

**Step 1: Write failing contract tests**

Add tests that serialize and schema-check these request families:

```rust
ClientRequest::ListRuntimeTools { workspace_root: Some(root) }
ClientRequest::ListMemoryItems { workspace_root: Some(root) }
ClientRequest::GetMemoryItem { workspace_root: Some(root), memory_id }
ClientRequest::DeleteMemoryItem { workspace_root: Some(root), memory_id }
ClientRequest::ListAutomations { workspace_root: Some(root) }
ClientRequest::SaveAutomation { workspace_root: Some(root), automation }
ClientRequest::SetAutomationEnabled { workspace_root: Some(root), automation_id, enabled }
ClientRequest::DeleteAutomation { workspace_root: Some(root), automation_id }
ClientRequest::RunAutomationNow { workspace_root: Some(root), automation_id }
ClientRequest::ListAutomationRuns { workspace_root: Some(root), automation_id: None }
```

Add a projection assertion that a child task carries an explicit attachment value:

```rust
assert_eq!(projection.parent.unwrap().attachment, ChildAttachment::Detached);
```

Add matching `ServerMessage` response variants for runtime tools, memory results, automation specs, and automation run records.

**Step 2: Run tests and verify RED**

Run: `cargo test -p jyowo-harness-contracts --test daemon_runtime_contract`

Expected: compilation fails because the new protocol variants and `ChildAttachment` do not exist.

**Step 3: Implement the minimal contract**

Add typed request/response payloads. Reuse existing `AutomationSpec`, `AutomationRunRecord`, and memory contract records rather than copying their fields. Add `Attached` and `Detached` as the only child attachment states. Keep `deny_unknown_fields` and camel-case field naming.

**Step 4: Generate the TypeScript protocol and verify GREEN**

Run: `cargo test -p jyowo-harness-contracts --test daemon_runtime_contract`

Run: `pnpm generate:daemon-protocol`

Run: `pnpm --dir apps/desktop exec vitest run src/shared/daemon/protocol.test.ts`

Expected: all pass and generated files are stable after a second generation.

**Step 5: Commit**

```bash
git add crates/jyowo-harness-contracts apps/desktop/src/generated
git commit -m "feat: extend daemon runtime protocol"
```

### Task 2: Resolve one daemon runtime configuration snapshot per workspace

**Files:**
- Create: `crates/jyowo-harness-daemon/src/runtime_config.rs`
- Modify: `crates/jyowo-harness-daemon/src/lib.rs`
- Modify: `crates/jyowo-harness-daemon/src/provider_config.rs`
- Modify: `crates/jyowo-harness-daemon/src/sdk_run_factory.rs`
- Modify: `crates/jyowo-harness-daemon/src/bin/jyowo-harness-daemon.rs`
- Modify: `crates/jyowo-harness-daemon/Cargo.toml`
- Create: `crates/jyowo-harness-daemon/tests/runtime_config.rs`
- Modify: `crates/jyowo-harness-daemon/tests/fault_injection.rs`
- Modify: `apps/desktop/src-tauri/src/commands/daemon.rs`

**Step 1: Write failing resolver tests**

Create fixtures under a temporary home and workspace. Assert:

- project provider selection overrides the global default;
- project execution overrides merge over global defaults;
- global and project MCP definitions are combined by ID;
- disabled global/project skills and plugins are absent;
- project provider routes override the same operation ID and inherit other global routes;
- project agent profile selection is resolved from global profile definitions;
- malformed configured records return redacted typed errors;
- the selected memory path is stable per canonical workspace, isolated between workspaces, and rooted in daemon-private storage;
- tasks without a workspace resolve the daemon-global memory database;
- replacing a workspace runtime path before or after directory creation cannot redirect SQLite or WAL writes.

Use the wished-for API:

```rust
let resolver = RuntimeConfigResolver::new(global_root);
let snapshot = resolver.resolve(&workspace_root, Some("model-config"))?;
assert!(snapshot.memory_database_path.starts_with(global_home.join("runtime/workspaces")));
assert!(!snapshot.memory_database_path.starts_with(&workspace_root));
```

**Step 2: Run tests and verify RED**

Run: `cargo test -p jyowo-harness-daemon --test runtime_config`

Expected: compilation fails because `RuntimeConfigResolver` is missing.

**Step 3: Implement immutable snapshot loading**

Load only established JSON formats from `~/.jyowo/config`, `~/.jyowo/{skills,plugins}`, and `<workspace>/.jyowo/{config,skills,plugins}`. Canonicalize the workspace before reading project paths. Treat absent project records as inheritance and malformed present records as errors.

Construct SDK inputs in the snapshot:

```rust
pub struct RuntimeConfigSnapshot {
    pub provider: ResolvedProviderConfig,
    pub execution_defaults: ExecutionDefaultsRecord,
    pub provider_routes: ProviderCapabilityRouteSettings,
    pub mcp_config: McpConfig,
    pub plugin_registry: PluginRegistry,
    pub skill_loader: SkillLoader,
    pub skill_config: SkillConfigSnapshot,
    pub agent_profiles: Vec<AgentProfile>,
    pub memory_database_path: PathBuf,
}
```

Pass `JYOWO_CONFIG_DIR` from Tauri when launching the daemon. Install the network broker and provider credential resolver in the daemon factory. Apply `with_mcp_config`, `with_plugin_registry`, `with_skill_loader`, `with_skill_config_snapshot`, and `with_provider_capability_routes` to both foreground and subagent Harness builders.

**Step 4: Verify GREEN and capability parity**

Run: `cargo test -p jyowo-harness-daemon --test runtime_config`

Run: `cargo test -p jyowo-harness-daemon sdk_run_factory::tests`

Run: `cargo test -p jyowo-harness-daemon --test fault_injection production_binary_assembles_the_sdk_factory_and_real_subagent_runner`

Expected: all pass and foreground/subagent snapshots expose the same configured capabilities.

**Step 5: Commit**

```bash
git add crates/jyowo-harness-daemon apps/desktop/src-tauri/src/commands/daemon.rs Cargo.lock
git commit -m "feat: assemble daemon runtime from workspace settings"
```

### Task 3: Move Memory management to the daemon database

**Files:**
- Create: `crates/jyowo-harness-daemon/src/memory_service.rs`
- Modify: `crates/jyowo-harness-daemon/src/lib.rs`
- Modify: `crates/jyowo-harness-daemon/src/ipc/server.rs`
- Create: `crates/jyowo-harness-daemon/tests/memory_service.rs`
- Modify: `crates/jyowo-harness-daemon/tests/ipc.rs`
- Modify: `apps/desktop/src/shared/daemon/client.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/features/memory/*`
- Delete or reduce: `apps/desktop/src-tauri/src/commands/memory.rs`
- Delete or reduce: `apps/desktop/src-tauri/src/commands/memory_settings.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`

**Step 1: Write failing service and UI tests**

Create a task-run memory item through `LocalMemoryProvider`, then list, update, and delete it through `MemoryService` using the same workspace. Assert the same SQLite path is opened.

Add a frontend test whose command client only exposes daemon requests; render the Memory page and assert list/update/delete requests include the active workspace root.

**Step 2: Run tests and verify RED**

Run: `cargo test -p jyowo-harness-daemon --test memory_service`

Run: `pnpm --dir apps/desktop exec vitest run src/features/memory`

Expected: missing service/protocol client behavior.

**Step 3: Implement the daemon service and remove the second owner**

Resolve workspace and no-workspace database paths through `RuntimeConfigResolver`, using the same daemon-private workspace-key mapping as task execution. Implement existing memory list/get/update/delete/candidate/settings operations against that provider. Route protocol requests in `ipc/server.rs`. Remove `DesktopSettingsRuntime` memory access and its memory database initialization.

**Step 4: Verify GREEN**

Run both RED commands again, then run:

`cargo test -p jyowo-harness-daemon --test ipc memory`

Expected: task and UI operations observe the same records.

**Step 5: Commit**

```bash
git add crates/jyowo-harness-daemon apps/desktop/src apps/desktop/src-tauri/src/commands
git commit -m "feat: make daemon memory authoritative"
```

### Task 4: Add the durable daemon automation scheduler

**Files:**
- Create: `crates/jyowo-harness-daemon/src/automation.rs`
- Modify: `crates/jyowo-harness-daemon/src/lib.rs`
- Modify: `crates/jyowo-harness-daemon/src/bin/jyowo-harness-daemon.rs`
- Modify: `crates/jyowo-harness-daemon/src/ipc/server.rs`
- Modify: `crates/jyowo-harness-journal/src/task_schema.rs`
- Modify: `crates/jyowo-harness-journal/src/task_store.rs`
- Create: `crates/jyowo-harness-daemon/tests/automation_scheduler.rs`
- Modify: `crates/jyowo-harness-daemon/tests/recovery.rs`
- Modify: `apps/desktop/src/features/settings/AutomationSettings.tsx`
- Modify: `apps/desktop/src/features/settings/AutomationSettings.test.tsx`
- Delete or reduce: `apps/desktop/src-tauri/src/commands/automations.rs`

**Step 1: Write failing scheduler tests with paused Tokio time**

Cover one behavior per test:

- run-now creates one daemon task and submits the stored prompt;
- a second request is rejected while the automation task is active;
- one interval creates one run after the due time;
- `skip` advances the schedule without replaying missed intervals;
- `run_once` creates one catch-up run after restart;
- committed run history survives restart;
- invalid workspace/configuration records a rejected run without creating a task.

**Step 2: Run tests and verify RED**

Run: `cargo test -p jyowo-harness-daemon --test automation_scheduler`

Expected: compilation fails because `AutomationScheduler` is missing.

**Step 3: Implement scheduler persistence and execution**

Persist scheduler cursor, active task ID, next due time, and run records in daemon-owned SQLite tables. Keep automation specs in the existing settings JSON files. Use the existing supervisor command path to create a normal task and submit its prompt. Never call the SDK Harness directly from the scheduler.

Start one scheduler loop with the daemon. Wake it on settings mutations and timer expiry. Use committed task projection state to clear active runs and determine completion.

**Step 4: Switch the settings page to daemon protocol and verify GREEN**

Run: `cargo test -p jyowo-harness-daemon --test automation_scheduler`

Run: `pnpm --dir apps/desktop exec vitest run src/features/settings/AutomationSettings.test.tsx`

Expected: run-now and scheduled execution use daemon requests; no Tauri `run_automation_now` invoke remains.

**Step 5: Commit**

```bash
git add crates/jyowo-harness-daemon crates/jyowo-harness-journal apps/desktop/src apps/desktop/src-tauri/src/commands
git commit -m "feat: schedule automations in daemon"
```

### Task 5: Switch Background Agents to detached child tasks and hide children from the sidebar

**Files:**
- Modify: `crates/jyowo-harness-journal/src/task_store.rs`
- Modify: `crates/jyowo-harness-daemon/src/subagent.rs`
- Modify: `crates/jyowo-harness-daemon/src/ipc/server.rs`
- Modify: `crates/jyowo-harness-daemon/tests/agent_starters.rs`
- Modify: `crates/jyowo-harness-daemon/tests/ipc.rs`
- Modify: `apps/desktop/src/features/tasks/TaskList.tsx`
- Modify: `apps/desktop/src/features/tasks/TaskList.test.tsx`
- Rewrite: `apps/desktop/src/features/background-agents/use-background-agents.ts`
- Rewrite: `apps/desktop/src/features/background-agents/BackgroundAgentsPanel.tsx`
- Rewrite: `apps/desktop/src/features/background-agents/BackgroundAgentsPanel.test.tsx`

**Step 1: Write failing projection and UI tests**

Assert the daemon projection differentiates attached and detached children. Assert `groupSidebarTasks` excludes any task with `parent`. Assert the Background Agents query selects only `parent.attachment === 'detached'` and maps actions to submit, stop, continue, archive, and remove daemon requests.

**Step 2: Run tests and verify RED**

Run: `cargo test -p jyowo-harness-daemon --test agent_starters`

Run: `pnpm --dir apps/desktop exec vitest run src/features/tasks/TaskList.test.tsx src/features/background-agents/BackgroundAgentsPanel.test.tsx`

Expected: projection lacks attachment and the UI still invokes legacy commands.

**Step 3: Implement projection and frontend mapping**

Persist attachment mode with the parent link and project it into `TaskProjection`. Keep `ListTasks` complete for clients, but filter ordinary sidebar presentation in React. Delete the old background-agent command hooks.

**Step 4: Verify GREEN**

Run the RED commands again and `cargo test -p jyowo-harness-daemon --test ipc`.

**Step 5: Commit**

```bash
git add crates/jyowo-harness-journal crates/jyowo-harness-daemon apps/desktop/src/features
git commit -m "feat: manage background agents as daemon child tasks"
```

### Task 6: Delete the obsolete conversation, eval, artifact, evidence, and workbench surface

**Files:**
- Delete: `apps/desktop/src/routes/evals.lazy.tsx`
- Modify: `apps/desktop/src/routeTree.gen.ts`
- Modify: `apps/desktop/src/features/workspace/SidebarNav.tsx`
- Delete: `apps/desktop/src/features/evals/**`
- Delete: `apps/desktop/src/features/artifacts/**`
- Delete: `apps/desktop/src/features/conversation/WelcomeWorkspace*`
- Delete: `apps/desktop/src/features/conversation/evidence/**`
- Delete: `apps/desktop/src/features/conversation/timeline/**`
- Delete: `apps/desktop/src/features/workbench/WorkbenchInspector*`
- Delete: `apps/desktop/src/features/workbench/artifacts/**`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Modify: `apps/desktop/src/testing/command-client/**`
- Delete obsolete Rust exports from: `apps/desktop/src-tauri/src/commands/**`

**Step 1: Add a failing legacy-surface boundary test**

Create `scripts/check-no-legacy-conversation-surface.test.mjs`. Scan production TypeScript for the removed route/imports and scan `commands.ts` for the 36 deleted invoke names. The fixture containing a legacy call must fail.

**Step 2: Run test and verify RED**

Run: `node --test scripts/check-no-legacy-conversation-surface.test.mjs`

Expected: fails on the current production tree.

**Step 3: Delete old code and narrow CommandClient**

Remove types, schemas, methods, helpers, mocks, fixtures, route entries, stories, and tests that only support the deleted runtime. Keep daemon task UI and settings commands. Regenerate the route tree with the existing TanStack Router command instead of hand-maintaining deleted route imports.

**Step 4: Verify GREEN and TypeScript reachability**

Run: `node --test scripts/check-no-legacy-conversation-surface.test.mjs`

Run: `pnpm --dir apps/desktop typecheck`

Run: `pnpm --dir apps/desktop test`

Expected: no removed command or module remains reachable.

**Step 5: Commit**

```bash
git add apps/desktop scripts
git commit -m "refactor: remove legacy conversation surface"
```

### Task 7: Enforce command registration and runtime-boundary consistency

**Files:**
- Create: `scripts/check-tauri-command-registration.mjs`
- Create: `scripts/check-tauri-command-registration.test.mjs`
- Modify: `package.json`
- Modify: `scripts/check-daemon-agent-capability-boundary.mjs`
- Modify: `scripts/check-agent-orchestration-no-fakes.mjs`

**Step 1: Write failing parser tests**

Feed fixtures containing:

- a `const command = 'missing_command'` absent from `generate_handler!`;
- a registered command;
- a daemon-only client request that does not use Tauri invoke.

Assert only the missing Tauri invoke fails.

**Step 2: Run tests and verify RED**

Run: `node --test scripts/check-tauri-command-registration.test.mjs`

Expected: fails because the checker is missing.

**Step 3: Implement and wire the gate**

Parse invoke command literals from the production Tauri client and registered handler paths from `apps/desktop/src-tauri/src/lib.rs`. Report sorted missing and orphaned names. Add root scripts for the checker and the legacy-surface checker.

Extend daemon boundaries to reject new task runtime assembly in Tauri and to require daemon-side MCP/plugin/skill/provider-route assembly calls.

**Step 4: Verify GREEN**

Run:

```bash
node --test scripts/check-tauri-command-registration.test.mjs
node scripts/check-tauri-command-registration.mjs
node scripts/check-no-legacy-conversation-surface.mjs
pnpm check:daemon-agent-capability-boundary
pnpm check:agent-orchestration-no-fakes
```

Expected: all pass.

**Step 5: Commit**

```bash
git add scripts package.json
git commit -m "test: enforce daemon migration boundaries"
```

### Task 8: Final integration verification

**Files:**
- Modify only files needed to resolve integration failures caused by Tasks 1–7.

**Step 1: Format and generated-code checks**

Run:

```bash
cargo fmt --all -- --check
pnpm generate:daemon-protocol
git diff --exit-code -- apps/desktop/src/generated
git diff --check
```

**Step 2: Rust verification**

Run:

```bash
cargo test -p jyowo-harness-contracts
cargo test -p jyowo-harness-journal
cargo test -p jyowo-harness-daemon
cargo clippy -p jyowo-harness-daemon --all-targets -- -D warnings
```

**Step 3: Desktop verification**

Run:

```bash
pnpm --dir apps/desktop typecheck
pnpm --dir apps/desktop lint
pnpm --dir apps/desktop test
```

**Step 4: Architecture verification**

Run all root `check:*` scripts related to daemon, commands, orchestration, production boundaries, and generated protocols. Confirm `rg` finds no deleted command names in production source.

**Step 5: Review repository state and commit integration fixes**

```bash
git status --short
git diff --check
git log --oneline --decorate -8
```

Commit only verified integration changes. Do not merge or overwrite the original worktree's user modifications.
