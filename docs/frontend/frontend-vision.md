# Jyowo Frontend Vision

Status: v0.1 foundation.

Jyowo is a desktop agent harness workbench. The frontend is trace-first. It must make agent execution observable, reviewable, and safe to operate.

## Scope

- MUST target `apps/desktop/src`.
- MUST use `pnpm`.
- MUST keep the first screen usable as an app surface, not a marketing page.
- MUST treat the React frontend as untrusted.
- SHOULD optimize for dense engineering workflows.
- MAY add richer workbench views after the foundation is stable.
- DEFERRED: full Run Timeline, permission review workflow, MCP manager, memory browser, model provider settings, replay, eval lab.

## Product Bias

The main future object is a Run. A Run is displayed through structured events, not only through final assistant text.

Initial foundation keeps one page: system status. It validates the desktop shell, IPC boundary, frontend tooling, and test gates.
