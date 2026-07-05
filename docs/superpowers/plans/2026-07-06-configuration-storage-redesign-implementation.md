# Configuration Storage Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Where the execution surface supports model selection, request this profile for the primary worker and all review subagents: `model: gpt-5.5`, `reasoning_effort: xhigh`, `service_tier: priority`.

**Goal:** Redesign Jyowo configuration and runtime storage into explicit global, project, and no-workspace conversation scopes, with deterministic migrations and no dual-authoritative state.

**Architecture:** Rust remains the policy and storage authority. The backend owns path resolution, scope classification, migration, permission boundaries, config overlays, redaction, and atomic persistence. React only renders typed state from Tauri commands and sends user intent; it must not infer storage scope, credential ownership, permission policy, or runtime authority.

**Tech Stack:** Rust 1.96, Tauri 2, React 19, TypeScript 6, Zod, TanStack Query, `jyowo-harness-contracts`, `jyowo-harness-skill`, `jyowo-harness-agent-runtime`, `jyowo-harness-provider-state`, `cargo test`, `pnpm check:rust`, `pnpm check:desktop`, `pnpm check:docs`, `pnpm check`.

---

## Required Model Profile

All implementation and audit work should request this profile where the execution surface supports model selection:

```text
model: gpt-5.5
reasoning_effort: xhigh
service_tier: priority
```

If the active tool cannot prove or select the requested profile, record that limitation in the task analysis or audit. Do not claim that a stronger profile was used.

## Current Code Facts

Current implementation is workspace-first. Settings and runtime state are mixed under `<workspace>/.jyowo/runtime/`.

Known current storage:

```text
~/Library/Application Support/com.goyacj.jyowo/ui-preferences.json
~/.jyowo/projects.json
~/.jyowo/unconfigured
<workspace>/.jyowo/runtime/
```

Known current settings locations:

```text
Models                  -> <workspace>/.jyowo/runtime/provider-settings.json
Execution               -> <workspace>/.jyowo/runtime/execution-settings.json
Provider routes         -> <workspace>/.jyowo/runtime/provider-capability-routes.json
Skills                  -> <workspace>/.jyowo/runtime/skills/
MCP                     -> <workspace>/.jyowo/runtime/mcp-servers.json
Automations             -> <workspace>/.jyowo/runtime/automations.json
Plugins                 -> <workspace>/.jyowo/runtime/plugins/ and <workspace>/.jyowo/plugins/
Agent profiles          -> <workspace>/.jyowo/runtime/agent-profiles.json
```

Known current runtime data under the same layer:

```text
events/
blobs/
conversation-read-model.sqlite*
conversation-metadata.json
provider-continuations.jsonl
provider-continuation-runtime.version
permission-decisions.json
permission-integrity.key
provider-diagnostics.json
provider-quota-cache.json
automation-runs.jsonl
mcp-diagnostics.jsonl
agent-runtime.sqlite
agent-worktrees/
memory/memory.sqlite3
attachments/
exports/
```

Observed implementation files:

```text
apps/desktop/src-tauri/src/project_registry.rs
apps/desktop/src-tauri/src/agent_supervisor.rs
apps/desktop/src/shared/local-store/ui-preferences-store.ts
apps/desktop/src-tauri/src/commands/providers.rs
apps/desktop/src-tauri/src/commands/runtime.rs
apps/desktop/src-tauri/src/commands/stores/mod.rs
apps/desktop/src-tauri/src/commands/stores/skill.rs
apps/desktop/src-tauri/src/commands/stores/plugin.rs
apps/desktop/src-tauri/src/commands/stores/mcp.rs
apps/desktop/src-tauri/src/commands/stores/automation.rs
crates/jyowo-harness-agent-runtime/src/store.rs
crates/jyowo-harness-agent-runtime/src/profiles.rs
crates/jyowo-harness-provider-state/src/lib.rs
crates/jyowo-harness-memory/
crates/jyowo-harness-skill/src/loader.rs
crates/jyowo-harness-skill/src/sources/user.rs
docs/backend/backend-engineering.md
```

Relevant current behavior:

- `apps/desktop/src-tauri/src/project_registry.rs` owns `~/.jyowo/projects.json` and `~/.jyowo/unconfigured`.
- `apps/desktop/src-tauri/src/agent_supervisor.rs` opens `AgentRuntimeStore` and event stores from the workspace root and derives supervisor identity from the workspace-root hash.
- `apps/desktop/src/shared/local-store/ui-preferences-store.ts` uses Tauri plugin-store `ui-preferences.json`.
- `apps/desktop/src-tauri/src/commands/runtime.rs` wires runtime state from `workspace_root`, including provider continuations, events, blobs, sqlite evidence refs, sandbox root, permission decisions, MCP diagnostics, agent runtime, and Memory at `.jyowo/runtime/memory/memory.sqlite3`.
- `apps/desktop/src-tauri/src/commands/stores/skill.rs` stores `<workspace>/.jyowo/runtime/skills/index.json` plus `enabled/<id>/SKILL.md` and `disabled/<id>/SKILL.md`.
- `apps/desktop/src-tauri/src/commands/stores/plugin.rs` stores `<workspace>/.jyowo/runtime/plugins/index.json`, `user/`, `extensions/`, and `<workspace>/.jyowo/plugins`.
- `crates/jyowo-harness-agent-runtime/src/store.rs` exposes `AgentRuntimeStore::open(workspace_root)` and appends `.jyowo/runtime` internally.
- `crates/jyowo-harness-provider-state/src/lib.rs` exposes `FileProviderContinuationStore::open(workspace_root)` and appends `.jyowo/runtime` internally.
- `crates/jyowo-harness-agent-runtime/src/profiles.rs` writes profile files with crate-local atomic write logic that is separate from desktop shell store helpers.
- `crates/jyowo-harness-skill/src/loader.rs` and `crates/jyowo-harness-skill/src/sources/user.rs` already support user/global skill sources at the lower layer, but desktop runtime does not load global user skills.
- `docs/backend/backend-engineering.md` currently documents provider API keys in workspace provider settings.
- `docs/backend/backend-engineering.md` currently documents agent profiles in `.jyowo/runtime/agent-profiles.json`.

Before editing, re-run focused searches in the implementation worktree. If any fact above has changed on `main`, update this plan in the worktree before implementation and commit that correction as the first task.

## Target Storage Design

Target global root:

```text
~/.jyowo/
  projects.json
  config/
    provider-profiles.json
    provider-secrets.json
    provider-selection.json
    execution-defaults.json
    mcp-presets.json
    agent-profiles.json
  skills/
    index.json
    packages/
  plugins/
    index.json
    packages/
  runtime/
    global-conversations/
      workdir/
        <conversationId>/
      events/
      blobs/
      conversation-read-model.sqlite
      conversation-metadata.json
      provider-continuations.jsonl
      provider-continuation-runtime.version
      permission-decisions.json
      permission-integrity.key
      agent-runtime.sqlite
      agent-worktrees/
      memory/
        memory.sqlite3
      attachments/
      exports/
```

Target project root:

```text
<workspace>/.jyowo/
  config/
    provider-selection.json
    provider-capability-routes.json
    execution-overrides.json
    mcp-servers.json
    automations.json
    skills.json
    plugins.json
    agent-profile-selection.json
  skills/
    packages/
  plugins/
    packages/
  runtime/
    events/
    blobs/
    conversation-read-model.sqlite
    conversation-metadata.json
    provider-diagnostics.json
    provider-quota-cache.json
    provider-continuations.jsonl
    provider-continuation-runtime.version
    permission-decisions.json
    permission-integrity.key
    automation-runs.jsonl
    mcp-diagnostics.jsonl
    agent-runtime.sqlite
    agent-worktrees/
    memory/
      memory.sqlite3
    attachments/
    exports/
```

Configuration overlay rule:

```text
effective config = global defaults + project overrides + run explicit params
```

Ownership table:

```text
Provider profile definitions       -> global config
Provider secrets                   -> global secret store
Global default model selection     -> global config
Project default model selection    -> project config
Provider capability routes         -> project config
Execution defaults                 -> global config
Execution overrides                -> project config
Run explicit execution params      -> run request only
Global skills                      -> global skills
Project-private skills             -> project skills
Enabled skill selection            -> project config
MCP presets                        -> global config
MCP custom/enabled servers         -> project config
Global plugin packages             -> global plugins
Project-private plugin packages    -> project plugins
Enabled plugin selection           -> project config
Diagnostics/events/sqlite/blobs    -> runtime
Memory store                       -> runtime
No-workspace conversations         -> global runtime scope
UI preferences                     -> Tauri plugin-store local UI state, not migrated into backend config
```

Provider secrets may start as owner-only local files only if no keychain abstraction already exists in the repository. If a keychain or OS credential abstraction exists, use it. In either case, expose only `hasSecret`, explicit reveal flow, and redacted diagnostics to React.

Runtime construction rule:

```rust
pub enum RuntimeScope {
    Project { workspace_root: PathBuf },
    GlobalConversation { conversation_id: SessionId },
}

pub enum ConfigScope {
    Project,
    GlobalOnly,
}

pub struct RuntimeLayout {
    pub scope: RuntimeScope,
    pub workspace_root: Option<PathBuf>,
    pub runtime_root: PathBuf,
    pub conversation_cwd: PathBuf,
    pub config_scope: ConfigScope,
}
```

Project runtime layout:

```text
workspace_root    -> Some(<workspace>)
runtime_root      -> <workspace>/.jyowo/runtime
conversation_cwd  -> <workspace>
config_scope      -> Project
```

No-workspace runtime layout:

```text
workspace_root    -> None
runtime_root      -> ~/.jyowo/runtime/global-conversations
conversation_cwd  -> ~/.jyowo/runtime/global-conversations/workdir/<conversationId>
config_scope      -> GlobalOnly
```

All runtime assembly code must consume `RuntimeLayout`. Do not pass a fake workspace root to lower stores, sandbox setup, permission persistence, Memory, agent supervisor, provider continuation store, event store, blob store, or sqlite evidence registry.

## No-Workspace Conversation Design

The current `~/.jyowo/unconfigured` pseudo-workspace is not the target model.

Target:

```text
~/.jyowo/runtime/global-conversations/
~/.jyowo/runtime/global-conversations/workdir/
~/.jyowo/runtime/global-conversations/workdir/<conversationId>/
```

Rules:

- A no-workspace conversation is not a project.
- Do not use `HOME` as a workspace root.
- Do not treat `~/.jyowo/unconfigured` as an effective workspace after migration.
- Default no-workspace sessions use a per-conversation app-owned scratch directory as cwd.
- No two no-workspace conversations share the same default cwd.
- Default no-workspace sessions cannot read or write arbitrary user files.
- File access outside the scratch directory requires an explicit attachment, file picker grant, or permission authorization recorded by the Rust permission authority.
- Permission decisions for no-workspace conversations live under `~/.jyowo/runtime/global-conversations/permission-decisions.json`.
- No-workspace permission decisions must carry conversation/runtime-scope metadata.
- A permanent no-workspace decision is permanent only within that conversation scope unless a later explicit global permission contract is added.
- Deleting a no-workspace conversation must remove, tombstone, or make unreadable permission decisions whose runtime scope metadata points at that conversation.
- Attachments and exports for no-workspace conversations live under the global conversation runtime root.
- Memory SQLite for no-workspace conversations lives under the active global-conversations runtime root.
- `MemoryGlobalSettings` are scoped to that runtime root. No-workspace conversations share the global-conversations runtime settings, while thread settings stay keyed by `SessionId`.
- No-workspace Memory evidence must not create `WorkspaceFile` evidence without a real `workspace_id`. File evidence outside the scratch directory must be represented as explicit attachment/imported-file origin after Rust authorization, not forged as workspace evidence.
- Deleting a no-workspace conversation must prune its scratch directory, attachments, exports, evidence refs, and runtime-visible conversation records, or mark them unreadable through the backend authority.

## Non-Negotiable Rules

- Implement in an isolated git worktree. Do not implement in `/Users/goya/Repo/Git/Jyowo`.
- The implementation branch must be created from `main`.
- This plan file must exist on `main` before the implementation worktree is created.
- No mock product data, fake implementation, noop path, hardcoded success, fake migration, fake permission result, fake provider state, fake skill source, or fake plugin source.
- Tests may use deterministic fixtures only when they verify parser, serializer, migration, policy, or UI state behavior. Fixtures must not replace real production paths.
- Rust backend owns path resolution, migration, policy, and persistence.
- Tauri commands stay thin IPC adapters.
- React does not inspect filesystem paths to infer policy.
- External payloads consumed by React must be validated with Zod.
- Shared Rust contracts are the public serde source. Tauri payloads consumed by React must keep stable camelCase wrappers where existing frontend command patterns require them.
- Stable persisted config DTOs for global and project config must be defined or reused in `crates/jyowo-harness-contracts` with `Serialize`, `Deserialize`, and `JsonSchema`.
- JSON config files and other rewrite-in-place files must use temp-file + fsync + rename semantics.
- Secret-bearing files must be owner-only.
- JSONL append logs must use append-safe locking/fsync or explicit segment semantics; do not rewrite them through the JSON atomic helper.
- SQLite state must use transactions/WAL and safe parent directory creation; do not treat SQLite databases as atomic JSON files.
- No symlink component may be followed while creating or writing app-controlled config/runtime path parents.
- Redaction must run before logs, traces, events, snapshots, support payloads, and frontend-visible diagnostics.
- Breaking refactors are allowed when they remove ambiguous ownership, duplicate state, or compatibility debt.
- Do not keep old and new storage paths as parallel authoritative stores.
- After a one-shot migration succeeds, writes go only to the new path.
- Invalid migrated files must fail closed with a typed error or be quarantined with an explicit reason.
- Do not use `git add .`.
- Do not use `git reset --hard`, `git checkout --`, or destructive cleanup against user changes.

## Required Worktree Setup

Run from the original repository:

```bash
cd /Users/goya/Repo/Git/Jyowo
PLAN_PATH="docs/superpowers/plans/2026-07-06-configuration-storage-redesign-implementation.md"
test "$(git branch --show-current)" = "main"
test -f "$PLAN_PATH"
test "$(git ls-files -- "$PLAN_PATH")" = "$PLAN_PATH"
test -z "$(git status --short -- "$PLAN_PATH")"
test -z "$(git branch --list goya/configuration-storage-redesign)"
git worktree add -b goya/configuration-storage-redesign ../Jyowo-configuration-storage-redesign main
cd ../Jyowo-configuration-storage-redesign
```

Expected:

- The current branch in `/Users/goya/Repo/Git/Jyowo` is `main`.
- The plan file is tracked on `main`.
- The plan file has no uncommitted changes on `main`.
- The implementation branch does not already exist.
- All implementation commands after setup run in `/Users/goya/Repo/Git/Jyowo-configuration-storage-redesign`.

If the plan file is not tracked on `main`, stop. Commit or otherwise land the plan on `main` before creating the implementation worktree.

If the branch or worktree already exists, stop and inspect:

```bash
git worktree list
git branch --list "goya/configuration-storage-redesign"
```

Do not reuse an existing worktree without proving it was created for this plan and has no unrelated changes.

## Mandatory Reading

Before Task 1, read these files in the implementation worktree:

```text
AGENTS.md
docs/testing/testing-strategy.md
docs/frontend/agent-harness-frontend-development-guidelines.md
docs/frontend/frontend-product-ux.md
docs/frontend/frontend-engineering.md
docs/frontend/frontend-quality.md
docs/backend/agent-harness-backend-development-guidelines.md
docs/backend/backend-runtime.md
docs/backend/backend-engineering.md
docs/backend/backend-quality.md
docs/superpowers/plans/2026-07-06-configuration-storage-redesign-implementation.md
```

Before touching any nested directory, search for deeper instructions:

```bash
find . -path "*/AGENTS.md" -print
```

Read every deeper `AGENTS.md` that governs a touched path.

## Per-Task Protocol

Every task starts with a written analysis before edits:

```text
Task N analysis:
- Objective:
- Current code facts:
- Files to touch:
- Tests that must fail before implementation:
- Security and privacy constraints:
- Migration behavior:
- Destructive refactor decision:
- What will not be changed:
```

Every task ends with this sequence before commit:

```text
1. Run task-specific tests.
2. Run task-specific gate.
3. Run `git diff --check`.
4. Inspect `git status --short` and `git diff --stat`.
5. Dispatch a fresh read-only subagent audit and request `model: gpt-5.5`, `reasoning_effort: xhigh`, `service_tier: priority` where supported.
6. Fix every confirmed audit finding.
7. Re-run changed tests and gates.
8. Commit only explicit files for the task.
```

Read-only audit prompt template:

```text
You are auditing Task N of docs/superpowers/plans/2026-07-06-configuration-storage-redesign-implementation.md.
Request model gpt-5.5 with reasoning_effort xhigh where supported. If unavailable, record that limitation.
Do not modify files.
Verify the implementation against:
- the task objective,
- the target storage design,
- the no-workspace conversation design,
- the non-negotiable rules,
- repository AGENTS.md and relevant frontend/backend docs.
Report only concrete defects with file paths and line numbers.
Also state whether the task is complete, incomplete, or blocked.
```

Tasks touching provider credentials, file permissions, IPC commands, permission policy, migration of secrets, MCP servers, plugins, skills, runtime cwd, or filesystem access also require a read-only security audit before commit:

```text
You are security-auditing Task N of docs/superpowers/plans/2026-07-06-configuration-storage-redesign-implementation.md.
Request model gpt-5.5 with reasoning_effort xhigh where supported. If unavailable, record that limitation.
Do not modify files.
Focus on secret exposure, path traversal, symlink handling, permission bypass, tenant/workspace scope confusion, redaction, IPC payload trust, and no-workspace file access.
Report concrete defects with file paths and line numbers.
```

Commit format:

```bash
git status --short
git diff --stat
git add apps/desktop/src-tauri/src/storage_layout.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "feat: <task-specific message>"
```

The `git add` line above is an example for Task 1. For other tasks, stage only the exact files changed by that task.

## File Responsibility Map

Expected backend files to create or modify:

```text
apps/desktop/src-tauri/src/storage_layout.rs
apps/desktop/src-tauri/src/project_registry.rs
apps/desktop/src-tauri/src/agent_supervisor.rs
apps/desktop/src-tauri/src/commands/runtime.rs
apps/desktop/src-tauri/src/commands/providers.rs
apps/desktop/src-tauri/src/commands/stores/mod.rs
apps/desktop/src-tauri/src/commands/stores/skill.rs
apps/desktop/src-tauri/src/commands/stores/plugin.rs
apps/desktop/src-tauri/src/commands/stores/mcp.rs
apps/desktop/src-tauri/src/commands/stores/automation.rs
crates/jyowo-harness-agent-runtime/src/store.rs
crates/jyowo-harness-agent-runtime/src/profiles.rs
crates/jyowo-harness-provider-state/src/lib.rs
crates/jyowo-harness-memory/
crates/jyowo-harness-skill/src/loader.rs
crates/jyowo-harness-skill/src/sources/user.rs
```

Expected frontend files to create or modify after backend contracts are stable:

```text
apps/desktop/src/shared/tauri/commands.ts
apps/desktop/src/features/settings/
apps/desktop/src/shared/
```

Expected docs to modify:

```text
docs/backend/backend-engineering.md
docs/backend/backend-runtime.md
docs/frontend/frontend-engineering.md
docs/testing/testing-strategy.md
```

If a listed path no longer exists, search with `rg` and record the replacement path in the task analysis before editing.

## Task 1: Storage Scope and Path Layout Primitives

**Files:**

- Create: `apps/desktop/src-tauri/src/storage_layout.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs` or the current module root that declares Tauri backend modules
- Test: Rust unit tests inside `apps/desktop/src-tauri/src/storage_layout.rs`

- [ ] **Step 1: Analyze the task**

Write the required `Task 1 analysis` block. Confirm how the Tauri crate declares modules and how current stores compute paths.

- [ ] **Step 2: Add failing tests for scope paths**

Add tests that assert these path mappings:

```text
global config root       -> ~/.jyowo/config
global runtime root      -> ~/.jyowo/runtime
global skills root       -> ~/.jyowo/skills
global plugins root      -> ~/.jyowo/plugins
project config root      -> <workspace>/.jyowo/config
project runtime root     -> <workspace>/.jyowo/runtime
project skills root      -> <workspace>/.jyowo/skills
project plugins root     -> <workspace>/.jyowo/plugins
no-workspace runtime     -> ~/.jyowo/runtime/global-conversations
no-workspace workdir     -> ~/.jyowo/runtime/global-conversations/workdir/<conversationId>
project memory           -> <workspace>/.jyowo/runtime/memory/memory.sqlite3
no-workspace memory      -> ~/.jyowo/runtime/global-conversations/memory/memory.sqlite3
no-workspace agent db    -> ~/.jyowo/runtime/global-conversations/agent-runtime.sqlite
no-workspace worktrees   -> ~/.jyowo/runtime/global-conversations/agent-worktrees/
```

Also test that no-workspace scope is not equal to `~/.jyowo/unconfigured`, not equal to `HOME`, and does not produce a shared cwd for two different conversation ids.

- [ ] **Step 3: Implement storage layout types**

Create focused types:

```rust
pub enum StorageScope {
    Project { workspace_root: PathBuf },
    GlobalConversation,
}

pub enum RuntimeScope {
    Project { workspace_root: PathBuf },
    GlobalConversation { conversation_id: SessionId },
}

pub enum ConfigScope {
    Project,
    GlobalOnly,
}

pub struct JyowoHome {
    root: PathBuf,
}

pub struct StorageLayout {
    home: JyowoHome,
}

pub struct RuntimeLayout {
    pub scope: RuntimeScope,
    pub workspace_root: Option<PathBuf>,
    pub runtime_root: PathBuf,
    pub conversation_cwd: PathBuf,
    pub config_scope: ConfigScope,
}
```

Expose methods for every target root and for these files:

```text
global_provider_profiles_file()
global_provider_secrets_file()
global_provider_selection_file()
global_execution_defaults_file()
global_mcp_presets_file()
global_agent_profiles_file()
project_provider_selection_file(workspace)
project_provider_routes_file(workspace)
project_execution_overrides_file(workspace)
project_mcp_servers_file(workspace)
project_automations_file(workspace)
project_skills_file(workspace)
project_plugins_file(workspace)
project_agent_profile_selection_file(workspace)
runtime_root_for(scope)
conversation_workdir_for(scope, conversation_id)
runtime_memory_file_for(scope)
runtime_events_dir_for(scope)
runtime_blobs_dir_for(scope)
runtime_conversation_read_model_file_for(scope)
runtime_provider_continuations_file_for(scope)
runtime_permission_decisions_file_for(scope)
runtime_agent_database_file_for(scope)
runtime_agent_worktrees_dir_for(scope)
runtime_layout_for_project(workspace)
runtime_layout_for_global_conversation(conversation_id)
```

Keep this module pure. It should not read or write files.

- [ ] **Step 4: Run Task 1 tests**

Run:

```bash
cargo test -p jyowo-desktop-shell storage_layout
```

- [ ] **Step 5: Run Task 1 gate**

Run:

```bash
cargo fmt --all --check
cargo check -p jyowo-desktop-shell
git diff --check
```

- [ ] **Step 6: Audit and commit**

Dispatch the required read-only subagent audit and security audit. Commit explicit files only.

## Task 2: Atomic Store Utilities and File Safety

**Files:**

- Create or modify: `apps/desktop/src-tauri/src/commands/stores/mod.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/profiles.rs`
- Modify: `crates/jyowo-harness-permission/src/persistence/file.rs`
- Modify: `crates/jyowo-harness-provider-state/src/lib.rs` only if provider continuation writes need equivalent safe file primitives
- Modify: existing store modules only where they can reuse the new utility without changing semantics yet
- Test: Rust unit tests near the utility

- [ ] **Step 1: Analyze the task**

Identify current JSON write helpers in provider, skill, plugin, MCP, automation, diagnostics, quota, permission persistence, provider continuation, and agent profile stores. State which helper is canonical per crate/layer.

- [ ] **Step 2: Add failing tests for safe writes**

Tests must cover:

```text
0600 mode for secret-bearing files on Unix
atomic temp-file then rename path
parent creation without following symlink components
invalid JSON returns typed error
invalid JSON quarantine behavior when migration asks for quarantine
permission decision persistence writes are atomic and do not follow symlink path parents
permission integrity key creation is owner-only on Unix
```

- [ ] **Step 3: Implement canonical helpers**

Create or consolidate helpers with explicit names for the desktop shell-owned stores:

```text
read_json_file<T>()
write_json_file_atomic<T>()
write_secret_json_file_atomic<T>()
quarantine_invalid_json_file()
ensure_app_dir_no_symlink()
```

Lower crates cannot depend on `apps/desktop/src-tauri`. Either implement equivalent safe write primitives in the lower crate that owns the store, or introduce a lower-level helper crate only if the existing crate layer rules allow that dependency direction.

Do not change provider, skill, plugin, MCP, automation, permission, provider continuation, or agent profile storage paths in this task except for replacing duplicated write mechanics with the correct same-layer helper.

- [ ] **Step 4: Run Task 2 tests and gates**

Run:

```bash
cargo test -p jyowo-desktop-shell commands::stores
cargo test -p jyowo-harness-agent-runtime profiles
cargo test -p jyowo-harness-permission --features integrity persistence
cargo test -p jyowo-harness-provider-state
cargo fmt --all --check
cargo check -p jyowo-desktop-shell
cargo check -p jyowo-harness-agent-runtime
cargo check -p jyowo-harness-permission --features integrity
cargo check -p jyowo-harness-provider-state
git diff --check
```

- [ ] **Step 5: Audit and commit**

Dispatch the required read-only subagent audit and security audit. Commit explicit files only.

## Task 3: Project Registry and Runtime Scope Identity

**Files:**

- Modify: `apps/desktop/src-tauri/src/project_registry.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/src/agent_supervisor.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/store.rs`
- Modify: `crates/jyowo-harness-provider-state/src/lib.rs`
- Modify: callers that currently accept the unconfigured pseudo-workspace
- Test: Rust tests for project registry and runtime scope construction

- [ ] **Step 1: Analyze the task**

List all references to:

```bash
rg -n "unconfigured|workspace_root|runtime_root|DesktopRuntimeState|ProjectRegistry|AgentRuntimeStore::open|FileProviderContinuationStore::open|LocalSandbox::new|memory.sqlite3" apps/desktop/src-tauri crates
```

Classify each reference as project registry, runtime scope, permission scope, store path API, agent supervisor identity, Memory runtime, or UI command adapter.

- [ ] **Step 2: Add failing tests for explicit scope**

Tests must prove:

```text
Project scope resolves to <workspace>/.jyowo/runtime
GlobalConversation scope resolves to ~/.jyowo/runtime/global-conversations
GlobalConversation cwd resolves to ~/.jyowo/runtime/global-conversations/workdir/<conversationId>
GlobalConversation memory resolves to ~/.jyowo/runtime/global-conversations/memory/memory.sqlite3
GlobalConversation agent runtime sqlite resolves to ~/.jyowo/runtime/global-conversations/agent-runtime.sqlite
GlobalConversation agent worktrees resolve to ~/.jyowo/runtime/global-conversations/agent-worktrees
GlobalConversation does not produce a workspace project registry entry
Existing project registry still reads ~/.jyowo/projects.json
Two GlobalConversation layouts with different conversation ids have different cwd paths
```

- [ ] **Step 3: Replace pseudo-workspace identity**

Introduce an explicit runtime scope enum in the backend boundary where runtime state is created:

```rust
pub enum RuntimeScope {
    Project { workspace_root: PathBuf },
    GlobalConversation { conversation_id: SessionId },
}
```

Use `StorageLayout` to create `RuntimeLayout` for runtime assembly. Project layout keeps `workspace_root = Some(<workspace>)`; global conversation layout uses `workspace_root = None`. Keep `~/.jyowo/unconfigured` only as a legacy migration source until migration cleanup.

Add lower-store path APIs before changing callers:

```rust
impl AgentRuntimeStore {
    pub fn open_runtime_dir(runtime_root: impl AsRef<Path>) -> Result<Self, AgentRuntimeStoreError>;
}

impl FileProviderContinuationStore {
    pub fn open_runtime_dir(runtime_root: impl AsRef<Path>) -> Result<Self, ProviderContinuationStoreError>;
}
```

Keep existing `open(workspace_root)` only as a compatibility wrapper or delete it after all callers are moved. New runtime code must call the runtime-root APIs.

Update `build_desktop_harness`, `runtime_state_from_stream_permission_runtime`, sandbox setup, permission decision persistence, Memory provider initialization, provider continuation store, event store, blob store, sqlite evidence registry, and agent supervisor entry points to consume `RuntimeLayout` instead of reconstructing `.jyowo/runtime` from `workspace_root`.

- [ ] **Step 4: Preserve project registry behavior**

`~/.jyowo/projects.json` remains global registry state. Do not move it into `config/`.

- [ ] **Step 5: Run Task 3 tests and gates**

Run:

```bash
cargo test -p jyowo-desktop-shell project_registry
cargo test -p jyowo-desktop-shell runtime_scope
cargo test -p jyowo-harness-agent-runtime store
cargo test -p jyowo-harness-provider-state
cargo fmt --all --check
cargo check -p jyowo-desktop-shell
cargo check -p jyowo-harness-agent-runtime
cargo check -p jyowo-harness-provider-state
git diff --check
```

- [ ] **Step 6: Audit and commit**

Dispatch the required read-only subagent audit and security audit. Commit explicit files only.

## Task 4: Global Config Store Framework

**Files:**

- Create: `apps/desktop/src-tauri/src/commands/stores/global_config.rs`
- Modify: `apps/desktop/src-tauri/src/commands/stores/mod.rs`
- Modify or reuse DTOs in `crates/jyowo-harness-contracts` for stable persisted config shapes
- Modify: module declarations
- Test: Rust tests for global config store

- [ ] **Step 1: Analyze the task**

State the exact schema owner for each global config file. Stable persisted config DTOs must be defined or reused in `crates/jyowo-harness-contracts` with `Serialize`, `Deserialize`, and `JsonSchema`. Desktop-shell DTOs may wrap them for IPC camelCase compatibility, but must not become the canonical persisted schema owner.

- [ ] **Step 2: Add failing tests for global config files**

Tests must assert:

```text
~/.jyowo/config/provider-profiles.json
~/.jyowo/config/provider-secrets.json
~/.jyowo/config/provider-selection.json
~/.jyowo/config/execution-defaults.json
~/.jyowo/config/mcp-presets.json
~/.jyowo/config/agent-profiles.json
```

Tests must assert secret file writes use owner-only permissions on Unix.

- [ ] **Step 3: Implement typed global stores**

Define typed store entry points. Do not use untyped maps for public persisted shape unless the existing contract already requires them.

Required store methods:

```text
load_provider_profiles()
save_provider_profiles()
load_provider_secrets_metadata()
save_provider_secret()
delete_provider_secret()
load_global_provider_selection()
save_global_provider_selection()
load_execution_defaults()
save_execution_defaults()
load_mcp_presets()
save_mcp_presets()
load_global_agent_profiles()
save_global_agent_profiles()
```

Provider secret metadata returns only redacted fields and `hasSecret`.

- [ ] **Step 4: Run Task 4 tests and gates**

Run:

```bash
cargo test -p jyowo-desktop-shell global_config
cargo fmt --all --check
cargo check -p jyowo-desktop-shell
git diff --check
```

- [ ] **Step 5: Audit and commit**

Dispatch the required read-only subagent audit and security audit. Commit explicit files only.

## Task 4A: Migration Framework and Conflict Types

**Files:**

- Create or modify migration module near backend stores
- Modify: `apps/desktop/src-tauri/src/commands/stores/mod.rs`
- Modify: domain store modules only to call the migration framework when their task starts moving paths
- Test: Rust migration framework tests

- [ ] **Step 1: Analyze the task**

List existing invalid-file handling and migration-like behavior. Define the typed migration result surface before changing any storage path.

- [ ] **Step 2: Add failing tests for migration framework**

Tests must prove:

```text
old missing + new missing returns NotNeeded
old present + new missing migrates atomically
old present + new present with identical content returns AlreadyMigrated
old present + new present with conflicting content returns Conflict and writes nothing
invalid old JSON is quarantined only when the domain requests quarantine
secret-bearing migration writes owner-only files on Unix
partial failure leaves no new authoritative file
```

- [ ] **Step 3: Implement typed migration primitives**

Define explicit result and conflict types. Do not represent conflicts as strings.

Required concepts:

```text
MigrationResult::NotNeeded
MigrationResult::Migrated
MigrationResult::AlreadyMigrated
MigrationResult::Conflict
MigrationConflictKind::IdCollision
MigrationConflictKind::SchemaMismatch
MigrationConflictKind::SecretFingerprintMismatch
MigrationConflictKind::SecretMaterialRequiresUserAction
MigrationConflictKind::InvalidSource
MigrationConflictKind::UnsafePath
MigrationConflictKind::PartialWritePrevented
```

Domain tasks must use this framework when they move a file. Task 13 is only the compatibility-removal and final sweep, not the first point where migrations are introduced.

- [ ] **Step 4: Run Task 4A tests and gates**

Run:

```bash
cargo test -p jyowo-desktop-shell migration
cargo fmt --all --check
cargo check -p jyowo-desktop-shell
git diff --check
```

- [ ] **Step 5: Audit and commit**

Dispatch the required read-only subagent audit and security audit. Commit explicit files only.

## Task 5: Provider Profiles, Secrets, and Project Model Selection

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands/providers.rs`
- Modify: `apps/desktop/src-tauri/src/commands/stores/mod.rs`
- Modify or create project config store module
- Modify contracts if provider settings DTOs live in `crates/jyowo-harness-contracts`
- Test: Rust tests for provider config overlay and migration

- [ ] **Step 1: Analyze the task**

List current provider settings structs, Tauri command payloads, reveal-token flow, and backend secret redaction points. State which persisted fields move to global profile, global secret, and project selection.

Current split source:

```text
ProviderSettingsRecord.defaultConfigId -> project provider-selection.json
ProviderSettingsRecord.configs[*].id/displayName/providerId/modelId/protocol/baseUrl/modelDescriptor -> global provider profile
ProviderSettingsRecord.configs[*].apiKey/officialQuotaApiKey -> global secret storage or keychain
```

No-workspace conversations have no project selection layer. Their saved default model/profile selection resolves from `~/.jyowo/config/provider-selection.json` unless a run request supplies an explicit `model_config_id`.

- [ ] **Step 2: Add failing tests for provider split**

Tests must prove:

```text
provider profile definitions persist globally
raw provider secrets persist only in global secret storage or existing keychain abstraction
global default model/profile selection persists in ~/.jyowo/config/provider-selection.json
project default model/profile selection persists in <workspace>/.jyowo/config/provider-selection.json
provider command list response never includes raw secret
explicit reveal flow remains required for raw secret
effective provider settings combine global profile + global secret availability + project selection
no-workspace effective provider settings use global provider-selection.json when run params omit model_config_id
old workspace provider-settings.json migrates deterministically into global profiles/secrets + project selection
old workspace provider-settings.json does not seed global provider-selection.json
two workspaces with identical profile id and identical non-secret fields reuse one migrated global profile
two workspaces with same profile id but different profile fields or secret fingerprint produce deterministic renamed ids
```

- [ ] **Step 3: Implement provider ownership split**

Move persisted provider profile definitions out of `<workspace>/.jyowo/runtime/provider-settings.json`.

Target files:

```text
~/.jyowo/config/provider-profiles.json
~/.jyowo/config/provider-secrets.json
~/.jyowo/config/provider-selection.json
<workspace>/.jyowo/config/provider-selection.json
```

Global `provider-selection.json` owns no-workspace and global default model/profile selection. Project `provider-selection.json` owns project-specific default model/profile selection. Neither file stores raw keys.

- [ ] **Step 4: Implement provider migration**

Use the migration framework from Task 4A.

Rules:

```text
global profile id = old config id when unused
same id + identical non-secret profile fields + same secret fingerprint -> reuse existing migrated profile
same id + different non-secret fields or different secret fingerprint -> mint <oldId>-<workspaceHash8>
project provider-selection.json remaps old defaultConfigId to the migrated profile id
raw secrets are written only to global secret storage or existing keychain under the migrated profile id
conflict or invalid source produces typed conflict/quarantine and no partial writes
```

- [ ] **Step 5: Keep reveal flow explicit**

Existing reveal-token behavior must still require explicit user intent. React may receive raw secret only through the existing reveal command path and only for the requested profile.

- [ ] **Step 6: Run Task 5 tests and gates**

Run:

```bash
cargo test -p jyowo-desktop-shell providers
cargo test -p jyowo-desktop-shell migration
cargo fmt --all --check
cargo check -p jyowo-desktop-shell
git diff --check
```

- [ ] **Step 7: Audit and commit**

Dispatch the required read-only subagent audit and security audit. Commit explicit files only.

## Task 6: Execution Defaults and Project Overrides

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: provider/execution settings command module if separate
- Modify or create project config store module
- Modify or reuse execution config DTOs in `crates/jyowo-harness-contracts`
- Test: Rust tests for config overlay

- [ ] **Step 1: Analyze the task**

Find all reads and writes of `execution-settings.json`. Identify run request fields that already override saved settings.

Current `ExecutionSettingsRecord` fields:

```text
permission_mode
tool_profile
context_compression_trigger_ratio
subagents_enabled
agent_teams_enabled
background_agents_enabled
```

- [ ] **Step 2: Add failing tests for overlay precedence**

Tests must prove:

```text
global execution defaults are read from ~/.jyowo/config/execution-defaults.json
project overrides are read from <workspace>/.jyowo/config/execution-overrides.json
run explicit params override both persisted layers
missing project override falls back to global default
missing global default falls back to contract default
old workspace execution-settings.json migrates to <workspace>/.jyowo/config/execution-overrides.json
old workspace execution-settings.json does not seed global defaults
```

- [ ] **Step 3: Implement overlay**

Use one backend function for effective execution settings:

```text
resolve_effective_execution_settings(scope, run_params)
```

Do not duplicate overlay logic in React.

- [ ] **Step 4: Implement execution migration**

Use the migration framework from Task 4A. Treat old workspace `execution-settings.json` as project-specific state. Do not infer global defaults from the first workspace, active workspace, or most recent workspace. Global defaults remain absent/default unless an existing global defaults source already exists.

- [ ] **Step 5: Run Task 6 tests and gates**

Run:

```bash
cargo test -p jyowo-desktop-shell execution
cargo test -p jyowo-desktop-shell migration
cargo fmt --all --check
cargo check -p jyowo-desktop-shell
git diff --check
```

- [ ] **Step 6: Audit and commit**

Dispatch the required read-only subagent audit. Add security audit if execution settings can affect filesystem, sandbox, network, approval, or destructive command policy. Commit explicit files only.

## Task 7: Provider Routes and Provider Diagnostics Separation

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands/providers.rs`
- Modify: `apps/desktop/src-tauri/src/commands/stores/mod.rs`
- Modify diagnostics/quota cache store code
- Reuse provider route DTOs from `crates/jyowo-harness-contracts`
- Test: Rust tests for route config and runtime diagnostics

- [ ] **Step 1: Analyze the task**

List current reads/writes of:

```text
provider-capability-routes.json
provider-diagnostics.json
provider-quota-cache.json
```

- [ ] **Step 2: Add failing tests for route/runtime split**

Tests must prove:

```text
provider routes persist in <workspace>/.jyowo/config/provider-capability-routes.json
provider diagnostics persist in <workspace>/.jyowo/runtime/provider-diagnostics.json
provider quota cache persists in <workspace>/.jyowo/runtime/provider-quota-cache.json
no diagnostics file is read as config
old provider-capability-routes.json migrates to project config
```

- [ ] **Step 3: Move route persistence**

Move only route config. Keep diagnostics and quota cache under runtime.

Use the migration framework from Task 4A. Do not migrate diagnostics or quota cache into config.

- [ ] **Step 4: Run Task 7 tests and gates**

Run:

```bash
cargo test -p jyowo-desktop-shell provider_routes
cargo test -p jyowo-desktop-shell provider_diagnostics
cargo test -p jyowo-desktop-shell migration
cargo fmt --all --check
cargo check -p jyowo-desktop-shell
git diff --check
```

- [ ] **Step 5: Audit and commit**

Dispatch the required read-only subagent audit and security audit. Commit explicit files only.

## Task 8: Global and Project Skills

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands/stores/skill.rs`
- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `crates/jyowo-harness-skill/src/loader.rs` only if source merge behavior is insufficient
- Modify: `crates/jyowo-harness-skill/src/sources/user.rs` only if global user source needs API adjustment
- Test: Rust tests for skill store and runtime loader

- [ ] **Step 1: Analyze the task**

State current skill package shape and loader source precedence. Confirm whether `DirectorySourceKind::User` already exists and how ids are derived.

- [ ] **Step 2: Add failing tests for global/project skills**

Tests must prove:

```text
global skills store lives under ~/.jyowo/skills
project skills store lives under <workspace>/.jyowo/skills
enabled skill selection lives in <workspace>/.jyowo/config/skills.json
runtime loads global enabled skills and project enabled skills
project skill id collision with global skill id is deterministic and reported
disabled project selection prevents project loading
old workspace runtime skills migrate to project-private skills by default
old workspace runtime skills are not promoted to global skills automatically
```

- [ ] **Step 3: Implement skill storage split**

Target files:

```text
~/.jyowo/skills/index.json
~/.jyowo/skills/packages/
<workspace>/.jyowo/skills/packages/
<workspace>/.jyowo/config/skills.json
```

Do not keep `<workspace>/.jyowo/runtime/skills/` as a write target after migration.

Use the migration framework from Task 4A. Global skills may only come from an existing user/global source; project runtime skills stay project-private unless there is explicit user intent outside this migration.

- [ ] **Step 4: Wire runtime loading**

Desktop runtime must load:

```text
global enabled skills
project enabled skills
system/plugin skills already provided by existing skill roots
```

Define precedence in code and tests. If project skill overrides global skill by id, return a diagnostic that names both sources and the selected winner.

- [ ] **Step 5: Run Task 8 tests and gates**

Run:

```bash
cargo test -p jyowo-harness-skill
cargo test -p jyowo-desktop-shell skill
cargo test -p jyowo-desktop-shell migration
cargo fmt --all --check
cargo check -p jyowo-desktop-shell
git diff --check
```

- [ ] **Step 6: Audit and commit**

Dispatch the required read-only subagent audit and security audit. Commit explicit files only.

## Task 9: Global and Project Plugins

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands/stores/plugin.rs`
- Modify runtime plugin loading code
- Test: Rust tests for plugin storage and activation

- [ ] **Step 1: Analyze the task**

List current plugin package roots and enabled plugin state. Identify whether plugin activation can register MCP tools, skills, apps, or filesystem access.

- [ ] **Step 2: Add failing tests for plugin split**

Tests must prove:

```text
global plugin packages live under ~/.jyowo/plugins
project plugin packages live under <workspace>/.jyowo/plugins
enabled plugin selection lives in <workspace>/.jyowo/config/plugins.json
runtime plugin diagnostics stay under runtime
plugin package data is not written under <workspace>/.jyowo/runtime/plugins after migration
old workspace runtime plugins migrate to project-private plugins by default
old workspace runtime plugins are not promoted to global plugins automatically
```

- [ ] **Step 3: Implement plugin storage split**

Target files:

```text
~/.jyowo/plugins/index.json
~/.jyowo/plugins/packages/
<workspace>/.jyowo/plugins/packages/
<workspace>/.jyowo/config/plugins.json
```

If plugin activation changes permission surface, keep Rust policy authority as the final decision point.

Use the migration framework from Task 4A. Global plugins may only come from an existing user/global source; project runtime plugins stay project-private unless there is explicit user intent outside this migration.

- [ ] **Step 4: Run Task 9 tests and gates**

Run:

```bash
cargo test -p jyowo-desktop-shell plugin
cargo test -p jyowo-desktop-shell migration
cargo fmt --all --check
cargo check -p jyowo-desktop-shell
git diff --check
```

- [ ] **Step 5: Audit and commit**

Dispatch the required read-only subagent audit and security audit. Commit explicit files only.

## Task 10: MCP Presets and Project Servers

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands/stores/mcp.rs`
- Modify MCP command wrappers if needed
- Modify or reuse MCP persisted config DTOs in `crates/jyowo-harness-contracts`
- Test: Rust tests for MCP config/runtime split

- [ ] **Step 1: Analyze the task**

List current MCP persisted fields and diagnostics fields. Identify which fields can be global presets and which are project-specific enabled/custom servers. Classify every `env`, `inheritEnv`, `headers`, `headersFromEnv`, `bearerTokenEnvVar`, URL, command, and working-directory field as non-sensitive inline config, environment-variable reference, secret reference, or secret-bearing inline material.

- [ ] **Step 2: Add failing tests for MCP split**

Tests must prove:

```text
global MCP presets persist in ~/.jyowo/config/mcp-presets.json
project MCP servers persist in <workspace>/.jyowo/config/mcp-servers.json
MCP diagnostics persist in <workspace>/.jyowo/runtime/mcp-diagnostics.jsonl
no diagnostic payload stores raw secrets
project MCP config stores non-sensitive inline values and env-var/secret refs only
raw `Authorization`, `Cookie`, bearer token, OAuth secret, private absolute path, and secret-like `env.value` or `headers.value` material is not serialized into new config
old workspace mcp-servers.json migrates to project MCP servers by default
old workspace MCP servers are not promoted to global presets automatically
old workspace MCP entries with inline secret-bearing material produce `SecretMaterialRequiresUserAction` conflict/quarantine unless an existing secret-store abstraction can preserve them as refs without exposing raw values
```

- [ ] **Step 3: Implement MCP store split**

Global presets are reusable definitions. Project servers select and configure what is active in that project.

Use the migration framework from Task 4A. Global MCP presets start from an existing global preset source only. Old workspace MCP servers remain project servers by default.

MCP migration rules:

```text
env/inheritEnv and headersFromEnv references may migrate as references
known non-sensitive inline stdio env values may migrate only after classifier tests prove they are non-secret
http static headers may migrate only when classifier tests prove they are non-secret
authorization headers, bearer tokens, OAuth secrets, cookies, *_TOKEN, *_SECRET, *_KEY, and private absolute paths fail closed into typed conflict/quarantine
diagnostics remain runtime JSONL and redacted
```

- [ ] **Step 4: Run Task 10 tests and gates**

Run:

```bash
cargo test -p jyowo-desktop-shell mcp
cargo test -p jyowo-desktop-shell migration
cargo fmt --all --check
cargo check -p jyowo-desktop-shell
git diff --check
```

- [ ] **Step 5: Audit and commit**

Dispatch the required read-only subagent audit and security audit. Commit explicit files only.

## Task 11: Automations and Agent Profiles

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands/stores/automation.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/profiles.rs`
- Modify: `crates/jyowo-harness-agent-runtime/src/store.rs`
- Modify project config store module
- Modify or reuse automation/profile config DTOs in `crates/jyowo-harness-contracts`
- Test: Rust tests for automation/profile storage

- [ ] **Step 1: Analyze the task**

List current automation config, automation run log, agent profile config, and agent runtime database paths.

- [ ] **Step 2: Add failing tests for storage separation**

Tests must prove:

```text
automations config persists in <workspace>/.jyowo/config/automations.json
automation run logs persist in <workspace>/.jyowo/runtime/automation-runs.jsonl
global agent profiles persist in ~/.jyowo/config/agent-profiles.json
project agent profile selection persists in <workspace>/.jyowo/config/agent-profile-selection.json
agent runtime sqlite persists in <workspace>/.jyowo/runtime/agent-runtime.sqlite
old automations.json migrates to project config
old agent-profiles.json migrates to global definitions with deterministic collision handling
migrated global agent profile definitions have persisted `scope: User`
```

- [ ] **Step 3: Implement automation split**

Move automation definitions to project config. Keep run history and diagnostics under runtime.

Use the migration framework from Task 4A. Automation run logs stay runtime and are not migrated into config.

- [ ] **Step 4: Implement agent profile split**

Global agent profiles are reusable definitions. Project selection chooses the default profile for a project. This plan targets global definitions plus project selection, not dual global/project profile definition stores.

Migration rules:

```text
old Builtin profiles are not written as user/global definitions
old User profiles migrate to global definitions with their id when unused and persisted `scope: User`
old Project profiles migrate to global definitions with id <workspaceHash8>-<oldId> unless that id is already used by an identical definition; their persisted scope is normalized to `User`
project agent-profile-selection.json stores the migrated default profile id when the old file had an explicit default
if no old default exists, do not invent a project default
id collisions with different profile behavior produce typed conflict/quarantine and no partial writes
```

- [ ] **Step 5: Run Task 11 tests and gates**

Run:

```bash
cargo test -p jyowo-desktop-shell automation
cargo test -p jyowo-harness-agent-runtime profiles
cargo test -p jyowo-desktop-shell migration
cargo fmt --all --check
cargo check -p jyowo-desktop-shell
cargo check -p jyowo-harness-agent-runtime
git diff --check
```

- [ ] **Step 6: Audit and commit**

Dispatch the required read-only subagent audit. Add security audit if profiles can change tool, permission, network, filesystem, MCP, or model routing behavior. Commit explicit files only.

## Task 12: No-Workspace Conversation Runtime

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands/runtime.rs`
- Modify: `apps/desktop/src-tauri/src/agent_supervisor.rs`
- Modify permission/sandbox integration files discovered by `rg -n "permission|sandbox|cwd|workspace" apps/desktop/src-tauri crates`
- Modify attachment/export path helpers if they currently assume project workspace
- Modify Memory runtime wiring if it currently derives from `workspace_root`
- Test: Rust integration/unit tests for no-workspace runtime

- [ ] **Step 1: Analyze the task**

List every runtime path currently derived from workspace root. Mark whether it applies to project sessions, no-workspace sessions, or both.

- [ ] **Step 2: Add failing tests for no-workspace runtime**

Tests must prove:

```text
no-workspace events persist in ~/.jyowo/runtime/global-conversations/events
no-workspace blobs persist in ~/.jyowo/runtime/global-conversations/blobs
no-workspace sqlite persists in ~/.jyowo/runtime/global-conversations/conversation-read-model.sqlite
no-workspace provider continuations persist in ~/.jyowo/runtime/global-conversations/provider-continuations.jsonl
no-workspace permission decisions persist in ~/.jyowo/runtime/global-conversations/permission-decisions.json
no-workspace permission decisions include conversation/runtime-scope metadata and do not apply across unrelated no-workspace conversations
no-workspace memory persists in ~/.jyowo/runtime/global-conversations/memory/memory.sqlite3
no-workspace Memory global settings are scoped to ~/.jyowo/runtime/global-conversations and thread settings are keyed by SessionId
no-workspace Memory evidence does not forge WorkspaceFile evidence without a workspace_id
no-workspace attachments persist in ~/.jyowo/runtime/global-conversations/attachments
no-workspace exports persist in ~/.jyowo/runtime/global-conversations/exports
no-workspace agent runtime sqlite persists in ~/.jyowo/runtime/global-conversations/agent-runtime.sqlite
no-workspace agent worktrees persist in ~/.jyowo/runtime/global-conversations/agent-worktrees
default cwd is ~/.jyowo/runtime/global-conversations/workdir/<conversationId>
two no-workspace conversations do not share default cwd
default no-workspace file access outside workdir is denied without explicit grant
permanent no-workspace permission approval in one conversation does not authorize another no-workspace conversation
no-workspace background agent recovery uses runtime scope identity, not a fake workspace root
deleting a no-workspace conversation prunes or makes unreadable its scratch/attachments/exports/evidence/runtime refs
```

- [ ] **Step 3: Implement global conversation runtime**

Use `RuntimeScope::GlobalConversation { conversation_id }` and `RuntimeLayout`. Remove runtime dependence on `~/.jyowo/unconfigured` except as a migration source. Runtime construction must pass `workspace_root = None` to components that only need identity and must pass `runtime_root` or `conversation_cwd` to components that need paths.

Agent supervisor identity for no-workspace sessions must derive from runtime scope id plus conversation id, not from a fake workspace path. Project supervisor identity can continue to derive from the normalized workspace path.

Memory runtime wiring must use the active `RuntimeLayout.runtime_root`. No-workspace Memory settings are shared within `~/.jyowo/runtime/global-conversations`, thread settings remain keyed by `SessionId`, and Memory evidence creation must reject `WorkspaceFile` evidence when there is no workspace id.

- [ ] **Step 4: Enforce no-workspace file policy**

Permission authority must fail closed for arbitrary file access outside the scratch workdir unless the request carries an explicit grant.

Extend permission persistence or the no-workspace persistence wrapper so persisted decisions include runtime scope metadata. `AllowAlways` decisions from a no-workspace conversation must match the same conversation/runtime scope before reuse. If an existing decision lacks no-workspace scope metadata, treat it as invalid for no-workspace authorization and quarantine or ignore it with a typed reason.

- [ ] **Step 5: Implement no-workspace cleanup**

Conversation deletion must remove the per-conversation scratch directory and either remove or backend-block access to attachments, exports, evidence refs, metadata, Memory thread links, and runtime records for that conversation. Do not delete unrelated global-conversation runtime data.

- [ ] **Step 6: Run Task 12 tests and gates**

Run:

```bash
cargo test --workspace no_workspace
cargo test --workspace global_conversation
cargo test --workspace memory
cargo test --workspace agent_supervisor
cargo fmt --all --check
cargo check --workspace
git diff --check
```

- [ ] **Step 7: Audit and commit**

Dispatch the required read-only subagent audit and security audit. Commit explicit files only.

## Task 13: Compatibility Removal and Final Migration Sweep

**Files:**

- Modify migration module near backend stores
- Modify all store modules touched above
- Modify startup/runtime initialization code that triggers migrations
- Test: Rust migration tests

- [ ] **Step 1: Analyze the task**

Update and verify the migration matrix created by the domain tasks:

```text
<workspace>/.jyowo/runtime/provider-settings.json              -> ~/.jyowo/config/provider-profiles.json + ~/.jyowo/config/provider-secrets.json + <workspace>/.jyowo/config/provider-selection.json
<workspace>/.jyowo/runtime/execution-settings.json             -> <workspace>/.jyowo/config/execution-overrides.json
<workspace>/.jyowo/runtime/provider-capability-routes.json     -> <workspace>/.jyowo/config/provider-capability-routes.json
<workspace>/.jyowo/runtime/skills/                             -> <workspace>/.jyowo/skills + <workspace>/.jyowo/config/skills.json
<workspace>/.jyowo/runtime/mcp-servers.json                    -> <workspace>/.jyowo/config/mcp-servers.json
<workspace>/.jyowo/runtime/automations.json                    -> <workspace>/.jyowo/config/automations.json
<workspace>/.jyowo/runtime/plugins/                            -> <workspace>/.jyowo/plugins + <workspace>/.jyowo/config/plugins.json
<workspace>/.jyowo/runtime/agent-profiles.json                 -> ~/.jyowo/config/agent-profiles.json + <workspace>/.jyowo/config/agent-profile-selection.json
~/.jyowo/unconfigured/.jyowo/runtime/*                         -> ~/.jyowo/runtime/global-conversations/*
~/.jyowo/unconfigured/.jyowo/runtime/memory/memory.sqlite3      -> ~/.jyowo/runtime/global-conversations/memory/memory.sqlite3
~/.jyowo/unconfigured/.jyowo/runtime/agent-runtime.sqlite       -> ~/.jyowo/runtime/global-conversations/agent-runtime.sqlite
~/.jyowo/unconfigured/.jyowo/runtime/agent-worktrees/           -> ~/.jyowo/runtime/global-conversations/agent-worktrees/
~/.jyowo/unconfigured/.jyowo/runtime/permission-decisions.json  -> ~/.jyowo/runtime/global-conversations/permission-decisions.json with conversation/runtime-scope metadata
```

Global promotion is intentionally absent for old project skills, plugins, and MCP servers. They remain project-private unless an existing global source already exists.

- [ ] **Step 2: Add failing migration tests**

Tests must cover:

```text
old-only file migrates to new file
new file already exists and old file exists returns deterministic conflict result
invalid old JSON is quarantined and not partly migrated
secret-bearing migrated file is owner-only
after migration, save writes only new path
after migration, load reads only new path
startup triggers every domain migration before the migrated store becomes writable
no-workspace permission decisions are migrated only when they can be scoped to a conversation; otherwise they are quarantined
```

- [ ] **Step 3: Implement migrations**

Verify every domain migration is idempotent and uses version markers only where needed. Do not make old paths authoritative after migration. This task may add missing startup wiring, but it must not invent field ownership rules that were omitted from earlier domain tasks.

- [ ] **Step 4: Remove old write paths**

Search and prove there are no writes to old config paths:

```bash
rg -n "provider-settings\\.json|execution-settings\\.json|provider-capability-routes\\.json|runtime/skills|mcp-servers\\.json|automations\\.json|runtime/plugins|agent-profiles\\.json|unconfigured|memory\\.sqlite3|AgentRuntimeStore::open\\(|FileProviderContinuationStore::open\\(|join\\(\"runtime\"\\)" apps/desktop/src-tauri crates
```

Remaining matches must be migration tests, migration source reads, docs describing migration, compatibility wrappers, or runtime-root APIs that do not reconstruct paths from a workspace root.

- [ ] **Step 5: Run Task 13 tests and gates**

Run:

```bash
cargo test --workspace migration
cargo fmt --all --check
cargo check --workspace
git diff --check
```

- [ ] **Step 6: Audit and commit**

Dispatch the required read-only subagent audit and security audit. Commit explicit files only.

## Task 14: Frontend Command Schemas and Settings UI

**Files:**

- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: settings feature files under `apps/desktop/src/features/settings/`
- Modify or create shared UI helpers only if existing primitives cannot express global/project scope labels
- Test: frontend unit/component tests near changed components

- [ ] **Step 1: Analyze the task**

List backend command payload changes from Tasks 4, 4A, and 5-13. List every frontend caller that reads provider, execution, MCP, skills, plugins, automation, agent profile, Memory, or no-workspace session settings. UI preferences remain non-sensitive local UI state owned by `@tauri-apps/plugin-store` and are not part of this backend storage migration.

- [ ] **Step 2: Add failing frontend tests**

Tests must prove:

```text
frontend validates new camelCase command responses with Zod
settings screens show whether a setting is global, project, or runtime-derived
project override UI never writes global defaults unless the command is explicitly global
provider secret is never stored in frontend local store or React query cache except explicit reveal flow response
no-workspace sessions do not show a project workspace path
migration conflict responses are rendered as backend-owned typed errors, not inferred from file paths
UI preferences continue to use the existing plugin-store local state and do not call backend config commands
```

- [ ] **Step 3: Update Tauri command schemas**

Update `apps/desktop/src/shared/tauri/commands.ts` so command wrappers match backend payloads. Keep external payload validation in Zod.

- [ ] **Step 4: Update settings UI**

Settings should distinguish:

```text
Global defaults
Project overrides
Runtime diagnostics
```

Use existing `shared/ui` primitives. Do not add marketing text, nested cards, or frontend-only policy decisions.

- [ ] **Step 5: Run Task 14 tests and gates**

Run:

```bash
pnpm check:desktop
pnpm check:frontend:fast
git diff --check
```

- [ ] **Step 6: Audit and commit**

Dispatch the required read-only subagent audit and security audit. Commit explicit files only.

## Task 15: Documentation and Repository Rules

**Files:**

- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-runtime.md`
- Modify: `docs/frontend/frontend-engineering.md`
- Modify: `docs/testing/testing-strategy.md` only if new test categories or fixtures are introduced
- Modify docs gates only if new architecture docs are added

- [ ] **Step 1: Analyze the task**

List every repository rule or backend/frontend doc statement made stale by the implementation. Include the current provider API key and agent profile storage statements.

- [ ] **Step 2: Update backend docs**

Document:

```text
global config root
project config root
runtime root
no-workspace runtime root
runtime layout object and per-conversation no-workspace cwd
provider profile/secret ownership
global and project model selection ownership
execution overlay rule
skill/plugin global/project ownership
MCP preset/project server ownership
automation config/run split
agent profile global/project selection split
Memory runtime storage under the active runtime root
no-workspace Memory settings/evidence constraints
agent supervisor runtime-scope identity
migration and no dual-authoritative state rule
UI preferences remain frontend plugin-store local UI state and are not migrated into backend config
```

- [ ] **Step 3: Update frontend docs**

Document:

```text
React renders global/project/runtime scope labels from backend data
React must not infer policy from paths
Tauri command payloads remain Zod-validated
secrets stay out of frontend state except explicit reveal response
non-sensitive UI preferences remain in `@tauri-apps/plugin-store`, not backend config
```

- [ ] **Step 4: Run docs gates**

Run:

```bash
pnpm check:docs
pnpm check:agent-docs
pnpm check:frontend-docs
pnpm check:backend-docs
git diff --check
```

- [ ] **Step 5: Audit and commit**

Dispatch the required read-only subagent audit. Commit explicit files only.

## Task 16: Full Verification, Cleanup, and Final Audit

**Files:**

- No planned source edits except fixes required by verification

- [ ] **Step 1: Analyze final state**

Write a final implementation analysis:

```text
Final analysis:
- Storage paths implemented:
- Old paths removed as write targets:
- Migrations implemented:
- No-workspace runtime implemented:
- Frontend command schemas updated:
- Docs updated:
- Known limitations:
```

- [ ] **Step 2: Search for forbidden old authoritative writes**

Run:

```bash
rg -n "provider-settings\\.json|execution-settings\\.json|provider-capability-routes\\.json|runtime/skills|mcp-servers\\.json|automations\\.json|runtime/plugins|agent-profiles\\.json|unconfigured|memory\\.sqlite3|AgentRuntimeStore::open\\(|FileProviderContinuationStore::open\\(|join\\(\"runtime\"\\)" apps/desktop/src-tauri crates docs
```

Every remaining match must be one of:

```text
migration source read
migration test
documentation of old-to-new migration
historical plan document
compatibility wrapper that delegates to runtime-root API
runtime-root API that does not derive authority from workspace root
```

- [ ] **Step 3: Run full gates**

Run:

```bash
pnpm check
pnpm check:docs
pnpm check:desktop
pnpm check:rust
pnpm audit:tests
pnpm check:test-architecture
pnpm check:testing-docs
pnpm check:agent-orchestration-no-fakes
pnpm check:agent-supervisor-sidecar
pnpm check:quick
pnpm check:frontend:fast
pnpm check:rust:fast
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
git diff --check
```

- [ ] **Step 4: Final read-only audits**

Dispatch two fresh read-only audits:

```text
Architecture audit:
- verifies target storage design,
- verifies global/project/runtime/no-workspace scope separation,
- verifies no dual-authoritative state,
- verifies frontend/backend ownership boundaries,
- verifies docs match code.
```

```text
Security audit:
- verifies provider secrets,
- verifies no-workspace filesystem access,
- verifies symlink and path traversal handling,
- verifies redaction,
- verifies IPC trust boundaries,
- verifies MCP/plugin/skill activation cannot bypass permission policy.
```

Both audits should request `model: gpt-5.5`, `reasoning_effort: xhigh`, `service_tier: priority` where supported and record any limitation.

- [ ] **Step 5: Prepare integration summary**

Produce a concise final summary with:

```text
commits created
storage paths changed
migrations included
tests and gates run
audit results
remaining risks, if any
```

Do not open a PR or push unless the user explicitly requests it.

## Acceptance Criteria

Implementation is complete only when all items are true:

```text
Global config exists under ~/.jyowo/config.
Project config exists under <workspace>/.jyowo/config.
Project runtime data remains under <workspace>/.jyowo/runtime.
No-workspace conversations use ~/.jyowo/runtime/global-conversations.
No-workspace cwd is ~/.jyowo/runtime/global-conversations/workdir/<conversationId>.
No two no-workspace conversations share the same default cwd.
~/.jyowo/unconfigured is not an active workspace or write target.
Runtime assembly uses RuntimeLayout instead of fake workspace roots.
Provider profile definitions are global.
Provider secrets are global and redacted from normal frontend/backend outputs.
Global model selection exists for no-workspace/global defaults.
Project model selection is project config.
Execution settings use global defaults + project overrides + run explicit params.
Provider routes are project config.
Provider diagnostics and quota cache are runtime.
Global skills exist and desktop runtime loads them.
Project skills remain project-private.
Enabled skill selection is project config.
Global plugins exist.
Project plugins remain project-private.
Enabled plugin selection is project config.
MCP presets are global config.
Project MCP servers are project config.
MCP diagnostics are runtime.
Automations config is project config.
Automation runs are runtime.
Agent profiles are global definitions with project selection.
Agent runtime sqlite remains runtime.
No-workspace agent runtime sqlite and worktrees live under ~/.jyowo/runtime/global-conversations.
Memory sqlite remains under the active runtime root.
No-workspace Memory settings are runtime-scoped and Memory evidence does not forge workspace evidence.
No-workspace permission decisions carry conversation/runtime-scope metadata and do not apply across unrelated no-workspace conversations.
Agent supervisor uses runtime scope identity and supports no-workspace recovery.
UI preferences remain Tauri plugin-store local UI state and are not migrated into backend config.
Old config paths are not write targets after migration.
Every task has a read-only subagent audit.
Security-sensitive tasks have a read-only security audit.
Full gates pass.
Docs match implemented behavior.
```

## Self-Review Checklist

Before handing this plan to implementation, verify:

```text
No task asks the worker to invent storage ownership.
Every storage path is explicitly named.
RuntimeLayout is required for runtime assembly.
Every task starts with implementation analysis.
Every task ends with subagent audit.
Security-sensitive tasks require security audit.
Implementation uses an isolated worktree.
No mock or fake production behavior is allowed.
No-workspace conversation data is covered.
No-workspace cwd is per conversation.
Memory runtime data is covered.
Agent supervisor runtime identity is covered.
Global skills are covered.
Destructive refactor is allowed only with stated reason.
Strict gates are listed.
```
