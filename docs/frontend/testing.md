# Jyowo Frontend Testing

## Unit

- MUST test schemas and adapter boundaries.
- MUST test IPC clients with mock invoke functions.
- MUST test `shared/text-layout` fallback behavior.
- MUST test Zustand stores as UI-only state.

## Component

- MUST test loading, ready, and error states for feature pages.
- MUST test providers and router boot.
- SHOULD test UI primitives for basic accessibility.

## Storybook

- MUST include a foundation smoke story.
- SHOULD add stories before adding complex business components.

## E2E

- MUST use Playwright for web mock smoke tests.
- MUST NOT depend on a real Tauri runtime in the first E2E phase.
- DEFERRED: native Tauri window E2E.
