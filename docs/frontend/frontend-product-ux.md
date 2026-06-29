# Jyowo Frontend Product And UX

Jyowo is a conversation-native local Agent Runtime workbench.

The frontend is product-first. The user designs, runs, inspects, evaluates, and governs
agent workflows locally, keeps workspace context visible, and continues from results.

It is not a ChatGPT clone, a CRUD admin panel, a prompt manager, an observability
console, a permission dashboard, or a thin UI over one provider or one CLI.

Runs, trace events, tool calls, permissions, Replay, Audit, and Raw JSON are internal
execution and transparency layers. They support trust and recovery, but they are not
the primary product language.

## Product Positioning

The core product object is a Conversation.

A Conversation is a user-facing work surface backed by structured execution:

```text
Workspace
Conversation
Work Intent
Plan
Run
ConversationTurn
AssistantWork
RunEvent
Tool Call
Permission Request
MCP Server
Memory Item
Model Provider
Artifact
Eval Case
```

The visible product path is:

```text
Ask
Understand
Plan
Work
Review
Continue
```

The internal execution path is:

```text
Conversation
Run
RunEvent
Tool Call
Permission Request
Artifact
Replay
```

The UI must make the first path dominant and keep the second path available as
details.

`ConversationCanvas` renders `ConversationTurn[]`. Each turn contains the user
message and one optional `AssistantWork` tree. `AssistantWork` owns thinking
summaries, assistant text, tool attempts, permissions, artifacts, review
requests, clarification requests, notices, errors, and final answers.

Raw `RunEvent` data belongs to Activity, Details, Replay, and Raw JSON. It must
not become the product model for the main conversation canvas.

Conversation canvas renders assistant work as narrative text plus execution evidence
blocks. Evidence blocks include status rows, diff blocks, command blocks, tool rows,
permission panels, artifact previews, historical attachment chips, and compaction
notices. Raw events remain in Activity, Details, Replay, and Raw JSON.

## Core Principles

Conversation-first:

- The main surface MUST be a natural conversation canvas.
- User intent, assistant reasoning summaries, plans, progress, artifacts, and review
  requests SHOULD appear inside the conversation.
- Tasks are derived work units. They MUST NOT be the first-level product metaphor.
- Runs are execution records. They MUST NOT replace the conversation as the main
  surface.

Project-native:

- Workspace scope MUST be visible.
- Project files, active artifacts, decisions, and next actions SHOULD be one glance
  away.
- Jyowo should feel like working inside a living project document, not operating a
  backend system.

Local-first:

- Local workspace state MUST be clear.
- API keys MUST NOT enter model context.
- Trace data defaults to local storage.
- Memory must be inspectable, editable, deletable, and exportable.
- High-risk operations require explicit approval unless the user explicitly selects
  full access for the current composer run. Rust hard policy deny rules still
  override every composer mode.

Trust without admin posture:

- Trace, permissions, Audit events, Raw JSON, and Replay MUST exist as support
  surfaces.
- They SHOULD sit behind Activity, Details, or Context affordances.
- They MUST NOT dominate navigation, visual hierarchy, or first-run copy.

Frontend is not trusted:

- React displays decision UI and requests operations.
- Final policy decisions belong to the Rust core.
- Frontend code must not directly expose shell execution, arbitrary filesystem access,
  raw Secret access, unrestricted network access, or destructive system operations.

Semantic styling:

- Feature code uses semantic tokens and variants.
- Feature code must not hardcode product colors.

## Information Architecture

AppShell:

- left sidebar for project navigation and conversations
- center conversation canvas
- right Context panel
- bottom Activity rail
- top actions for layout, command palette, and local workspace utilities
- panels may be resizable where useful

Recommended primary layout:

```text
Top Bar: Workspace Actions | Layout | Command Palette
Sidebar: Search | Recent Conversations | Home | Conversations | Projects | Artifacts | Agents | Tools | Settings
Main: Conversation Header | Message Blocks | Plan | Diff Preview | Artifact Result | Composer
Context: Project | Path | Files | Active Artifact | Decisions Needed | Next Actions
Activity: Tool Status | Run Status | Compact Event Stream | View All Activity
```

Sidebar:

```text
Home
Conversations
Projects
Artifacts
Agents
Tools
Settings
```

Rules:

- Recent conversations SHOULD appear above global navigation.
- The active conversation row SHOULD be quiet but unmistakable.
- Tooling and settings belong below product work surfaces.
- Runs, trace, and permissions SHOULD NOT be primary sidebar items.

Conversation page:

- page title
- optional short project-context subtitle
- user message blocks
- assistant message blocks
- inline plan block
- compact progress state
- diff or command preview
- artifact summary
- review or continue actions
- bottom composer

The composer is the main action entry. It should support plain language, attachments,
context references, per-run permission mode, tool selection, and send.
The permission mode control belongs in the composer toolbar, not as the primary
setting page action. Settings stores the default mode only.

Context panel:

- current project
- workspace path
- relevant files
- active artifact
- decisions needed
- next actions

The Context panel is a drawer, not a dashboard. It should use rows, subtle dividers,
and direct actions.

Activity rail:

- compact execution status
- recent tool calls
- current Run state
- link to detailed activity

The Activity rail is secondary. It must never compete with the conversation or
composer.

## Visual Direction

Jyowo uses a Notion-derived warm-neutral visual system: a white document canvas,
warm-white surfaces, whisper-weight borders, and a single blue accent.

The interface should feel:

```text
calm
precise
editorial
local
focused
product-grade
```

It should not feel:

```text
admin
observability
security-console
terminal-first
generic SaaS dashboard
marketing page
```

Reference posture:

- borrow Notion's quiet document rhythm, left sidebar calmness, and block editing
  readability
- borrow Linear's visual precision, action clarity, and restrained interaction states
- borrow AI chat products' natural conversation flow
- do not copy any product's branding, logo, exact layout, icon set, or wording

Color tokens:

```text
background: #FFFFFF
surface: #FFFFFF
sidebar: #F7F6F3
muted: #F6F5F4
foreground: #1C1B19
muted-foreground: #615D59
border: #E9E8E4
input: #DCDBD6
ring: #097FE8
primary: #0075DE
accent: #2A9D99
accent-soft: #F2F9FF
badge: #F2F9FF
badge-foreground: #097FE8
success: #1AAE39
warning: #DD5B00
destructive: #D93F3F
code-background: #F6F5F4
terminal-background: #2B2926
```

Borders are whisper-weight. Elevation uses multi-layer low-opacity shadows
(`shadow-card`, `shadow-deep`) and is reserved for overlays such as dialogs,
dropdowns, the command palette, and starter cards. Flat surfaces do not cast
shadows.

Token families:

```text
background
foreground
surface
muted
muted-foreground
border
input
ring
primary
primary-foreground
secondary
secondary-foreground
accent
accent-foreground
accent-soft
badge
badge-foreground
destructive
destructive-foreground
success
success-foreground
warning
warning-foreground
info
info-foreground
code-background
terminal-background
```

Status color rules:

```text
success: completed, validated, tests passed
warning: waiting approval, missing config, risk
destructive: failed, blocked, destructive action
info: neutral runtime information
primary: primary action and send action
muted: secondary information
```

Visual hierarchy:

- The conversation canvas is dominant.
- The bottom composer is the main visual anchor.
- The Context panel is secondary.
- Activity is tertiary.
- Diff and code previews must be useful but compact.
- Plans should be inline, not a separate task dashboard.

Shape and spacing:

```text
sidebar width: 240-280px
context width: 320-420px
conversation max-width: 820-980px
composer height: 64-96px
compact row padding: 8-12px
block padding: 12-18px
radius: 8px or less
line-height: 1.45-1.65
```

Allowed:

```tsx
<div className="bg-surface text-foreground border-border" />
<Badge variant="success">Ready</Badge>
```

Forbidden:

```tsx
<div className="bg-zinc-950 text-zinc-100 border-zinc-800" />
<div style={{ color: '#ef4444' }} />
```

Also forbidden:

- gray card stacks
- cards inside cards
- decorative gradient blobs
- full-screen terminal look
- monitoring dashboard composition
- large marketing hero sections
- purple-blue gradient branding
- using chips for every small piece of state
- dense file trees as the main visual subject

Typography:

```text
UI: bundled Inter (Inter Variable), system-ui fallback
Code: SF Mono / Menlo / Cascadia Code / JetBrains Mono
Numbers: tabular-nums
```

Headings use Inter with tightened tracking that scales with size. Body text uses
normal tracking and a 1.45-1.65 line height.

Use page-like headings and readable body text. Do not use oversized hero type inside
the app shell.

Motion should aid comprehension, not decorate.

Theme support:

```text
light
dark
system
```

Light theme is the design baseline.

## Core Components

Primitive UI:

- Source-owned shadcn/ui style primitives live under `apps/desktop/src/shared/ui`.
- Core primitives include `Button`, `Badge`, `Tooltip`, `Input`, `Textarea`,
  `Dialog`, `Dropdown`, `Tabs`, and `ScrollArea`.
- Primitives have no feature dependency and no business logic.
- Variants use `class-variance-authority`.
- Conditional classes use `cn()`.
- Radix and shadcn patterns are implementation sources, not visible product
  language.
- lucide icons are used for toolbar, navigation, status, and action controls.

Product component hierarchy:

```text
AppShell
  SidebarNav
    ProjectSwitcher
    ConversationList
  ConversationCanvas
    ConversationTurn
      UserMessage
      AssistantWork
        ThinkingSummary
        AssistantText
        ToolGroup
          ToolAttempt
            PermissionState
        ArtifactPreview
        ReviewRequest
        ClarificationRequest
        Notice
        Error
    Composer
  ContextPanel
    ContextSection
    FileReferenceList
    NextActionList
  ActivityRail
    ActivityItem
    RunEventDetails
```

Hierarchy rules:

- `Composer` is the primary action surface.
- `ConversationCanvas` is the primary reading and work surface.
- `ConversationCanvas` reads the Rust-owned worktree projection, not
  `get_conversation.messages`.
- `ContextPanel` is secondary and supports the current conversation.
- `ActivityRail` is tertiary and shows execution status without taking over the
  product.
- `RunTimeline`, `ToolCallCard`, `PermissionDialog`, `DiffViewer`, and
  `CommandPreview` appear inside conversation or activity surfaces.
- Raw JSON is available only as a drill-down support view.
- Thinking visible in the conversation must be status-derived, explicitly safe,
  or withheld. Raw thought text must not appear in the canvas.
- MCP, Memory, provider, permission, trace, and audit concepts must not become the
  main navigation model.

Shell and product components:

- `AppShell`
- `TopBar`
- `SidebarNav`
- `ConversationCanvas`
- `ConversationMessage`
- `Composer`
- `ContextPanel`
- `ActivityRail`
- `EmptyState`
- `ErrorState`
- `LoadingState`

Conversation blocks:

- `PlanBlock`
- `ProgressBlock`
- `ArtifactPreview`
- `ReviewRequest`
- `DecisionCard`
- `DiffViewer`
- `CommandPreview`

Agent domain components:

- `RunTimeline`
- `RunEventCard`
- `ToolCallCard`
- `PermissionDialog`
- `MCPToolCard`
- `MemoryHitCard`
- `ProviderSettingsForm`

`ProviderSettingsForm` includes a capability routing section under provider and model settings.

Capability routing UX rules:

- The main conversation model and capability routes are separate settings.
- Image input describes the main model accepting image attachments.
- Image generation describes a routed provider service for creating images.
- Video input and video generation follow the same distinction.
- Route rows are grouped by `CapabilityRouteKind` returned from `listProviderCapabilityRouteOptions`.
- Only options with `runtimeSupported = true` are selectable.
- The frontend must not infer runtime support from provider catalog data.
- `speech_to_text` appears only when the backend returns an eligible option.
- Each row shows current route status, selected provider profile, operation id, output artifact, execution mode, and cost risk.
- Save uses `saveProviderCapabilityRoute`. Delete or disable uses `deleteProviderCapabilityRoute`.
- Show a warning when the selected main model lacks tool calling.
- Capability routing must cover loading, empty, error, and ready states.

`ToolCallCard` must show:

- tool name
- status
- start/end time
- duration
- arguments summary
- output summary
- permission state
- error details
- expandable Raw JSON

It must not render only raw JSON.

Status Badge variants:

```text
queued
running
success
warning
failed
blocked
cancelled
```

`DiffViewer` must support:

- file path
- added/removed lines
- syntax-aware display when feasible
- large diff fallback
- copy patch action
- explicit risk marker for destructive changes

`CommandPreview` must show executable, args, cwd, environment redaction, risk level,
and approval state.

Raw JSON views are support tools. They must show redacted payload only and must not
reveal withheld payloads or secrets.

Complex components need Storybook stories for loading, empty, success, failure,
permission pending, large output, and redacted states.

Component visual rules:

- Conversation components use page-like rhythm and readable blocks.
- Execution components use compact status and progressive disclosure.
- Context components use lists, references, and concise metadata.
- Activity components use small rows, status dots, and short labels.
- Primitive controls must look consistent across conversation, context, activity,
  and settings.
- Product components should expose actions inline where the user reads the related
  work.

Forbidden visual patterns:

- nested cards
- large dashboard cards for every metric
- terminal or trace views as the default screen
- permission and audit surfaces as the main product identity
- generic gray panels replacing the conversation document
- visible implementation words such as router, query cache, plugin, reducer, or
  store in primary UI copy

## Security And Permission UI

RiskLevel:

```ts
export type RiskLevel = 'low' | 'medium' | 'high' | 'critical'
```

Examples:

```text
read file: low
write file: medium
delete file: high
shell command: high or critical
network call: medium or high
MCP tool call: based on tool scope
```

`PermissionDialog` and inline decision UI must explain:

- operation
- target object
- risk level
- reason
- exact command or diff when available
- workspace boundary
- data exposure
- whether the decision applies once, for this run, or permanently

Composer permission mode labels:

```text
Request approval
Auto approve
Full access
```

`Full access` must use warning styling and explain that it skips permission
confirmation for the run. It must not imply bypassing workspace, sandbox,
redaction, payload validation, or hard deny policy.

Button rules:

- Low and medium risk can use normal approve/deny labels.
- High risk must use explicit labels such as `Approve write` or `Allow command once`.
- Critical risk must require typed confirmation when destructive or externally exposing
  data.

Forbidden:

- hiding full commands behind a generic label
- approving a batch without itemized operations
- making destructive approval the default focused action
- storing raw secrets in UI state
- putting secrets into trace, logs, prompt text, Raw JSON, Storybook, tests, or
  screenshots

Security UI is explanatory only. Rust remains the policy authority.

Audit events are required for permission approval/denial, destructive file changes,
shell execution, network calls, provider key changes, MCP server install/update/delete,
and Memory deletion/export.

Audit must not be the product's main visual language.

## Forms And Feedback

Forms use React Hook Form and Zod.

ProviderSettings schema example:

```ts
const ProviderSettingsSchema = z.object({
  provider: z.enum(['openai', 'anthropic', 'local']),
  baseUrl: z.string().url().optional(),
  apiKeyRef: z.string().min(1),
  defaultModel: z.string().min(1),
})
```

API keys:

- never show raw values after save
- store only references in UI state
- validate by backend health check
- never include in RunEvent, logs, trace, prompt, or Storybook

Each page must have four states:

```text
loading
empty
error
ready
```

Empty states must say what is missing and what action is available. Error states must
show a concise message, retry action, and diagnostic details when useful. Toasts are
for transient feedback only, not primary error handling.

## Trace, Replay, And Activity

Trace is structured product data, but not the primary user surface.

Trace should answer:

```text
what happened
why it happened
what was approved
what changed
what failed
what can be replayed
```

Activity views derive from trace and RunEvent data.

Default Activity surface:

- compact status dots
- current operation
- recent tool status
- link to full activity

Detailed Activity surface:

- run timeline
- tool call cards
- permission decisions
- logs
- token usage
- redacted Raw JSON

Replay:

- must preserve original event order
- must mark replayed runs as replayed
- must not re-execute tools unless the user explicitly starts an execution replay mode

Export formats:

```text
JSONL event stream
Markdown report
redacted support bundle
```

Raw JSON must follow `visibility` rules: `public`, `redacted`, `withheld`.

## Performance And Accessibility

Timeline virtualization:

- Long timelines use TanStack Virtual.
- Variable-height rows use stable estimates.
- `shared/text-layout` may use `@chenglou/pretext` for text measurement.

Large output:

- truncate by default
- lazy load full output
- do not stringify huge JSON during render
- keep copy/export explicit

Streaming:

- batch updates
- keep sequence order stable
- avoid global rerenders for every token

Accessibility:

- interactive elements need keyboard access
- dialogs need focus trap and restore
- icon-only buttons need accessible labels or tooltips
- color cannot be the only state signal
- contrast must work in light and dark themes
- text must not overflow fixed controls

## Required Product Capabilities

Conversation workspace:

```text
conversation list
conversation page
natural composer
project context panel
artifact preview
inline plan
review request
continue action
```

Execution transparency:

```text
run creation
RunEvent stream
RunTimeline
ToolCallCard
PermissionDialog
DiffViewer
CommandPreview
Activity rail
redacted Raw JSON
Replay
```

Workspace operations:

```text
workspace selector
provider/model settings
capability routing
MCP manager
Memory browser
search
command palette
keyboard shortcuts
light/dark/system theme
local settings persistence
```

Quality and evaluation:

```text
Eval lab
usage analytics
artifact history
test result preview
support bundle export
```

Out of scope for the core product:

```text
full IDE
web collaboration dashboard
mobile adaptation
theme marketplace
assistant catalog clutter
```

## Benchmark Lessons

Notion:

- borrow quiet document rhythm, page-like navigation, readable block structure, and
  calm editing posture
- avoid copying branding, exact block controls, or generic document-product behavior

Linear:

- borrow visual precision, compact action rows, focused issue-like clarity, and
  restrained status expression
- avoid making every workflow feel like ticket management

Cline:

- borrow Plan/Act clarity, diff review, tool execution visibility, and human-in-loop
  decisions
- avoid tying Jyowo to a VS Code plugin mental model

OpenHands Agent Canvas:

- borrow the idea of a developer control surface for coding agents and automations
- avoid making backend control the primary product feeling

Langfuse:

- borrow detail density for trace inspection and observation drill-down
- avoid observability-console information architecture

Goose:

- borrow local agent, MCP, provider, and desktop navigation lessons
- avoid putting permissions and tools ahead of the user's work
