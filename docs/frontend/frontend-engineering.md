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
@tauri-apps/plugin-process
@tauri-apps/plugin-store
@tauri-apps/plugin-updater
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
- `@tauri-apps/plugin-updater` and `@tauri-apps/plugin-process` are used only
  through `shared/tauri/updater`. Feature code must not import raw updater or
  process plugin modules.
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

## Workbench State Boundaries

The workbench selection state lives in `shared/state/workbench-selection.ts`:

```ts
type WorkbenchSelection =
  | { kind: 'context' }
  | { kind: 'decision'; conversationId: string; requestId: string }
  | { kind: 'tool'; conversationId: string; toolUseId: string }
  | { kind: 'command'; conversationId: string; fullOutputRef?: string; eventRef?: ConversationEventRef }
  | { kind: 'diff'; conversationId: string; changeSetId: string }
  | { kind: 'artifact'; conversationId: string; artifactId: string; revisionId?: string; previewRef?: string }
```

Rules:
- `shared/state/ui-store.ts` stores `workbenchSelection: WorkbenchSelection | null` and `setWorkbenchSelection`. It may import from `shared/state/workbench-selection.ts` but MUST NOT import from `features/workbench`.
- Workbench and timeline components read or update selection through `useUiStore` selectors. Do not add a feature-level state wrapper that imports shared state back into `features/workbench`.
- React stores only UI selection and draft state. Policy decisions stay in Rust.
- Full output, full patch, and artifact content are fetched by opaque `EvidenceRefId` via `getConversationCommandOutput`, `getConversationDiffPatch`, and `getArtifactRevisionContent` commands. No raw `RunEvent` drives the main canvas.
- Secrets and private paths are redacted before frontend state; React never sees raw tool input, command output, or chain-of-thought.

## Paged Timeline State

The conversation timeline uses a page-aware state model:

```ts
type ConversationTimelineState = {
  pages: Array<{ cursor: ConversationTurnCursor | null; turns: ConversationTurn[] }>
  loadedRange: { first?: ConversationTurnCursor; last?: ConversationTurnCursor }
  hasMoreBefore: boolean
  hasMoreAfter: boolean
  gapMarkers: Array<{ id: string; afterCursor: ConversationCursor | null }>
  optimisticTurnsByClientMessageId: Record<string, ConversationTurn>
  ...
}
```

Actions include `hydrateInitialPage`, `prependPage`, `appendPage`, `markGap`, `retryGap`.
Optimistic turns reconcile by `clientMessageId` across pages. The `useConversationTimeline` hook exposes `loadEarlier`, `loadLater`, and `retryGap`.

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
  composer/ComposerToolbar
  composer/ReferenceCombobox
  composer/SlashCommandMenu
  PlanBlock
  ProgressBlock
  ProcessStatusRow
  CommandEvidenceBlock
  DiffEvidenceBlock
  ToolEvidenceSummary
  UserAttachmentStrip
  ContextCompactionNotice
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

features/workbench
  WorkbenchInspector
  artifacts/ArtifactPane

features/activity
  ActivityRail
  ActivityItem
  RunEventDetails
```

Component ownership:

- `shared/ui` owns reusable interaction primitives and visual variants.
- `features/conversation` owns the natural chat surface and embedded work blocks.
- `features/conversation/timeline` owns conversation evidence blocks:
  `CommandEvidenceBlock`, `DiffEvidenceBlock`, `ProcessStatusRow`,
  `ToolEvidenceSummary`, `UserAttachmentStrip`, and `ContextCompactionNotice`.
- `features/workspace` owns project and conversation navigation.
- `features/context` owns secondary project context and references.
- `features/workbench` owns the right-side inspector. `features/workbench/artifacts`
  owns inspector artifact UI because it is selected from `WorkbenchInspector`;
  route-level artifact pages remain under `features/artifacts`.
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
- Unit and component tests may inject test-only `CommandClient` fixtures.
- Storybook and Playwright must not replace the command runtime with fixture data.
- Tauri capabilities remain minimal and explicit.
- Tauri updater/process plugin APIs go through `shared/tauri/updater`, not
  feature components. The About settings tab may call this wrapper and must
  reuse `get_app_info` for the installed version.
- Release notes loaded from the updater source must render as plain text or
  safe Markdown without raw HTML.
- RunEvent schemas must reject raw thinking text. `assistant.delta` carries
  `messageId` and UI-safe `text`; `assistant.thinking.delta` carries only
  status or explicit UI-safe reasoning summaries.

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

delete_project(path: string): {
  activePath: string | null
  path: string
  status: 'deleted'
}

start_run(request: {
  attachments?: AttachmentReference[]
  clientMessageId: string
  contextReferences?: ContextReference[]
  conversationId: string
  permissionMode?: 'default' | 'auto' | 'bypass_permissions'
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

get_artifact_media_preview(request: {
  conversationId: string
  artifactId: string
  revisionId?: string
  contentRef?: string
}): {
  dataUrl: string
  mimeType: string
  sizeBytes: number
}

get_attachment_media_preview(request: {
  conversationId: string
  attachmentId: string
}): {
  dataUrl: string
  mimeType: string
  sizeBytes: number
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

list_provider_capability_routes(): {
  version: number
  routes: Array<{
    kind:
      | 'image_generation'
      | 'video_generation'
      | 'text_to_speech'
      | 'speech_to_text'
      | 'music_generation'
    configId: string
    providerId: string
    operationIds: string[]
    enabled: boolean
  }>
}

list_provider_capability_route_options(): {
  options: Array<{
    kind:
      | 'image_generation'
      | 'video_generation'
      | 'text_to_speech'
      | 'speech_to_text'
      | 'music_generation'
    configId: string
    providerId: string
    operationId: string
    outputArtifact: 'image' | 'video' | 'audio' | 'file'
    execution: 'sync' | 'async_job'
    costRisk: 'low' | 'medium' | 'high'
    runtimeSupported: boolean
    unavailableReason?: string
  }>
}

save_provider_capability_route(request: {
  route: {
    kind:
      | 'image_generation'
      | 'video_generation'
      | 'text_to_speech'
      | 'speech_to_text'
      | 'music_generation'
    configId: string
    providerId: string
    operationIds: string[]
    enabled: boolean
  }
}): {
  version: number
  routes: Array<{
    kind:
      | 'image_generation'
      | 'video_generation'
      | 'text_to_speech'
      | 'speech_to_text'
      | 'music_generation'
    configId: string
    providerId: string
    operationIds: string[]
    enabled: boolean
  }>
}

delete_provider_capability_route(request: {
  kind:
    | 'image_generation'
    | 'video_generation'
    | 'text_to_speech'
    | 'speech_to_text'
    | 'music_generation'
  configId: string
  providerId: string
}): {
  version: number
  routes: Array<{
    kind:
      | 'image_generation'
      | 'video_generation'
      | 'text_to_speech'
      | 'speech_to_text'
      | 'music_generation'
    configId: string
    providerId: string
    operationIds: string[]
    enabled: boolean
  }>
}

get_execution_settings(): {
  autoModeAvailable: boolean
  contextCompressionTriggerRatio: number
  permissionMode: 'default' | 'auto' | 'bypass_permissions'
}

set_execution_settings(request: {
  contextCompressionTriggerRatio: number
  permissionMode: 'default' | 'auto' | 'bypass_permissions'
}): {
  autoModeAvailable: boolean
  contextCompressionTriggerRatio: number
  permissionMode: 'default' | 'auto' | 'bypass_permissions'
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
    enabled: boolean
    exposedToolCount: number
    id: string
    lastDiagnostic?: string
    lastDiagnosticAt?: string
    lastDiagnosticSeverity?: 'error' | 'info' | 'warning'
    lastError?: string
    manageable: boolean
    origin: 'managed' | 'plugin' | 'policy' | 'user' | 'workspace'
    scope: 'agent' | 'global' | 'session'
    status:
      | 'closed'
      | 'configured'
      | 'connecting'
      | 'disabled'
      | 'failed'
      | 'ready'
      | 'reconnecting'
    transport: 'http' | 'inProcess' | 'sse' | 'stdio' | 'websocket'
  }>
}

get_mcp_server_config(id: string): {
  server: {
    displayName: string
    enabled: boolean
    id: string
    scope: 'agent' | 'global' | 'session'
    transport:
      | {
          args: string[]
          command: string
          env: Array<{ key: string; value: string }>
          inheritEnv: string[]
          kind: 'stdio'
          workingDir?: string
        }
      | {
          bearerTokenEnvVar?: string
          headers: Array<{ key: string; value: string }>
          headersFromEnv: Array<{ envVar: string; key: string }>
          kind: 'http'
          url: string
        }
  }
}

save_mcp_server(request: {
  displayName: string
  enabled?: boolean
  id: string
  scope: 'agent' | 'global' | 'session'
  transport:
    | {
        args?: string[]
        command: string
        env?: Array<{ key: string; value: string }>
        inheritEnv?: string[]
        kind: 'stdio'
        workingDir?: string
      }
    | {
        bearerTokenEnvVar?: string
        headers?: Array<{ key: string; value: string }>
        headersFromEnv?: Array<{ envVar: string; key: string }>
        kind: 'http'
        url: string
      }
}): {
  server: McpServerSummary
}

set_mcp_server_enabled(request: {
  enabled: boolean
  id: string
}): {
  server: McpServerSummary
}

restart_mcp_server(id: string): {
  server: McpServerSummary
}

delete_mcp_server(id: string): {
  id: string
  status: 'deleted'
}

list_mcp_diagnostics(request?: {
  serverId?: string
}): {
  events: Array<{
    eventType: string
    id: string
    serverId: string
    severity: 'error' | 'info' | 'warning'
    summary: string
    timestamp: string
  }>
}

clear_mcp_diagnostics(request?: {
  serverId?: string
}): {
  status: 'cleared'
}

subscribe_mcp_diagnostics(request?: {
  serverId?: string
}): {
  replayEvents: McpDiagnosticRecord[]
  serverId?: string
  subscriptionId: string
}

unsubscribe_mcp_diagnostics(subscriptionId: string): {
  status: 'alreadyClosed' | 'unsubscribed'
  subscriptionId: string
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

page_conversation_worktree(request: {
  conversationId: string
  pageCursor?: ConversationTurnCursor
  direction?: 'before' | 'after'
  limit?: number
}): ConversationWorktreePage

unsubscribe_conversation_events(subscriptionId: string): {
  subscriptionId: string
  status: 'unsubscribed' | 'alreadyClosed'
}

type ConversationCursor = {
  eventId: string
  conversationSequence: number
}

type ConversationTurnCursor = {
  turnId: string
  position: number
}

type ConversationWorktreePage = {
  turns: ConversationTurn[]
  pageCursor?: ConversationTurnCursor
  eventCursor?: ConversationCursor
  hasMoreBefore: boolean
  hasMoreAfter: boolean
  gap: boolean
}
```

Composer model selection is a per-run control. The frontend keeps the selected
`modelConfigId` in local composer state and includes it in `startRun`. Changing
the selector must not persist a conversation setting or call a separate model
config command. Rust persists the selected model as the conversation default
only after `start_run` succeeds.

`page_conversation_worktree` is the `ConversationCanvas` data source.
`page_conversation_timeline` remains for Activity, Replay, and details views.
`get_conversation.messages` may support metadata and compatibility surfaces,
but it must not drive the conversation canvas.

Command naming:

- Rust commands use `snake_case`.
- Frontend wrapper functions use `camelCase`.
- Command names should be domain verbs: `start_run`, `cancel_run`, `resolve_permission`.
- Avoid generic names such as `execute`, `handle`, `send`, and `process`.

Provider capability route commands:

```text
list_provider_capability_routes
list_provider_capability_route_options
save_provider_capability_route
delete_provider_capability_route
```

Rules:

- Route command schemas use `.strict()` and reject unknown fields.
- `listProviderCapabilityRouteOptions` is the only frontend source for runtime support eligibility.
- Provider catalog service capabilities are read-only context and must not authorize route selection.
- Route payloads must not include API keys.
- `CapabilityRouteEditorDrawer` may display `runtimeSupported = false` options as disabled with backend reasons, but backend validation remains authoritative.

Capability route schema fields:

```text
kind
configId
providerId
operationIds
enabled
operationId
outputArtifact
execution
costRisk
runtimeSupported
unavailableReason?
version
routes
```

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

- `ConversationTurn[]` from `page_conversation_worktree` is the only render
  source for the conversation canvas.
- Raw `RunEvent` data and `page_conversation_timeline` are execution surfaces
  for Activity, Replay, details, and Raw JSON. They must not become product
  render models.
- Live `conversation_event_batch` events are invalidation signals for worktree
  refresh. They must not be reduced directly into render segments.
- `get_conversation.messages`, artifact snapshots, command results, and local
  submits must not feed a frontend timeline reducer. Local submits may create
  temporary optimistic turns and are reconciled by `clientMessageId` when the
  worktree projection arrives.
- `ConversationWorkspace` composes `useConversationTimeline`,
  `ConversationTimeline`, and `Composer`; it must not merge messages, activity
  events, artifacts, and local optimistic arrays itself.
- `ConversationWorkspace` loads `get_execution_settings` to initialize the
  composer's permission mode. Composer permission changes are local UI state
  for later sends and must not call `set_execution_settings`.
- `clientMessageId` is generated before `start_run` and is the only key that
  confirms an optimistic user message. Body text must not be used for matching.
- `Composer` submits the current `permissionMode` with every run request.
  Settings only stores the default mode used to initialize new composer state.
- `conversationSequence` is the backend-provided conversation order key.
  Frontend code must not sort conversation turns by `(runId, sequence)`.
- Feature code must not call Tauri `listen` directly. `shared/tauri` owns the
  typed listener for `conversation_event_batch` and exposes parsed payloads.
- Replay returned by `subscribe_conversation_events` must be applied before
  live batches for the same `subscriptionId`.
- Gaps or cursor mismatches mark the reducer gap state and request replay or
  snapshot recovery. The UI must not guess missing event order locally.
- Streaming assistant deltas request projected worktree refreshes. The product
  canvas renders the latest Rust-owned `AssistantWork` tree, including
  process, text, tool attempts, permissions, artifacts, review requests,
  clarification requests, notices, and errors.
- New assistant work should use `ProcessSegment` for UI-safe work process
  steps. `ThinkingSegment` is a compatibility shape only; the canvas must not
  read raw thought events or raw thought text.
- Image artifacts render from `ArtifactSegment.media` metadata and lazy-load
  preview bytes through `get_artifact_media_preview`. The preview command
  accepts `conversationId`, `artifactId`, and optional `revisionId`/`contentRef`.
  React must pass the Rust-projected `previewRef` or `contentRef` when present.
  The command returns only an image data URL, MIME type, and byte count. It must
  not expose blob paths, filesystem paths, remote URLs, signed URLs, or
  provider-native payloads to React.
- Image attachments render from `ConversationTurn.user.attachments` metadata and
  lazy-load preview bytes through `get_attachment_media_preview`. The preview
  command accepts only `conversationId` and `attachmentId`; React must not pass
  `blobRef`. The command returns only an image data URL, MIME type, and byte
  count. The returned MIME type describes the safe preview, not necessarily the
  original attachment MIME: JPEG, GIF, and WebP may return PNG preview data, and
  AVIF may return AVIF preview data after Rust-side validation. It must not
  expose blob paths, filesystem paths, remote URLs, signed URLs, or
  provider-native payloads to React.

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

Permission UI is display-only. Backend-authored permission review (summary, risk,
scope, sandbox policy, effective mode, required confirmation) is presented as-is.
The UI submits user intent (approve/deny with optional confirmation text) and never
infers safety, sandbox enforceability, or bypass status. Rust validates confirmation
text and remains the final policy authority.

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
- keep test-only command fixtures outside runtime code

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

## Agent Orchestration UI

Agent orchestration UI renders backend-owned state. React may hold local form
state, selected ids, filters, and optimistic submit state. Capability decisions,
run acceptance, background registry state, permission source, and worktree
isolation decisions come from Rust.

Settings switches:

- Settings > General renders Subagents, Agent teams, and Background agents
  switches from backend command responses.
- unavailable switches display backend-provided reason payloads.
- saving settings stores requested tool capability gates only.
- a saved switch does not authorize execution by itself. Rust policy still
  decides which model-visible tools may be installed or executed.
- failed saves refetch backend truth or restore the last backend-confirmed
  snapshot.

Agent tools:

- Composer must not submit run-level agent mode fields.
- `agent`, `agent_team`, and `background_agent` are model-visible tools
  installed only when backend capability responses and Rust policy allow them.
- the UI must not expose child/team/background execution mode controls from
  local constants.
- background user-facing start path is the `background_agent` model tool.

Background agents panel:

- the panel reads durable records with backend list/detail commands.
- pause, resume, cancel, send input, archive, and delete actions call Tauri
  commands through `shared/tauri`.
- input replies include the backend-provided pending input request id.
- archived records may be deleted only through the backend command.
- local UI filters and selection do not change background agent lifecycle state.

Conversation projection:

- `AgentActivitySegment` renders subagent, team, and background activity from
  `ConversationTurn` data projected by Rust.
- `AgentActivitySegment` must not reconstruct activity from raw `RunEvent`
  streams.
- permission UI uses `actorSource` from projected events and never infers child,
  team, or background identity from display text.
- background timeline links use backend ids from projection payloads.

Zod requirements:

- all agent orchestration IPC requests and responses in `shared/tauri` use Zod.
- all agent orchestration run events in `shared/events` use Zod.
- schemas must reject unknown actor-source tags and invalid background/team
  payloads.
- tests must include valid and invalid payloads for settings capability,
  agent tool policy parsing, `AgentActivitySegment` projection input, and
  background agent command responses.

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
  source: 'user' | 'assistant' | 'tool' | 'engine' | 'policy' | 'agent' | 'background' | 'plugin'
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
  | 'plugin.loaded'
  | 'plugin.rejected'
  | 'plugin.failed'
  | 'engine.failed'
```

Rules:

- parse all event payloads with Zod
- render by discriminated union
- use exhaustive checks
- `run.started` payloads may include
  `permissionMode: 'default' | 'plan' | 'accept_edits' | 'bypass_permissions' | 'dont_ask' | 'auto'`
- `permission.requested` payloads may include `autoResolved: true` when a run
  authorization mode automatically allowed the request without interactive UI.
- `permission.requested.payload.actorSource` must be one of `parentRun`,
  `subagent`, `teamMember`, or `backgroundAgent`. Team member sources carry
  `teamId`, `agentId`, `role`, and optional `parentRunId`.
- Raw JSON shows redacted payload only
- withheld payloads are not rendered
- frontend schema does not replace Rust canonical events

Mapping targets include plan events, token usage, diff events, artifact events, memory events, MCP lifecycle events, model provider events, eval events, and audit events.

The Rust-to-frontend adapter, event versioning policy, and replay timeline must preserve event order, visibility, and source semantics.
