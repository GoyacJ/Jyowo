# Skill Module Hardening Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make global skills configurable, durably injectable, integrity checked, safely scriptable, and consistently manageable across Desktop, Tauri, Daemon, SDK, and the model context path.

**Architecture:** Keep the skill package and registry rules in `jyowo-harness-skill`, shared wire/persistence types in `jyowo-harness-contracts`, turn assembly in SDK, task durability in Journal/Daemon, sandboxed execution in Sandbox/Tool, and global storage plus UI in Desktop. Each turn resolves references from one registry/config snapshot; secrets remain outside serialized state and model context.

**Tech Stack:** Rust, Tokio, Serde, Tauri 2, OS keychain, React, TypeScript, TanStack Query, Zod, Vitest, Cargo tests

---

### Task 1: Make registry replacement atomic and restore shadowed candidates

**Files:**
- Modify: `crates/jyowo-harness-skill/src/registry.rs`
- Modify: `crates/jyowo-harness-skill/src/skill.rs`
- Modify: `crates/jyowo-harness-skill/tests/governance.rs`
- Modify: `crates/jyowo-harness-skill/tests/hooks.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/skills.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/run_state.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/mcp_server.rs`
- Modify: `crates/jyowo-harness-sdk/tests/runtime_assembly_tools.rs`

**Step 1: Write failing registry tests**

Add tests proving that:

```rust
// workspace shadows user, then removing the workspace source restores user
registry.register(user_skill("review", "user body"))?;
registry.register(workspace_skill("review", "workspace body"))?;
registry.replace_source(workspace_source, Vec::new())?;
assert_eq!(registry.get("review").unwrap().body, "user body");
```

Use two synchronized threads to register distinct names against the same starting generation and assert neither update is lost. Add a stale-candidate commit regression test.

**Step 2: Run RED**

```bash
cargo test -p jyowo-harness-skill --test governance registry_
```

Expected: FAIL because the snapshot stores only winners and registry mutations read outside the write lock.

**Step 3: Implement the candidate stack**

Add `candidates: BTreeMap<String, Vec<Arc<Skill>>>` to `SkillRegistrySnapshot`. Rank candidates with the existing source precedence, derive `entries` from the winning candidate, and rebuild `by_source`/`status` from all candidates. Hold `snapshot.write()` for the complete clone-mutate-publish sequence in `register`, `register_batch`, source replacement, and removal. Replace the SDK's winner-only rebuild with `replace_source`.

**Step 4: Add failing hook replacement tests**

Assert handler IDs differ when source or hook transport changes, a successful replacement removes the former handler, and failed registration retains the former handler.

**Step 5: Implement stable hook fingerprints and transactional replacement**

Construct handler IDs from skill name, source identity, hook ID, and a deterministic declaration fingerprint. Register all new handlers before removing old handlers and publishing the registry snapshot. Reject `mTLS` at frontmatter validation until a certificate provider exists.

Carry the loader's `SkillRenderPolicy` into `HarnessInner`/`EngineSessionTurnRunner` and apply it to the initial skill service plus every turn snapshot renderer. Cover all three runner construction paths, including MCP-server sessions.

**Step 6: Run GREEN and commit**

```bash
cargo test -p jyowo-harness-skill
cargo test -p jyowo-harness-sdk --test runtime_assembly_tools skill
git add crates/jyowo-harness-skill crates/jyowo-harness-sdk/src/harness crates/jyowo-harness-sdk/tests/runtime_assembly_tools.rs
git commit -m "fix: make skill registry replacement atomic"
```

### Task 2: Add the global skill configuration and secret-store contract

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/storage_layout.rs`
- Modify: `crates/jyowo-harness-contracts/src/global_config.rs`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Create: `apps/desktop/src-tauri/src/commands/stores/skill_config.rs`
- Modify: `apps/desktop/src-tauri/src/commands/stores/mod.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `crates/jyowo-harness-sdk/src/skill_config.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/skills.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- Modify: `crates/jyowo-harness-sdk/tests/skill_config_resolver.rs`
- Modify: `crates/jyowo-harness-daemon/src/runtime_config.rs`
- Modify: `crates/jyowo-harness-daemon/tests/runtime_config.rs`
- Create: `apps/desktop/src-tauri/tests/skill_commands.rs`

**Step 1: Write failing storage tests with a fake secret store**

Define a test-only in-memory implementation of:

```rust
pub trait SkillSecretStore: Send + Sync {
    fn get(&self, skill_id: &str, key: &str) -> Result<Option<SecretString>, SkillConfigStoreError>;
    fn set(&self, skill_id: &str, key: &str, value: SecretString) -> Result<(), SkillConfigStoreError>;
    fn delete(&self, skill_id: &str, key: &str) -> Result<(), SkillConfigStoreError>;
}
```

Test public value round-trip, secret set/clear, secret presence serialization, and absence of secret plaintext from JSON/debug/error output. Assert the path is `~/.jyowo/config/skill-config.json`.

**Step 2: Run RED**

```bash
cargo test -p jyowo-desktop-shell --test skill_commands skill_config
```

Expected: FAIL because the store and commands do not exist.

**Step 3: Implement the versioned document and keychain adapter**

Add versioned `SkillConfigDocument`, per-skill public values, and `configured` secret metadata. Namespace keychain accounts as `<canonical-skill-id>/<config-key>` under one Jyowo service name so one package cannot claim another package's secret. Add an OS-keychain adapter using the workspace keyring dependency. Persist JSON atomically with the existing config-store helpers. Never return secret values from read commands.

**Step 4: Write the failing per-skill isolation tests**

Create two skills where one lacks required config. Assert session creation and the configured skill remain available, while only the missing skill receives `SkillStatus::PrerequisiteMissing` and render returns a typed missing-config error.

**Step 5: Implement per-skill resolution**

Namespace configuration by canonical skill ID. Replace the global `validate_required_skill_config` gate in session creation and turn snapshot assembly with per-skill status calculation. `SkillConfigSnapshotResolver` receives the selected skill's declarations and refuses secret resolution through ordinary template substitution.

Load the same document in `RuntimeConfigResolver` and merge it with the injected secret store instead of constructing `SkillConfigSnapshot::new()`. Missing configuration changes only the target skill's status. Any `${config.<key>:secret}` interpolation returns `SecretInterpolationForbidden`; required secrets are checked for presence without reading them into the renderer.

**Step 6: Run GREEN and commit**

```bash
cargo test -p jyowo-harness-sdk --test skill_config_resolver
cargo test -p jyowo-harness-daemon --test runtime_config skill_config
cargo test -p jyowo-desktop-shell --test skill_commands skill_config
git add Cargo.toml Cargo.lock apps/desktop/src-tauri crates/jyowo-harness-contracts/src/global_config.rs crates/jyowo-harness-sdk crates/jyowo-harness-daemon
git commit -m "feat: persist global skill configuration safely"
```

### Task 3: Declare scripts and replace the ad-hoc script sandbox

**Files:**
- Modify: `crates/jyowo-harness-skill/src/skill.rs`
- Modify: `crates/jyowo-harness-skill/src/frontmatter.rs`
- Modify: `crates/jyowo-harness-skill/src/error.rs`
- Modify: `crates/jyowo-harness-skill/tests/frontmatter.rs`
- Modify: `crates/jyowo-harness-sandbox/src/skill_script.rs`
- Modify: `crates/jyowo-harness-sandbox/src/lib.rs`
- Modify: `crates/jyowo-harness-sandbox/tests/skill_script.rs`

**Step 1: Write failing declaration tests**

Parse script declarations containing `id`, relative `path`, timeout, `network: deny`, environment-to-config mappings, output limits, and artifact limits. Reject duplicate IDs, absolute/parent paths, secret mappings not declared as secret config, unsupported network policy, and unknown script fields.

**Step 2: Run RED**

```bash
cargo test -p jyowo-harness-skill --test frontmatter script
```

Expected: FAIL because `SkillFrontmatter` has no script declarations.

**Step 3: Implement minimal frontmatter types and validation**

Add `SkillScriptDecl`, `SkillScriptNetworkPolicy`, and `SkillScriptEnvDecl`. Default to network denied and a bounded timeout. Preserve `deny_unknown_fields` behavior in the hand-written YAML parser.

**Step 4: Write failing sandbox enforcement tests**

Prove the runner rejects a backend that cannot enforce network denial, injects only explicitly supplied environment variables, times out, truncates output, limits artifact count/bytes, and does not report fake memory/network success fields.

**Step 5: Rebuild the runner on `SandboxBackend`**

Replace direct `tokio::process::Command` execution with `execute_with_lifecycle` and a policy derived from the declaration. Materialize only validated package files. Return enforced policy, bounded output, and bounded artifacts; remove `memory_mb`, `memory_limit_mb`, and `network_enabled` from the result.

**Step 6: Run GREEN and commit**

```bash
cargo test -p jyowo-harness-skill --test frontmatter script
cargo test -p jyowo-harness-sandbox --test skill_script
git add crates/jyowo-harness-skill crates/jyowo-harness-sandbox
git commit -m "feat: declare and sandbox skill scripts"
```

### Task 4: Expose `skills_run_script` as an independently authorized tool

**Files:**
- Modify: `crates/jyowo-harness-contracts/src/capability.rs`
- Modify: `crates/jyowo-harness-contracts/src/enums.rs`
- Modify: `crates/jyowo-harness-contracts/src/tool.rs`
- Modify: `crates/jyowo-harness-skill/src/service.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/skills.rs`
- Modify: `crates/jyowo-harness-tool/src/builtin/mod.rs`
- Modify: `crates/jyowo-harness-tool/src/builder.rs`
- Modify: `crates/jyowo-harness-tool/src/skill_script.rs`
- Modify: `crates/jyowo-harness-tool/tests/builtin_skills.rs`
- Modify: `crates/jyowo-harness-tool/tests/skill_script.rs`
- Modify: `crates/jyowo-harness-tool/tests/feature_gates.rs`

**Step 1: Write failing descriptor, permission, and execution tests**

Assert `skills_run_script` is registered separately, plans `ActionResource::Skill { action: "run_script" }`, asks for a script-specific permission, declares `ProcessSandbox`, rejects undeclared IDs/path overrides, and leaves `skills_invoke` render-only.

**Step 2: Run RED**

```bash
cargo test -p jyowo-harness-tool --test builtin_skills skills_run_script
cargo test -p jyowo-harness-tool --test skill_script
```

Expected: FAIL because the tool and runner capability are absent.

**Step 3: Implement the capability boundary and tool**

Add a capability method that resolves one visible skill and declared script from the captured turn snapshot, validates arguments, resolves only declared config environment entries, and returns a runner request. Execute it through `ctx.sandbox`; bind permission to skill ID, script ID, package hash, arguments, workspace access, and network policy.

**Step 4: Run GREEN and commit**

```bash
cargo test -p jyowo-harness-tool --test builtin_skills
cargo test -p jyowo-harness-tool --test skill_script
cargo test -p jyowo-harness-tool --test feature_gates
git add crates/jyowo-harness-contracts crates/jyowo-harness-skill crates/jyowo-harness-tool
git commit -m "feat: add explicit skill script tool"
```

### Task 5: Verify installed package integrity and scan auxiliary text

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands/stores/mod.rs`
- Modify: `apps/desktop/src-tauri/src/commands/stores/skill.rs`
- Modify: `apps/desktop/src-tauri/src/commands/skills.rs`
- Modify: `apps/desktop/src-tauri/tests/skill_commands.rs`
- Modify: `crates/jyowo-harness-skill/src/loader.rs`
- Modify: `crates/jyowo-harness-skill/src/sources/user.rs`
- Modify: `crates/jyowo-harness-skill/src/scanner.rs`
- Modify: `crates/jyowo-harness-skill/tests/scanner.rs`
- Modify: `crates/jyowo-harness-skill/tests/sources.rs`
- Modify: `crates/jyowo-harness-daemon/src/runtime_config.rs`

**Step 1: Write failing tamper and auxiliary scan tests**

Install a package, mutate `SKILL.md`, and assert list/reload marks it rejected before runtime registration. Put a blocked instruction in `README.md` or another supported text file and assert package validation rejects it even when `SKILL.md` is clean.

**Step 2: Run RED**

```bash
cargo test -p jyowo-desktop-shell --test skill_commands package_integrity
cargo test -p jyowo-harness-skill --test scanner auxiliary
```

Expected: FAIL because runtime trusts the recorded hash and scans only the parsed skill fields.

**Step 3: Implement verification and bounded text scanning**

Recompute the package hash before list/detail/reload. Carry expected package hashes, not only allowed IDs, into `SkillSourceConfig::DirectoryPackages`; the loader recomputes before parsing and records mismatches as rejected. Scan regular UTF-8 text files with the existing file-count/size/symlink limits and supported text extensions; do not execute or decode binary files.

**Step 4: Run GREEN and commit**

```bash
cargo test -p jyowo-desktop-shell --test skill_commands package_integrity
cargo test -p jyowo-harness-skill --test scanner --test sources
git add apps/desktop/src-tauri crates/jyowo-harness-skill
git commit -m "fix: verify and scan installed skill packages"
```

### Task 6: Make catalog HTTP and install operations durable

**Files:**
- Modify: `apps/desktop/src-tauri/src/skill_catalog.rs`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/src/commands/skills.rs`
- Create: `apps/desktop/src-tauri/src/commands/stores/skill_catalog_tasks.rs`
- Modify: `apps/desktop/src-tauri/src/commands/stores/mod.rs`
- Modify: `apps/desktop/src-tauri/tests/skill_commands.rs`

**Step 1: Write failing timeout, recovery, reinstall, and lock-scope tests**

Use `wiremock` to stall connection/headers/body and assert typed timeouts. Persist a `running` task, reconstruct runtime state, and assert it becomes `interrupted`. Assert a new operation ID can reinstall an entry after delete/completion. Use a blocking downloader test seam and prove an unrelated store mutation can acquire the skill-store lock during download.

**Step 2: Run RED**

```bash
cargo test -p jyowo-desktop-shell --test skill_commands catalog_
```

Expected: FAIL because tasks are in memory, task identity is entry based, and download holds the store lock.

**Step 3: Implement durable operation records**

Key task state by `operationId`, persist every transition atomically, and convert stale `running` tasks to `interrupted` during runtime startup. Deduplicate only the same active operation. Keep historical terminal records without blocking a new operation.

**Step 4: Narrow the lock and enforce HTTP budgets**

Configure connect and request timeouts on the client and wrap response-body reads with a separate timeout. Download, validate, hash, and stage outside `skill_store_lock`; acquire it only for final package swap, index/selection update, and reload.

**Step 5: Run GREEN and commit**

```bash
cargo test -p jyowo-desktop-shell --test skill_commands catalog_
git add apps/desktop/src-tauri
git commit -m "fix: make skill catalog operations durable"
```

### Task 7: Migrate task context references to a versioned typed contract

**Files:**
- Modify: `crates/jyowo-harness-contracts/src/messages.rs`
- Modify: `crates/jyowo-harness-contracts/src/daemon.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `crates/jyowo-harness-contracts/tests/daemon_contract.rs`
- Modify: `crates/jyowo-harness-journal/src/task_event.rs`
- Modify: `crates/jyowo-harness-journal/src/task_store.rs`
- Modify: `crates/jyowo-harness-journal/src/task_projection.rs`
- Modify: `crates/jyowo-harness-journal/tests/task_projection.rs`
- Modify: `crates/jyowo-harness-daemon/src/queue.rs`
- Modify: `crates/jyowo-harness-daemon/src/task_actor.rs`
- Modify: `crates/jyowo-harness-daemon/src/ipc/server.rs`
- Modify: `crates/jyowo-harness-daemon/src/sdk_run_factory.rs`
- Modify: `crates/jyowo-harness-daemon/tests/task_actor.rs`
- Modify: `apps/desktop/src/features/tasks/TaskComposer.tsx`
- Modify: `apps/desktop/src/features/tasks/TaskComposer.test.tsx`
- Regenerate: `apps/desktop/src/generated/daemon-protocol.schema.json`
- Regenerate: `apps/desktop/src/generated/daemon-protocol.ts`

**Step 1: Write failing compatibility tests**

Deserialize a legacy task event containing `"contextReferences":["src/lib.rs"]` and assert it normalizes to `WorkspaceFile`. Round-trip a skill reference containing version, canonical skill ID, label, non-secret parameters, and source metadata.

**Step 2: Run RED**

```bash
cargo test -p jyowo-harness-contracts --test daemon_contract context_reference
cargo test -p jyowo-harness-journal context_reference
```

Expected: FAIL because task persistence uses `Vec<String>`.

**Step 3: Implement the typed migration**

Use `ConversationContextReference` throughout Journal and Daemon. Extend `Skill` with versioned fields and defaults. Implement compatibility deserialization so strings become workspace-file references, while serialization emits only the typed current form. Preserve non-secret parameters as JSON and enforce size/count limits.

Add `queue_item_revision: Option<u64>` to immutable segment input. Use it with task ID, queue item ID, and reference index to derive a stable delivery key that distinguishes edited queue content from the earlier revision.

Stop `TaskComposer` from reducing tagged references to strings. Send the full reference object through Daemon protocol commands.

**Step 4: Regenerate and verify protocol bindings**

```bash
pnpm generate:daemon-protocol
pnpm check:daemon-protocol
```

**Step 5: Run GREEN and commit**

```bash
cargo test -p jyowo-harness-contracts --test daemon_contract
cargo test -p jyowo-harness-journal
cargo test -p jyowo-harness-daemon --test task_actor
git add crates/jyowo-harness-contracts crates/jyowo-harness-journal crates/jyowo-harness-daemon apps/desktop/src/generated
git commit -m "feat: persist typed context references"
```

### Task 8: Inject selected skills on the first turn with durable recovery

**Files:**
- Modify: `crates/jyowo-harness-contracts/src/events/skill.rs`
- Modify: `crates/jyowo-harness-contracts/src/events/mod.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`
- Modify: `crates/jyowo-harness-session/src/projection.rs`
- Modify: `crates/jyowo-harness-engine/src/turn.rs`
- Modify: `crates/jyowo-harness-journal/src/task_event.rs`
- Modify: `crates/jyowo-harness-journal/src/task_event_adapter.rs`
- Modify: `crates/jyowo-harness-journal/src/task_store.rs`
- Modify: `crates/jyowo-harness-journal/tests/task_event_adapter.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/conversation.rs`
- Modify: `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- Modify: `crates/jyowo-harness-sdk/tests/sdk_session_flow.rs`
- Create: `crates/jyowo-harness-sdk/tests/skill_context_recovery.rs`
- Modify: `crates/jyowo-harness-daemon/src/sdk_run_factory.rs`
- Create: `crates/jyowo-harness-daemon/tests/skill_context_flow.rs`

**Step 1: Write failing first-turn injection test**

Submit a typed skill reference and capture the model request. Assert the rendered body appears once, the label-only placeholder does not, required parameters are validated, and a secret value never appears in the request or events.

**Step 2: Run RED**

```bash
cargo test -p jyowo-harness-sdk --test sdk_session_flow selected_skill
```

Expected: FAIL because skill references render only metadata.

**Step 3: Implement one-snapshot assembly**

Capture one registry snapshot and matching config resolver before hydrating references. Render every selected skill against that snapshot, hash each body, add fenced transient context patches, and remove skill references from the label renderer. Return typed missing-parameter/config/visibility errors.

**Step 4: Write failing recovery tests**

Cover `prepared`, assembled/provider-accepted, and `consumed` transitions. Simulate a crash after `prepared`, resume with identical content, and assert at-least-once delivery. Mutate the package before resume and assert a hash-mismatch integrity error. Assert no event contains the body or secret.

**Step 5: Implement recovery events and projection**

Persist reference, parameters, source, stable delivery key, and body hash at `prepared`; persist assembly when the context patch is queued. Emit provider acceptance only after `model.infer(...)` returns `Ok(ModelStream)`, then append a separate `consumed` marker. Route these events through `TaskEventStoreAdapter`. On resume, re-render unresolved prepared injections and compare hashes before provider submission. Retry prepared, assembled, and accepted-but-not-consumed deliveries; skip consumed deliveries. Never mark consumed before provider acceptance.

**Step 6: Run GREEN and commit**

```bash
cargo test -p jyowo-harness-sdk --test sdk_session_flow --test skill_context_recovery
cargo test -p jyowo-harness-daemon --test skill_context_flow
git add crates/jyowo-harness-contracts crates/jyowo-harness-session crates/jyowo-harness-sdk crates/jyowo-harness-daemon
git commit -m "feat: inject and recover selected skill context"
```

### Task 9: Expose global configuration and runtime skill candidates through Tauri

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/skills.rs`
- Modify: `apps/desktop/src-tauri/src/commands/daemon.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/tests/skill_commands.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Modify: `apps/desktop/src/shared/daemon/client.ts`
- Modify: `crates/jyowo-harness-contracts/src/daemon.rs`
- Create: `crates/jyowo-harness-daemon/src/reference_candidates.rs`
- Modify: `crates/jyowo-harness-daemon/src/ipc/server.rs`
- Modify: `crates/jyowo-harness-daemon/src/ipc/transport_unix.rs`
- Modify: `crates/jyowo-harness-daemon/src/ipc/transport_windows.rs`
- Modify: `crates/jyowo-harness-daemon/src/bin/jyowo-harness-daemon.rs`

**Step 1: Write failing command tests**

Assert globally managed skills report `sourceKind: "user"`; config read returns declarations/public values/secret presence only; set/clear commands update the stores; reference candidates come from the effective workspace runtime snapshot; and malformed date/event payloads are rejected.

**Step 2: Run RED**

```bash
cargo test -p jyowo-desktop-shell --test skill_commands commands_
pnpm -C apps/desktop test src/shared/tauri/commands.test.ts
```

Expected: FAIL because config commands and runtime candidates are missing and source kind is wrong.

**Step 3: Implement commands and schemas**

Add list/update/clear config commands. Keep scope global. Add script metadata and per-skill prerequisite state to detail responses. Add a native Daemon candidate request/response. Resolve the current task workspace through `RuntimeConfigResolver`, then return effective visible skills from that runtime snapshot; Tauri forwards the result and adds workspace-file candidates without inventing a separate skill list. Use open non-empty strings for catalog source IDs and `.datetime({ offset: true })` for all task timestamps.

**Step 4: Run GREEN and commit**

```bash
cargo test -p jyowo-desktop-shell --test skill_commands
pnpm -C apps/desktop test src/shared/tauri/commands.test.ts src/shared/daemon/client.test.ts
git add apps/desktop/src-tauri apps/desktop/src/shared
git commit -m "feat: expose global skill configuration commands"
```

### Task 10: Split and harden the Desktop skill settings feature

**Files:**
- Modify: `apps/desktop/src/features/settings/SkillSettings.tsx`
- Create: `apps/desktop/src/features/skills/api/queries.ts`
- Create: `apps/desktop/src/features/skills/installed/InstalledSkillsManager.tsx`
- Create: `apps/desktop/src/features/skills/catalog/SkillCatalogManager.tsx`
- Create: `apps/desktop/src/features/skills/config/SkillConfigPanel.tsx`
- Create: `apps/desktop/src/features/skills/components/SkillFileTree.tsx`
- Create: `apps/desktop/src/features/skills/components/catalog-task-reducer.ts`
- Modify: `apps/desktop/src/features/skills/SkillsPage.test.tsx`
- Modify: `apps/desktop/src/features/settings/SkillSettings.test.tsx`
- Modify: `apps/desktop/src/testing/command-client/skills.ts`
- Modify: `apps/desktop/src/testing/command-client/skills-handlers.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`

**Step 1: Write failing UI tests**

Cover public config save, secret set/clear without echo, file-read error state, rejected import/toggle/delete/install mutations, invalid progress event error state, out-of-order events for separate operation IDs, reinstall after completion, and catalog refresh after terminal polling.

**Step 2: Run RED**

```bash
pnpm -C apps/desktop test src/features/skills/SkillsPage.test.tsx src/features/settings/SkillSettings.test.tsx
```

Expected: FAIL on the newly asserted behaviors.

**Step 3: Implement behavior while splitting the feature**

Move query/mutation hooks to `api`, installed/catalog/config views to their directories, and shared tree/reducer code to `components`. Catch every `mutateAsync` rejection and render `getCommandErrorMessage`. Reduce task events by `operationId` and monotonic parsed datetime, preserve multiple operations for one entry, refresh installed and catalog queries at terminal state, and surface schema/listener failures.

Render ordinary fields from declarations. Render secrets as set/replace and clear controls with presence only; never initialize an input from a secret value.

Keep Composer skill references structured and verify selected skill parameters/source/version reach the submitted task command unchanged.

**Step 4: Run GREEN and commit**

```bash
pnpm -C apps/desktop test src/features/skills/SkillsPage.test.tsx src/features/settings/SkillSettings.test.tsx src/features/tasks/TaskComposer.test.tsx src/shared/tauri/commands.test.ts
git add apps/desktop/src/features apps/desktop/src/testing apps/desktop/src/shared/i18n/locales
git commit -m "feat: harden global skill settings UI"
```

### Task 11: Remove false MCP/script claims and add an isolated Desktop skill test target

**Files:**
- Modify: `crates/jyowo-harness-skill/src/sources/mcp.rs`
- Modify: `crates/jyowo-harness-skill/tests/sources.rs`
- Modify: `crates/jyowo-harness-sandbox/src/skill_script.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Create: `apps/desktop/src-tauri/tests/skill_commands.rs`
- Modify: `apps/desktop/src-tauri/tests/commands/skills.rs`

**Step 1: Write failing boundary tests**

Assert MCP prompts are not exposed as a local production skill source, while `McpSkillRecord` can still be loaded explicitly by an extension test. Assert script results contain only enforced fields. Verify `skill_commands` compiles without automation/app-info test modules.

**Step 2: Run RED**

```bash
cargo test -p jyowo-harness-skill --test sources mcp_
cargo test -p jyowo-desktop-shell --test skill_commands
```

**Step 3: Tighten the boundaries**

Keep the core adapter but remove Desktop/product assertions that imply an automatic MCP prompt-to-skill bridge. Move skill command support imports and tests into the isolated target. Remove the obsolete public ad-hoc runner surface after `skills_run_script` is wired.

**Step 4: Run GREEN and commit**

```bash
cargo test -p jyowo-harness-skill --test sources
cargo test -p jyowo-desktop-shell --test skill_commands
git add crates/jyowo-harness-skill crates/jyowo-harness-sandbox apps/desktop/src-tauri/tests
git commit -m "test: isolate desktop skill command coverage"
```

### Task 12: End-to-end verification

**Files:**
- Verify all modified files

**Step 1: Format and validate generated files**

```bash
cargo fmt --all -- --check
pnpm check:daemon-protocol
pnpm -C apps/desktop typecheck
```

**Step 2: Run relevant Rust suites**

```bash
cargo test -p jyowo-harness-contracts
cargo test -p jyowo-harness-skill
cargo test -p jyowo-harness-sandbox --test skill_script
cargo test -p jyowo-harness-tool --test builtin_skills --test skill_script --test feature_gates
cargo test -p jyowo-harness-journal
cargo test -p jyowo-harness-sdk --test skill_config_resolver --test sdk_session_flow --test skill_context_recovery
cargo test -p jyowo-harness-daemon --test task_actor --test skill_context_flow
cargo test -p jyowo-desktop-shell --test skill_commands
```

**Step 3: Run relevant Desktop suites**

```bash
pnpm -C apps/desktop test src/features/skills/SkillsPage.test.tsx src/features/settings/SkillSettings.test.tsx src/shared/tauri/commands.test.ts src/shared/daemon/client.test.ts src/features/conversation/Composer.test.tsx
```

**Step 4: Inspect security and persistence invariants**

Search serialized events, command payloads, logs, snapshots, and UI fixtures for secret values. Verify package tampering is rejected, no catalog network wait holds the store lock, no runner reports unenforced isolation, legacy task records still load, and the model receives the selected skill body from one turn snapshot.

**Step 5: Request final code review**

Use `superpowers:requesting-code-review` against the design and this plan. Fix all Critical and Important findings, rerun the full verification commands, then use `superpowers:finishing-a-development-branch`.
