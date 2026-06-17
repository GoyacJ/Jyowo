# Jyowo Frontend Architecture

## Monorepo Boundary

- MUST keep the desktop frontend under `apps/desktop`.
- MUST keep source code under `apps/desktop/src`.
- MUST keep Tauri Rust code under `apps/desktop/src-tauri`.
- SHOULD keep future shared JS packages under `packages/*` only when they have more than one consumer.

## Source Layout

```text
apps/desktop/src/
  app/
  routes/
  features/
  shared/
```

- `app` MUST contain providers, router setup, shell, and global error boundaries.
- `routes` MUST contain TanStack Router file routes.
- `features` MUST contain domain features. Current feature: `system-status`.
- `shared` MUST contain reusable infrastructure: IPC, events, text layout, state, UI primitives, styles, test helpers.

## State Ownership

- TanStack Query MUST own backend and IPC data.
- Zustand MUST only store local UI state.
- React component state MAY hold short-lived view state.
- Components MUST NOT call Tauri `invoke` directly.

## Dependency Direction

- `app` MAY import `routes`, `features`, and `shared`.
- `routes` MAY import `features` and `shared`.
- `features` MAY import `shared`.
- `shared` MUST NOT import `features`, `routes`, or `app`.
