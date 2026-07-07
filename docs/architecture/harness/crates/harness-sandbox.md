# harness-sandbox

`jyowo-harness-sandbox` owns process execution isolation, policy-specific capability reporting, sandbox lifecycle, and fail-closed sandbox errors.

## Scope

The crate owns `SandboxBackend`, `ExecSpec`, `ExecContext`, process sandbox policy validation, backend capability reporting, resource limits, activity handles, and lifecycle hooks.

It does not own permission decisions, HTTP provider transport, frontend availability state, route credential resolution, Journal persistence, Redactor policy, or Tauri command assembly.

## Execution Channels

Every `ToolActionPlan` declares the backend component that can enforce execution:

- `ProcessSandbox`: command execution, diagnostics, shell-like tools, and any tool that starts a process.
- `HttpBroker`: provider service calls, web fetch, web search backends that issue HTTP, and tool-originated media downloads.
- `ExternalCapability`: backend-owned capabilities that are neither process nor HTTP execution, such as outbound user messaging.
- `DirectAuthorizedRust`: authorized Rust code paths that do not start a process or issue HTTP.

`AuthorizationService` resolves permission first, then preflights the component named by the execution channel. It must not route every `ActionResource::Network` through process sandbox preflight.

## Sandbox Capabilities

Sandbox capability reporting is policy-specific. A backend reports the network policies and workspace policies it can enforce. Coarse booleans such as network supported or write supported are not authority for preflight.

Required network policy reporting:

- `none`: backend can enforce no network.
- `loopback_only`: backend can enforce loopback-only network.
- `allowlist`: backend can enforce exact host and port allowlists.
- `unrestricted`: backend allows unrestricted network.

Required workspace policy reporting:

- `read_write_all`: backend can expose the workspace read-write.
- `read_only`: backend can enforce read-only workspace access.
- `writable_subpaths`: backend can enforce write access limited to approved subpaths.

A backend must not report support for a policy it cannot enforce. `LocalIsolation::None` is not enforcement and must not claim restricted network or restricted workspace policy support.

## Routing Process Sandbox

Desktop process tools use a routing sandbox backend. The router selects one child backend per execution with deterministic strategy order:

1. OS-level `LocalSandbox` for the current platform when it can enforce the requested policy.
2. Docker ephemeral-per-exec sandbox when Docker is available, the configured image runs, and the workspace mount contract applies.
3. `LocalIsolation::None` only for unrestricted process policies with unrestricted workspace access.

For `NetworkAccess::None`, the router prefers OS-level local sandbox, then Docker, then fails closed with backend-authored unavailable reasons.

For `NetworkAccess::Unrestricted`, the router prefers OS-level local sandbox, then Docker, then `LocalIsolation::None` only when workspace policy is not restricted.

For `NetworkAccess::LoopbackOnly` or `NetworkAccess::AllowList`, process tools fail closed until a backend reports exact enforcement for that policy.

The router must not use a global selected-backend slot. Concurrent executions must keep separate selected child backends.

## Router Lifecycle

`execute_with_lifecycle` creates an opaque execution id in `ExecContext` before preflight. The id is internal and is not a public serde or frontend contract.

Router lifecycle rules:

- `preflight_execute` checks whether at least one child can enforce the requested `ExecSpec`.
- `before_execute` selects exactly one child backend, calls that child's `before_execute`, and stores a lease keyed by execution id.
- `execute` removes the matching lease and calls that selected child's `execute`.
- If `execute` has no lease, the router fails closed because lifecycle was bypassed.
- `RoutingActivityHandle` owns the selected child backend and calls that child's `after_execute` exactly once after `wait`.
- Router-level `after_execute` must not call a child backend.
- Leases are cleaned up on child `before_execute` failure, child `execute` failure, wait completion, kill, and observable dropped-handle paths.

The selector must not run again in `execute` after child `before_execute` succeeds.

## Docker Fallback

Docker fallback is a real runtime dependency. The desktop factory may report Docker fallback available only after checking Docker binary, daemon, configured image, and a trivial command with the active workspace mounted.

Required mount contract:

- host workspace root is mounted at `/workspace`.
- `VolumeMount::workspace(host_workspace_root, "/workspace")` is used only for `read_write_all` workspace policy.
- `ExecSpec.cwd` host paths under the workspace root are rewritten to `/workspace` relative paths.
- default workdir is `/workspace` when `ExecSpec.cwd` is absent.
- Docker fallback does not support `read_only` or `writable_subpaths` until Docker-specific mount enforcement exists and is tested.
- Unix user mapping uses the current uid/gid when Docker accepts it; otherwise ownership behavior must be documented near the factory.

Missing Docker binary, daemon, image, or mount support produces a backend-authored unavailable reason. It must not silently fall back to no-isolation for restricted policy.

## Authorized HTTP Broker Boundary

HTTP and provider tools use an authorized HTTP broker, not process sandbox network preflight.

The broker accepts an opaque permit derived from `AuthorizedToolInput`. Permit claims include session, run, tool use, tool name, approved host rules, and action plan hash. The permit is immutable and cannot be constructed by frontend state or tool-supplied strings. Before dispatch, the broker verifies the permit ticket proof against the same `TicketLedger` authority key used by the runtime authorization service.

Broker v1 supports allowlist-only HTTP(S) requests. It must:

- reject missing broker registration before ticket mint.
- reject non-HTTP schemes, invalid hosts, username/password URL authority, public raw IP literals, and host or port outside the approved rules.
- allow loopback IP literals only when the exact loopback host and port are explicitly approved.
- disable automatic redirects so 3xx responses return to the tool without following unreviewed targets.
- enforce timeout and response byte limits.
- redact secrets from errors before returning tool errors.
- avoid raw request and response body events unless a redacted event shape is explicitly required.

Desktop runtime assembly creates one broker instance and injects that same instance into authorization preflight and execution capability lookup.

## Permission Modes

`BypassPermissions` and `DontAsk` skip only interactive waiting. They do not bypass hard policy, tenant scope, workspace scope, sandbox preflight, HTTP broker validation, authorization ticket validation, Redactor, event ordering, or capability presence.

No tool can execute command, filesystem, network, MCP, or outbound message work without an authorization ticket.

## Runtime Status

Desktop startup computes backend-authored runtime capability status for process sandbox, HTTP broker, and tool availability. Status includes selected backend ids, supported policy sets, unavailable reasons, broker availability, and tool-level reasons.

React may render this status. React must not decide, upgrade, or forge execution availability.

## Forbidden

Forbidden production behavior:

- production mock data, fake runtime paths, noop success, or placeholder behavior.
- UI-only policy.
- a backend reporting policy support it cannot enforce.
- fallback from restricted policy to `LocalIsolation::None`.
- raw `reqwest` execution by HTTP/provider tools outside the authorized broker.
- Tauri commands granting, upgrading, or forging tool permission.
- frontend availability flags that mark a tool executable without backend status.
- compatibility wrappers that preserve coarse sandbox capability behavior.
- raw secrets in prompt, events, logs, traces, screenshots, frontend state, fixtures, snapshots, or exports.
