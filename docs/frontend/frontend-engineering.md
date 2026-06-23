# Jyowo Frontend Engineering

This document defines the React/Tauri frontend implementation rules.

## Stack

Runtime stack:

```text
React 19
TypeScript 6
Vite 8
Tauri 2
Tailwind CSS v4
TanStack Router
TanStack Query
TanStack Virtual
Zustand
React Hook Form
Zod
react-markdown
remark-gfm
shiki
cmdk
react-resizable-panels
@chenglou/pretext
@tauri-apps/plugin-store
lucide-react
clsx
tailwind-merge
class-variance-authority
```

Tooling:

```text
Node 24 LTS
pnpm 11.7
Vitest 4
Testing Library
Playwright 1
Storybook 10 React Vite
Biome 2
Knip 6
```

`@chenglou/pretext` is used only through `shared/text-layout`. It is for multiline text measurement and virtualized Timeline/log/Diff/Raw JSON layout estimation. Feature code must not import it directly.

## Plugin And Library Boundaries

Existing frontend plugins and libraries are part of the product foundation. Use them
through clear ownership boundaries instead of adding parallel solutions.

Vite plugins:

- `@vitejs/plugin-react` owns the React transform only. Do not add custom Babel or
  compile-time behavior for product logic.
- `@tanstack/router-plugin/vite` owns TanStack Router code generation. Do not edit
  generated route tree files manually.
- `@tailwindcss/vite` owns Tailwind CSS v4 integration. Keep theme tokens in
  `shared/styles/global.css`.

UI primitives:

- shadcn/ui style source-owned primitives live in `shared/ui`.
- Radix primitives are wrapped by `shared/ui` before feature usage.
- Required Radix-backed primitives are Slot, Tooltip, Dialog, Dropdown Menu, Tabs,
  Scroll Area, Popover, Checkbox, and Switch.
- Feature code may import `Button`, `Tooltip`, `Dialog`, `Tabs`, `Dropdown`,
  `ScrollArea`, `Popover`, `Checkbox`, `Switch`, and similar primitives from
  `shared/ui`.
- Feature code must not create one-off primitive variants when an existing
  primitive can be extended.
- `lucide-react` is the icon source. Use accessible labels or tooltips for
  icon-only controls.
- `class-variance-authority` defines primitive variants.
- `clsx` and `tailwind-merge` are used through `cn()`.

Application libraries:

- TanStack Router owns route state, route params, search params, and navigation.
- TanStack Query owns backend and IPC-derived server state.
- TanStack Virtual owns long conversation, activity, log, diff, and Raw JSON lists.
- Zustand owns local UI state only.
- React Hook Form owns form interaction state.
- Zod owns validation at IPC, event, form, and storage boundaries.
- `react-markdown` and `remark-gfm` own assistant/user Markdown rendering.
- `shiki` owns code block, command output, and diff-adjacent syntax highlighting.
- `cmdk` owns command palette behavior.
- `react-resizable-panels` owns resizable shell regions.
- `@tauri-apps/plugin-store` owns non-sensitive local UI preferences.
- `@chenglou/pretext` owns text measurement only through `shared/text-layout`.

Forbidden:

- storing backend state in Zustand because it is convenient
- using component `useEffect` chains for data fetching that belongs in TanStack
  Query
- adding another router, cache, form, validation, icon, or CSS system
- importing Radix directly from feature components unless creating a new
  `shared/ui` primitive
- hand-writing SVG icons when a lucide icon exists
- storing provider keys, tokens, or credentials in `@tauri-apps/plugin-store`
- returning provider keys through list/save settings payloads; explicit key display
  must request a short-lived reveal token and render the returned key only in the
  settings reveal UI
- using Markdown rendering to pass through raw HTML from model output
- using `shiki` in render paths without caching or lazy loading
- exposing plugin names as the product's main user language

Package manager:

- pnpm only
- root lockfile only
- no npm, yarn, or bun lockfiles

## Monorepo And Source Layout

Frontend root:

```text
apps/desktop/src
```

Required shape:

```text
app/
  providers/
  router/
  shell/
  error-boundary/
routes/
features/
  conversation/
  workspace/
  context/
  activity/
  system-status/
shared/
  events/
  styles/
  tauri/
  text-layout/
  ui/
  utils/
```

Layer rules:

- `app` owns providers, router creation, shell composition, and global error boundary.
- `routes` compose feature screens and should not contain complex business logic.
- `features` own domain UI and feature-local state.
- `shared` owns cross-feature primitives, schemas, utilities, and infrastructure adapters.

Dependency direction:

```text
app -> routes -> features -> shared
```

Forbidden imports:

```text
shared -> app
shared -> routes
shared -> features
features -> routes
features -> app
```

Feature-to-feature imports should be avoided. If a feature needs another feature's data, create a shared model or explicit integration boundary.

## Component Architecture

Component layers:

```text
shared/ui
  Button
  Badge
  Tooltip
  Input
  Textarea
  Dialog
  Dropdown
  Tabs
  ScrollArea

features/conversation
  ConversationCanvas
  ConversationMessage
  Composer
  PlanBlock
  ProgressBlock
  ArtifactPreview
  ReviewRequest
  DecisionCard

features/workspace
  SidebarNav
  ConversationList
  ProjectSwitcher

features/context
  ContextPanel
  ContextSection
  FileReferenceList
  NextActionList

features/activity
  ActivityRail
  ActivityItem
  RunEventDetails
```

Component ownership:

- `shared/ui` owns reusable interaction primitives and visual variants.
- `features/conversation` owns the natural chat surface and embedded work blocks.
- `features/workspace` owns project and conversation navigation.
- `features/context` owns secondary project context and references.
- `features/activity` owns compact execution visibility and detailed drill-down.
- `app/shell` composes regions and layout only.
- `routes` compose screens from features and providers.

Component API rules:

- Components use explicit props and callbacks.
- Components emit user intent, not IPC commands.
- Feature leaf components do not import `CommandClient`.
- Data loading hooks live beside the feature that owns the screen.
- Shared primitives must not know about Conversation, RunEvent, MCP, Memory, or
  provider concepts.
- Domain components accept domain models or view models, not raw IPC payloads.
- Large components split by product responsibility, not by arbitrary visual rows.
- `className` pass-through is allowed for primitives and layout wrappers. Domain
  components should prefer named variants and explicit props.

State placement:

- backend data belongs to TanStack Query hooks
- URL selection belongs to TanStack Router
- temporary form data belongs to React Hook Form
- panel visibility, selected local tab, and dimensions belong to Zustand
- streaming buffers belong to reducers or adapters close to the event source

Forbidden component patterns:

- `AppShell` containing product business logic
- route files containing reducers, IPC calls, or complex rendering branches
- generic `Card` wrappers around every product area
- dashboard-style gray card stacks for the main conversation product
- raw RunEvent payloads flowing directly into visual components
- component names that describe implementation shape instead of product meaning

## Naming And Imports

File naming:

```text
components: kebab-case.tsx
hooks: use-*.ts
schemas: *.schema.ts
tests: *.test.ts or *.test.tsx
stories: *.stories.tsx
generated files: *.gen.ts
```

Component names use domain nouns:

```text
ConversationCanvas
ConversationMessage
Composer
ContextPanel
ActivityRail
PlanBlock
ArtifactPreview
ReviewRequest
```

Forbidden names:

```text
DataCard
InfoPanel
CommonModal
Manager
Handler
```

Use `@` for `apps/desktop/src`.

Import order:

```text
React/runtime
third-party libraries
app/routes/features/shared absolute imports
relative imports
styles
types
```

No deep imports into another feature's private folders.

## TypeScript

Rules:

- no explicit `any`
- no `@ts-ignore`
- prefer `unknown` at boundaries
- parse external data with Zod
- use discriminated unions for events and renderer switches
- generated files are isolated from manual lint churn

Exhaustive checks:

```ts
export function assertNever(value: never): never {
  throw new Error(`Unhandled case: ${String(value)}`)
}
```

Use it for RunEvent rendering and permission risk handling.

## Tauri IPC

The React app must not call Tauri `invoke` directly from components.

All commands go through `shared/tauri` and `CommandClient`.

Rules:

- Frontend code exposes IPC through `shared/tauri`.
- Components use hooks or feature services that depend on `CommandClient`.
- Every command payload is validated with Zod at the frontend boundary.
- Invalid payloads become typed errors.
- Production uses the Tauri invoke client.
- Tests, Storybook, and Playwright web mock E2E use mock clients.
- Mock clients must not be selectable in production builds.
- Tauri capabilities remain minimal and explicit.

Current commands:

```ts
get_app_info(): {
  name: 'Jyowo'
  version: string
  shell: 'tauri2-react'
  harness: {
    sdkCrate: 'jyowo_harness_sdk'
    mode: 'in-process'
  }
}

harness_healthcheck(): {
  status: 'available'
  sdkCrate: 'jyowo_harness_sdk'
}

list_conversations(): {
  conversations: Array<{
    id: string
    lastMessagePreview?: string
    title: string
    updatedAt: string
  }>
}

get_conversation(conversationId: string): {
  conversation: {
    id: string
    messages: Array<{
      author: 'assistant' | 'user'
      body: string
      id: string
      timestamp: string
    }>
    title: string
    updatedAt: string
  }
}

delete_conversation(conversationId: string): {
  conversationId: string
  status: 'deleted'
}

start_run(request: {
  clientMessageId: string
  contextReferences?: string[]
  conversationId: string
  prompt: string
}): {
  runId: string
  status: 'started'
}

cancel_run(runId: string): {
  runId: string
  status: 'cancelled'
}

resolve_permission(request: {
  decision: 'approve' | 'deny'
  requestId: string
}): {
  decision: 'approve' | 'deny'
  requestId: string
  status: 'resolved'
}

list_activity(request: {
  conversationId: string
  runId?: string
}): {
  events: RunEvent[]
}

get_replay_timeline(request: {
  conversationId: string
  runId?: string
}): {
  events: RunEvent[]
  replayed: true
}

export_support_bundle(request: {
  conversationId: string
  runId?: string
}): {
  bundlePath: string
  eventCount: number
  exportedAt: string
  jsonlPath: string
  markdownPath: string
  redacted: true
}

list_artifacts({
  conversationId: string
}): {
  artifacts: Array<{
    actionLabel: string
    description: string
    id: string
    kind: string
    preview?: string
    status: 'failed' | 'pending' | 'ready' | 'running'
    title: string
  }>
}

list_eval_cases(): {
  cases: Array<{
    id: string
    lastRun?: {
      completedAt?: string
      failed: number
      passed: number
      status: 'failed' | 'passed' | 'running' | 'unavailable'
    }
    title: string
  }>
}

run_eval_case(caseId: string): {
  case: {
    id: string
    lastRun: {
      completedAt?: string
      failed: number
      passed: number
      status: 'failed' | 'passed' | 'running' | 'unavailable'
    }
    title: string
  }
  status: 'completed'
}

get_context_snapshot(request: {
  conversationId?: string
  runId?: string
}): {
  activeArtifact: string | null
  decisions: Array<{ detail: string; title: string }>
  files: Array<{ label: string; state?: 'missing' | 'ready' | 'stale' }>
  nextActions: string[]
  path: string
  project: string
}

validate_provider_settings(request: {
  modelId: string
  providerId: string
}): {
  modelId: string
  providerId: string
  status: 'accepted'
}

save_provider_settings(request: {
  apiKey?: string
  baseUrl?: string
  configId?: string
  displayName?: string
  modelId: string
  providerId: string
  setDefault?: boolean
}): {
  config: ProviderConfig
  status: 'saved'
}

list_provider_settings(): {
  defaultConfigId: string | null
  configs: ProviderConfig[]
}

get_provider_config_api_key(request: {
  configId: string
  revealToken: string
}): {
  apiKey: string
  configId: string
}

request_provider_config_api_key_reveal(request: {
  configId: string
}): {
  configId: string
  expiresInSeconds: number
  revealToken: string
  status: 'ready'
}

set_conversation_model_config(request: {
  conversationId: string
  modelConfigId: string
}): {
  conversationId: string
  modelConfigId: string
  status: 'saved'
}

type ProviderConfig = {
  baseUrl?: string
  displayName: string
  hasApiKey: boolean
  id: string
  isDefault: boolean
  protocol: 'chat_completions' | 'responses' | 'messages' | 'generate_content'
  modelId: string
  providerId: string
}

list_mcp_servers(): {
  servers: Array<{
    displayName: string
    exposedToolCount: number
    id: string
    lastError?: string
    origin: 'managed' | 'plugin' | 'policy' | 'user' | 'workspace'
    scope: 'agent' | 'global' | 'session'
    status: 'closed' | 'configured' | 'connecting' | 'failed' | 'ready' | 'reconnecting'
    transport: 'http' | 'inProcess' | 'sse' | 'stdio' | 'websocket'
  }>
}

save_mcp_server(request: {
  displayName: string
  id: string
  scope: 'agent' | 'global' | 'session'
  transport: {
    args: string[]
    command: string
    kind: 'stdio'
  }
}): {
  server: McpServerSummary
}

delete_mcp_server(id: string): {
  id: string
  status: 'deleted'
}

list_memory_items(): {
  items: MemoryItemSummary[]
}

get_memory_item(id: string): {
  item: MemoryItem
}

update_memory_item(request: {
  content: string
  id: string
}): {
  item: MemoryItem
}

delete_memory_item(id: string): {
  id: string
  status: 'deleted'
}

export_memory_items(): {
  exportedAt: string
  format: 'json'
  itemCount: number
  path: string
}

list_skills(): {
  skills: SkillSummary[]
}

get_skill_detail(request: {
  id: string
}): {
  skill: SkillDetail
}

get_skill_file(request: {
  id: string
  path: string
}): {
  file: {
    content: string
    path: string
  }
}

import_skill(request: {
  sourcePath: string
}): {
  skill: SkillSummary
}

set_skill_enabled(request: {
  id: string
  enabled: boolean
}): {
  skill: SkillSummary
}

delete_skill(id: string): {
  id: string
  status: 'deleted'
}

subscribe_conversation_events(request: {
  conversationId: string
  afterCursor?: ConversationCursor
}): {
  subscriptionId: string
  conversationId: string
  replayEvents: RunEvent[]
  cursor?: ConversationCursor
  gap: boolean
}

page_conversation_timeline(request: {
  conversationId: string
  afterCursor?: ConversationCursor
  limit?: number
}): {
  events: RunEvent[]
  cursor?: ConversationCursor
  gap: boolean
}

unsubscribe_conversation_events(subscriptionId: string): {
  subscriptionId: string
  status: 'unsubscribed' | 'alreadyClosed'
}

type ConversationCursor = {
  eventId: string
  conversationSequence: number
}
```

Command naming:

- Rust commands use `snake_case`.
- Frontend wrapper functions use `camelCase`.
- Command names should be domain verbs: `start_run`, `cancel_run`, `resolve_permission`.
- Avoid generic names such as `execute`, `handle`, `send`, and `process`.

Memory IPC payloads must be Zod validated in `shared/tauri`, loaded through
TanStack Query, and rendered with sanitized error text. Components must not
render backend error bodies or raw audit event payloads. Memory export writes a
JSON file under `.jyowo/runtime/exports` and returns only the relative `path`,
`itemCount`, `format`, and `exportedAt`; export content must not be stored in
frontend state or rendered into the DOM.

Skill IPC payloads must be Zod validated in `shared/tauri`, loaded through
TanStack Query, and rendered with sanitized error text. `list_skills` loads
only summaries. `get_skill_detail` loads manifest metadata and the file index.
`get_skill_file` is the only skill command that reads file content, and the UI
must call it lazily for the selected file. Config display must show keys only,
never secret values. Skill import must use the system directory picker and pass
a local skill package directory containing `SKILL.md`; the frontend must not
offer single Markdown file import.

Streaming:

- streamed events must have `runId`, `sequence`, `conversationSequence`,
  `timestamp`, `type`
- frontend reducers must be idempotent by `id` and sequence-aware
- UI must batch streaming updates

Conversation timeline:

- `ConversationBlock[]` is the only render source for the conversation canvas.
- `get_conversation`, replay events, live `conversation_event_batch` events,
  artifact snapshots, local submits, and command results feed the timeline
  reducer.
- `ConversationWorkspace` composes `useConversationTimeline`,
  `ConversationTimeline`, and `Composer`; it must not merge messages, activity
  events, artifacts, and local optimistic arrays itself.
- `clientMessageId` is generated before `start_run` and is the only key that
  confirms an optimistic user message. Body text must not be used for matching.
- `conversationSequence` is the backend-provided conversation order key.
  Frontend code must not sort conversation blocks by `(runId, sequence)`.
- Feature code must not call Tauri `listen` directly. `shared/tauri` owns the
  typed listener for `conversation_event_batch` and exposes parsed payloads.
- Replay returned by `subscribe_conversation_events` must be applied before
  live batches for the same `subscriptionId`.
- Gaps or cursor mismatches mark the reducer gap state and request replay or
  snapshot recovery. The UI must not guess missing event order locally.
- Streaming assistant text stays in the reducer buffer until
  `assistant.completed` provides a redacted final body or snapshot
  reconciliation finds the final message by `messageId`.

IPC error shape:

```ts
type CommandErrorCode =
  | 'IPC_UNAVAILABLE'
  | 'INVALID_PAYLOAD'
  | 'RUNTIME_UNAVAILABLE'
  | 'PERMISSION_DENIED'
  | 'RUN_NOT_FOUND'
  | 'MCP_CONNECTION_FAILED'
  | 'UNKNOWN'
```

Frontend forbidden behavior:

- no bare `invoke` in components
- no command strings assembled from component state
- no secret values in React state beyond short-lived input fields
- no frontend-only security decision
- no broad Tauri capabilities for convenience

## State Management

State classes:

| State | Owner | Examples |
|---|---|---|
| Backend/server state | TanStack Query | runs, models, MCP servers, Memory items |
| Local UI state | Zustand | sidebar collapsed, selected event, panel size |
| Form state | React Hook Form + Zod | provider settings, MCP config, run creation |
| URL state | TanStack Router | selected run, filters, tabs |
| Streaming event state | reducer/adapter | timeline buffer |

TanStack Query:

- use it for backend data
- use stable query keys
- do not store backend data again in Zustand
- invalidate by domain
- keep mock clients testable

Zustand:

- UI state only
- no backend cache
- no secret values
- no command execution logic

Forbidden:

```ts
type UiStore = {
  runs: Run[]
  mcpServers: MCPServer[]
  apiKey: string
}
```

Streaming reducers:

- append by sequence
- deduplicate by id
- preserve order
- normalize by id where needed
- expose derived views instead of recomputing in render

## RunEvent Schema

Rust contracts are canonical. Frontend `RunEvent` is a rendering model for UI stability.

Every frontend `RunEvent` includes:

```ts
type RunEventBase = {
  id: string
  runId: string
  sequence: number
  timestamp: string
  type: RunEventType
  source: 'user' | 'assistant' | 'tool' | 'engine' | 'policy'
  visibility: 'public' | 'redacted' | 'withheld'
}
```

Required event types:

```ts
type RunEventType =
  | 'run.started'
  | 'run.ended'
  | 'assistant.delta'
  | 'assistant.completed'
  | 'tool.requested'
  | 'tool.approved'
  | 'tool.denied'
  | 'tool.completed'
  | 'tool.failed'
  | 'permission.requested'
  | 'permission.resolved'
  | 'engine.failed'
```

Rules:

- parse all event payloads with Zod
- render by discriminated union
- use exhaustive checks
- Raw JSON shows redacted payload only
- withheld payloads are not rendered
- frontend schema does not replace Rust canonical events

Mapping targets include plan events, token usage, diff events, artifact events, memory events, MCP lifecycle events, model provider events, eval events, and audit events.

The Rust-to-frontend adapter, event versioning policy, and replay timeline must preserve event order, visibility, and source semantics.
