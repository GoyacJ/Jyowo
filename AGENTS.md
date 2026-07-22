# AGENTS.md

Jyowo is a local AI agent desktop app: React UI → Tauri 2 command bridge → durable daemon sidecar (Unix socket / named pipe) → agent harness crates. The daemon owns task execution, recovery, permissions, memory, tools, and orchestration. The UI never executes agent work itself.

## Repository map

| Path | What it is |
|---|---|
| `apps/desktop` | React 19 frontend + Tauri shell (see `apps/desktop/AGENTS.md`) |
| `apps/browser-runtime` | Packaged browser automation runtime |
| `crates/jyowo-harness-*` | Layered Rust harness crates (see `crates/AGENTS.md`) |
| `crates/jyowo-harness-daemon` | Daemon binary; the only Rust entry point the desktop app talks to |
| `scripts/` | Build scripts and machine-enforced policy checks (`check-*.mjs`) |
| `docs/plans/` | Design + implementation plan documents |

## Commands

- `pnpm dev` — desktop app in dev mode (builds daemon sidecar first)
- `pnpm build` — full production build

Verification, cheapest first. Run the narrowest one that covers your change; run `pnpm check:quick` before declaring a task done:

- `pnpm check:frontend:fast` — typecheck + biome lint + vitest (frontend-only change)
- `pnpm check:rust:fast` — fmt + contracts/shell tests (small Rust change)
- `pnpm check:rust` — full cargo check/test workspace
- `pnpm check:quick` — all policy checks + fast frontend + fast Rust
- `pnpm check` — everything, including desktop full build (slow; CI-level)

## Hard rules (each one is enforced by a script in `scripts/`; violations fail CI)

- **No fakes in agent orchestration or authorization code.** Never introduce mock/stub/placeholder implementations in production paths touching subagents, teams, permissions, sandbox, or authorization (`check-agent-orchestration-no-fakes`).
- **Daemon owns agent capabilities.** Desktop shell (`apps/desktop/src-tauri`, settings UI, `shared/tauri/commands.ts`) and the SDK facade must not reference agent-capability internals like `AgentCapabilityResolver` or `AgentRuntimeStore` (`check-daemon-agent-capability-boundary`).
- **All tool HTTP goes through the network broker.** In `crates/jyowo-harness-tool`, raw `reqwest` usage is only allowed inside `network_broker.rs` (`check-tool-network-broker-boundary`).
- **Design tokens only in frontend styling.** No raw Tailwind palette classes (`bg-blue-500` etc.), no arbitrary shadows in `apps/desktop/src` outside `shared/ui` (`check-design-tokens`).
- **No legacy conversation commands.** The old conversation-era Tauri invoke names are banned (`check-no-legacy-conversation-surface`).
- **Test files have architecture limits** (size caps, placement rules). Do not add to the temporary allowlist in `check-test-architecture.mjs`; split the test file instead.
- **`Cargo.lock` must be fully resolved** — after dependency changes, `cargo update --dry-run` must produce no plan (`check-rust-deps`).

## Generated files — never hand-edit

- `apps/desktop/src/generated/daemon-protocol.ts` and `.schema.json` — regenerate with `pnpm generate:daemon-protocol` after changing daemon protocol types in Rust; verify with `pnpm check:daemon-protocol`.
- `apps/desktop/src/routeTree.gen.ts` — regenerate with `pnpm -C apps/desktop routes:generate`.

## Conventions

- Commit messages: Conventional Commits (`feat:`, `fix:`, `test:`, `ci:`, `chore:`), imperative, English, no scope-less prose.
- Non-trivial features get a plan first: `docs/plans/YYYY-MM-DD-<topic>-design.md`, then `...-implementation.md` with verifiable steps. Match the existing documents' structure.
- UI-facing strings go through i18next resources (zh + en); never hardcode display text.
- Rust: `unsafe` is forbidden workspace-wide; clippy pedantic is on (with a curated allow list in root `Cargo.toml`) — do not add new `#[allow]` attributes without need.
- Toolchain: Node ≥ 24, pnpm 11.7.0, Rust ≥ 1.96.
