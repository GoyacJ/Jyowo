# Jyowo Frontend Quality

This document defines tests, gates, CI, docs policy, review checklist, and references.

## Testing

Frontend tests follow the project-wide strategy defined in [../testing/testing-strategy.md](../testing/testing-strategy.md). This section covers frontend-specific requirements.

Test layers:

| Layer | Tool | Scope |
|---|---|---|
| Unit | Vitest | schemas, reducers, utilities |
| Component | Testing Library | UI behavior and states |
| Story | Storybook | complex UI state matrices |
| E2E | Playwright | browser smoke coverage without fixture command runtime |
| Contract | Zod/schema tests | IPC payloads, ConversationWorktreePage, RunEvent schema |

Must test:

- conversation list, conversation page, and natural composer behavior
- `ConversationTimeline` loading, empty, running, completed, permission, artifact,
  review, clarification, withheld, and error states
- `ProcessPanel` reasoning, activity, command, diff, tool, artifact, withheld,
  and failed steps
- Codex-style evidence test conversation, dark-theme evidence screenshot target,
  large diff, failed command, historical attachments, and collapsed completed history
- worktree projection store behavior, `clientMessageId` optimistic confirmation,
  turn ordering, throttled live invalidation, gap recovery, and refetch
  reconciliation
- `Composer` typing, submit, disabled, pending, retry, and continue states
- `ContextPanel` file references, selected context, and empty context states
- `ActivityRail` compact activity, failed activity, and drill-down behavior
- `PlanBlock`, `ProgressBlock`, `ArtifactPreview`, `ReviewRequest`, and
  `DecisionCard` state transitions
- `shared/events` valid and invalid RunEvent payloads
- `shared/tauri` worktree page parsing, conversation event subscription parsing,
  artifact media preview parsing, replay-before-live dispatch, stale
  subscription filtering, and listener cleanup
- event renderer exhaustiveness
- `shared/tauri` schema validation and error normalization
- test-only `CommandClient` fixtures
- `shared/text-layout` fallback path and measured path
- Zustand stores as UI-only state
- system status page loading, ready, and error
- PermissionDialog decision flow
- ProviderSettingsForm validation
- ProviderSettingsForm capability routing loading, empty, error, ready, save, and delete flows
- About settings version display, update check, update available, download
  progress, installed pending restart, failure, and release notes rendering
- MCP server config validation
- Memory edit/delete UI
- DiffViewer large diff fallback

Storybook is required for complex business UI:

```text
ConversationTimeline
Composer
ContextPanel
ActivityRail
PlanTimelineBlock
ConversationTurnView
AssistantWorkView
ToolGroupSegmentView
ArtifactSegmentView
ProcessPanel
ReviewRequestSegmentView
ClarificationRequestSegmentView
PermissionInlinePanel
DiffViewer
MCPServerCard
MemoryItemCard
ProviderSettingsForm
```

Storybook stories should cover loading, empty, success, failure, permission pending, high risk, redacted, and large output states.

Component acceptance matrix:

| Component class | Required coverage |
|---|---|
| `shared/ui` primitives | variants, keyboard behavior, focus state, disabled state, accessible name |
| Conversation components | loading, empty, streaming, completed, permission, artifact, review, withheld, error, retry, continue |
| Context components | no context, selected context, long lists, missing files, stale references |
| Activity components | queued, running, success, failed, blocked, redacted, drill-down |
| Execution components | permission pending, approved, denied, high risk, large output, Raw JSON |
| Form components | invalid input, pending submit, backend error, saved, secret masking, capability route empty state, unsupported runtime option hidden |

Playwright:

- do not use a fixture command runtime
- open `/`
- verify the browser shell renders without seeded conversations
- keep runtime-backed conversation flows in native Tauri E2E
- verify the composer remains the primary action entry
- verify Context and Activity surfaces do not replace the conversation canvas
- no dependency on fixture command payloads

Native Tauri E2E belongs in the desktop build gate when Tauri runtime coverage is required.

## Quality Gates

Root scripts:

```text
pnpm dev
pnpm build
pnpm lint
pnpm format
pnpm check:docs
pnpm check:release-version
pnpm check:release-workflow
pnpm check:tauri-updater
pnpm check:agent-orchestration-no-fakes
pnpm check:agent-supervisor-sidecar
pnpm check:desktop
pnpm check:desktop:full
pnpm check:rust
pnpm check
```

Desktop scripts:

```text
pnpm -C apps/desktop typecheck
pnpm -C apps/desktop lint
pnpm -C apps/desktop lint:fix
pnpm -C apps/desktop test
pnpm -C apps/desktop build
pnpm -C apps/desktop knip
pnpm -C apps/desktop storybook
pnpm -C apps/desktop build-storybook
pnpm -C apps/desktop test:e2e
pnpm -C apps/desktop check
pnpm -C apps/desktop check:full
```

`pnpm check` must run:

```text
release version consistency
release workflow policy
Tauri updater policy
agent orchestration no-fake policy
agent supervisor sidecar policy
frontend docs structure and required concepts
desktop typecheck
desktop lint
desktop unit/component tests
desktop build
desktop Knip
Rust format check
Rust workspace check
Rust workspace tests
```

`pnpm check:desktop:full` additionally runs Storybook build, Playwright smoke E2E, and Tauri build.

`pnpm check:docs` validates:

- the frontend docs directory contains only the approved active docs
- active frontend docs do not contain old project names
- required foundation concepts are present in active frontend docs

Naming gate:

```bash
rg -n "octo[p]us|Octo[p]us|OCTO[P]US" . \
  -g '!target/**' \
  -g '!node_modules/**' \
  -g '!dist/**' \
  -g '!storybook-static/**' \
  -g '!test-results/**' \
  -g '!.git/**'
```

Conversation canvas drift gate:

```bash
rg -n "ConversationBlockRow|blocks\\?: ConversationTurn\\[\\]|pendingPermissionBlocks|get_conversation\\.messages|PermissionRequestBlock|kind: 'permissionRequest'|Tool error withheld from conversation timeline" \
  apps/desktop/src/features/conversation/timeline \
  apps/desktop/src/features/conversation/ConversationWorkspace.tsx \
  -g '!**/*.test.ts' \
  -g '!**/*.test.tsx' \
  -g '!**/*.stories.tsx'
```

This guard is intentionally scoped to production conversation canvas files. It
must not scan raw event schemas, tests, Storybook fixtures, or the legal
`toolGroup` assistant segment kind.

Biome rules:

```text
2 spaces
line width 100
single quotes
semicolons as needed
no explicit any
no @ts-ignore
```

Knip checks unused dependencies, devDependencies, files, and exports. Ignore generated TanStack Router files, Tauri config entrypoints, Storybook entrypoints, and Playwright entrypoints where needed.

## CI

GitHub Actions should separate:

```text
frontend
rust
docs
desktop-build
```

PR requirements:

- `pnpm check`
- `pnpm -C apps/desktop build-storybook` for UI changes
- `pnpm -C apps/desktop test:e2e` for route or workflow changes
- security review for new Tauri commands, new capabilities, permission changes, secret handling, or IPC surface changes

Native desktop CI may be slower and can run on protected branches or release candidates.

Release workflow:

- tag pushes matching `v*.*.*` run release packaging
- release jobs run `pnpm check:release-version` before platform builds
- the build matrix covers `windows-latest`, `macos-latest`, and `ubuntu-22.04`
- Linux release builds install WebKit, GTK, AppIndicator, librsvg, and patchelf
- Tauri updater artifacts are signed with `TAURI_SIGNING_PRIVATE_KEY`
- updater/process plugin imports stay inside `shared/tauri`

## Documentation Policy

Active frontend docs:

```text
agent-harness-frontend-development-guidelines.md
frontend-product-ux.md
frontend-engineering.md
frontend-quality.md
```

Keep the file count low. Add a new file only when one existing document becomes too large to review.

Update docs when changing:

- directory structure
- IPC command surface
- Tauri capabilities
- updater/process plugin wrapper usage
- permission UI behavior
- provider capability route command schemas and settings UX
- RunEvent schema
- state ownership
- quality gates
- source-owned UI primitive policy
- testing strategy

PR checklist:

```text
[ ] Do docs/frontend/* need updates?
[ ] Did ConversationWorktreePage or ConversationTurn shape change?
[ ] Did RunEvent schema change?
[ ] Was a Tauri command added?
[ ] Was a permission type added?
[ ] Was secret handling changed?
[ ] Did a new UI primitive or component pattern appear?
```

## Review Checklist

Architecture:

```text
[ ] code is placed by feature/domain
[ ] route files only compose
[ ] shared/ui has no feature dependency
[ ] shared modules do not import app/routes/features
[ ] no circular dependency
[ ] no bare Tauri invoke outside shared/tauri
[ ] capability route eligibility is not derived from provider catalog on the frontend
[ ] TanStack Query owns backend and IPC-derived server state
[ ] Zustand owns local UI state only
[ ] TanStack Router owns route and URL state
[ ] Radix usage is wrapped by shared/ui
[ ] lucide-react is used for product icons
```

TypeScript:

```text
[ ] no explicit any
[ ] no @ts-ignore
[ ] union renderers are exhaustive
[ ] external input uses Zod
[ ] IPC payloads have types
[ ] generated files are isolated
```

UI:

```text
[ ] conversation-first hierarchy is preserved
[ ] composer remains the primary action entry
[ ] Context panel stays secondary
[ ] Activity and execution details stay tertiary
[ ] product components follow the documented component hierarchy
[ ] ConversationCanvas renders ConversationTurn[] from page_conversation_worktree
[ ] primitive UI comes from shared/ui
[ ] Markdown rendering rejects unsafe raw HTML
[ ] code highlighting is cached or lazy loaded
[ ] command palette behavior is keyboard accessible
[ ] resizable panels keep usable min and max sizes
[ ] local store contains no secrets or credentials
[ ] loading / empty / error / ready states exist
[ ] dark/light/system theme behavior is preserved
[ ] semantic tokens are used
[ ] no hardcoded feature colors
[ ] no dashboard-style gray card stacks
[ ] keyboard access exists
[ ] complex components have Storybook stories
[ ] text does not overflow fixed controls
```

Conversation and execution:

```text
[ ] RunEvent schema changes are documented
[ ] worktree contract and Zod schema changes are documented
[ ] every event type has a renderer
[ ] raw RunEvent data is not used as the conversation canvas product model
[ ] get_conversation.messages does not drive ConversationCanvas
[ ] permissions render nested under tool attempts
[ ] thinking text is status-derived, explicitly safe, or withheld
[ ] process steps and artifact previews are Rust projection derived only
[ ] Raw JSON display follows visibility rules
[ ] secrets are masked
[ ] Replay/export behavior is preserved when relevant
[ ] sequence ordering is tested
```

Security:

```text
[ ] new Tauri commands are documented
[ ] new permissions are documented
[ ] final policy decision remains in Rust
[ ] destructive actions require explicit approval
[ ] API keys do not enter trace/log/prompt
[ ] command runtime cannot be replaced by fixture data
[ ] capabilities remain minimal
```

Performance:

```text
[ ] long lists use virtualization
[ ] large output is truncated or lazy loaded
[ ] streaming is batched
[ ] large components are lazy loaded
[ ] render does not stringify large JSON
[ ] no unnecessary global rerender path
```

Testing:

```text
[ ] unit tests updated
[ ] component tests updated
[ ] Storybook updated for complex UI
[ ] E2E updated for changed flows
[ ] pnpm check passes
[ ] pnpm check:desktop:full passes when needed
```

## References

Official:

- Tauri 2: https://v2.tauri.app/
- React: https://react.dev/
- Vite: https://vite.dev/
- Tailwind CSS: https://tailwindcss.com/docs
- shadcn/ui: https://ui.shadcn.com/docs/tailwind-v4
- TanStack Router: https://tanstack.com/router/latest/docs/framework/react/overview
- TanStack Query: https://tanstack.com/query/latest/docs/framework/react/overview
- TanStack Virtual: https://tanstack.com/virtual/latest
- Zustand: https://zustand.docs.pmnd.rs/
- React Hook Form: https://react-hook-form.com/
- Zod: https://zod.dev/
- React Markdown: https://github.com/remarkjs/react-markdown
- remark-gfm: https://github.com/remarkjs/remark-gfm
- Shiki: https://shiki.style/
- cmdk: https://cmdk.paco.me/
- React Resizable Panels: https://github.com/bvaughn/react-resizable-panels
- Tauri Store Plugin: https://v2.tauri.app/plugin/store/
- Vitest: https://vitest.dev/
- Testing Library: https://testing-library.com/docs/react-testing-library/intro/
- Playwright: https://playwright.dev/
- Storybook: https://storybook.js.org/docs
- Biome: https://biomejs.dev/
- Knip: https://knip.dev/
- Pretext: https://github.com/chenglou/pretext

Benchmarks:

- Opcode / Claudia: https://github.com/winfunc/opcode
- Jan: https://github.com/janhq/jan
- Cherry Studio: https://github.com/CherryHQ/cherry-studio
- LobeHub / LobeChat: https://github.com/lobehub/lobehub
- OpenCovibe: https://github.com/AnyiWang/OpenCovibe
