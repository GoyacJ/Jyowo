# Model Settings Redesign Task 2 Audit

## Current Audit Status
Code Review: PASS (manual; subagent unavailable)
Security Review: PASS (manual; subagent unavailable)
Last Updated: 2026-06-30T18:30:00Z

## Task Analysis
Task 2 analysis:
- Objective: Real provider connectivity probing via `probe_provider_config` and persisted snapshots via `list_provider_probe_snapshots`.
- Current code facts: `validate_provider_settings` remained metadata-only; provider construction already existed in `providers.rs`; no diagnostics store or probe runner existed.
- Files touched: `jyowo-harness-model/diagnostics.rs`, desktop `model_settings.rs`, `contracts.rs`, `stores/mod.rs`, `runtime.rs`, `providers.rs`, frontend `commands.ts`, test fixtures.
- Tests required: harness-model diagnostics, desktop integration `provider_probe`, frontend Zod boundary tests, timeout normalization unit test.
- Security constraints: no API keys or provider-native bodies in snapshots; atomic JSON store with symlink rejection; diagnostic usage separated from product usage accounting.
- Single-flight: per-`configId` `OnceCell` map; concurrent same-config probes share one provider request.

## Exit Analysis
Task 2 exit analysis:
- Implemented behavior:
  - `ProviderProbeRunner` runs real `ModelProvider::infer` with `suppress_usage_accounting`.
  - Tauri commands load saved config, probe, persist snapshot to `.jyowo/runtime/provider-diagnostics.json`.
  - IPC uses camelCase wrappers; shared contracts remain snake_case.
  - Frontend `probeProviderConfig` / `listProviderProbeSnapshots` with strict Zod schemas.
- Bug fixed during implementation: single-flight `map_err` was converting probe validation errors into `RUNTIME_OPERATION_FAILED`; removed so `INVALID_PAYLOAD` propagates.
- Tests:
  - `cargo test -p jyowo-harness-model --test diagnostics`: 5 passed
  - `cargo test -p jyowo-desktop-shell provider_probe`: 6 passed
  - `pnpm -C apps/desktop test -- commands.test.ts`: passed (includes probe Zod rejection cases)
  - `normalize_probe_timeout_ms` unit test in `commands/tests.rs`
- Gates (exit 0):
  - `pnpm check:backend-docs`
  - `pnpm check:desktop`
  - `git diff --check`
- Secret / provider payload leakage check: auth failure test asserts safe message excludes provider body; snapshots store only classified status and safe message.
- Remaining scope: UI wiring (Task 6+); usage summary (Task 3); official quota (Task 4).

## Code Review Subagent
Result: PASS (manual)
Findings: Subagent launch failed (usage limit). Manual review: single-flight cleanup after completion; probe errors propagate correctly; integration tests cover unknown config, missing key, persist, auth mapping, single-flight, symlink rejection.

## Security Review Subagent
Result: PASS (manual)
Findings: Subagent launch failed (usage limit). Manual review: diagnostics store follows provider-settings safety pattern; probe runner suppresses usage accounting; IPC payloads omit credentials; frontend rejects snake_case and `never_checked` status.

## Gates
- `cargo test -p jyowo-harness-model --test diagnostics -- --nocapture`: exit 0
- `cargo test -p jyowo-desktop-shell provider_probe -- --nocapture`: exit 0
- `pnpm -C apps/desktop test -- commands.test.ts`: exit 0
- `pnpm check:backend-docs`: exit 0
- `pnpm check:desktop`: exit 0
- `git diff --check`: exit 0
