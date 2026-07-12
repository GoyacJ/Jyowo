# Daemon Runtime Quality Remediation Plan

**Goal:** Complete the daemon-owned runtime migration by closing lifecycle, validation, snapshot immutability, precedence, and route-integrity gaps, then remove the remaining legacy behavior.

**Architecture:** Persisted settings are validated and merged once by the daemon resolver. The resulting immutable snapshot owns all runtime resources required by foreground and child runs. Per-run MCP registries expose explicit async shutdown, while plugin sidecars execute only frozen daemon-private binaries. UI and protocol fields preserve the distinction between an explicit task override and an inherited default.

**Tech Stack:** Rust, Tokio, Serde, Tauri, TypeScript, React, Vitest.

## 1. MCP lifecycle ownership

- Add failing fake-stdio-child tests for normal shutdown, partial connection failure, and drop fallback.
- Add `kill_on_drop(true)` to stdio children as the synchronous failure fallback.
- Give each run/child its own connected registry and explicit async shutdown guard.
- Ensure partial registry construction shuts down already-connected servers.

## 2. Persisted MCP validation

- Add failing daemon resolver tests for invalid IDs/scopes, stdio command/args/env/env keys/secrets/working directories, HTTP scheme/headers, and persisted in-process transports.
- Move reusable platform-neutral persisted-record validation to the MCP or contracts crate.
- Validate every merged record during daemon resolution and fail closed.

## 3. Runtime diagnostics redaction

- Add failing tests with malicious IDs, plugin failures, secret-like values, paths, serde errors, and oversized text.
- Replace derived error displays with one bounded sanitizer used for all user-controlled fields and nested sources.
- Keep error kinds actionable without exposing raw persisted content or filesystem paths.

## 4. Disabled plugin filtering

- Add failing tests proving disabled missing/corrupt packages are ignored and enabled corrupt packages fail closed.
- Filter effective plugin selection before reading manifests or package contents.
- Preserve disabled plugin configuration without touching its package directory.

## 5. Frozen plugin sidecar executables

- Add a failing source-binary replacement test covering foreground and child registries.
- Copy selected executable bytes into a daemon-private per-snapshot directory using no-follow reads, create-new writes, restrictive permissions, and a content hash.
- Rewrite frozen manifest origins to the copied executable and share the snapshot root through an `Arc` cleanup guard.
- Remove the snapshot directory when the final foreground/child snapshot reference is dropped.

## 6. Provider and permission precedence

- Add failing TaskWorkspace/daemon integration and desktop tests for explicit task choice over project selection over global default.
- Represent inherited UI defaults as `None`; only user actions create explicit task overrides.
- Apply project execution overrides before a truly explicit task permission choice.
- Regenerate protocol bindings if contract fields change.

## 7. Provider route integrity

- Add failing resolver tests for missing config IDs, missing credentials, provider mismatches, and invalid operation/kind combinations.
- Validate merged routes against resolved provider profiles and secrets during snapshot resolution.
- Remove deferred acceptance of invalid persisted routes.

## 8. Verification and commits

- Run each targeted test once red and again green.
- Run contracts/protocol generation checks when affected.
- Run daemon all-targets/factory, MCP, plugin, SDK, desktop Vitest/typecheck, formatting, and diff checks.
- Commit by coherent remediation topic and record each RED/GREEN result and commit SHA.
