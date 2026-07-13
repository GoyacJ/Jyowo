# Model Module Gap Remediation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Close the confirmed model-module correctness gaps across provider codecs, Engine, daemon, Tauri, catalog, usage projection, and React UI without changing the established public configuration or daemon IPC formats unnecessarily.

**Architecture:** Keep daemon and Tauri runtimes separate, but wire the same continuation abstraction into both. Derive authentication from provider descriptors. Treat explicit normalized terminal events as the only successful stream completion signal and durable daemon journal usage events as the desktop usage source of truth. Prepare provider configuration as a validated candidate, then publish it as one logical generation. Only registry-backed models are runnable.

**Tech Stack:** Rust, Tokio, serde, Tauri 2, TypeScript, React, TanStack Query, Vitest, Cargo, pnpm.

---

### Task 1: Correct provider stream and Engine accounting semantics

**Files:**
- Modify: `crates/jyowo-harness-engine/src/turn.rs`
- Modify: `crates/jyowo-harness-engine/src/turn_assembly.rs`
- Modify: `crates/jyowo-harness-engine/tests/main_loop.rs`
- Modify: `crates/jyowo-harness-engine/tests/usage.rs`
- Modify: `crates/jyowo-harness-model/src/openai_protocol/responses_codec.rs`
- Modify: `crates/jyowo-harness-model/src/openai_protocol/chat_codec.rs`
- Modify: `crates/jyowo-harness-model/src/openai_protocol/completions_codec.rs`
- Modify: `crates/jyowo-harness-model/src/anthropic/client.rs`
- Modify: `crates/jyowo-harness-model/src/gemini/mod.rs`
- Modify: `crates/jyowo-harness-model/src/qwen.rs`
- Modify: relevant provider codec tests under `crates/jyowo-harness-model/tests/`
- Modify: `crates/jyowo-harness-model/tests/registry.rs`

**RED:** Add tests proving truncated EOF never completes a run, non-stream usage appears exactly once, tool calls are present in `UsageAccumulatedEvent`, and the default-feature registry test compiles.

Run:

```bash
cargo test -p jyowo-harness-engine --test main_loop --test usage
cargo test -p jyowo-harness-model --tests
```

**GREEN:** Require `StreamAggregator` terminal state after provider EOF. Remove synthetic success at unverified transport EOF. Emit non-stream usage only as a delta. Populate model-call tool count before the usage event. Make `ModelProtocol` available to the unconditional registry helper.

**Verify:** Repeat both commands and require zero failures.

### Task 2: Wire provider continuation stores into every runtime builder

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: desktop runtime command tests
- Modify: `crates/jyowo-harness-daemon/src/sdk_run_factory.rs`
- Modify: `crates/jyowo-harness-daemon/src/bin/jyowo-harness-daemon.rs`
- Modify: daemon factory tests

**RED:** Add recording/file-backed tests proving desktop runs, daemon foreground runs, and daemon child runs append provider continuations.

Run:

```bash
cargo test -p jyowo-desktop-shell runtime
cargo test -p jyowo-harness-daemon sdk_run_factory
```

**GREEN:** Open one file continuation store from each runtime's existing private runtime directory. Inject it into desktop Harness construction and share one daemon instance across foreground and child Harness builders.

**Verify:** Repeat targeted tests and verify continuation files retain owner-only permissions.

### Task 3: Make authentication-free providers executable end to end

**Files:**
- Modify: `crates/jyowo-harness-daemon/src/provider_config.rs`
- Modify: `crates/jyowo-harness-daemon/tests/provider_config.rs`
- Modify: `apps/desktop/src-tauri/src/commands/providers.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/provider_settings.rs`
- Modify: `apps/desktop/src/features/tasks/TaskWorkspace.tsx`
- Modify: `apps/desktop/src/features/tasks/TaskWorkspace.test.tsx`

**RED:** Cover Local Llama and Bedrock with no secret entry, while authenticated providers still reject missing or empty keys. Prove task selection includes runnable `authScheme=none` configurations even when `hasApiKey=false`.

Run:

```bash
cargo test -p jyowo-harness-daemon --test provider_config
cargo test -p jyowo-desktop-shell --test command_contracts provider_settings
pnpm --dir apps/desktop vitest run src/features/tasks/TaskWorkspace.test.tsx
```

**GREEN:** Derive secret requirements exclusively from provider `auth_scheme`. Preserve `hasApiKey` as a literal secret-presence field and use provider executability for task filtering.

**Verify:** Repeat all three commands.

### Task 4: Commit provider settings as a validated logical transaction

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands/stores/global_config.rs`
- Modify: `apps/desktop/src-tauri/src/commands/providers.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/provider_settings_store.rs`
- Modify: `apps/desktop/src-tauri/src/commands/tests.rs`

**RED:** Inject failure after each staged file and candidate-runtime construction. Assert profiles, secrets, selection, active runtime, reveal tokens, and `0600` permissions remain consistent.

Run:

```bash
cargo test -p jyowo-desktop-shell --test command_contracts provider_settings_store
cargo test -p jyowo-desktop-shell --lib save_provider_settings
```

**GREEN:** Build a candidate record and candidate runtime before publication. Serialize writers/readers with the existing store lock boundary, stage all three files, commit them with rollback metadata, replace the active runtime only after persistence succeeds, and invalidate reveal tokens only after commit.

**Verify:** Repeat targeted tests and run all desktop-shell command contract tests.

### Task 5: Incrementally project durable daemon usage

**Files:**
- Modify: `crates/jyowo-harness-contracts/src/daemon.rs` only if the existing journal API cannot page exact history
- Modify: `crates/jyowo-harness-daemon/src/ipc/server.rs`
- Modify: `apps/desktop/src-tauri/src/daemon_client.rs`
- Modify: `apps/desktop/src-tauri/src/commands/daemon.rs`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/model_settings.rs`
- Modify: daemon IPC and desktop model-settings tests

**RED:** Cover exact historical pagination, duplicate-batch idempotency, durable cursor restart, failed-write cursor retention, diagnostic exclusion, tool usage, pending run durations, slash-containing model IDs, and calendar-window rebuilds.

Run:

```bash
cargo test -p jyowo-harness-daemon ipc
cargo test -p jyowo-desktop-shell model_usage
```

**GREEN:** Page durable journal history without subscription-gap semantics. Fold typed usage/run events into day-level model buckets plus a durable global offset, and atomically save projection and cursor. Rebuild time-window summaries from retained day buckets.

**Verify:** Repeat targeted tests, then run full daemon and desktop-shell tests.

### Task 6: Make catalog runnable state match runtime construction

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands/model_settings.rs`
- Modify: catalog refresh tests

**RED:** Prove unknown Anthropic and DeepSeek inventory cannot be marked runnable and every runnable catalog entry can pass descriptor lookup and provider construction.

Run:

```bash
cargo test -p jyowo-desktop-shell model_catalog
```

**GREEN:** Merge dynamic metadata only into registry-backed descriptors. Keep unsupported inventory visible but non-runnable. Remove unused DeepSeek snapshot state if it cannot influence the buildable catalog.

**Verify:** Repeat targeted catalog tests.

### Task 7: Close task and settings frontend state gaps

**Files:**
- Modify: `apps/desktop/src/features/tasks/TaskWorkspace.tsx`
- Modify: `apps/desktop/src/features/tasks/TaskComposer.tsx`
- Modify: `apps/desktop/src/features/tasks/TaskWorkspace.test.tsx`
- Modify: `apps/desktop/src/features/tasks/TaskComposer.test.tsx`
- Modify: `apps/desktop/src/features/settings/models/model-settings-view-model.ts`
- Modify: `apps/desktop/src/features/settings/models/model-settings-view-model.test.ts`
- Modify: `apps/desktop/src/features/settings/models/model-settings-queries.ts`
- Modify: `apps/desktop/src/features/settings/models/ModelSettingsPage.tsx`
- Modify: `apps/desktop/src/features/settings/models/ModelSettingsPage.test.tsx`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`

**RED:** Cover empty override inheritance, task query error/retry, slash model IDs, conditional rebuilding polling, task-query invalidation, and visible mutation errors that preserve open editing state.

Run:

```bash
pnpm --dir apps/desktop vitest run src/features/tasks src/features/settings/models src/shared/tauri/commands.test.ts
```

**GREEN:** Normalize empty overrides to inheritance. Surface query/mutation errors through existing error UI. Use structured usage identities. Poll only while rebuilding. Invalidate the shared provider-settings query prefix after model mutations. Remove the duplicate capability-route schema literal.

**Verify:** Repeat the targeted Vitest command and run desktop type checking.

### Task 8: Integrated verification and review

**Files:**
- Review all changed files

Run fresh checks:

```bash
cargo test -p jyowo-harness-model --tests
cargo test -p jyowo-harness-engine --tests
cargo test -p jyowo-harness-daemon --tests
cargo test -p jyowo-desktop-shell --tests
pnpm --dir apps/desktop vitest run
pnpm --dir apps/desktop check
git diff --check
git status --short
```

Confirm the diff excludes the original workspace-only changes to `apps/desktop/src-tauri/capabilities/default.json`, `package.json`, and `scripts/tauri-window-policy.test.mjs`. Request a specification review, then a code-quality review, and resolve findings before completion.
