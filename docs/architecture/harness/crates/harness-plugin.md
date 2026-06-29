# harness-plugin

`jyowo-harness-plugin` owns plugin discovery, manifest validation, lifecycle state,
capability registration, and host-side plugin policy.

## Scope

The supported plugin formats are:

- local directory plugins with a manifest file
- cargo-extension sidecar plugins exposed as `jyowo-plugin-*` executables

The unsupported plugin formats are:

- dynamic library loading
- Wasm runtime loading
- remote marketplace installation
- plugin-provided host registry handles

Plugin installation records a local directory source. Installation and validation
must not run package scripts, shell hooks, build steps, or network fetches.

## Authority

Rust remains the Policy authority.

The frontend can request plugin operations through Tauri IPC, but final decisions
stay in Rust. Plugin processes never own Tool, Hook, MCP, Skill, filesystem,
network, sandbox, PermissionBroker, Journal, Replay, Audit, or Redactor handles.

Public product payloads belong in `jyowo-harness-contracts`. Stable payloads use
`serde` and `JsonSchema`.

## Sources

The desktop shell enables these sources:

- user plugin directories
- workspace plugin directories
- standalone cargo-extension binaries in the workspace-owned
  `.jyowo/runtime/plugins/extensions` directory

Project plugin directories are disabled unless an explicit Rust command writes
the allow setting.

Source trust is checked against manifest trust. User and project plugins are
`UserControlled`. Workspace-managed plugins are `AdminTrusted`.

Desktop v1 does not scan `$PATH` or arbitrary host binary directories for
`jyowo-plugin-*`. Cargo-extension binaries must live inside the workspace-owned
plugin runtime directory so sidecar discovery and execution stay inside the
`WorkspaceOnly` sandbox scope.

## Lifecycle

```text
Discovered -> Validated -> Disabled
                         -> Activating -> Activated
                                      -> Rejected
                                      -> Failed
Activated -> Deactivated
Failed -> Activating
```

State meanings:

- `Discovered`: a source produced a manifest candidate.
- `Validated`: the manifest passed schema, source, trust, signature, dependency,
  admission, and configuration checks.
- `Disabled`: the manifest is visible to product UI but activation is blocked by
  persisted plugin settings.
- `Activating`: the host is loading a runtime and constructing scoped capability
  registrars.
- `Activated`: declared capabilities are registered through host-owned registries.
- `Rejected`: policy, manifest, trust, or capability registration failed.
- `Failed`: runtime activation or RPC execution failed after admission.
- `Deactivated`: registered capabilities were revoked and runtime deactivation
  completed or was contained.

Deactivation must revoke registered Tool, Hook, MCP, and Skill capabilities even
when the plugin runtime returns an error.

## Manifest Policy

Manifest validation is fail-closed.

Validation covers:

- manifest serde shape and semantic version ranges
- source trust and reserved namespace rules
- signature requirements for `AdminTrusted`
- admission allowlist and denylist
- destructive Tool declarations
- exec Hook declarations
- remote MCP declarations
- Skill and MCP names owned by the plugin
- configuration schema compilation and persisted config values

A plugin may register only capabilities declared by its manifest. Undeclared
Tool, Hook, MCP, Skill, memory, steering, coordinator, or custom toolset
registrations are rejected.

## Product Model

The registry exposes product snapshots without giving product code runtime
handles.

The product model contains:

- plugin identity, source, trust, version, and lifecycle state
- declared capabilities
- registered capabilities
- configuration schema
- redacted configuration values
- manifest origin and hash
- warnings and recent lifecycle summaries

Secret configuration fields are owned by secure storage. They are represented as
managed fields and are never returned as frontend state, events, logs, traces,
snapshots, screenshots, Journal entries, Replay payloads, or support bundles.

## Sidecar Runtime

Cargo-extension plugins communicate with the host through JSON-RPC over a
host-owned process boundary.

Supported methods:

- `metadata`
- `activate`
- `deactivate`
- `tool.execute`
- `hook.handle`
- `skill.read`

The host converts sidecar responses into scoped capability registrations.
Sidecars do not receive registry handles. Every registration still passes the
same Rust trust and capability checks as an in-process plugin.

Runtime errors are redacted before entering Activity, Journal, Replay, logs,
traces, or product state.

## Audit

Plugin loaded, rejected, failed, configuration changed, installed, and
uninstalled events must be observable through the same redacted event chain as
other harness activity. Product detail may also expose bounded in-memory
lifecycle summaries for the current registry instance; these summaries are not a
durable audit source.

Audit payloads must include enough reason data for diagnosis and must not include
secret config values, raw RPC payloads, environment variables, credentials, or
unredacted filesystem content.
