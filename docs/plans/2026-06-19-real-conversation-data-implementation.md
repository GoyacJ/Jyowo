# Real Conversation Data Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace prototype conversation data with real local Jyowo runtime and journal data.

**Architecture:** `jyowo-harness-sdk` owns conversation session listing and runtime reads. Tauri commands stay thin IPC adapters. React uses existing command client APIs, TanStack Query for server state, and TanStack Router search params for selected conversation state.

**Tech Stack:** Rust 1.96, Tauri 2, `jyowo-harness-sdk`, `jyowo-harness-journal`, React 19, TanStack Router, TanStack Query, Zod, Vitest.

---

## Preconditions

- Read `docs/plans/2026-06-19-real-conversation-data-design.md`.
- Keep existing uncommitted user changes. Do not reset or overwrite them.
- Do not change IPC command names or frontend Zod payload shapes unless a test proves the current shape is insufficient.

## Task 1: Backend Empty Runtime Test

**Files:**

- Modify: `apps/desktop/src-tauri/tests/commands.rs`

**Step 1: Write failing tests**

Add tests that prove:

- `list_conversations_with_runtime_state` on an empty runtime returns one real `SessionId`.
- Calling `get_conversation_with_runtime_state` for that returned id returns an empty `messages` array.
- Calling `get_conversation_with_runtime_state` for a fresh arbitrary `SessionId` still fails.

**Step 2: Verify failure**

Run:

```bash
cargo test -p jyowo-desktop-shell list_conversations_with_runtime_state -- --nocapture
```

Expected: at least one new test fails because `list_conversations_with_runtime_state` is still synchronous placeholder logic or `get_conversation` reads a missing session.

## Task 2: SDK Conversation Session Listing

**Files:**

- Modify: `crates/jyowo-harness-sdk/src/harness.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`

**Step 1: Add SDK facade**

Add a public SDK method on `Harness`, named close to `list_conversation_sessions`.

Behavior:

- Accept tenant/session options or a desktop-suitable request type.
- Call `event_store.list_sessions`.
- Filter out ended sessions unless existing runtime behavior needs ended sessions.
- Sort by `last_event_at` descending.
- Return session summaries without exposing the journal store to Tauri.

**Step 2: Use SDK from desktop runtime**

Change desktop conversation list logic to call the SDK facade.

If no sessions exist:

- call `open_or_create_conversation_session` for `state.default_conversation_id()`;
- return that session in the list.

**Step 3: Verify backend tests**

Run:

```bash
cargo test -p jyowo-desktop-shell list_conversations_with_runtime_state -- --nocapture
```

Expected: new empty runtime tests pass.

## Task 3: Backend Summary Mapping

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`

**Step 1: Write failing tests**

Add or update tests proving a real started conversation returns:

- title from latest user message first line.
- preview from latest user or assistant message.
- `updatedAt` from the latest persisted message timestamp.
- most recently updated session first when there is more than one session.

**Step 2: Implement mapping**

In desktop command code:

- Keep `ConversationSummaryPayload` shape unchanged.
- Read conversation messages through existing runtime event paging helpers.
- Use `New conversation` for empty sessions.
- Use `Start from the composer when ready.` for empty previews.
- Use redacted display helpers already present in command code.

**Step 3: Verify backend tests**

Run:

```bash
cargo test -p jyowo-desktop-shell conversation -- --nocapture
```

Expected: conversation list and detail tests pass.

## Task 4: Context And Activity Empty Session Behavior

**Files:**

- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Test: `apps/desktop/src-tauri/tests/commands.rs`

**Step 1: Update tests**

Replace tests that expect the default conversation id to fail only when the session has never been opened.

Keep tests that prove arbitrary missing ids fail closed.

Add tests:

- after `list_conversations_with_runtime_state`, `get_context_snapshot_with_runtime_state` returns workspace context for the listed id.
- after `list_conversations_with_runtime_state`, `list_activity_with_runtime_state` returns an empty event list for the listed id.

**Step 2: Implement behavior**

Make read paths distinguish:

- existing empty session: return empty/ready payload.
- missing arbitrary session: return existing runtime error.

**Step 3: Verify backend tests**

Run:

```bash
cargo test -p jyowo-desktop-shell context_snapshot -- --nocapture
cargo test -p jyowo-desktop-shell list_activity -- --nocapture
```

Expected: empty default session is readable; arbitrary missing sessions still fail.

## Task 5: Frontend Error Normalization

**Files:**

- Create: `apps/desktop/src/shared/tauri/errors.ts`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
- Modify: `apps/desktop/src/app/shell/AppShell.tsx`
- Modify if needed: `apps/desktop/src/features/system-status/SystemStatusPage.tsx`
- Test: `apps/desktop/src/shared/tauri/commands.test.ts`
- Test: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`

**Step 1: Write failing tests**

Add tests proving object-shaped Tauri errors render their `message`, not `[object Object]`.

Use this error shape:

```ts
{ code: 'RUNTIME_OPERATION_FAILED', message: 'conversation read failed' }
```

**Step 2: Implement normalizer**

Create a shared function:

```ts
export function getCommandErrorMessage(error: unknown): string
```

Rules:

- `Error` -> `message`
- object with string `message` -> `message`
- otherwise -> `Unknown command error`

**Step 3: Replace local helpers**

Replace local `getErrorMessage` functions in conversation and shell with the shared helper.

**Step 4: Verify frontend tests**

Run:

```bash
pnpm -C apps/desktop test -- commands.test.ts ConversationWorkspace.test.tsx
```

Expected: error normalization tests pass.

## Task 6: Frontend Selected Conversation URL State

**Files:**

- Modify: `apps/desktop/src/routes/index.tsx`
- Modify: `apps/desktop/src/features/conversation/ConversationWorkspace.tsx`
- Modify: `apps/desktop/src/features/conversation/use-conversation.ts`
- Modify: `apps/desktop/src/app/shell/AppShell.tsx`
- Test: `apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx`
- Test: `apps/desktop/src/app/shell/AppShell.test.tsx`

**Step 1: Write failing tests**

Add tests proving:

- `/?conversationId=conversation-002` causes `getConversation('conversation-002')`.
- `AppShell` sends context and activity requests for the same selected id.
- when URL has no id, the first listed conversation is used.

**Step 2: Implement route search parsing**

Use TanStack Router search state for `conversationId`.

Rules:

- optional string.
- no backend state in Zustand.
- route file only composes and passes props.

**Step 3: Pass selected id**

Make `ConversationWorkspace` accept an optional `conversationId` prop and pass it to `useConversation`.

Make `AppShell` derive selected id from the same route search source.

**Step 4: Verify frontend tests**

Run:

```bash
pnpm -C apps/desktop test -- ConversationWorkspace.test.tsx AppShell.test.tsx
```

Expected: selected id is consistent across workspace, context, and activity.

## Task 7: Sidebar Real Conversation List

**Files:**

- Modify: `apps/desktop/src/features/workspace/ConversationList.tsx`
- Modify: `apps/desktop/src/features/workspace/SidebarNav.tsx`
- Delete if unused: `apps/desktop/src/features/workspace/prototype-data.ts`
- Test: `apps/desktop/src/features/workspace/SidebarNav.test.tsx`

**Step 1: Write failing tests**

Update sidebar tests to use a command client with two conversations.

Assert:

- backend titles render.
- prototype titles do not render.
- search filters backend title or preview.
- clicking a conversation navigates to `/?conversationId=<id>`.

**Step 2: Update component contract**

Change `ConversationList` props to:

```ts
type ConversationListItem = {
  id: string
  title: string
  lastMessagePreview?: string
  updatedAt: string
}
```

Props:

- `conversations`
- `activeConversationId`
- `loading`
- `errorMessage`
- `onSelectConversation`

**Step 3: Load real data in SidebarNav**

Use `CommandClient` through `useCommandClient` and TanStack Query.

Query key should share the conversation list key with `useConversation` or use an exported shared key.

Filter by title and preview.

**Step 4: Verify sidebar tests**

Run:

```bash
pnpm -C apps/desktop test -- SidebarNav.test.tsx
```

Expected: sidebar renders real command data and navigates by conversation id.

## Task 8: Final Gates And Manual Check

**Files:**

- Review all changed files.

**Step 1: Run targeted gates**

```bash
cargo test -p jyowo-desktop-shell conversation -- --nocapture
pnpm -C apps/desktop test
```

**Step 2: Run required gates**

```bash
pnpm check:desktop
pnpm check:rust
```

**Step 3: Run full gate**

```bash
pnpm check
```

**Step 4: Manual dev check**

```bash
pnpm dev
```

Verify:

- no `Conversation unavailable` on first render.
- no `[object Object]` errors.
- sidebar uses real conversation ids.
- submitting a prompt uses the selected conversation id.

## Commit Strategy

Use small commits:

1. `docs: plan real conversation data implementation`
2. `feat: list real runtime conversations`
3. `fix: normalize desktop command errors`
4. `feat: route selected conversation state`
5. `feat: render real sidebar conversations`

Do not stage unrelated existing user changes.
