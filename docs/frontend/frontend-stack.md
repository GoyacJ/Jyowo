# Jyowo Frontend Stack

## Runtime Stack

- MUST use React 19, TypeScript strict, Vite, and Tauri 2.
- MUST use Tailwind CSS v4 through `@tailwindcss/vite`.
- MUST use TanStack Router for routes.
- MUST use TanStack Query for IPC/server state.
- MUST use Zustand for local UI state.
- MUST use Zod for external payload validation.
- MUST use shadcn/ui style source-owned primitives.
- MUST use lucide-react for icons.
- SHOULD use TanStack Virtual for long event streams, logs, and timelines.
- SHOULD use React Hook Form with Zod for settings and creation flows.
- SHOULD use `@chenglou/pretext` through `shared/text-layout`.

## Text Layout

`@chenglou/pretext` is used as infrastructure for text measurement and layout. It is not a UI framework.

Source: [pretext README](https://raw.githubusercontent.com/chenglou/pretext/main/README.md).

Use cases:

- Timeline row height estimates.
- Log and raw JSON virtualized rendering.
- Diff block height estimates.
- Layout shift reduction when streaming text.

Business code MUST NOT import `@chenglou/pretext` directly. Use `shared/text-layout`.

## Tooling

- MUST use Vitest for unit/component tests.
- MUST use Playwright for web mock smoke E2E.
- MUST use Storybook React Vite for primitive and foundation stories.
- MUST use Biome for formatting and lint.
- MUST use Knip for unused dependency/export/file detection.
