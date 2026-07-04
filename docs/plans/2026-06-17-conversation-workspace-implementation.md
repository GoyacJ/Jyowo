# Conversation Workspace Implementation Plan

> **For Codex:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task.

**Goal:** Restore `docs/ui/image.png` as the Jyowo desktop product surface and then connect it to the real Rust harness runtime.

**Architecture:** Build the product as a conversation-first desktop workspace. React owns layout, view state, forms, schema validation, and requests. Rust owns policy, execution, events, journal, replay, permissions, tools, models, memory, secrets, and persistence.

**Tech Stack:** React 19, TypeScript 6, Vite 8, Tauri 2, TanStack Router, TanStack Query, Zustand UI-only state, React Hook Form, Zod, Tailwind CSS v4, Storybook, Playwright, Rust harness crates.

---

## Current State

Prototype:

- Source image: `docs/ui/image.png`
- Size: `1586x992`
- Product shape: left sidebar, center conversation canvas, right Context panel, bottom Activity rail.

Frontend:

- App route `/` renders `SystemStatusPage`.
- `AppShell` exists but uses admin/runtime navigation: `Workspaces`, `Runs`, `Tools`, `MCP`, `Memory`, `Evals`, `Models`, `Settings`.
- Existing shared primitives live under `apps/desktop/src/shared/ui`.
- Existing IPC wrapper lives under `apps/desktop/src/shared/tauri`.
- Existing mock command client supports only app info and healthcheck.

Backend:

- Tauri commands currently exposed:
  - `get_app_info`
  - `harness_healthcheck`
- `crates/jyowo-harness-contracts` already contains a rich event contract surface.
- `crates/jyowo-harness-sdk` is the desktop-facing facade.
- Tauri command handlers must stay thin.

Required reading before implementation:

- `AGENTS.md`
- `docs/frontend/agent-harness-frontend-development-guidelines.md`
- `docs/frontend/frontend-product-ux.md`
- `docs/frontend/frontend-engineering.md`
- `docs/frontend/frontend-quality.md`
- `docs/backend/agent-harness-backend-development-guidelines.md`
- `docs/backend/backend-runtime.md`
- `docs/backend/backend-engineering.md`
- `docs/backend/backend-quality.md`

## Prototype Contract

The desktop shell must preserve this hierarchy:

- Conversation canvas is the primary work surface.
- Composer is the primary action entry.
- Context panel is secondary.
- Activity rail is tertiary.
- Tool calls, permissions, Raw JSON, Replay, and audit data are support surfaces.
- Runs and trace data must not become primary navigation.

Required regions:

- Left sidebar:
  - workspace identity
  - search
  - recent conversations
  - Home
  - Conversations
  - Projects
  - Artifacts
  - Agents
  - Tools
  - Settings
  - local workspace identity
- Center:
  - conversation title
  - top actions
  - user message
  - assistant message
  - inline plan block
  - work progress text
  - diff preview
  - artifact summary
  - composer
- Right Context panel:
  - project
  - path
  - files
  - active artifact
  - decisions needed
  - next actions
- Bottom Activity rail:
  - collapsed/expanded affordance
  - recent tool statuses
  - current run status
  - link to full activity

Visual acceptance:

- Use semantic tokens, not feature hardcoded colors.
- Use `lucide-react` icons.
- Keep radius at `8px` or less.
- Avoid nested cards.
- Avoid dashboard gray card stacks.
- Avoid full-screen terminal posture.
- Avoid making MCP, permission, audit, or trace the product identity.
- Text must not overflow fixed controls.

## Non-goals

Do not build these while executing this plan unless a later task explicitly asks for them:

- Mobile layout.
- Team/public dashboard.
- Theme marketplace.
- Full IDE file editor.
- Provider-specific product branding.
- Raw Secret viewing.
- Frontend-only security decisions.
- A generic `execute` Tauri command.

## Architecture Rules

Frontend:

- Route files compose screens only.
- `app/shell` composes shell regions only.
- `features/conversation` owns conversation canvas, messages, composer, plan, progress, artifacts, review, and decisions.
- `features/workspace` owns sidebar, conversation list, project switcher, workspace identity, and search entry.
- `features/context` owns the Context panel and related rows.
- `features/activity` owns Activity rail, compact events, detailed activity, tool cards, permission cards, and drill-down.
- `features/settings` owns provider/model/MCP/local settings forms when added.
- Shared primitives remain in `shared/ui`.
- Feature code must not import Radix directly.
- Feature code must not import `@tauri-apps/api`.
- Feature code must not import `@chenglou/pretext`.
- TanStack Query owns backend and IPC-derived state.
- Zustand owns local UI state only.
- Zod validates IPC, event, storage, and form boundaries.

Backend:

- Public serialized contracts go in `crates/jyowo-harness-contracts`.
- Application-facing assembly goes through `crates/jyowo-harness-sdk`.
- Tauri shell calls the SDK facade.
- Runtime decisions remain in Rust.
- `PermissionBroker` cannot be bypassed.
- `Redactor` runs before journal, replay, logs, traces, export, and UI-visible raw payloads.
- Secret values must not enter prompt, event, log, trace, snapshot, screenshot, frontend state, or Storybook.

## Execution Protocol

Each task must follow this order:

1. Read this plan and the relevant AGENTS/spec files.
2. Check worktree state with `git status --short`.
3. Write or update the smallest failing test first.
4. Run the test and confirm it fails for the expected reason.
5. Implement the smallest code change.
6. Run the local test.
7. Add Storybook or Playwright coverage when the task changes complex UI or workflow.
8. Run the task gate.
9. Review diff.
10. Use `/code-review-expert` after code changes.
11. Use `/security-review` before commit if the task touches auth, user input, API, IPC, secrets, permissions, filesystem, network, tools, model providers, MCP, Journal, Replay, Audit, or redaction.
12. Commit only the task-related files when explicitly performing commit work.

Subagent rules:

- One subagent handles one task.
- The main Codex session owns review and final integration.
- Subagents must not edit files outside the task file list unless they report why.
- Subagents must not broaden scope to unrelated refactors.

## Quality Gates

Docs-only changes:

```bash
pnpm check:docs
```

Frontend changes:

```bash
pnpm -C apps/desktop test
pnpm -C apps/desktop typecheck
pnpm -C apps/desktop lint
pnpm check:desktop
```

Complex UI changes:

```bash
pnpm -C apps/desktop build-storybook
pnpm -C apps/desktop test:e2e
```

Backend changes:

```bash
pnpm check:rust
```

Cross frontend/backend changes:

```bash
pnpm check
```

Visual verification:

```text
Open /
Capture 1586x992 screenshot
Compare against docs/ui/image.png
Verify shell hierarchy, spacing, visible states, and composer prominence
```

## Full Product Capability Map

Conversation workspace:

- conversation list
- conversation page
- natural composer
- attachments and context references
- inline plan
- progress block
- diff preview
- artifact preview
- review request
- continue action
- retry action
- cancelled/failed recovery

Project context:

- current project
- workspace path
- relevant files
- active artifact
- decisions needed
- next actions
- stale/missing file states
- context empty state

Execution transparency:

- run creation
- run cancellation
- RunEvent stream
- Activity rail
- detailed Activity view
- ToolCallCard
- PermissionDialog
- CommandPreview
- DiffViewer
- redacted Raw JSON
- Replay read mode
- support bundle export

Workspace operations:

- workspace selector
- local workspace identity
- search
- command palette
- keyboard shortcuts
- light/dark/system theme
- local UI preferences

Provider/model:

- provider settings form
- model selection
- provider health check
- secret reference storage
- no raw secret readback

MCP:

- MCP server list
- server add/edit/delete
- server status
- exposed tools
- tool origin display
- permission scope

Memory:

- memory browser
- inspect/edit/delete
- export
- visibility labels
- recall trace summary

Artifacts:

- artifact history
- artifact preview
- open artifact
- copy/export
- diff-to-artifact relation

Quality and evaluation:

- Eval lab
- usage analytics
- test result preview
- support bundle export

## Workstream Overview

Use vertical slices. A vertical slice must leave the app in a usable, testable state.

1. Product shell and prototype restoration.
2. Mock conversation runtime.
3. Component state matrix and Storybook.
4. View models, schemas, and event adapter.
5. Tauri IPC boundary.
6. Rust runtime integration.
7. Permission, activity, and redacted detail views.
8. Workspace operations.
9. Provider/model settings.
10. MCP manager.
11. Memory browser.
12. Artifacts and history.
13. Replay, audit, and support bundle.
14. Eval lab and usage analytics.
15. Performance, accessibility, and release hardening.

---

## Slice 1: Product Shell And Prototype Restoration

### Task 1.1: Replace Admin Shell Navigation

**Goal:** Make the shell match the prototype IA and frontend product spec.

**Files:**

- Modify: `apps/desktop/src/app/shell/AppShell.tsx`
- Modify: `apps/desktop/src/app/shell/AppShell.test.tsx`

**Steps:**

1. Update the test to require these regions:
   - `navigation` named `Workspace`
   - `main`
   - `complementary` named `Context`
   - `region` named `Activity`
2. Update the test to require sidebar labels:
   - `Recent conversations`
   - `Home`
   - `Conversations`
   - `Projects`
   - `Artifacts`
   - `Agents`
   - `Tools`
   - `Settings`
3. Update the test to reject primary `Runs`, `MCP`, `Memory`, `Evals`, and `Models` navigation labels.
4. Run:

```bash
pnpm -C apps/desktop test AppShell
```

Expected: fail before implementation.

5. Implement the shell layout with static prototype data.
6. Use existing `Button`, `Tooltip`, and `ScrollArea` only where useful.
7. Run:

```bash
pnpm -C apps/desktop test AppShell
```

Expected: pass.

**Acceptance:**

- Left sidebar matches prototype structure.
- Context panel exists even before real data.
- Activity rail exists and stays visually secondary.
- No feature imports from `app/shell`.

### Task 1.2: Add Conversation Workspace Route

**Goal:** Make `/` render the conversation workspace instead of the system status page.

**Files:**

- Modify: `apps/desktop/src/routes/index.tsx`
- Create: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
- Create: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`

**Steps:**

1. Write a component test that renders `ConversationWorkspace`.
2. Assert the page title `Build the desktop foundation` exists.
3. Assert composer placeholder `Ask Jyowo anything about this project...` exists.
4. Assert `Plan`, `Desktop foundation created`, and `Run app` exist.
5. Run:

```bash
pnpm -C apps/desktop test ConversationWorkspace
```

Expected: fail before implementation.

6. Implement `ConversationWorkspace` with static prototype view data.
7. Change `/` route to render `ConversationWorkspace`.
8. Keep `SystemStatusPage` available for later system diagnostics.
9. Run:

```bash
pnpm -C apps/desktop test ConversationWorkspace
pnpm -C apps/desktop typecheck
```

Expected: pass.

**Acceptance:**

- `/` is conversation-first.
- System health no longer dominates first screen.
- Route file contains no business logic.

### Task 1.3: Extract Prototype View Data

**Goal:** Keep prototype data out of large JSX components.

**Files:**

- Create: `apps/desktop/src/features/conversation/prototype-data.ts`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
- Test: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`

**Steps:**

1. Create typed view data for:
   - workspace identity
   - recent conversations
   - messages
   - plan items
   - diff preview
   - artifact summary
   - context files
   - decisions
   - next actions
   - activity items
2. Avoid backend contract names unless the data is already backend-derived.
3. Update component to render from this data.
4. Run:

```bash
pnpm -C apps/desktop test ConversationWorkspace
pnpm -C apps/desktop typecheck
```

**Acceptance:**

- JSX stays readable.
- Prototype copy is centralized.
- Data can be replaced by mock runtime later.

---

## Slice 2: Conversation Components

### Task 2.1: Create Conversation Canvas Components

**Goal:** Split the center workspace into product components.

**Files:**

- Create: `apps/desktop/src/features/conversation/ConversationCanvas.tsx`
- Create: `apps/desktop/src/features/conversation/ConversationMessage.tsx`
- Create: `apps/desktop/src/features/conversation/PlanBlock.tsx`
- Create: `apps/desktop/src/features/conversation/DiffPreview.tsx`
- Create: `apps/desktop/src/features/conversation/ArtifactSummary.tsx`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
- Test: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`

**Steps:**

1. Write tests for user message, assistant message, plan items, diff filename, and artifact action.
2. Extract one component at a time.
3. Keep props explicit.
4. Use domain model props, not raw IPC payloads.
5. Run:

```bash
pnpm -C apps/desktop test ConversationWorkspace
pnpm -C apps/desktop typecheck
```

**Acceptance:**

- Main component reads as composition.
- Plan block supports completed and in-progress items.
- Diff preview supports added-line count.
- Artifact summary supports open/run action labels.

### Task 2.2: Create Composer

**Goal:** Add a reusable composer with ready, pending, disabled, and error states.

**Files:**

- Create: `apps/desktop/src/features/conversation/Composer.tsx`
- Create: `apps/desktop/src/features/conversation/Composer.test.tsx`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`

**Steps:**

1. Test typing into composer.
2. Test submit callback receives text.
3. Test empty submit is blocked.
4. Test pending state disables send.
5. Test error state can show retry.
6. Implement with `textarea`, icon buttons, and accessible labels.
7. Run:

```bash
pnpm -C apps/desktop test Composer
```

**Acceptance:**

- Composer remains the dominant action surface.
- No secret values are persisted.
- Component emits intent, not IPC command names.

### Task 2.3: Add Conversation State Matrix Story

**Goal:** Make complex conversation UI visible in Storybook.

**Files:**

- Create: `apps/desktop/src/features/conversation/ConversationWorkspace.stories.tsx`

**Steps:**

1. Add stories:
   - ready
   - loading
   - empty
   - error
   - streaming
   - permission pending
   - completed
2. Reuse prototype data.
3. Run:

```bash
pnpm -C apps/desktop build-storybook
```

**Acceptance:**

- Storybook covers the component states required by frontend quality docs.
- Stories do not contain secrets or private absolute paths.

---

## Slice 3: Workspace Sidebar

### Task 3.1: Extract Sidebar Navigation

**Goal:** Move sidebar product UI into `features/workspace`.

**Files:**

- Create: `apps/desktop/src/features/workspace/SidebarNav.tsx`
- Create: `apps/desktop/src/features/workspace/ConversationList.tsx`
- Create: `apps/desktop/src/features/workspace/ProjectSwitcher.tsx`
- Create: `apps/desktop/src/features/workspace/SidebarNav.test.tsx`
- Modify: `apps/desktop/src/app/shell/AppShell.tsx`

**Steps:**

1. Test active conversation row.
2. Test global navigation labels.
3. Test search input accessible name.
4. Test local workspace identity.
5. Extract sidebar components.
6. Run:

```bash
pnpm -C apps/desktop test SidebarNav
pnpm -C apps/desktop test AppShell
```

**Acceptance:**

- `AppShell` composes sidebar but does not own sidebar data logic.
- Recent conversations appear above global navigation.
- Tooling and settings stay below product work surfaces.

### Task 3.2: Add Sidebar UI State

**Goal:** Support local sidebar collapse and active row state without storing backend data in Zustand.

**Files:**

- Modify: `apps/desktop/src/shared/state/ui-store.ts`
- Modify: `apps/desktop/src/shared/state/ui-store.test.ts`
- Modify: `apps/desktop/src/features/workspace/SidebarNav.tsx`

**Steps:**

1. Add tests for `sidebarCollapsed`.
2. Ensure store contains no conversations, runs, tools, servers, or secrets.
3. Add collapse button to sidebar.
4. Run:

```bash
pnpm -C apps/desktop test ui-store
pnpm -C apps/desktop test SidebarNav
```

**Acceptance:**

- Zustand remains UI-only.
- Collapsed state does not hide the primary composer.

---

## Slice 4: Context Panel

### Task 4.1: Create Context Panel

**Goal:** Implement the right-side Context panel as a secondary support surface.

**Files:**

- Create: `apps/desktop/src/features/context/ContextPanel.tsx`
- Create: `apps/desktop/src/features/context/ContextSection.tsx`
- Create: `apps/desktop/src/features/context/FileReferenceList.tsx`
- Create: `apps/desktop/src/features/context/NextActionList.tsx`
- Create: `apps/desktop/src/features/context/ContextPanel.test.tsx`
- Modify: `apps/desktop/src/app/shell/AppShell.tsx`

**Steps:**

1. Test project, path, files, artifact, decisions, and next actions render.
2. Test empty context state.
3. Test long file labels do not lose accessible names.
4. Implement row-based panel with subtle dividers.
5. Run:

```bash
pnpm -C apps/desktop test ContextPanel
pnpm -C apps/desktop typecheck
```

**Acceptance:**

- Context panel is not a dashboard.
- It does not compete with the conversation canvas.
- Actions are direct and local to context rows.

### Task 4.2: Add Context Stories

**Goal:** Capture context states for review.

**Files:**

- Create: `apps/desktop/src/features/context/ContextPanel.stories.tsx`

**Steps:**

1. Add stories:
   - ready
   - empty
   - missing file
   - stale context
   - long file list
2. Run:

```bash
pnpm -C apps/desktop build-storybook
```

**Acceptance:**

- Long lists stay usable.
- Missing/stale states are visible without turning into errors.

---

## Slice 5: Activity Rail And Execution Details

### Task 5.1: Create Activity Rail

**Goal:** Implement compact execution visibility at the bottom of the shell.

**Files:**

- Create: `apps/desktop/src/features/activity/ActivityRail.tsx`
- Create: `apps/desktop/src/features/activity/ActivityItem.tsx`
- Create: `apps/desktop/src/features/activity/ActivityRail.test.tsx`
- Modify: `apps/desktop/src/app/shell/AppShell.tsx`

**Steps:**

1. Test recent tool rows.
2. Test running/success/failed labels.
3. Test `View all activity` action.
4. Implement compact rail.
5. Run:

```bash
pnpm -C apps/desktop test ActivityRail
pnpm -C apps/desktop test AppShell
```

**Acceptance:**

- Activity is tertiary.
- It shows status without replacing the conversation.
- It does not expose Raw JSON by default.

### Task 5.2: Create Execution Detail Components

**Goal:** Add drill-down components for tool calls, commands, permission requests, and redacted payloads.

**Files:**

- Create: `apps/desktop/src/features/activity/RunEventDetails.tsx`
- Create: `apps/desktop/src/features/activity/ToolCallCard.tsx`
- Create: `apps/desktop/src/features/activity/CommandPreview.tsx`
- Create: `apps/desktop/src/features/activity/PermissionDialog.tsx`
- Create: `apps/desktop/src/features/activity/RawJsonView.tsx`
- Create: `apps/desktop/src/features/activity/RunEventDetails.test.tsx`

**Steps:**

1. Test tool name, status, duration, arguments summary, output summary, permission state, and error details.
2. Test command executable, args, cwd, environment redaction, risk level, and approval state.
3. Test permission labels for low, medium, high, and critical risk.
4. Test withheld payloads do not render.
5. Implement components using shared primitives.
6. Run:

```bash
pnpm -C apps/desktop test RunEventDetails
```

**Acceptance:**

- Raw JSON is redacted and drill-down only.
- Destructive approval is never the default focused action.
- Critical destructive operations require explicit confirmation when wired to backend.

---

## Slice 6: Mock Conversation Runtime

### Task 6.1: Add Mock Runtime Types And Adapter

**Goal:** Let the prototype behave like a real conversation without using production-only mocks.

**Files:**

- Create: `apps/desktop/src/features/conversation/conversation-models.ts`
- Create: `apps/desktop/src/features/conversation/mock-conversation-runtime.ts`
- Create: `apps/desktop/src/features/conversation/mock-conversation-runtime.test.ts`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`

**Steps:**

1. Define view models for conversation, message, plan item, diff preview, artifact, decision, next action, and activity item.
2. Add mock runtime actions:
   - submit message
   - produce plan
   - mark activity running
   - mark plan item complete
   - produce artifact summary
   - request review
3. Test ordering and idempotent updates.
4. Ensure mock runtime is imported only by development/test/story paths or explicit feature-local demo path.
5. Run:

```bash
pnpm -C apps/desktop test mock-conversation-runtime
pnpm -C apps/desktop typecheck
```

**Acceptance:**

- Mock runtime cannot become a production security decision engine.
- UI can demonstrate Ask, Plan, Work, Review, Continue.

### Task 6.2: Connect Composer To Mock Runtime

**Goal:** Make the first screen interactive.

**Files:**

- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
- Modify: `apps/desktop/src/features/conversation/Composer.test.tsx`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`

**Steps:**

1. Test submitting text appends a user message.
2. Test assistant plan appears.
3. Test activity item changes to running.
4. Test completed state shows review/continue actions.
5. Implement local reducer or feature-local runtime adapter.
6. Run:

```bash
pnpm -C apps/desktop test ConversationWorkspace
```

**Acceptance:**

- User can interact with the composer.
- The product path is visible: Ask, Plan, Work, Review, Continue.

---

## Slice 7: Schemas, View Models, And Event Adapter

### Task 7.1: Define Frontend Rendering Schemas

**Goal:** Create stable UI schemas for events crossing the frontend boundary.

**Files:**

- Modify: `apps/desktop/src/shared/events/run-event-schema.ts`
- Modify: `apps/desktop/src/shared/events/run-event-schema.test.ts`
- Create: `apps/desktop/src/features/activity/run-event-view-model.ts`
- Create: `apps/desktop/src/features/activity/run-event-view-model.test.ts`

**Steps:**

1. Add or refine schema coverage for required UI event types:
   - run started
   - assistant delta
   - assistant completed
   - tool requested
   - tool approved
   - tool denied
   - tool completed
   - tool failed
   - permission requested
   - permission resolved
   - engine failed
2. Test valid and invalid payloads.
3. Test withheld visibility.
4. Create adapter from parsed event to UI view model.
5. Use exhaustive discriminated union handling.
6. Run:

```bash
pnpm -C apps/desktop test run-event-schema
pnpm -C apps/desktop test run-event-view-model
```

**Acceptance:**

- UI does not render unparsed event payloads.
- Raw JSON uses redacted payloads only.
- Adapter preserves event order fields.

### Task 7.2: Align Rust Contract Mapping

**Goal:** Make frontend event names map cleanly to canonical Rust contracts.

**Files:**

- Modify: `crates/jyowo-harness-contracts/src/events/mod.rs`
- Modify: `crates/jyowo-harness-contracts/tests/m1_contracts.rs`
- Modify: `apps/desktop/src/shared/events/run-event-schema.ts`
- Modify: `apps/desktop/src/shared/events/run-event-schema.test.ts`

**Steps:**

1. Inspect existing Rust event variants before adding anything.
2. Prefer adapter mapping over new Rust contract variants if existing events carry the data.
3. Add contract types only when the runtime needs a stable public payload not already represented.
4. Add serde/snapshot tests for changed contract shape.
5. Run:

```bash
cargo test -p jyowo-harness-contracts
pnpm -C apps/desktop test run-event-schema
```

**Acceptance:**

- Rust remains contract source of truth.
- No frontend-only stable event contract.
- Contract changes have tests.

---

## Slice 8: Tauri IPC Boundary

### Task 8.1: Add Conversation Commands To Frontend Client

**Goal:** Add typed frontend IPC wrapper functions before adding Rust implementations.

**Files:**

- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Modify: `apps/desktop/src/shared/tauri/mock-client.ts`

**Commands to model:**

- `list_conversations`
- `get_conversation`
- `start_run`
- `cancel_run`
- `resolve_permission`
- `list_activity`
- `get_context_snapshot`

**Steps:**

1. Add Zod schemas for request and response payloads.
2. Add `CommandClient` methods.
3. Add parse failure tests.
4. Add mock client behavior.
5. Do not call `invoke` outside `shared/tauri`.
6. Run:

```bash
pnpm -C apps/desktop test commands
pnpm -C apps/desktop typecheck
```

**Acceptance:**

- Invalid payloads fail at the frontend boundary.
- Mock client remains explicit.
- No production build path selects the mock client.

### Task 8.2: Add Thin Rust Tauri Commands

**Goal:** Expose the IPC surface through thin Rust handlers.

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/tests/commands.rs`
- Modify: `docs/frontend/frontend-engineering.md`
- Modify: `docs/backend/backend-engineering.md`

**Steps:**

1. Define payload structs with explicit `serde` shape.
2. Register commands in `generate_handler!`.
3. For initial implementation, return SDK-backed or explicitly typed fixture metadata.
4. Add Rust command tests.
5. Update frontend/backend docs command lists.
6. Run:

```bash
cargo test -p jyowo-desktop-shell
pnpm check:docs
pnpm check:rust
```

**Acceptance:**

- Command handlers stay thin.
- No generic execute command exists.
- Docs match implemented and registered commands.
- Security review required before merge.

---

## Slice 9: Rust Runtime Integration

### Task 9.1: Add SDK Conversation Facade

**Goal:** Give the desktop shell a safe SDK-facing conversation API.

**Files:**

- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Modify: `crates/jyowo-harness-sdk/src/session.rs`
- Modify: `crates/jyowo-harness-sdk/tests/runtime_assembly.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`

**Steps:**

1. Inspect existing `Harness`, `Session`, and `TurnInput` APIs.
2. Add facade methods only if existing methods do not already satisfy desktop needs.
3. Support:
   - create/open session
   - submit turn input
   - stream or page events
   - cancel run
4. Add tests with mock/testing adapters.
5. Run:

```bash
cargo test -p jyowo-harness-sdk
pnpm check:rust
```

**Acceptance:**

- Desktop shell does not reach around SDK into lower layers.
- Runtime behavior remains behind existing harness boundaries.

### Task 9.2: Connect UI To Real Commands With TanStack Query

**Goal:** Replace mock runtime where real commands are available.

**Files:**

- Create: `apps/desktop/src/features/conversation/use-conversation.ts`
- Create: `apps/desktop/src/features/activity/use-activity.ts`
- Create: `apps/desktop/src/features/context/use-context-snapshot.ts`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
- Test: related feature tests

**Steps:**

1. Add query hooks beside owning features.
2. Use stable query keys.
3. Keep local UI state out of query cache.
4. Add loading, empty, error, ready states.
5. Add tests with mock `CommandClient`.
6. Run:

```bash
pnpm -C apps/desktop test ConversationWorkspace
pnpm -C apps/desktop test ActivityRail
pnpm -C apps/desktop test ContextPanel
pnpm check:desktop
```

**Acceptance:**

- Components request intent through hooks/client.
- Backend state is not duplicated in Zustand.
- Mock runtime remains available for Storybook and tests only.

---

## Slice 10: Permission And Policy UI

### Task 10.1: Wire Permission Requests

**Goal:** Show and resolve permission requests without making frontend policy decisions.

**Files:**

- Modify: `apps/desktop/src/features/activity/PermissionDialog.tsx`
- Modify: `apps/desktop/src/features/activity/RunEventDetails.tsx`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `crates/jyowo-harness-permission/tests/*`

**Steps:**

1. Map backend permission events to UI view models.
2. Show operation, target, risk, reason, command/diff, workspace boundary, exposure, and decision scope.
3. Send approve/deny intent to Rust.
4. Rust resolves through `PermissionBroker`.
5. Add tests for approved, denied, high-risk, critical, and tampered/unknown request.
6. Run:

```bash
pnpm check:desktop
pnpm check:rust
```

**Acceptance:**

- Permission UI explains the decision.
- Rust remains authority.
- Missing or invalid permission fails closed.
- Security review required.

---

## Slice 11: Workspace Operations

### Task 11.1: Workspace Selector And Local Preferences

**Goal:** Support project-native local workspace selection and persisted UI preferences.

**Files:**

- Create: `apps/desktop/src/features/workspace/WorkspaceSelector.tsx`
- Modify: `apps/desktop/src/shared/local-store/ui-preferences-store.ts`
- Modify: `apps/desktop/src/shared/local-store/ui-preferences-store.test.ts`
- Modify: `apps/desktop/src/features/workspace/SidebarNav.tsx`

**Steps:**

1. Store only non-sensitive UI preferences.
2. Test theme, sidebar, panel sizing, and last selected workspace reference.
3. Do not store secrets or backend data.
4. Run:

```bash
pnpm -C apps/desktop test ui-preferences-store
pnpm check:desktop
```

**Acceptance:**

- Local-first workspace state is visible.
- Store contains no credentials.

### Task 11.2: Search And Command Palette

**Goal:** Add fast navigation and actions without creating an admin dashboard.

**Files:**

- Create: `apps/desktop/src/features/workspace/WorkspaceSearch.tsx`
- Create: `apps/desktop/src/features/workspace/CommandPalette.tsx`
- Tests: matching `*.test.tsx`

**Steps:**

1. Use `cmdk` through shared command primitives.
2. Add commands for new conversation, open artifact, search files, view activity, settings.
3. Keep command labels product-facing.
4. Add keyboard accessibility tests.
5. Run:

```bash
pnpm check:desktop
```

**Acceptance:**

- Keyboard users can open and use palette.
- Implementation words such as reducer, router, cache, plugin, or store do not appear in primary copy.

---

## Slice 12: Provider And Model Settings

### Task 12.1: Provider Settings Form

**Goal:** Configure model providers without exposing raw secrets to UI state.

**Files:**

- Create: `apps/desktop/src/features/settings/ProviderSettingsForm.tsx`
- Create: `apps/desktop/src/features/settings/ProviderSettingsForm.test.tsx`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: backend crate owning provider/keyring behavior as needed

**Steps:**

1. Use React Hook Form and Zod.
2. Store secret references only after save.
3. Validate provider by backend health check.
4. Never show raw key after save.
5. Add tests for invalid input, pending submit, backend error, saved, and secret masking.
6. Run:

```bash
pnpm check:desktop
pnpm check:rust
```

**Acceptance:**

- Raw provider credentials are not serialized to UI state, events, logs, trace, screenshots, Storybook, or tests.
- Security review required.

---

## Slice 13: MCP Manager

### Task 13.1: MCP Server List And Config

**Goal:** Manage MCP servers as a support surface, not the main product.

**Files:**

- Create: `apps/desktop/src/features/settings/MCPServerCard.tsx`
- Create: `apps/desktop/src/features/settings/MCPManager.tsx`
- Tests: matching `*.test.tsx`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/mock-client.ts`
- Modify: `crates/jyowo-harness-mcp/*` only when required by missing runtime API
- Modify: `crates/jyowo-harness-sdk/*` only when required by missing desktop facade API
- Modify: `apps/desktop/src-tauri/src/commands.rs`

**Steps:**

1. Read the existing MCP registry, SDK facade, Tauri command, and settings form patterns.
2. Write failing frontend tests for:
   - empty MCP server list
   - ready server card with status, origin, exposed tool count, and scope
   - invalid config blocked by Zod before IPC
   - connection failure rendered without leaking raw backend details
   - delete flow removes the server after confirmation
3. Add typed IPC wrappers in `CommandClient`:
   - `listMcpServers()`
   - `saveMcpServer(input)`
   - `deleteMcpServer(id)`
4. Add `MCPServerCard`:
   - render server name, status, origin, scope, transport, exposed tool count, and last error summary
   - expose delete action
   - defer edit until there is a non-secret config read model for transport command and arguments
   - avoid raw config, env, token, header, or secret display
5. Add `MCPManager`:
   - own loading, empty, error, ready, saving, and deleting states
   - use TanStack Query for IPC-derived server state
   - validate form payload with Zod
   - send only structured config and secret references to IPC
   - keep MCP inside Settings/support surfaces, not primary navigation
6. Write failing Rust command tests for:
   - invalid server config is rejected fail-closed
   - secret-bearing stdio args and raw credential-shaped args are rejected
   - unknown IPC transport fields are rejected fail-closed
   - workspace config cannot declare in-process transport
   - saved stdio servers register into runtime MCP registry and inject tools
   - deleting a runtime server removes registry state and injected tools
   - deleting a missing server is idempotent
   - listed tools include origin and scope metadata needed by permission display
7. Implement thin Tauri commands:
   - `list_mcp_servers`
   - `save_mcp_server`
   - `delete_mcp_server`
8. If the SDK has no suitable facade, add the smallest SDK-level MCP manager API. Do not move policy into Tauri.
9. If the MCP crate lacks registry metadata needed by UI, add the smallest read model there. Preserve existing permission checks.
10. Run focused frontend tests.
11. Run focused Rust tests.
12. Run:

```bash
pnpm check:desktop
pnpm check:rust
```

**Acceptance:**

- MCP tools carry origin and scope through permission checks.
- MCP is not a primary sidebar product object.
- Frontend never stores or renders raw secrets, env values, bearer tokens, or request headers.
- Tauri commands stay thin and call SDK/runtime APIs.
- Invalid config and missing capabilities fail closed.
- Security review required.

---

## Slice 14: Memory Browser

### Task 14.1: Memory Inspect, Edit, Delete, Export

**Goal:** Make memory inspectable, editable, deletable, and exportable.

**Files:**

- Create: `apps/desktop/src/features/memory/MemoryBrowser.tsx`
- Create: `apps/desktop/src/features/memory/MemoryItemCard.tsx`
- Tests: matching `*.test.tsx`
- Modify: `crates/jyowo-harness-memory/*` only when required by missing runtime API
- Modify: `apps/desktop/src-tauri/src/commands.rs`

**Steps:**

1. List memory items with visibility labels.
2. Support inspect/edit/delete/export intents.
3. Route all operations through backend tenant and visibility checks.
4. Add tests for empty, selected, long list, delete confirmation, and export.
5. Run:

```bash
pnpm check:desktop
pnpm check:rust
```

**Acceptance:**

- Memory writes do not bypass tenant or visibility checks.
- Memory deletion/export produces audit events.
- Security review required.

---

## Slice 15: Artifacts

### Task 15.1: Artifact History And Preview

**Goal:** Make produced artifacts navigable and reviewable.

**Files:**

- Create: `apps/desktop/src/features/artifacts/ArtifactHistory.tsx`
- Create: `apps/desktop/src/features/artifacts/ArtifactPreview.tsx`
- Tests: matching `*.test.tsx`
- Modify: conversation artifact summary integration

**Steps:**

1. Show artifact title, kind, status, source run, and actions.
2. Link artifacts back to the conversation block that produced them.
3. Support preview loading/error/ready.
4. Add tests for missing artifact and large preview fallback.
5. Run:

```bash
pnpm check:desktop
```

**Acceptance:**

- Artifacts remain tied to conversation work.
- Artifact view is not a separate document app.

---

## Slice 16: Replay, Audit, And Support Bundle

### Task 16.1: Replay Read Mode

**Goal:** Let users inspect prior runs without re-executing tools.

**Files:**

- Create: `apps/desktop/src/features/activity/ReplayTimeline.tsx`
- Tests: matching `*.test.tsx`
- Modify: `crates/jyowo-harness-journal/*`
- Modify: `crates/jyowo-harness-observability/*`
- Modify: `apps/desktop/src-tauri/src/commands.rs`

**Steps:**

1. Read replay cursors through backend.
2. Preserve original event order.
3. Mark replayed runs as replayed.
4. Do not execute tools in read mode.
5. Add tests for redacted and withheld payloads.
6. Run:

```bash
pnpm check:desktop
pnpm check:rust
```

**Acceptance:**

- Replay does not reveal withheld data.
- Replay does not execute tools unless an explicit execution replay mode is later designed.
- Security review required.

### Task 16.2: Audit And Support Bundle

**Goal:** Provide redacted support data for debugging and recovery.

**Files:**

- Create: `apps/desktop/src/features/activity/SupportBundleExport.tsx`
- Tests: matching `*.test.tsx`
- Modify: backend journal/observability crates as needed

**Steps:**

1. Export JSONL event stream, Markdown report, and redacted support bundle.
2. Ensure Redactor runs before export.
3. Add tests that secrets and withheld payloads do not appear.
4. Run:

```bash
pnpm check:desktop
pnpm check:rust
```

**Acceptance:**

- Support bundle is useful and redacted.
- Audit is available as detail, not the main product language.
- Security review required.

---

## Slice 17: Eval Lab And Usage Analytics

### Task 17.1: Eval Lab

**Goal:** Add quality evaluation as a support workflow.

**Files:**

- Create: `apps/desktop/src/features/evals/EvalLab.tsx`
- Tests: matching `*.test.tsx`
- Backend files only as required by existing eval contracts

**Steps:**

1. Add eval case list, run action, result preview, and failure state.
2. Keep eval lab behind navigation or command palette.
3. Do not make evals the default screen.
4. Run:

```bash
pnpm check:desktop
pnpm check:rust
```

**Acceptance:**

- Evaluation supports product confidence without replacing conversation workspace.

### Task 17.2: Usage Analytics

**Goal:** Show local usage and cost summaries without leaking secrets.

**Files:**

- Create: `apps/desktop/src/features/activity/UsageSummary.tsx`
- Tests: matching `*.test.tsx`
- Modify: observability/sdk command surface as needed

**Steps:**

1. Show token usage, tool calls, and local cost estimate when available.
2. Keep telemetry failures fail-open only for non-security telemetry.
3. Add tests for unavailable analytics and redacted provider data.
4. Run:

```bash
pnpm check:desktop
pnpm check:rust
```

**Acceptance:**

- Analytics failures do not grant access or reveal secrets.
- Usage stays secondary.

---

## Slice 18: Performance, Accessibility, And Release Hardening

### Task 18.1: Virtualize Large Timelines And Outputs

**Goal:** Keep large conversations, event streams, and diffs responsive.

**Files:**

- Modify: activity timeline components
- Modify: diff/raw JSON components
- Use: `apps/desktop/src/shared/text-layout/*`

**Steps:**

1. Add TanStack Virtual for long timelines.
2. Use `shared/text-layout` for measurement where needed.
3. Add fallback path tests.
4. Add large output truncation/lazy load.
5. Run:

```bash
pnpm check:desktop
```

**Acceptance:**

- Large output does not stringify huge JSON during render.
- Streaming does not global-rerender every token.

### Task 18.2: Accessibility Pass

**Goal:** Verify keyboard, focus, labels, contrast, and text overflow.

**Files:**

- Modify affected frontend components
- Modify Playwright tests

**Steps:**

1. Test command palette keyboard behavior.
2. Test dialogs trap and restore focus.
3. Test icon-only buttons have labels/tooltips.
4. Test color is not the only state signal.
5. Run:

```bash
pnpm -C apps/desktop test:e2e
pnpm -C apps/desktop build-storybook
pnpm check:desktop
```

**Acceptance:**

- Primary workflows are keyboard usable.
- Fixed controls do not overflow at desktop and narrow widths.

### Task 18.3: Full Gate

**Goal:** Prove the integrated product is ready for review.

**Files:**

- No planned file changes unless failures are found.

**Steps:**

1. Run:

```bash
pnpm check
```

2. Run:

```bash
pnpm check:desktop:full
```

3. Capture a 1586x992 screenshot of `/`.
4. Compare against `docs/ui/image.png`.
5. Review Storybook state matrix.
6. Review security-sensitive diff.

**Acceptance:**

- Full root gate passes.
- Desktop full gate passes or documented platform blocker exists.
- Visual hierarchy matches prototype and frontend product spec.

## Testing Matrix

Component tests:

- `ConversationWorkspace`
- `ConversationCanvas`
- `ConversationMessage`
- `Composer`
- `PlanBlock`
- `DiffPreview`
- `ArtifactSummary`
- `SidebarNav`
- `ContextPanel`
- `ActivityRail`
- `ToolCallCard`
- `PermissionDialog`
- `CommandPreview`
- `RawJsonView`
- settings forms
- MCP cards
- Memory cards

Schema tests:

- IPC command payloads
- RunEvent parsing
- event-to-view-model adapter
- provider settings
- MCP config
- local preferences

Rust tests:

- command payload identity
- SDK runtime assembly
- contract serde shape
- permission fail-closed defaults
- redactor before durable writes
- replay withheld/redacted output
- support bundle redaction
- migration/restart-stable behavior when persistence changes

Storybook:

- loading
- empty
- ready
- streaming
- completed
- error
- permission pending
- high risk
- redacted
- large output

Playwright:

- open `/`
- verify shell regions
- submit message
- see plan
- see activity update
- see review/continue action
- open context action
- open activity details
- verify composer stays primary

## Risk Register

Risk: UI becomes a static screenshot.

- Mitigation: every shell and conversation slice includes interaction and tests.

Risk: Runs/tools become the product language.

- Mitigation: sidebar and route tests reject primary admin/runtime navigation.

Risk: frontend makes security decisions.

- Mitigation: permission UI sends intent only; Rust `PermissionBroker` decides.

Risk: mock runtime leaks into production behavior.

- Mitigation: mock runtime is feature-local and test/story scoped until replaced.

Risk: contracts split between Rust and frontend.

- Mitigation: Rust `harness-contracts` remains canonical; frontend schemas validate render payloads.

Risk: secrets enter UI or snapshots.

- Mitigation: secret references only; security tests and security review required.

Risk: large event streams degrade performance.

- Mitigation: virtualized timelines, batched streaming, lazy large output.

Risk: plan becomes stale.

- Mitigation: update this file only when scope changes materially; task status belongs in commits/issues, not this plan.

## Definition Of Done

The plan is complete when:

- `/` opens to the conversation workspace.
- UI matches `docs/ui/image.png` at desktop size.
- Conversation path works: Ask, Understand, Plan, Work, Review, Continue.
- Context panel shows project context, active artifact, decisions, and next actions.
- Activity rail shows compact execution state and opens details.
- Typed IPC connects frontend to Rust commands.
- Rust runtime owns execution, permissions, tool routing, redaction, journal, replay, and secrets.
- Provider, MCP, Memory, Artifacts, Replay, Audit, support bundle, Eval lab, usage analytics, search, command palette, theme, and local preferences are implemented or intentionally removed from scope by a reviewed plan update.
- `pnpm check` passes.
- `pnpm check:desktop:full` passes or has a documented platform blocker.
- Security-sensitive changes have security review.
