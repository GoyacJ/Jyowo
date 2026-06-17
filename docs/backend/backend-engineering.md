# Jyowo Backend Engineering

This document defines Rust backend implementation rules.

## Stack

Runtime stack:

```text
Rust 1.96
Tauri 2
tauri-plugin-store
Tokio
serde
serde_json
schemars JsonSchema
thiserror
tracing
tracing-subscriber
tracing-appender
rusqlite
SQLite FTS5
refinery
keyring
reqwest
tokio::process
portable-pty
```

Tooling:

```text
Node 24 LTS
pnpm 11.7
cargo fmt
cargo check
cargo test
cargo update --dry-run
insta
proptest
GitHub Actions
```

Root Rust policy:

```toml
unsafe_code = "forbid"
```

All Rust code MUST preserve the workspace lint policy. Do not add `unsafe` to application or harness code.

## Library Boundaries

Backend libraries are selected by runtime ownership. Do not add a parallel library
when the existing stack already owns the capability.

Persistence:

- `rusqlite` owns local SQLite access.
- SQLite FTS5 owns local search for conversations, artifacts, Memory, and project
  metadata.
- `refinery` owns SQLite schema migrations.
- Migration definitions belong beside the crate that owns the persisted schema.
- Schema changes require migration tests and restart-stable compatibility coverage.

Secrets:

- `secrecy` owns in-memory secret handling.
- `zeroize` owns explicit memory clearing where needed.
- `keyring` owns OS-backed provider key and token storage.
- UI and Journal state store secret references only.

Observability:

- `tracing` owns structured instrumentation.
- `tracing-subscriber` owns local subscriber setup.
- `tracing-appender` owns local rolling file sinks.
- OpenTelemetry crates own optional external telemetry export.
- Telemetry failures must not bypass policy or reveal secrets.

Contracts:

- `serde` and `serde_json` own serialized payload shape.
- `schemars` owns JsonSchema export.
- Contract schema export must be generated from Rust types, not hand-written.

Execution:

- `tokio::process` owns non-interactive command execution.
- `portable-pty` owns interactive terminal sessions only when a real PTY is needed.
- Command execution remains behind Tool, Sandbox, PermissionBroker, and Redactor
  boundaries.

Testing:

- `cargo test` owns Rust test execution.
- `insta` owns contract and event snapshot tests.
- `proptest` owns property tests for permission, redaction, budget, ordering, and
  migration invariants.

Forbidden:

- adding an ORM on top of `rusqlite`
- adding an external search service for local workspace search
- using OS keyring values as prompt, event, log, trace, screenshot, or snapshot data
- using `anyhow` across public crate, IPC, or contract boundaries
- using `portable-pty` for simple non-interactive commands

## Workspace Layers

Dependency direction:

```text
Tauri shell -> L4 -> L3 -> L2 -> L1 -> L0
```

Lower layers MUST NOT depend on higher layers.

| Package | Path | Layer | Rule |
|---|---|---|---|
| `jyowo-desktop-shell` | `apps/desktop/src-tauri` | Tauri shell | Exposes desktop IPC and starts the in-process harness facade. |
| `jyowo-harness-contracts` | `crates/jyowo-harness-contracts` | L0 | Owns public IDs, messages, events, errors, serde shape, and JsonSchema exports. |
| `jyowo-harness-budget` | `crates/jyowo-harness-budget` | L1 | Owns shared quota and token budget carriers. |
| `jyowo-harness-journal` | `crates/jyowo-harness-journal` | L1 | Owns event stores, snapshots, audit projections, blobs, and Replay cursors. |
| `jyowo-harness-memory` | `crates/jyowo-harness-memory` | L1 | Owns Memory primitives, recall, consolidation, and visibility rules. |
| `jyowo-harness-model` | `crates/jyowo-harness-model` | L1 | Owns provider abstractions, model errors, and usage reporting. |
| `jyowo-harness-permission` | `crates/jyowo-harness-permission` | L1 | Owns PermissionBroker, rule providers, deduplication, fingerprints, and persistence. |
| `jyowo-harness-sandbox` | `crates/jyowo-harness-sandbox` | L1 | Owns sandbox policies, execution isolation, resource limits, and backend errors. |
| `jyowo-harness-context` | `crates/jyowo-harness-context` | L2 | Owns context assembly, compaction, token budget behavior, and context events. |
| `jyowo-harness-hook` | `crates/jyowo-harness-hook` | L2 | Owns hook execution, hook outcomes, and hook event contracts. |
| `jyowo-harness-mcp` | `crates/jyowo-harness-mcp` | L2 | Owns MCP connection state, tool injection, resource updates, sampling, and elicitation. |
| `jyowo-harness-session` | `crates/jyowo-harness-session` | L2 | Owns sessions, workspace bootstrap, stream handles, and session lifecycle. |
| `jyowo-harness-skill` | `crates/jyowo-harness-skill` | L2 | Owns skill loading, validation, threat detection, and invocation contracts. |
| `jyowo-harness-tool` | `crates/jyowo-harness-tool` | L2 | Owns Tool traits, registry, orchestration, built-ins, result budget, and permission checks. |
| `jyowo-harness-tool-search` | `crates/jyowo-harness-tool-search` | L2 | Owns on-demand tool search and schema materialization. |
| `jyowo-harness-engine` | `crates/jyowo-harness-engine` | L3 | Owns run orchestration, model/tool loop, budgets, and runtime event emission. |
| `jyowo-harness-observability` | `crates/jyowo-harness-observability` | L3 | Owns tracing, usage accounting, Replay helpers, and Redactor implementations. |
| `jyowo-harness-plugin` | `crates/jyowo-harness-plugin` | L3 | Owns plugin loading, manifest validation, and plugin rejection. |
| `jyowo-harness-subagent` | `crates/jyowo-harness-subagent` | L3 | Owns subagent lifecycle, permission forwarding, and stalled-worker behavior. |
| `jyowo-harness-team` | `crates/jyowo-harness-team` | L3 | Owns multi-agent teams, member routing, topology, quotas, and team termination. |
| `jyowo-harness-sdk` | `crates/jyowo-harness-sdk` | L4 | Owns the business-facing facade, builder, prelude, builtins, and testing adapters. |

Rules:

- New public contract types belong in `jyowo-harness-contracts`.
- New primitive runtime capability crates belong in L1.
- New composite domains belong in L2.
- New orchestration across domains belongs in L3.
- Application-facing assembly belongs in `jyowo-harness-sdk`.
- Tauri command code must not reach around the SDK into lower layers unless the command is only exposing shell metadata.

## Contracts

`harness-contracts` is the source of truth for backend-to-frontend and backend-to-backend public contracts.

Rules:

- Public payloads use `serde` derives.
- Stable schemas use `JsonSchema`.
- Event enums use explicit serde tags.
- Contract enums that can grow externally SHOULD be `#[non_exhaustive]`.
- Field renames require migration or compatibility handling.
- Error enums exposed across crate or IPC boundaries are contract surface.
- Tests must cover serialization shape, deserialization, and representative error variants.

Forbidden:

```text
ad hoc JSON assembled with string concatenation
frontend-only event names without Rust contract mapping
renaming serialized fields without tests
placing public contract structs in application crates
```

## Tauri Commands

Tauri command is an IPC boundary. It is not a place for business logic.

Rules:

- Command names use `snake_case`.
- Command payload structs use explicit `serde` shape.
- Command handlers stay thin.
- Validation happens at the Rust boundary before touching runtime state.
- New command output shape must be documented in backend and frontend docs.
- New command exposure must be registered in `generate_handler!`.
- Commands that touch files, network, tools, model providers, permissions, MCP, Memory, Journal, Replay, Audit, or Secret data require security review.

Current Tauri commands:

```text
get_app_info
harness_healthcheck
```

Command payloads:

```rust
get_app_info() -> AppInfoPayload
harness_healthcheck() -> HarnessHealthcheckPayload
```

Forbidden:

```text
generic execute command
command string built from frontend input
command returning untyped serde_json::Value as the stable API
command reading or writing Secret values without a policy check
command bypassing PermissionBroker for tool or filesystem operations
```

## Runtime Bypass Rules

Backend code MUST NOT bypass:

- `PermissionBroker` for Tool, filesystem, network, sandbox, MCP, or destructive operations.
- `Redactor` before Journal persistence, Replay, logs, traces, or export.
- `Journal` for product trace events.
- tenant and workspace scope checks for Memory, Replay, and Audit reads.
- result budget handling for large Tool output.

Bypass code is allowed only for tests that explicitly use mock or noop adapters.

## Naming

Rust crate package names use `jyowo-harness-*`.

Rust library crate names use `harness_*` for harness crates and `jyowo_desktop_shell` for the Tauri shell.

Domain nouns should match contract names:

```text
Run
Event
Tool
Permission
MCP
Memory
Model
Journal
Replay
Audit
Secret
```

Avoid generic names:

```text
Manager
Processor
Handler
Data
Payload
```

`Payload` is allowed only at IPC edges where the type is an explicit command payload.
