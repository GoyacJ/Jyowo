# Jyowo

Jyowo is a local AI agent desktop application built with Tauri 2, React, and Rust.

The desktop UI communicates with a durable local daemon over Unix sockets or Windows named pipes. The daemon owns task execution, recovery, scheduling, permissions, memory, tools, and agent orchestration.

## Architecture

```text
React desktop UI
       │
       ▼
Tauri command bridge
       │ local IPC
       ▼
Jyowo daemon sidecar
       │
       ▼
Agent harness crates
```

- `apps/desktop`: React frontend and Tauri desktop shell.
- `apps/browser-runtime`: packaged browser automation runtime.
- `crates/jyowo-harness-daemon`: durable local task daemon.
- `crates/jyowo-harness-*`: model, tool, MCP, skill, plugin, memory, session, sandbox, and agent runtime modules.
- `scripts`: build, release, protocol generation, and architecture checks.

## Requirements

- Node.js 24 or later
- pnpm 11.7.0
- Rust 1.96 or later
- The platform dependencies required by Tauri 2

## Development

Install dependencies and start the desktop application:

```sh
pnpm install
pnpm dev
```

`pnpm dev` builds the daemon sidecar before starting the Tauri development application.

## Build

```sh
pnpm build
```

The production build bundles the daemon sidecar and browser runtime with the desktop application.

## Checks

Run the complete repository checks:

```sh
pnpm check
```

For a faster local pass:

```sh
pnpm check:quick
```

Frontend-only checks are available through `pnpm check:frontend:fast`. Rust checks are available through `pnpm check:rust:fast` or `pnpm check:rust`.
