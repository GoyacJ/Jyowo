# Runtime Tools Settings Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Restore the Tools settings page by exposing the complete `DesktopSettingsRuntime` tool catalog and capability status through read-only Tauri settings commands.

**Architecture:** Add a dedicated Rust settings-query module that reads the existing desktop settings runtime. Register only the two settings commands in Tauri, keep task commands on the daemon, remove the stale test classification, and preserve primitive Tauri error messages in the renderer.

**Tech Stack:** Rust, Tauri 2, React, TypeScript, TanStack Query, Vitest, Cargo tests

---

### Task 1: Define the settings command boundary with failing Rust tests

**Files:**
- Create: `apps/desktop/src-tauri/tests/commands/runtime_tools.rs`
- Create: `apps/desktop/src-tauri/tests/commands/runtime_execution_status.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `apps/desktop/src-tauri/tests/daemon_bridge.rs`

**Step 1: Write the failing tests**

Restore focused tests against `DesktopRuntimeState::settings_runtime()`:

```rust
let response = list_runtime_tools_with_runtime_state(&state).expect("tools should list");
assert!(response.generation > 0);
assert!(response.tools.iter().any(|tool| tool.name == "FileRead"));
```

Cover built-in summaries, plugin/MCP/skill origins, stable sorting, runtime status, and `RUNTIME_NOT_READY`. Update the handler architecture test so conversation/run commands remain forbidden while the two settings queries must be registered.

**Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p jyowo-desktop-shell --test commands runtime_tools
cargo test -p jyowo-desktop-shell --test commands runtime_execution_status
cargo test -p jyowo-desktop-shell --test daemon_bridge
```

Expected: FAIL because the settings helpers and active handler registrations are absent.

### Task 2: Implement the runtime-tools settings module

**Files:**
- Create: `apps/desktop/src-tauri/src/commands/runtime_tools.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

**Step 1: Add read-only helpers and commands**

Implement:

```rust
pub fn list_runtime_tools_with_runtime_state(
    state: &DesktopRuntimeState,
) -> Result<ListRuntimeToolsResponse, CommandErrorPayload>;

#[tauri::command]
pub async fn list_runtime_tools(
    runtime: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<ListRuntimeToolsResponse, CommandErrorPayload>;

#[tauri::command]
pub async fn get_runtime_execution_status(
    runtime: tauri::State<'_, ManagedDesktopRuntime>,
) -> Result<RuntimeExecutionStatus, CommandErrorPayload>;
```

Both commands obtain the existing `DesktopSettingsRuntime`. Missing initialization returns `RUNTIME_NOT_READY`. Tool summaries are produced from one registry snapshot and sorted by group label, display name, then name.

**Step 2: Register and export only the settings commands**

Add the module export and add both commands to `tauri::generate_handler!`. Do not restore healthcheck, conversation, run, or supervisor commands.

**Step 3: Run Rust tests to verify they pass**

Run the three commands from Task 1.

Expected: PASS.

### Task 3: Preserve real Tauri string errors

**Files:**
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Modify: `apps/desktop/src/shared/tauri/errors.ts`

**Step 1: Write the failing test**

```ts
expect(getCommandErrorMessage('Command list_runtime_tools not found')).toBe(
  'Command list_runtime_tools not found',
)
```

Retain the existing object-shaped and unknown-value cases.

**Step 2: Run the test to verify it fails**

Run:

```bash
pnpm -C apps/desktop test src/shared/tauri/commands.test.ts
```

Expected: FAIL with `Unknown command error`.

**Step 3: Implement primitive string handling**

Return non-empty string errors before checking `Error` and object-shaped payloads. Empty strings continue to use the fallback.

**Step 4: Run the test to verify it passes**

Run the command from Step 2.

Expected: PASS.

### Task 4: Verify the Tools settings page end to end

**Files:**
- Modify if needed: `apps/desktop/src/features/settings/SettingsPage.test.tsx`
- Modify if needed: `apps/desktop/src/features/settings/RuntimeExecutionStatusPanel.tsx`
- Modify if needed: `apps/desktop/src/features/settings/SkillSettings.tsx`

**Step 1: Add or tighten page tests**

Verify the Tools tab renders the backend status and catalog, and renders the actual command error when either query rejects. Do not add frontend capability inference or a static fallback catalog.

**Step 2: Run focused frontend tests**

Run:

```bash
pnpm -C apps/desktop test src/features/settings/SettingsPage.test.tsx src/features/skills/SkillsPage.test.tsx
```

Expected: PASS.

### Task 5: Full verification and cleanup

**Files:**
- Verify all modified files

**Step 1: Format and lint changed code**

Run the repository Rust formatter and the desktop type/lint checks defined in package scripts.

**Step 2: Run complete relevant suites**

```bash
cargo test -p jyowo-desktop-shell --test commands
cargo test -p jyowo-desktop-shell --test daemon_bridge
pnpm -C apps/desktop test src/shared/tauri/commands.test.ts src/features/settings/SettingsPage.test.tsx src/features/skills/SkillsPage.test.tsx
```

**Step 3: Inspect the diff**

Confirm there is one catalog owner, no daemon protocol addition, no static frontend catalog, no restored task-runtime command, and no unrelated user changes included.

