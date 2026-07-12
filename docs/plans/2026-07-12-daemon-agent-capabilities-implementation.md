# Daemon-Native Agent Capabilities Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make subagents, agent teams, and background agents executable through the task daemon, expose their availability to desktop settings, and delete the legacy desktop capability runtime.

**Architecture:** The authenticated daemon handshake is the only capability source of truth. Every agent execution is a daemon-owned durable child task created by `SubagentSupervisor`; teams group those children with durable team events, while background agents detach the child before the tool returns. Desktop settings combine daemon capabilities with saved preferences and reject invalid dependency combinations.

**Tech Stack:** Rust, Tokio, serde/schemars, Tauri 2, TypeScript, React, Vitest, pnpm, Cargo.

---

### Task 1: Publish executable daemon capabilities in the handshake

**Files:**
- Modify: `crates/jyowo-harness-contracts/src/daemon.rs`
- Modify: `crates/jyowo-harness-contracts/tests/daemon_contract.rs`
- Modify: `crates/jyowo-harness-daemon/src/ipc/server.rs`
- Modify: `crates/jyowo-harness-daemon/tests/ipc.rs`
- Modify: `apps/desktop/src-tauri/tests/daemon_bridge.rs`
- Regenerate: `apps/desktop/src/generated/daemon-protocol.schema.json`
- Regenerate: `apps/desktop/src/generated/daemon-protocol.ts`

**Step 1: Write the failing contract and IPC tests**

Add a handshake assertion for this shape:

```rust
AgentCapabilities {
    subagents: true,
    agent_teams: true,
    background_agents: true,
}
```

Assert the JSON schema exposes `agentCapabilities` and all three camel-case fields. Update daemon-bridge fixtures only after the contract test has failed for the missing field.

**Step 2: Run tests to verify RED**

Run:

```bash
cargo test -p jyowo-harness-contracts --test daemon_contract
cargo test -p jyowo-harness-daemon --test ipc
```

Expected: compilation or assertion failure because `HandshakeResponse` has no capability payload.

**Step 3: Implement the handshake contract**

Add a closed, serializable `AgentCapabilities` value to `HandshakeResponse`. Construct it in `IpcConnection` from capabilities actually installed in the production daemon. Do not infer support in Tauri or from environment variables.

**Step 4: Regenerate and verify protocol artifacts**

Run:

```bash
pnpm generate:daemon-protocol
pnpm check:daemon-protocol
cargo test -p jyowo-harness-contracts --test daemon_contract
cargo test -p jyowo-harness-daemon --test ipc
```

Expected: all commands exit 0.

**Step 5: Commit**

```bash
git add crates/jyowo-harness-contracts/src/daemon.rs crates/jyowo-harness-contracts/tests/daemon_contract.rs crates/jyowo-harness-daemon/src/ipc/server.rs crates/jyowo-harness-daemon/tests/ipc.rs apps/desktop/src-tauri/tests/daemon_bridge.rs apps/desktop/src/generated/daemon-protocol.schema.json apps/desktop/src/generated/daemon-protocol.ts
git commit -m "feat: publish daemon agent capabilities"
```

### Task 2: Make desktop settings consume authenticated daemon capabilities

**Files:**
- Modify: `apps/desktop/src-tauri/src/daemon_client.rs`
- Modify: `apps/desktop/src-tauri/src/commands/daemon.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/commands/providers.rs`
- Modify: `apps/desktop/src-tauri/tests/daemon_bridge.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/execution_settings.rs`

**Step 1: Write failing cache and settings tests**

Test that a successful authenticated handshake caches its `AgentCapabilities`; reconnect replaces the cached value; disconnected or incompatible daemon access fails closed. Test that execution settings report saved `enabled` values independently from handshake-derived `available` values.

**Step 2: Run tests to verify RED**

Run:

```bash
cargo test -p jyowo-desktop-shell --test daemon_bridge
cargo test -p jyowo-desktop-shell --test command_contracts execution_settings
```

Expected: failure because the bridge cannot expose handshake capabilities and settings still use `AgentCapabilityResolutionContext`.

**Step 3: Implement the authenticated capability cache**

Return both the stream and `HandshakeResponse` from the daemon client's connection path. Keep the last response in the Tauri daemon bridge and expose a read-only capability accessor to settings commands. Clear the cache when the client cannot authenticate or the protocol is incompatible.

**Step 4: Replace the settings resolver**

Map handshake booleans directly to `subagentsAvailable`, `agentTeamsAvailable`, and `backgroundAgentsAvailable`. When no authenticated capability value exists, return all three as unavailable with one typed `daemonUnavailable` reason per capability. Remove workspace/runtime-store/profile probes from this path.

**Step 5: Run tests to verify GREEN**

Run:

```bash
cargo test -p jyowo-desktop-shell --test daemon_bridge
cargo test -p jyowo-desktop-shell --test command_contracts execution_settings
```

Expected: all selected tests pass.

**Step 6: Commit**

```bash
git add apps/desktop/src-tauri/src/daemon_client.rs apps/desktop/src-tauri/src/commands/daemon.rs apps/desktop/src-tauri/src/commands/mod.rs apps/desktop/src-tauri/src/commands/providers.rs apps/desktop/src-tauri/tests/daemon_bridge.rs apps/desktop/src-tauri/tests/commands/execution_settings.rs
git commit -m "fix: resolve settings from daemon capabilities"
```

### Task 3: Enforce agent capability dependencies at storage and policy boundaries

**Files:**
- Modify: `crates/jyowo-harness-contracts/src/global_config.rs`
- Modify: `crates/jyowo-harness-contracts/tests/agent_orchestration_contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/providers.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/execution_settings.rs`
- Modify: `apps/desktop/src/features/settings/ExecutionSettings.tsx`
- Modify: `apps/desktop/src/features/settings/ExecutionSettings.test.tsx`

**Step 1: Write failing dependency tests**

Cover both invariants:

```text
agentTeamsEnabled -> subagentsEnabled
backgroundAgentsEnabled -> subagentsEnabled
```

Test that disabling subagents in the UI sends both dependent preferences as false in the same write. Test that direct invalid Tauri payloads are rejected before persistence.

**Step 2: Run tests to verify RED**

Run:

```bash
cargo test -p jyowo-harness-contracts --test agent_orchestration_contracts
cargo test -p jyowo-desktop-shell --test command_contracts execution_settings
pnpm -C apps/desktop test --run src/features/settings/ExecutionSettings.test.tsx
```

Expected: dependency assertions fail.

**Step 3: Implement minimal validation and atomic normalization**

Add one shared Rust validation helper for `ExecutionDefaultsRecord`. Call it on global and project writes and on daemon run-policy assembly. In React, normalize dependent toggles only when subagents are disabled; do not silently enable subagents when a dependent switch is selected.

**Step 4: Run tests to verify GREEN**

Run the three commands from Step 2. Expected: all pass.

**Step 5: Commit**

```bash
git add crates/jyowo-harness-contracts/src/global_config.rs crates/jyowo-harness-contracts/tests/agent_orchestration_contracts.rs apps/desktop/src-tauri/src/commands/providers.rs apps/desktop/src-tauri/tests/commands/execution_settings.rs apps/desktop/src/features/settings/ExecutionSettings.tsx apps/desktop/src/features/settings/ExecutionSettings.test.tsx
git commit -m "fix: enforce agent capability dependencies"
```

### Task 4: Make SDK team startup a public injected capability

**Files:**
- Modify: `crates/jyowo-harness-contracts/src/capability.rs`
- Modify: `crates/jyowo-harness-contracts/tests/agent_orchestration_contracts.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/tool_pool.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- Modify: `crates/jyowo-harness-sdk/tests/runtime_assembly_agent_policy.rs`

**Step 1: Write failing SDK tests**

Define the expected public `AgentTeamStarterCap`, request, and response contracts. Test that `agent_team` is installed only when policy is allowed and the capability exists, and that executing it forwards the immutable policy/session snapshot to the injected starter.

**Step 2: Run tests to verify RED**

Run:

```bash
cargo test -p jyowo-harness-sdk --test runtime_assembly_agent_policy
```

Expected: compile failure because the public team starter contract does not exist.

**Step 3: Refactor the tool boundary**

Replace private `AgentTeamRunnerCap`, `SdkAgentTeamRunner`, and the legacy runtime-root lookup with `AgentTeamStarterCap` in `harness-contracts`. Keep tool validation, authorization planning, and stable structured output in the SDK. Do not let the SDK open an agent runtime database or own member execution.

**Step 4: Run tests to verify GREEN**

Run:

```bash
cargo test -p jyowo-harness-contracts --test agent_orchestration_contracts
cargo test -p jyowo-harness-sdk --test runtime_assembly_agent_policy
cargo test -p jyowo-harness-sdk harness::tool_pool
```

Expected: all pass.

**Step 5: Commit**

```bash
git add crates/jyowo-harness-contracts/src/capability.rs crates/jyowo-harness-contracts/tests/agent_orchestration_contracts.rs crates/jyowo-harness-sdk/src/harness/tool_pool.rs crates/jyowo-harness-sdk/src/harness/session_runtime.rs crates/jyowo-harness-sdk/tests/runtime_assembly_agent_policy.rs
git commit -m "refactor: inject agent team starter capability"
```

### Task 5: Add daemon-native detached child startup

**Files:**
- Modify: `crates/jyowo-harness-daemon/src/subagent.rs`
- Modify: `crates/jyowo-harness-daemon/src/run_coordinator.rs`
- Modify: `crates/jyowo-harness-daemon/tests/subagent_supervisor.rs`
- Modify: `crates/jyowo-harness-daemon/tests/supervisor.rs`

**Step 1: Write failing lifecycle tests**

Test a new daemon child-start API that returns only after `Starting -> Running -> Background` has committed. Assert the returned ID is the durable `child_task_id`, parent safe/force stop ignores the detached child, startup failure leaves no running child, and detach failure cancels and releases the lease.

**Step 2: Run tests to verify RED**

Run:

```bash
cargo test -p jyowo-harness-daemon --test subagent_supervisor
cargo test -p jyowo-harness-daemon --test supervisor
```

Expected: compile failure because detached startup is not exposed.

**Step 3: Refactor child startup**

Split `SubagentSupervisor::spawn_bound` into a shared start phase and completion phase. Attached callers await the completion receiver. Detached callers durably apply `SubagentLifecycleTransition::Background`, disarm caller cancellation, and return the child task identity while the same finalizer owns completion and lease cleanup.

**Step 4: Run tests to verify GREEN**

Run the two commands from Step 2. Expected: all pass.

**Step 5: Commit**

```bash
git add crates/jyowo-harness-daemon/src/subagent.rs crates/jyowo-harness-daemon/src/run_coordinator.rs crates/jyowo-harness-daemon/tests/subagent_supervisor.rs crates/jyowo-harness-daemon/tests/supervisor.rs
git commit -m "feat: add durable detached daemon children"
```

### Task 6: Install daemon-native background and team starters

**Files:**
- Create: `crates/jyowo-harness-daemon/src/agent_starters.rs`
- Modify: `crates/jyowo-harness-daemon/src/lib.rs`
- Modify: `crates/jyowo-harness-daemon/src/run_coordinator.rs`
- Modify: `crates/jyowo-harness-daemon/src/sdk_run_factory.rs`
- Create: `crates/jyowo-harness-daemon/tests/agent_starters.rs`

**Step 1: Write failing background and team tests**

Background test: start a goal, assert the response ID equals a durable detached child task, then stop the parent and observe the child remain nonterminal or complete independently.

Team test: validate built-in `reviewer` and `worker` profiles before side effects; create a durable `TeamCreated` event; start lead/member child actors through `SubagentSupervisor`; record `TeamMemberJoined`; enforce one active team per parent run and `maxTeamMembers`.

**Step 2: Run tests to verify RED**

Run:

```bash
cargo test -p jyowo-harness-daemon --test agent_starters
```

Expected: compile failure because daemon starter implementations do not exist.

**Step 3: Implement background startup**

Translate `BackgroundAgentToolStartRequest` to the existing `SubagentSpec`, `TurnInput`, and daemon parent binding. Use the detached start API. Return `status: "background"` only after the detach transition is durable.

**Step 4: Implement team startup**

Translate the public team request to one lead and bounded member specs. Use daemon-owned detached child startup for every participant. Persist team/member events in the parent task stream and retain only bounded child references/summaries. If topology/profile validation fails, write nothing.

**Step 5: Inject starters into each SDK run**

Pass run-scoped starter handles from `WorkspaceBoundRunCoordinatorFactory` into `SdkRunCoordinatorFactory`. Install them with `HarnessBuilder::with_capability` only when the corresponding immutable policy is allowed.

**Step 6: Run tests to verify GREEN**

Run:

```bash
cargo test -p jyowo-harness-daemon --test agent_starters
cargo test -p jyowo-harness-daemon sdk_run_factory
```

Expected: all pass.

**Step 7: Commit**

```bash
git add crates/jyowo-harness-daemon/src/agent_starters.rs crates/jyowo-harness-daemon/src/lib.rs crates/jyowo-harness-daemon/src/run_coordinator.rs crates/jyowo-harness-daemon/src/sdk_run_factory.rs crates/jyowo-harness-daemon/tests/agent_starters.rs
git commit -m "feat: run teams and background agents in daemon"
```

### Task 7: Enable all three tools from execution defaults

**Files:**
- Modify: `crates/jyowo-harness-daemon/src/sdk_run_factory.rs`
- Modify: `crates/jyowo-harness-daemon/tests/sdk_run_factory.rs` if present, otherwise module tests in `src/sdk_run_factory.rs`

**Step 1: Write the failing policy matrix test**

Cover all valid preference combinations and assert exact tool policy values. For enabled teams use the built-in coordinator-worker configuration with `reviewer` lead, `worker` member, summaries-only memory, and a bounded member count. Assert invalid dependency combinations are rejected before a run starts.

**Step 2: Run test to verify RED**

Run:

```bash
cargo test -p jyowo-harness-daemon execution_defaults_control_the_immutable_agent_policy
```

Expected: failure because team/background policy is hardcoded off.

**Step 3: Implement policy derivation**

Map all three saved preferences to `AgentUsePolicy`. Set nonzero `max_team_members` and supply team config only when teams are enabled. Keep tool installation policy-gated.

**Step 4: Run tests to verify GREEN**

Run:

```bash
cargo test -p jyowo-harness-daemon sdk_run_factory
cargo test -p jyowo-harness-sdk --test runtime_assembly_agent_policy
```

Expected: all pass.

**Step 5: Commit**

```bash
git add crates/jyowo-harness-daemon/src/sdk_run_factory.rs
git commit -m "feat: enable daemon agent tool policies"
```

### Task 8: Remove the legacy capability runtime and add an architecture guard

**Files:**
- Delete: `crates/jyowo-harness-sdk/src/agent_runtime.rs` capability-resolution facade portions
- Modify: `crates/jyowo-harness-sdk/src/lib.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/accessors.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/policy.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/lib.rs`
- Delete: `crates/jyowo-harness-agent-runtime/tests/agent_orchestration_policy.rs`
- Modify: `crates/jyowo-harness-contracts/src/capability.rs`
- Modify: `crates/jyowo-harness-contracts/tests/agent_orchestration_contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/providers.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/features/settings/ExecutionSettings.tsx`
- Modify: `apps/desktop/src/features/settings/ExecutionSettings.test.tsx`
- Create: `scripts/check-daemon-agent-capability-boundary.mjs`
- Modify: `package.json`

**Step 1: Write the failing architecture guard**

Reject Tauri/desktop references to:

```text
AgentCapabilityResolutionContext
AgentCapabilityEnvironment
AgentCapabilityResolver
AgentRuntimeStore
resolve_agent_capabilities_with_context
```

Also reject reintroduction of legacy unavailable reasons in desktop settings.

**Step 2: Run guard to verify RED**

Run:

```bash
node scripts/check-daemon-agent-capability-boundary.mjs
```

Expected: failure listing current legacy references.

**Step 3: Delete legacy code and narrow reason contracts**

Remove desktop resolver/store probes and SDK exports that only supported it. Replace legacy-only unavailable variants (`NotCompiled`, runtime store, permission runtime, background supervisor, workspace isolation, invalid profile) with `DaemonUnavailable { capability, message }` for the settings boundary. Preserve agent profiles and workspace isolation types still used by daemon execution.

**Step 4: Update generated command schemas and UI reason rendering**

Regenerate the Tauri TypeScript command contract if the repository generator requires it. Render a deterministic daemon-unavailable explanation for each affected switch.

**Step 5: Run focused verification**

Run:

```bash
node scripts/check-daemon-agent-capability-boundary.mjs
cargo test -p jyowo-harness-contracts -p jyowo-harness-agent-runtime -p jyowo-harness-sdk -p jyowo-desktop-shell --no-fail-fast
pnpm -C apps/desktop test --run src/features/settings/ExecutionSettings.test.tsx
```

Expected: guard exits 0 and all tests pass.

**Step 6: Commit**

```bash
git add -A crates/jyowo-harness-contracts crates/jyowo-harness-agent-runtime crates/jyowo-harness-sdk apps/desktop/src-tauri apps/desktop/src/shared/tauri apps/desktop/src/features/settings scripts/check-daemon-agent-capability-boundary.mjs package.json
git commit -m "refactor: remove legacy agent capability runtime"
```

### Task 9: Verify restart, permissions, UI state, and full repository gates

**Files:**
- Modify: `crates/jyowo-harness-daemon/tests/fault_injection.rs`
- Modify: `crates/jyowo-harness-daemon/tests/permission_broker.rs`
- Modify: `apps/desktop/src/features/settings/ExecutionSettings.test.tsx`
- Modify: relevant snapshots only when behavior intentionally changed

**Step 1: Add failing boundary regressions**

Test daemon restart recovery for detached children and team projections, permission requests from team/background child tasks through `PermissionBroker`, and desktop rendering with all three available and enabled without stale warnings.

**Step 2: Run tests to verify RED**

Run the individual new test names. Expected: each fails for the missing recovery, permission, or UI assertion before its minimal production correction.

**Step 3: Implement only required corrections**

Reuse existing task/subagent recovery and permission routing. Do not add another registry, database, or lifecycle service.

**Step 4: Run full verification**

Run:

```bash
cargo fmt --all -- --check
pnpm check:daemon-protocol
node scripts/check-daemon-agent-capability-boundary.mjs
pnpm -C apps/desktop test --run
pnpm -C apps/desktop typecheck
pnpm -C apps/desktop lint
cargo test -p jyowo-harness-daemon -p jyowo-desktop-shell --no-fail-fast
cargo test -p jyowo-harness-contracts -p jyowo-harness-agent-runtime -p jyowo-harness-sdk --no-fail-fast
```

Expected: every command exits 0 with zero failed tests.

**Step 5: Review the final diff for legacy debt**

Run:

```bash
git status --short
git diff --check
rg -n "AgentCapabilityResolutionContext|AgentCapabilityEnvironment|AgentCapabilityResolver|resolve_agent_capabilities_with_context|background_agents_compiled" apps/desktop/src-tauri crates/jyowo-harness-sdk
```

Expected: only intended files are modified, `git diff --check` exits 0, and `rg` finds no legacy desktop/SDK capability-resolution references.

**Step 6: Commit**

```bash
git add crates/jyowo-harness-daemon/tests apps/desktop/src/features/settings
git commit -m "test: verify daemon agent capability lifecycle"
```
