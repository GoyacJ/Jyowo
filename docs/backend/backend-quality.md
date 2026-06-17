# Jyowo Backend Quality

This document defines backend tests, gates, CI, docs policy, and review checklist.

## Required Gates

Root commands:

```text
pnpm check:rust-deps
pnpm check:backend-docs
pnpm check:docs
pnpm check:rust
pnpm check
```

Rust commands:

```text
cargo update --dry-run --workspace --verbose
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
```

`pnpm check:rust` MUST run:

```text
pnpm check:rust-deps
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
```

`pnpm check` MUST run docs, desktop, and Rust gates.

## Test Coverage

Backend tests must cover behavior at the owning layer.

Required test groups:

| Area | Required coverage |
|---|---|
| Contracts | serde shape, JsonSchema exports, IDs, Event tags, errors, permission payloads, snapshot stability |
| Permission | PermissionBroker decisions, deduplication, persistence, tamper handling, fail-closed defaults |
| Tool | registry behavior, orchestration, builtin filesystem/shell IO, result budgets, permission checks |
| Journal | append/read order, snapshots, migrations, version handling, audit query behavior |
| Redaction | Redactor before every durable store path |
| Replay | cursor behavior, redacted output, snapshot reads |
| Tauri command | command payload identity, shell metadata, SDK availability |
| SDK | builder requirements, runtime assembly, mock/testing adapters |
| Budget | quota serialization and token budget defaults |
| Search | SQLite FTS5 indexing, query behavior, deleted item removal, visibility filtering |
| Secret | keyring reference handling, missing secret behavior, no raw Secret serialization |
| Migration | forward migration, idempotence, incompatible schema rejection, rollback-safe failure |
| Property | permission, redaction, budget, event ordering, and migration invariants |

Snapshot tests use `insta` for public contract shapes, event JSON, schema exports,
and representative error payloads. Snapshots must not contain secrets or private
absolute paths.

Property tests use `proptest` for invariants that are easy to regress with narrow
examples.

Critical backend tests:

```text
apps/desktop/src-tauri/tests/commands.rs
crates/jyowo-harness-budget/tests/budget_contract.rs
crates/jyowo-harness-contracts/tests/m1_contracts.rs
crates/jyowo-harness-journal/tests/version.rs
crates/jyowo-harness-observability/tests/journal_redactor_pipeline.rs
crates/jyowo-harness-sdk/tests/runtime_assembly.rs
crates/jyowo-harness-tool/tests/builtin_exec.rs
crates/jyowo-harness-tool/tests/builtin_io.rs
crates/jyowo-harness-tool/tests/orchestrator.rs
```

New backend behavior MUST add or update tests in the crate that owns the behavior.

## Dependency Audit

`pnpm check:rust-deps` MUST run `cargo update --dry-run --workspace --verbose`.

The dependency audit fails when a Rust dependency is outdated and not classified
as an upstream-held transitive dependency.

Allowed upstream-held transitive dependencies:

| Package | Held at | Available | Upstream owner | Constraint |
|---|---:|---:|---|---|
| `generic-array` | `0.14.7` | `0.14.9` | `crypto-common 0.1.7` | exact dependency required by the RustCrypto `digest 0.10` chain used by Tauri/Wry SHA-2 code |
| `matchit` | `0.8.4` | `0.8.6` | `axum 0.8.9` | exact dependency selected by the latest stable Axum release |
| `toml` | `0.8.2` | `0.8.23` | `system-deps 6.2.2` | Linux GTK/Tauri build dependency chain |
| `toml_datetime` | `0.6.3` | `0.6.11` | `proc-macro-crate 2.0.2` | exact dependency required by GTK proc-macro chain |
| `toml_edit` | `0.20.2` | `0.20.7` | `proc-macro-crate 2.0.2` | exact dependency required by GTK proc-macro chain |

These entries are not direct project dependencies. Do not add `[patch.crates-io]`
or force a transitive version to hide the audit output. Remove an entry when its
upstream owner releases a compatible stable version and `cargo update --dry-run
--workspace --verbose` no longer reports it as unchanged.

## Documentation Gate

`pnpm check:backend-docs` validates:

- active backend docs contain only the approved four files
- active backend docs do not contain old project names
- active backend docs do not use stage-based language
- required backend concepts are present
- documented Tauri command names match `#[tauri::command]`
- every command is registered in `generate_handler!`
- the workspace layer table matches Cargo workspace members
- critical backend tests are documented and present
- `unsafe_code = "forbid"` remains documented and enforced
- Rust dependency audit policy is documented

Update backend docs when changing:

- workspace members
- crate layer ownership
- public contract shape
- event variants
- permission behavior
- tool execution behavior
- MCP tool registration
- Memory visibility or persistence
- model provider boundaries
- Journal, Replay, Audit, or Redactor behavior
- Tauri command surface
- Rust quality gates
- Rust dependency audit policy

## CI

GitHub Actions should keep these jobs separate:

```text
docs
frontend
rust
desktop-build
```

The `docs` job runs `pnpm check:docs`.

The `rust` job runs:

```text
pnpm check:rust-deps
cargo fmt --all --check
cargo check --workspace
cargo test --workspace
```

The `desktop-build` job runs the full desktop gate and Tauri build on macOS.

## Review Checklist

Architecture:

```text
[ ] dependency direction follows Tauri shell -> L4 -> L3 -> L2 -> L1 -> L0
[ ] public contracts are in harness-contracts
[ ] command handlers stay thin
[ ] no lower layer imports a higher layer
[ ] no runtime bypass of PermissionBroker, Redactor, Journal, or tenant scope checks
```

Contracts:

```text
[ ] serde shape is stable and tested
[ ] JsonSchema export is updated when needed
[ ] contract snapshots are updated intentionally
[ ] Event tags are tested
[ ] error variants crossing boundaries are tested
[ ] frontend schema changes are coordinated
```

Security:

```text
[ ] final policy decision remains in Rust
[ ] fail-closed applies to policy, permission, Secret, sandbox, and scope errors
[ ] fail-open applies only to non-security telemetry
[ ] Secret values do not enter events, logs, traces, prompts, screenshots, or tests
[ ] destructive Tool execution requires explicit permission or persisted scoped approval
[ ] MCP tools carry origin and scope through permission checks
```

Persistence:

```text
[ ] SQLite migrations are covered
[ ] SQLite FTS5 visibility filtering is covered when search changes
[ ] Journal append order is preserved
[ ] redaction runs before durable writes
[ ] Replay does not reveal withheld data
[ ] Audit records can be derived from events and permission decisions
[ ] restart-stable guarantees are backed by persistence
```

Testing:

```text
[ ] cargo fmt --all --check passes
[ ] cargo check --workspace passes
[ ] cargo test --workspace passes
[ ] pnpm check:rust-deps passes
[ ] insta snapshots are reviewed when changed
[ ] proptest covers changed invariants when relevant
[ ] pnpm check:backend-docs passes
[ ] changed contract shape has tests
[ ] changed command surface has Rust and frontend boundary tests
```
