# AGENTS.md — apps/desktop

React 19 + TypeScript frontend and Tauri 2 shell. All agent behavior lives in the daemon; this app renders state and forwards commands over the Tauri bridge.

## Layout

- `src/features/<domain>/` — feature modules (tasks, conversation, settings, skills, memory, …). New UI goes in a feature folder, colocated with its tests and stories.
- `src/shared/` — cross-feature code: `ui/` (shadcn-style primitives), `daemon/` (protocol client), `tauri/` (invoke wrappers), `i18n/`, `state/`, `styles/`.
- `src/routes/` — TanStack Router file routes; `routeTree.gen.ts` is generated (`pnpm routes:generate`).
- `src/generated/` — daemon protocol types; generated, never hand-edit.
- `src-tauri/` — Rust shell. New Tauri commands must be registered; `check:tauri-command-registration` enforces this.

## Stack choices (use these, don't introduce alternatives)

- State: zustand for client state, TanStack Query for daemon/server state.
- Styling: Tailwind 4 with project design tokens only. Raw palette classes fail `check:design-tokens`; raw control styling is only allowed under `shared/ui/`.
- Forms: react-hook-form + zod. Icons: lucide-react. Markdown: react-markdown + remark-gfm.
- i18n: react-i18next, resources in `shared/i18n` for both zh and en. Every user-visible string.

## Lint / format / test

- Biome is the only linter/formatter: `pnpm lint`, `pnpm lint:fix`, `pnpm format`. Do not add ESLint/Prettier config.
- Unit tests: vitest + Testing Library, colocated `*.test.tsx`. Respect size limits from `check:test-architecture`.
- Component states belong in Storybook stories (`*.stories.tsx`); e2e in `e2e/` with Playwright.
- `pnpm knip` must stay clean — remove unused exports/deps you create.
- Full local gate: `pnpm -C apps/desktop check` (typecheck, lint, test, build, knip).

## Daemon protocol changes

Rust protocol types are the source of truth. After changing them: `pnpm generate:daemon-protocol`, commit both the Rust change and regenerated `src/generated/*`. CI runs `check:daemon-protocol`.
