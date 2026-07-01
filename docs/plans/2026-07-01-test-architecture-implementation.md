# Test Architecture Cleanup Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refactor Jyowo's test architecture so tests are categorized, maintainable, enforceable by gates, and practical for AI-assisted development without deleting valuable safety coverage.

**Architecture:** Add one central testing strategy document, one generated inventory report, and two policy gates that keep test structure from drifting. Then refactor oversized and poorly named tests into domain-owned files, split the frontend test command fixture into domain builders, move UI state matrices into Storybook where appropriate, and update CI so pull requests run fast gates while main/manual runs full gates.

**Tech Stack:** Node 24, pnpm 11.7, Vitest 4, Testing Library, Storybook 10, Playwright 1, Rust 1.96, Cargo test, Tauri 2, Git worktrees, existing Jyowo docs gates.

---

## Mandatory Execution Rules

Run this plan from an isolated worktree. Do not implement it directly in the developer's active checkout.

```bash
SOURCE_CHECKOUT="$(pwd)"
PLAN_PATH="docs/plans/2026-07-01-test-architecture-implementation.md"
test -f "$PLAN_PATH"
git status --short
# Record this output. Existing unrelated changes in the active checkout are allowed.
# Do not stash, commit, revert, or edit them.
git fetch origin
git worktree add -b goya/test-architecture-cleanup ../Jyowo-test-architecture-cleanup origin/main
cd ../Jyowo-test-architecture-cleanup
mkdir -p "$(dirname "$PLAN_PATH")"
cp "$SOURCE_CHECKOUT/$PLAN_PATH" "$PLAN_PATH"
git add -- "$PLAN_PATH"
git status --short
git branch --show-current
```

Expected:

- the active checkout is not modified by this setup
- the new worktree branch is `goya/test-architecture-cleanup`
- the new branch starts from `origin/main`, not from a possibly stale local `main`
- `docs/plans/2026-07-01-test-architecture-implementation.md` exists in the new worktree and is staged as an exact owned file
- if branch or worktree names already exist, stop and ask for a new name instead of reusing a dirty worktree

The copied plan file is setup-owned. It is allowed to remain in `git diff --name-only` and `git diff --cached --name-only` during every task. Do not treat it as unrelated task drift. Include it in final commit evidence separately from Task 1-12 owned files.

Before every task:

1. Read root `AGENTS.md`.
2. Read the files listed in that task.
3. Write a short task-intake note in the assistant response before editing:
   - task objective
   - exact in-scope files
   - exact out-of-scope files
   - test behavior that must remain unchanged
   - gate commands for this task
   - why the task does not introduce mock data or fake implementation

After every task:

1. Run that task's verification commands.
2. Inspect `git diff --check`.
3. Dispatch a read-only subagent audit for the completed task before moving on.
4. The subagent must return `PASS` or `FAIL` with file and line evidence.
5. If the audit returns `FAIL`, fix the issue and rerun the same audit.
6. Do not mark a task complete without a passing subagent audit.

Before dispatching the subagent, include an audit evidence block in the assistant response:

```text
Task N evidence:
- owned files changed:
  - exact/path
- commands run:
  - command: <exact command>
    exit: <exit code>
- git diff --check: exit <exit code>
- git diff --name-only: <exact changed files for this task>
```

The subagent must audit against this evidence and the actual diff. A read-only subagent cannot infer command execution from files alone.

Subagent audit prompt template:

```text
Read-only audit for Task N of docs/plans/2026-07-01-test-architecture-implementation.md.
Check only this task's intended files and behavior.
Verify:
- implementation matches the task objective
- no mock/fake/noop production behavior was introduced
- no unrelated refactor was made
- no valuable safety coverage was deleted without replacement
- naming and placement follow the new testing strategy
- gates listed in the task are sufficient
- the provided command evidence includes every required gate with exit code 0
- changed files are limited to the task-owned files
Return PASS or FAIL.
For FAIL, include exact file and line evidence.
```

If multi-agent tools are not available, stop and ask for tool availability. Do not self-certify a task audit.

Destructive refactoring is allowed only when it removes clear test architecture debt. It must preserve product behavior and safety coverage. Do not keep compatibility wrappers solely to avoid touching tests; this project is still in development.

Forbidden throughout this plan:

- adding production mocks, fake runtime paths, placeholder behavior, or noop success returns
- deleting permission, redaction, Secret, Journal, Replay, schema, IPC, contract, sandbox, or agent orchestration coverage without equivalent replacement
- expanding the global frontend command fixture after it has been split
- adding new `spike_*`, `m[0-9]+_`, `t[0-9]+_`, or vague catch-all test files
- changing product code behavior while doing pure test file moves
- staging broad directories such as `crates`, `scripts`, `docs`, or `apps/desktop/src`; stage exact files only

## Design

Testing ownership follows the same ownership model as the product:

- Rust behavior is tested at the owning crate.
- Public serde contracts are tested in `jyowo-harness-contracts`.
- Tauri command tests verify IPC payload validation, SDK delegation, redaction, and fail-closed behavior.
- Frontend tests verify Zod parsing, state reducers, hooks, component behavior, and user-visible states.
- Storybook owns complex visual state matrices.
- Playwright remains smoke/workflow coverage and must not become a fixture command runtime.

The target taxonomy is:

```text
unit
component
contract
policy
integration
smoke
manual-live
stress
```

Rust file naming rules:

```text
tests/contract.rs
tests/<domain>_contract.rs
tests/policy.rs
tests/<domain>_policy.rs
tests/integration_<domain>.rs
tests/<domain>_regression.rs
tests/smoke_<domain>.rs
tests/manual_live_<provider>.rs
tests/stress_<domain>.rs
tests/<large_subject>_<domain>.rs
```

Allowed semantic suffixes include `_contract`, `_policy`, `_regression`, `_settings`, `_probe`, `_quota`, `_routes`, and domain-specific split suffixes used by an oversized source file. Disallowed prefixes are stage or spike names such as `spike_`, `m[0-9]+_`, and `t[0-9]+_`.

Frontend file naming rules:

```text
*.schema.test.ts
*.store.test.ts
*.view-model.test.ts
*.render.test.tsx
*.component.test.tsx
*.workflow.test.tsx
*.permission.test.tsx
*.artifacts.test.tsx
*.redaction.test.tsx
*.large-output.test.tsx
*.stories.tsx
```

Frontend split test suffixes must describe user-visible behavior or boundary ownership. Do not use stage names, issue names, or generic buckets such as `misc`, `new`, or `temp`.

Size rules:

- hard fail: any tracked test file over 1200 lines unless explicitly allowlisted during active cleanup
- warning inventory: test file over 800 lines
- preferred frontend component test file: under 600 lines
- preferred Rust integration test file: under 1000 lines

The initial known cleanup targets are:

```text
crates/jyowo-harness-sdk/tests/runtime_assembly.rs
apps/desktop/src/testing/command-client.ts
apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx
apps/desktop/src-tauri/tests/commands/providers.rs
apps/desktop/src-tauri/tests/commands/activity_replay.rs
apps/desktop/src-tauri/tests/commands/runs_permissions.rs
crates/jyowo-harness-contracts/tests/m1_contracts.rs
crates/jyowo-harness-journal/tests/m2_t06.rs
crates/jyowo-harness-model/tests/t01_contract.rs
crates/jyowo-harness-session/tests/spike_steering.rs
crates/jyowo-harness-hook/tests/spike_replay_idempotent.rs
```

## Task 1: Add Test Inventory Audit

**Files:**

- Create: `scripts/audit-tests.mjs`
- Create: `docs/testing/test-inventory.md`
- Modify: `package.json`

**Step 1: Read current policy sources**

Read:

```text
AGENTS.md
package.json
apps/desktop/package.json
docs/frontend/frontend-quality.md
docs/backend/backend-quality.md
scripts/check-agent-docs.mjs
scripts/check-frontend-docs.mjs
scripts/check-backend-docs.mjs
```

**Step 2: Implement a read-only audit script**

Create `scripts/audit-tests.mjs`.

Required behavior:

- scan `apps/desktop/src`, `apps/desktop/e2e`, `apps/desktop/src-tauri/tests`, `crates/*/tests`, and `scripts`
- output Markdown to stdout
- output deterministic content with stable sorting
- use repo-relative paths only
- do not include timestamps, absolute paths, machine-local paths, environment values, or command durations
- include totals by layer:
  - frontend Vitest files
  - frontend Vitest test cases
  - Storybook files
  - Playwright spec files
  - Rust test files
  - Rust `#[test]` / `#[tokio::test]` count
  - script policy test files
- include largest test files by line count
- include files over 800 and over 1200 lines
- include disallowed or suspect names:
  - `spike_*`
  - `m[0-9]+_`
  - `t[0-9]+_`
  - `*_e2e.rs` outside real desktop/browser E2E
  - duplicate `contract.rs` / `api_contract.rs` pairs in the same crate
- include ignored/manual/live/stress tests
- include `createTestCommandClient` usage by file
- include Storybook file count by feature

Do not write files from this script.

**Step 3: Generate the initial inventory**

Run:

```bash
node scripts/audit-tests.mjs > docs/testing/test-inventory.md
```

Expected:

- `docs/testing/test-inventory.md` exists
- it lists the known large files and suspect names above
- it does not contain raw secrets or private environment values
- it is a committed canonical audit report, not temporary state
- it can be regenerated byte-for-byte with `pnpm audit:tests`

**Step 4: Add script entry**

Modify root `package.json`:

```json
"audit:tests": "node scripts/audit-tests.mjs"
```

**Step 5: Verify**

Run:

```bash
pnpm audit:tests
git diff --check -- scripts/audit-tests.mjs docs/testing/test-inventory.md package.json
```

**Step 6: Subagent audit**

Dispatch the required read-only subagent audit for Task 1.

## Task 2: Add Central Testing Strategy And Agent Rules

**Files:**

- Create: `docs/testing/testing-strategy.md`
- Modify: `AGENTS.md`
- Modify: `docs/frontend/frontend-quality.md`
- Modify: `docs/backend/backend-quality.md`

**Step 1: Write testing strategy**

Create `docs/testing/testing-strategy.md`.

Required sections:

```text
# Jyowo Testing Strategy
## Purpose
## Test Taxonomy
## Ownership Rules
## File Naming Rules
## Fixture Rules
## Deletion Rules
## Refactor Rules
## Local Test Selection
## Full Gates
## AI Agent Rules
```

Required content:

- tests are executable product constraints, not temporary scaffolding
- feature completion is not a reason to delete tests
- Rust behavior is tested at the owning crate
- frontend does not make final policy decisions
- Zod/serde/JsonSchema boundaries must have contract coverage
- permission, sandbox, Secret, Redactor, Journal, Replay, IPC, MCP, Memory, agent orchestration, and provider routing tests default to retained
- deletion requires one of:
  - product behavior was removed
  - assertion covers old behavior
  - duplicate test has no extra boundary value
  - test only checks implementation detail and a behavior-level test replaces it
  - snapshot has no stable business value
- fixture data must be domain-owned and minimal
- Storybook owns complex visual state matrices
- Playwright is smoke/workflow only
- manual-live/stress tests must be ignored by default and documented
- AI agents must add or update tests in the owning layer before claiming completion

**Step 2: Update AGENTS.md**

Add `docs/testing/testing-strategy.md` to the start-of-task reading order after root `AGENTS.md`.

Add these rules to the execution/quality sections:

```text
涉及测试、门禁、CI、前端组件、后端 runtime、IPC、contract、安全边界时，必须读取 docs/testing/testing-strategy.md。
新增或修改测试时必须遵守测试分类、命名、fixture、删除规则。
不得因为功能完成而删除测试。
```

Add new commands:

```text
pnpm audit:tests
pnpm check:test-architecture
pnpm check:testing-docs
pnpm check:quick
pnpm check:frontend:fast
pnpm check:rust:fast
```

**Step 3: Update frontend and backend quality docs**

In `docs/frontend/frontend-quality.md`:

- add a short reference to `../testing/testing-strategy.md`
- keep frontend-specific requirements
- do not duplicate the whole taxonomy

In `docs/backend/backend-quality.md`:

- add a short reference to `../testing/testing-strategy.md`
- keep backend-specific required coverage table
- do not duplicate the whole taxonomy

**Step 4: Verify**

Run:

```bash
pnpm check:agent-docs
pnpm check:frontend-docs
pnpm check:backend-docs
git diff --check -- AGENTS.md docs/testing/testing-strategy.md docs/frontend/frontend-quality.md docs/backend/backend-quality.md
```

Expected:

- docs gates pass or fail only because later tasks have not yet added new required script checks
- no forbidden stage-language is added to active frontend/backend docs

**Step 5: Subagent audit**

Dispatch the required read-only subagent audit for Task 2.

## Task 3: Add Testing Docs And Architecture Gates

**Files:**

- Create: `scripts/check-testing-docs.mjs`
- Create: `scripts/check-test-architecture.mjs`
- Modify: `scripts/check-agent-docs.mjs`
- Modify: `package.json`

**Step 1: Implement docs gate**

Create `scripts/check-testing-docs.mjs`.

Required checks:

- `docs/testing/testing-strategy.md` exists
- `docs/testing/test-inventory.md` exists
- `docs/testing/test-inventory.md` exactly matches current `node scripts/audit-tests.mjs` output
- `AGENTS.md` references `docs/testing/testing-strategy.md`
- root `package.json` has:
  - `audit:tests`
  - `check:test-architecture`
  - `check:testing-docs`
  - `check:quick`
  - `check:frontend:fast`
  - `check:rust:fast`
- testing strategy contains required terms:
  - `PermissionBroker`
  - `Redactor`
  - `Journal`
  - `Replay`
  - `Secret`
  - `Tauri command`
  - `Zod`
  - `serde`
  - `Storybook`
  - `Playwright`
  - `manual-live`
  - `stress`
  - `fixture`
  - `no mock`

The script must be read-only.

**Step 2: Implement architecture gate**

Create `scripts/check-test-architecture.mjs`.

Required checks:

- scan all tracked test and test-fixture files with `git ls-files`; do not use diff-only logic for final enforcement
- fail on any tracked test file name matching:
  - `spike_`
  - `m[0-9]+_`
  - `t[0-9]+_`
- fail on any tracked test file over 1200 lines unless listed in an explicit temporary allowlist inside the script
- fail if `manual_live_*.rs` lacks `#[ignore`
- fail if `stress_*.rs` lacks `#[ignore`
- fail if `apps/desktop/src/testing/command-client.ts` grows or remains a large monolith after Task 6 removes it
- warn, but do not fail, on files over 800 lines while cleanup is in progress

Bootstrap allowlist is required in Task 3 because cleanup happens in later tasks.

Temporary allowlist rules:

- allowlist entries are allowed only for files scheduled in Tasks 4-9
- allowlisted historical names must include their cleanup task number
- allowlisted over-1200-line files must include their split task number
- any unallowlisted historical name or oversized file must fail immediately
- Task 10 must remove completed allowlist entries and switch the gate to strict final behavior

**Step 3: Wire scripts**

Modify root `package.json`:

```json
"check:testing-docs": "node scripts/check-testing-docs.mjs",
"check:test-architecture": "node scripts/check-test-architecture.mjs",
"check:frontend:fast": "pnpm -C apps/desktop typecheck && pnpm -C apps/desktop lint && pnpm -C apps/desktop test",
"check:rust:fast": "pnpm check:agent-supervisor-sidecar && cargo fmt --all --check && cargo test -p jyowo-harness-contracts -p jyowo-desktop-shell",
"check:quick": "pnpm check:release-version && pnpm check:release-workflow && pnpm check:tauri-updater && pnpm check:docs && pnpm check:test-architecture && pnpm check:agent-orchestration-no-fakes && pnpm check:agent-supervisor-sidecar && pnpm check:frontend:fast && pnpm check:rust:fast"
```

Update `check:docs` to include `pnpm check:testing-docs`.

Update root `pnpm check` to include `pnpm check:test-architecture` before `pnpm check:desktop`.

**Step 4: Update agent docs check**

Modify `scripts/check-agent-docs.mjs` required references and commands to include the new testing doc and commands.

**Step 5: Verify**

Run:

```bash
pnpm check:testing-docs
pnpm check:agent-docs
pnpm check:docs
pnpm check:test-architecture
git diff --check -- scripts/check-testing-docs.mjs scripts/check-test-architecture.mjs scripts/check-agent-docs.mjs package.json
```

Expected:

- if `check:test-architecture` fails from known historical names or large files, add only temporary allowlist entries for files scheduled in Tasks 4-9
- no allowlist entry may be permanent

**Step 6: Subagent audit**

Dispatch the required read-only subagent audit for Task 3.

## Task 4: Normalize Historical Test Names

**Files:**

- Rename: `crates/jyowo-harness-contracts/tests/m1_contracts.rs`
- Rename: `crates/jyowo-harness-journal/tests/m2_t06.rs`
- Rename: `crates/jyowo-harness-model/tests/t01_contract.rs`
- Rename or delete with replacement: `crates/jyowo-harness-session/tests/spike_steering.rs`
- Rename or delete with replacement: `crates/jyowo-harness-hook/tests/spike_replay_idempotent.rs`

**Step 1: Classify each file before moving**

For each file, inspect its test names and classify it as:

- contract
- policy
- integration
- smoke
- stress
- manual-live

Do not change assertions in this task.

**Step 2: Rename files**

Use domain names, not stage names.

Expected target examples:

```text
crates/jyowo-harness-contracts/tests/core_contracts.rs
crates/jyowo-harness-journal/tests/conversation_projection_regression.rs
crates/jyowo-harness-model/tests/provider_contracts.rs
crates/jyowo-harness-session/tests/steering_regression.rs
crates/jyowo-harness-hook/tests/replay_idempotence_regression.rs
```

If a `spike_*` file duplicates existing coverage, delete it only after identifying the retained replacement test in the task-intake note.

**Step 3: Verify targeted Rust tests**

Run:

```bash
cargo test -p jyowo-harness-contracts --tests
cargo test -p jyowo-harness-journal --tests
cargo test -p jyowo-harness-model --tests
cargo test -p jyowo-harness-session --tests
cargo test -p jyowo-harness-hook --tests
pnpm check:test-architecture
git diff --check
```

**Step 4: Refresh inventory**

Run:

```bash
pnpm audit:tests > docs/testing/test-inventory.md
```

**Step 5: Subagent audit**

Dispatch the required read-only subagent audit for Task 4.

## Task 5: Split SDK Runtime Assembly Tests

**Files:**

- Split: `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`
- Create as needed:
  - `crates/jyowo-harness-sdk/tests/runtime_assembly_contract.rs`
  - `crates/jyowo-harness-sdk/tests/runtime_assembly_tools.rs`
  - `crates/jyowo-harness-sdk/tests/runtime_assembly_memory.rs`
  - `crates/jyowo-harness-sdk/tests/runtime_assembly_agents.rs`
  - `crates/jyowo-harness-sdk/tests/runtime_assembly_observability.rs`
  - `crates/jyowo-harness-sdk/tests/runtime_assembly_context.rs`
  - `crates/jyowo-harness-sdk/tests/runtime_assembly_support/mod.rs`

**Step 1: Map tests before moving**

Create a temporary local note in the assistant response listing:

- each existing test name
- target file
- reason for placement

Do not create a repo file for the temporary map.

**Step 2: Move tests mechanically**

Move test functions and only the helpers they need.

Rules:

- no assertion behavior changes
- no production code changes
- helpers used by one target file stay in that target file
- helpers used by two target files may be duplicated if duplication is smaller than a shared module
- helpers used by three or more target files go in `tests/runtime_assembly_support/mod.rs`
- each split test file that needs shared helpers imports them with `mod runtime_assembly_support;`
- keep `runtime_assembly_support` narrow; split nested modules by domain if it starts collecting unrelated helpers
- do not create `tests/runtime_assembly_support.rs`, because Cargo would treat it as another integration test binary

**Step 3: Verify**

Run:

```bash
cargo test -p jyowo-harness-sdk --tests
pnpm check:test-architecture
git diff --check -- crates/jyowo-harness-sdk/tests
```

Expected:

- no SDK test file exceeds 1200 lines
- removed allowlist entry for `runtime_assembly.rs` from `scripts/check-test-architecture.mjs`

**Step 4: Refresh inventory**

Run:

```bash
pnpm audit:tests > docs/testing/test-inventory.md
```

**Step 5: Subagent audit**

Dispatch the required read-only subagent audit for Task 5.

## Task 6: Split Frontend Command Client Fixture

**Files:**

- Split: `apps/desktop/src/testing/command-client.ts`
- Create:
  - `apps/desktop/src/testing/command-client/index.ts`
  - `apps/desktop/src/testing/command-client/base.ts`
  - `apps/desktop/src/testing/command-client/conversation.ts`
  - `apps/desktop/src/testing/command-client/settings.ts`
  - `apps/desktop/src/testing/command-client/memory.ts`
  - `apps/desktop/src/testing/command-client/agents.ts`
  - `apps/desktop/src/testing/command-client/plugins.ts`
  - `apps/desktop/src/testing/command-client/skills.ts`
  - `apps/desktop/src/testing/command-client/mcp.ts`
  - `apps/desktop/src/testing/command-client/artifacts.ts`
- Delete after migration: `apps/desktop/src/testing/command-client.ts`

**Step 1: Preserve public testing API**

Keep this import working:

```ts
import { createTestCommandClient } from '@/testing/command-client'
```

Do this with directory `index.ts`, not with a compatibility file that keeps the old monolith.

Resolution sequence:

1. Create `apps/desktop/src/testing/command-client/` files.
2. Move exports into `apps/desktop/src/testing/command-client/index.ts`.
3. Delete `apps/desktop/src/testing/command-client.ts` before running verification.
4. Run `test ! -f apps/desktop/src/testing/command-client.ts` before `typecheck`.

Do not rely on verification while both `command-client.ts` and `command-client/index.ts` exist. The old file can win module resolution and hide a broken split.

**Step 2: Extract domain fixture builders**

Each domain file must export only:

- default fixture values for that domain
- builder helpers for that domain
- domain-specific command handlers

Do not create production mocks. This is test-only fixture code under `apps/desktop/src/testing`.

**Step 3: Keep fixture behavior stable**

The existing tests should not need broad fixture rewrites. They may keep calling `createTestCommandClient`.

**Step 4: Verify**

Run:

```bash
test ! -f apps/desktop/src/testing/command-client.ts
pnpm -C apps/desktop test -- src/shared/tauri/commands.test.ts src/features/conversation/ConversationWorkspace.test.tsx src/features/settings/MCPManager.test.tsx src/features/skills/SkillsPage.test.tsx
pnpm -C apps/desktop typecheck
pnpm check:test-architecture
git diff --check -- apps/desktop/src/testing
```

Expected:

- no `apps/desktop/src/testing/command-client.ts` file remains
- no replacement file exceeds 1200 lines
- `createTestCommandClient` import path still works

**Step 5: Refresh inventory**

Run:

```bash
pnpm audit:tests > docs/testing/test-inventory.md
```

**Step 6: Subagent audit**

Dispatch the required read-only subagent audit for Task 6.

## Task 7: Split Conversation Timeline Component Tests

**Files:**

- Split: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`
- Create:
  - `apps/desktop/src/features/conversation/timeline/conversation-timeline.render.test.tsx`
  - `apps/desktop/src/features/conversation/timeline/conversation-timeline.permission.test.tsx`
  - `apps/desktop/src/features/conversation/timeline/conversation-timeline.artifacts.test.tsx`
  - `apps/desktop/src/features/conversation/timeline/conversation-timeline.redaction.test.tsx`
  - `apps/desktop/src/features/conversation/timeline/conversation-timeline.large-output.test.tsx`
- Delete after split: `apps/desktop/src/features/conversation/timeline/conversation-timeline.test.tsx`

**Step 1: Map test intent before moving**

Group existing tests by behavior:

- baseline render and ordering
- permission states
- artifact and attachment preview
- safe summaries and redaction
- large output / collapsed history / diff behavior

**Step 2: Move tests mechanically**

Rules:

- do not change component behavior
- do not weaken assertions
- extract repeated render helper into a local `conversation-timeline-test-utils.tsx` only if it reduces duplication across three or more files
- do not add new global fixture dependencies

**Step 3: Verify**

Run:

```bash
pnpm -C apps/desktop test -- src/features/conversation/timeline
pnpm -C apps/desktop typecheck
pnpm check:test-architecture
git diff --check -- apps/desktop/src/features/conversation/timeline
```

**Step 4: Refresh inventory**

Run:

```bash
pnpm audit:tests > docs/testing/test-inventory.md
```

**Step 5: Subagent audit**

Dispatch the required read-only subagent audit for Task 7.

## Task 8: Split Tauri Command Tests

**Files:**

- Split:
  - `apps/desktop/src-tauri/tests/commands/providers.rs`
  - `apps/desktop/src-tauri/tests/commands/activity_replay.rs`
  - `apps/desktop/src-tauri/tests/commands/runs_permissions.rs`
  - `apps/desktop/src-tauri/tests/commands/artifacts.rs`

**Step 1: Split providers tests**

Target files:

```text
apps/desktop/src-tauri/tests/commands/provider_settings.rs
apps/desktop/src-tauri/tests/commands/provider_probe.rs
apps/desktop/src-tauri/tests/commands/provider_quota.rs
apps/desktop/src-tauri/tests/commands/provider_routes.rs
```

Delete or shrink `providers.rs` after moving tests. Do not leave a catch-all file.

`apps/desktop/src-tauri/tests/commands/provider_probe.rs` already exists in the current repository. Do not overwrite it. First inspect its current tests, then merge only provider probe coverage from `providers.rs` into that existing file when the ownership matches. If coverage belongs to `official_quota.rs`, keep it there instead of forcing it into `provider_quota.rs`.

**Step 2: Split activity/replay tests**

Target files:

```text
apps/desktop/src-tauri/tests/commands/activity.rs
apps/desktop/src-tauri/tests/commands/replay.rs
apps/desktop/src-tauri/tests/commands/support_bundle.rs
```

**Step 3: Split runs/permissions tests**

Target files:

```text
apps/desktop/src-tauri/tests/commands/runs.rs
apps/desktop/src-tauri/tests/commands/permissions.rs
apps/desktop/src-tauri/tests/commands/run_subscriptions.rs
```

**Step 4: Split artifact tests**

Target files:

```text
apps/desktop/src-tauri/tests/commands/artifact_listing.rs
apps/desktop/src-tauri/tests/commands/artifact_preview.rs
apps/desktop/src-tauri/tests/commands/attachment_preview.rs
```

**Step 5: Update test module registration**

Update `apps/desktop/src-tauri/tests/commands.rs` or its module tree so Cargo discovers every new file.

**Step 6: Verify**

Run:

```bash
cargo test -p jyowo-desktop-shell --test commands
pnpm check:test-architecture
git diff --check -- apps/desktop/src-tauri/tests
```

Expected:

- no modified Tauri command test file exceeds 1200 lines
- no command test loses fail-closed coverage

**Step 7: Refresh inventory**

Run:

```bash
pnpm audit:tests > docs/testing/test-inventory.md
```

**Step 8: Subagent audit**

Dispatch the required read-only subagent audit for Task 8.

## Task 9: Move UI State Matrices Into Storybook

**Files:**

- Modify or create Storybook files for:
  - `apps/desktop/src/features/conversation/Composer.stories.tsx`
  - `apps/desktop/src/features/context/ContextPanel.stories.tsx`
  - `apps/desktop/src/features/activity/ActivityRail.stories.tsx`
  - `apps/desktop/src/features/settings/models/CapabilityRoutesPanel.stories.tsx`
  - `apps/desktop/src/features/artifacts/ArtifactPreview.stories.tsx`
- Modify only if needed:
  - corresponding component tests
  - `apps/desktop/e2e/conversation-evidence-storybook.spec.ts`
  - `apps/desktop/e2e/model-settings-storybook.spec.ts`

**Step 1: Identify test assertions that are visual state matrices**

Candidates:

- loading / empty / error / ready snapshots
- high risk / redacted / large output display
- disabled visual states
- long list visual arrangements

Keep user interactions and command calls in component tests.

**Step 2: Add missing stories**

Each complex component story set must include at least:

```text
Loading
Empty
Ready
Error
PermissionPending or HighRisk when relevant
Redacted when relevant
LargeOutput when relevant
```

Use realistic test fixtures. Do not use fake command runtime for production code.

**Step 3: Trim redundant component assertions**

Only remove component assertions when the same visual state is now covered by Storybook and behavior-level component coverage remains.

**Step 4: Verify**

Run:

```bash
pnpm -C apps/desktop test -- src/features/conversation/Composer.test.tsx src/features/context/ContextPanel.test.tsx src/features/activity/ActivityRail.test.tsx src/features/settings/models/CapabilityRoutesPanel.test.tsx src/features/artifacts/ArtifactPreview.test.tsx
pnpm -C apps/desktop build-storybook
pnpm -C apps/desktop test:e2e:storybook
git diff --check -- apps/desktop/src apps/desktop/e2e
```

**Step 5: Refresh inventory**

Run:

```bash
pnpm audit:tests > docs/testing/test-inventory.md
```

**Step 6: Subagent audit**

Dispatch the required read-only subagent audit for Task 9.

## Task 10: Tighten Test Architecture Gate And Remove Temporary Allowlists

**Files:**

- Modify: `scripts/check-test-architecture.mjs`
- Modify: `docs/testing/test-inventory.md`

**Step 1: Remove completed allowlist entries**

Remove allowlist entries for files cleaned in Tasks 4-9.

Remaining allowlist entries must include:

- exact file path
- reason
- follow-up task or issue reference

No allowlist entry may remain for a `spike_*`, `m[0-9]+_`, or `t[0-9]+_` file after its cleanup task is complete.

**Step 2: Make architecture gate strict**

Required final behavior:

- fail on disallowed historical names
- fail on any tracked test file over 1200 lines unless explicitly allowlisted
- fail on non-ignored `manual_live_*.rs`
- fail on non-ignored `stress_*.rs`
- fail if frontend test fixtures reintroduce a giant `command-client.ts`
- fail if `docs/testing/testing-strategy.md` is not referenced from `AGENTS.md`
- fail if `docs/testing/test-inventory.md` differs from `pnpm audit:tests` output

The final script must use all tracked files from `git ls-files`. It must not depend on only changed files.

**Step 3: Verify**

Run:

```bash
pnpm audit:tests > docs/testing/test-inventory.md
pnpm check:test-architecture
pnpm check:testing-docs
git diff --check -- scripts/check-test-architecture.mjs docs/testing/test-inventory.md
```

**Step 4: Subagent audit**

Dispatch the required read-only subagent audit for Task 10.

## Task 11: Update CI For Fast PR And Full Main Gates

**Files:**

- Modify: `.github/workflows/ci.yml`
- Modify only if needed: `.github/workflows/release.yml`
- Modify if needed: `scripts/release-workflow-policy.test.mjs`

**Step 1: Add manual trigger**

Add:

```yaml
workflow_dispatch:
```

**Step 2: Make PR gates fast**

For `pull_request`, run:

```text
policy-fast: pnpm check:release-version && pnpm check:release-workflow && pnpm check:tauri-updater && pnpm check:agent-orchestration-no-fakes && pnpm check:agent-supervisor-sidecar
docs: pnpm check:docs
test-architecture: pnpm check:test-architecture
frontend-fast: pnpm check:frontend:fast
rust-fast: pnpm check:rust:fast
```

Do not remove an existing policy gate from PR coverage unless this plan replaces it with an equivalent stricter gate.

**Step 3: Keep full gates on main and manual**

For `push` to `main` and `workflow_dispatch`, run:

```text
frontend: pnpm check:desktop
rust: pnpm check:rust
desktop-build: pnpm check:desktop:full
```

Use GitHub Actions `if:` conditions rather than duplicating workflows.

**Step 4: Ensure job runtimes are complete**

Every job that runs `pnpm` must include:

```yaml
- uses: pnpm/action-setup@v4
  with:
    version: 11.7.0
- uses: actions/setup-node@v4
  with:
    node-version: 24
    cache: pnpm
    cache-dependency-path: pnpm-lock.yaml
- run: pnpm install --frozen-lockfile
```

Rust jobs still need `dtolnay/rust-toolchain@stable`. A Rust job that runs `pnpm check:rust` or `pnpm check:rust:fast` needs both Node/pnpm setup and Rust setup.

**Step 5: Add Rust cache**

Use `swatinem/rust-cache@v2` for Rust jobs.

**Step 6: Verify workflow policy**

Run:

```bash
pnpm check:release-workflow
git diff --check -- .github/workflows/ci.yml .github/workflows/release.yml scripts/release-workflow-policy.test.mjs
```

If `scripts/release-workflow-policy.test.mjs` does not cover CI shape, add a focused script test for this workflow policy instead of relying on visual inspection.

The workflow policy test must verify:

- PR jobs include the policy gates listed above
- full main/manual jobs still run `pnpm check:desktop`, `pnpm check:rust`, and `pnpm check:desktop:full`
- every job that runs `pnpm` has Node, pnpm, and `pnpm install --frozen-lockfile`
- Rust jobs include `dtolnay/rust-toolchain@stable`

**Step 7: Subagent audit**

Dispatch the required read-only subagent audit for Task 11.

## Task 12: Full Verification And Final Audit

**Files:**

- Modify: `docs/testing/test-inventory.md`
- No other planned source edits unless a previous gate failure requires a fix

**Step 1: Regenerate inventory**

Run:

```bash
pnpm audit:tests > docs/testing/test-inventory.md
```

**Step 2: Run full gates**

Run:

```bash
pnpm check:testing-docs
pnpm check:test-architecture
pnpm check:docs
pnpm check:quick
pnpm check:desktop
pnpm check:rust
pnpm check
git diff --check
```

If `pnpm check:rust` fails because the supervisor sidecar is missing, run the existing documented sidecar build path through `pnpm check:rust`; do not bypass the root script.

**Step 3: Review final inventory**

Confirm:

- no `spike_*` test files remain
- no `m[0-9]+_` or `t[0-9]+_` test files remain
- no cleaned file remains in the architecture gate allowlist
- large files over 1200 lines are either gone or explicitly justified by a remaining temporary allowlist
- frontend command client fixture is split by domain
- `AGENTS.md` references the testing strategy
- CI uses fast PR gates without dropping existing policy gates
- CI uses full main/manual gates

**Step 4: Final read-only audit**

Dispatch one final subagent audit across the whole branch.

Prompt:

```text
Read-only final audit for the full test architecture cleanup branch.
Verify the branch implements docs/plans/2026-07-01-test-architecture-implementation.md.
Check test strategy docs, AGENTS.md integration, docs gates, test architecture gates, CI policy, Rust test splits, frontend fixture split, Storybook migration, and final inventory.
Return PASS or FAIL with file and line evidence.
```

**Step 5: Commit**

Only after all gates and audits pass:

```bash
git status --short
git diff --name-only
# Build the final owned-file list from Task 1-12 evidence.
# Inspect every path. Remove any unrelated user or other-agent change.
git add -- <exact-owned-file-1> <exact-owned-file-2> <exact-owned-file-n>
git diff --cached --name-only
git diff --cached --check
git commit -m "chore: clean up test architecture"
```

Do not use broad directories or globs in `git add`. Stop if `git diff --cached --name-only` contains any path that was not listed in the task evidence.

## Acceptance Criteria

- `docs/testing/testing-strategy.md` exists and is referenced by `AGENTS.md`.
- `docs/testing/test-inventory.md` is generated by `pnpm audit:tests`.
- `pnpm check:testing-docs` passes.
- `pnpm check:test-architecture` passes.
- `pnpm check:docs` passes.
- `pnpm check:quick` passes.
- `pnpm check:desktop` passes.
- `pnpm check:rust` passes.
- `pnpm check` passes.
- Every task has a passing read-only subagent audit.
- No production mock, fake, placeholder, noop, or compatibility shim is introduced.
- No safety-critical coverage is deleted without equivalent replacement.
- Large tests and global fixtures are split into domain-owned files.
- Historical test names are removed or renamed to semantic names.
- CI runs fast gates on PR and full gates on main/manual.
