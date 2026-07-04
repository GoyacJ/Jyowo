# Jyowo Testing Strategy

## Purpose

Tests are executable product constraints, not temporary scaffolding. Every test asserts a behavior that must remain true across changes. Feature completion is not a reason to delete tests.

This document defines the test taxonomy, ownership rules, naming rules, fixture rules, deletion rules, refactor rules, local selection, full gates, and AI agent rules for the Jyowo project.

## Test Taxonomy

```text
unit
component
contract
policy
integration
smoke
manual-live
stress
```

| Layer | Description | Tool |
|---|---|---|
| unit | Single function, schema, reducer, utility | Vitest (frontend), Cargo test (Rust) |
| component | UI behavior and states, hook behavior | Testing Library, Vitest |
| contract | Serde shape, Zod schema, JsonSchema, IPC payloads | `harness-contracts`, Zod `.parse()` |
| policy | Invariant tests, property tests, boundary enforcement | Cargo test, `proptest` |
| integration | Cross-crate or cross-layer behavior | Cargo test integration binary |
| smoke | Browser shell renders, critical path walkthrough | Playwright |
| manual-live | Real provider API calls, live external services | `#[ignore]` by default, manual run |
| stress | Load, concurrency, large-data behavior | `#[ignore]` by default |

## Ownership Rules

- Rust behavior is tested at the owning crate.
- Public serde contracts are tested in `jyowo-harness-contracts`.
- Tauri command tests verify IPC payload validation, SDK delegation, redaction, and fail-closed behavior.
- Frontend tests verify Zod parsing, state reducers, hooks, component behavior, and user-visible states.
- Storybook owns complex visual state matrices.
- Playwright remains smoke/workflow coverage and must not become a fixture command runtime.
- Frontend does not make final policy decisions. Policy tests belong in Rust.

## File Naming Rules

### Rust

```text
tests/contract.rs
tests/<domain>_contract.rs
tests/policy.rs
tests/<domain>_policy.rs
tests/integration_<domain>.rs
tests/<domain>_regression.rs
tests/smoke_<domain>.rs
tests/manual_live_<provider>.rs
tests/stress_<domain>.rs
tests/<large_subject>_<domain>.rs
```

Allowed semantic suffixes include `_contract`, `_policy`, `_regression`, `_settings`, `_probe`, `_quota`, `_routes`, and domain-specific split suffixes used by an oversized source file.

Disallowed prefixes: `spike_`, `m[0-9]+_`, `t[0-9]+_`.

### Frontend

```text
*.schema.test.ts
*.store.test.ts
*.view-model.test.ts
*.render.test.tsx
*.component.test.tsx
*.workflow.test.tsx
*.permission.test.tsx
*.artifacts.test.tsx
*.redaction.test.tsx
*.large-output.test.tsx
*.stories.tsx
```

Frontend split test suffixes must describe user-visible behavior or boundary ownership. Do not use stage names, issue names, or generic buckets such as `misc`, `new`, or `temp`.

## Size Rules

- Hard fail: any tracked test file over 1200 lines unless explicitly allowlisted during active cleanup.
- Warning inventory: test file over 800 lines.
- Preferred frontend component test file: under 600 lines.
- Preferred Rust integration test file: under 1000 lines.

## Fixture Rules

- Fixture data must be domain-owned and minimal.
- `createTestCommandClient` is the frontend test fixture entry point via `@/testing/command-client`.
- Domain fixture builders export default fixture values, builder helpers, and domain-specific command handlers.
- Do not create production mocks. Test-only fixture code lives under `apps/desktop/src/testing`.
- Do not expand the global command client fixture into a monolith after it has been split.

## Deletion Rules

Test deletion requires one of:

- Product behavior was removed.
- Assertion covers old behavior.
- Duplicate test has no extra boundary value.
- Test only checks implementation detail and a behavior-level test replaces it.
- Snapshot has no stable business value.

The following coverage defaults to retained:

- PermissionBroker
- Redactor
- Journal
- Replay
- Secret
- Tauri command
- Zod
- serde
- Storybook
- Playwright
- manual-live
- stress
- fixture
- no mock
- Sandbox
- IPC
- MCP
- Memory
- Agent orchestration
- Provider routing

## Refactor Rules

- Destructive refactoring is allowed only when it removes clear test architecture debt.
- It must preserve product behavior and safety coverage.
- Do not keep compatibility wrappers solely to avoid touching tests.
- When splitting large test files, move test functions and only the helpers they need.
- Helpers used by one target file stay in that target file.
- Helpers used by two target files may be duplicated if duplication is smaller than a shared module.
- Helpers used by three or more target files go in a shared support module.

## Local Test Selection

Fast iteration gates for local development:

```text
pnpm check:frontend:fast   # typecheck + lint + Vitest
pnpm check:rust:fast       # contracts + desktop-shell tests
pnpm check:quick           # fast policy + docs + frontend + rust
pnpm check:agent-orchestration-no-fakes
pnpm check:agent-supervisor-sidecar
node --test scripts/memory-architecture-policy.test.mjs
node scripts/memory-architecture-policy.mjs
pnpm check:test-architecture  # naming and size enforcement
pnpm audit:tests           # regenerate test inventory
```

Full gates:

```text
pnpm check:desktop         # typecheck + lint + Vitest + build + Knip
pnpm check:rust            # fmt + check + full workspace tests
pnpm check                 # all gates combined
```

## Full Gates

`pnpm check` must run:

```text
release version consistency
release workflow policy
Tauri updater policy
docs validation (agent, frontend, backend, testing)
memory architecture policy
test architecture enforcement
desktop typecheck, lint, test, build, Knip
Rust format check, workspace check, workspace tests
```

## AI Agent Rules

When an AI agent performs development tasks:

- Read `docs/testing/testing-strategy.md` before adding, modifying, or deleting tests.
- Add or update tests in the owning layer before claiming completion.
- Comply with test taxonomy, naming, fixture, and deletion rules.
- Do not delete tests because a feature is complete.
- Use `pnpm check:quick` as the minimum verification gate before claiming completion.
- Use `pnpm check:test-architecture` to verify naming and size rules are satisfied.
- Use `pnpm audit:tests` to regenerate the inventory after structural changes.
- Changes to authorization, permission, sandbox, or agent orchestration behavior
  require a read-only subagent audit with PASS/FAIL verdict before the task
  commit. Fix FAIL results and re-audit before closing the task.
