# Jyowo Quality Gates

## Root Scripts

- `pnpm lint`
- `pnpm format`
- `pnpm check:desktop`
- `pnpm check:desktop:full`
- `pnpm check:rust`
- `pnpm check`

## Desktop Scripts

- `pnpm -C apps/desktop typecheck`
- `pnpm -C apps/desktop lint`
- `pnpm -C apps/desktop lint:fix`
- `pnpm -C apps/desktop test`
- `pnpm -C apps/desktop build`
- `pnpm -C apps/desktop knip`
- `pnpm -C apps/desktop storybook`
- `pnpm -C apps/desktop build-storybook`
- `pnpm -C apps/desktop test:e2e`
- `pnpm -C apps/desktop check`

## Default Gate

`pnpm check` MUST run:

- desktop typecheck
- desktop lint
- desktop unit/component tests
- desktop build
- desktop Knip
- Rust format check
- Rust workspace check
- Rust workspace tests

## Full Desktop Gate

`pnpm check:desktop:full` MUST additionally run:

- Storybook build
- Playwright smoke E2E
- Tauri build

## Naming Gate

This command MUST produce no output:

```bash
rg -n "octo[p]us|Octo[p]us|OCTO[P]US" . -g '!target/**' -g '!node_modules/**' -g '!dist/**' -g '!.git/**'
```
