# Real Conversation Data Design

## Goal

Connect the desktop conversation workflow to real local Jyowo runtime data.

The scope is local runtime and journal data only. This does not add a remote
database, cloud account, or external data source.

## Chosen Approach

Use a `jyowo-harness-sdk` facade as the conversation data authority.

Tauri commands stay as IPC boundaries. They validate input, call the SDK, and
map results into existing desktop payloads. React continues to use the existing
command client and Zod schemas.

This matches the backend layer rule:

```text
Tauri shell -> SDK facade -> runtime/journal
```

## Data Flow

Conversation list:

```text
Sidebar
  -> list_conversations
  -> SDK list conversation sessions
  -> EventStore list_sessions
  -> ConversationSummaryPayload[]
```

Conversation detail:

```text
Route search conversationId
  -> get_conversation
  -> SDK page_conversation_events
  -> ConversationPayload
```

Secondary surfaces:

```text
selected conversationId
  -> list_activity
  -> get_context_snapshot
```

## Empty Runtime Behavior

When the journal has no sessions, `list_conversations` opens or creates the
desktop default conversation session.

The first rendered conversation is therefore a real session with a real
`SessionId`. It has no messages until the user submits a prompt.

`get_conversation` for that session returns an empty conversation instead of a
runtime read error.

Unknown arbitrary session ids still fail closed.

## Conversation Summary Mapping

For each real session:

- `id`: session id.
- `title`: latest user message first line, truncated; otherwise `New conversation`.
- `lastMessagePreview`: latest user or assistant message preview; otherwise
  `Start from the composer when ready.`
- `updatedAt`: latest message timestamp, otherwise session `last_event_at`, then
  `created_at`.

The list is sorted by most recent activity first.

## Frontend State

Server data belongs to TanStack Query.

The selected conversation id belongs to TanStack Router search params:

```text
/?conversationId=<session-id>
```

Zustand remains UI-only. It may keep panel state and active run state, but it must
not store conversation list or conversation detail data.

The sidebar reads real conversation summaries through the command client. It no
longer imports prototype conversation data.

## Error Handling

Frontend error display uses one shared normalizer:

- `Error` -> `error.message`
- `{ message: string }` -> `message`
- `{ code: string, message: string }` -> `message`
- unknown -> fixed fallback message

This prevents Tauri reject payloads from rendering as `[object Object]`.

## Tests

Backend tests cover:

- empty runtime list creates or opens a default session.
- listed default session can be read as an empty conversation.
- real started sessions appear in the list.
- summaries use real title, preview, and updated time.
- arbitrary missing session ids still fail.

Frontend tests cover:

- sidebar renders command-backed conversation data.
- prototype conversation titles are not used.
- selecting a conversation writes `conversationId` to the URL.
- workspace, activity, and context use the same selected id.
- object-shaped Tauri errors render their `message`.

## Verification

Run targeted tests first:

```bash
cargo test -p jyowo-desktop-shell list_conversations
pnpm -C apps/desktop test
```

Then run required gates:

```bash
pnpm check:desktop
pnpm check:rust
pnpm check
```
