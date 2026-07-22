# AGENTS.md — crates

Layered Rust workspace for the Jyowo agent harness. Dependencies flow strictly downward; a crate may only depend on lower layers.

## Layers

| Layer | Crates | Role |
|---|---|---|
| L0 | `contracts` | Shared types/traits; no business logic |
| L1 | `journal`, `memory`, `model`, `permission`, `sandbox`, `fs`, `execution`, `budget`, `provider-state` | Independent primitives |
| L2 | `context`, `session`, `tool`, `hook` (composites); `mcp`, `skill`, `tool-search` (extensions) | Compose L1 primitives |
| L3 | `engine`, `subagent`, `team`, `plugin`, `observability`, `agent-runtime` | Orchestration and runtime |
| L4 | `sdk` | Facade; the only crate external consumers (daemon, desktop shell) should use for harness features |
| — | `daemon` | Binary. Owns task execution, recovery, scheduling, agent capabilities |

Never add an upward dependency (e.g. L1 → L2) or make the desktop shell bypass `sdk`/`daemon` to reach internal crates — `check:daemon-agent-capability-boundary` enforces part of this.

## Rules

- `unsafe_code = "forbid"` workspace-wide. Clippy pedantic is on; the allow list lives in the root `Cargo.toml` — extend it only for a whole class of lint, never per-file `#[allow]` sprinkling.
- Network access from tools must go through `jyowo-harness-tool/src/network_broker.rs`. Raw `reqwest` anywhere else in that crate fails `check:tool-network-broker-boundary`.
- No mock/stub/fake implementations in production code paths for orchestration, permission, sandbox, or authorization (`check:agent-orchestration-no-fakes`). Test doubles belong behind `#[cfg(test)]` or `testing` features.
- Dependency versions are declared once in `[workspace.dependencies]`; member crates use `workspace = true`. After changing dependencies run `pnpm check:rust-deps`.
- Integration tests live in `tests/` per crate and are subject to file-size limits (`check:test-architecture`). Split rather than grow.
- Protocol types consumed by the frontend are generated from Rust via `pnpm generate:daemon-protocol`; changing daemon-facing types requires regenerating.

## Verify

- Small change: `pnpm check:rust:fast` (fmt + contracts/shell tests)
- Full: `pnpm check:rust` (fmt, check, full test suite incl. feature-gated tests)
