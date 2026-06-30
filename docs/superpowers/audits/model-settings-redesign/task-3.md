# Model Settings Redesign Task 3 Audit

## Current Audit Status
Code Review: PASS
Security Review: PASS
Last Updated: 2026-06-30T20:00:13Z

## Task Analysis
Task 3 analysis:
- Objective: Restart-stable model usage summary with today, month-to-date, and all-time windows backed by persisted journal events.
- Current code facts: `UsageAccumulatedEvent` existed; `UsageAccumulator` was in-memory only; Settings had no usage command; probe usage was suppressed at infer time via `suppress_usage_accounting`.
- Files touched: `jyowo-harness-observability/model_usage.rs`, desktop `model_settings.rs`, `contracts.rs`, frontend `commands.ts`, `backend-engineering.md`, test fixtures.
- Tests required: observability aggregation (boundaries, DST, diagnostic exclusion), desktop command integration, frontend Zod validation.
- Security constraints: aggregate only `TenantId::SINGLE`; fail closed on event read failure; no prompts, raw events, or credentials in summary payloads; exclude `diagnostic` usage events.
- Added `UsageAccumulatedEvent.diagnostic` (default false) for persisted exclusion of probe-tagged usage without merging probe quota into product totals.

## Exit Analysis
Task 3 exit analysis:
- Implemented behavior:
  - Pure `summarize_model_usage` in observability with IANA/local timezone resolver and per-event DST-safe classification.
  - `get_model_usage_summary` reads all tenant journal events via paginated `query_after`, aggregates `UsageAccumulated`, returns camelCase IPC response.
  - Frontend `getModelUsageSummary()` with strict Zod schema for three windows plus timezone identity fields.
- Tests:
  - `cargo test -p jyowo-harness-observability model_usage`: 5 passed
  - `cargo test -p jyowo-desktop-shell model_usage_summary`: 3 passed
  - `pnpm -C apps/desktop test -- commands.test.ts`: passed (includes usage summary Zod cases)
- Gates (exit 0):
  - `pnpm check:backend-docs`
  - `pnpm check:desktop`
  - `git diff --check`
- Secret / provider payload / private path leakage check: summary contains only aggregated token/cost counts and model keys; no journal bodies exposed.
- Remaining scope: official quota (Task 4); UI matrix (Task 6).

## Code Review Subagent
Result: PASS
Findings: Retrospective fresh code-review subagent returned PASS for Task 3. Usage summary is backend-owned, reads persisted `UsageAccumulatedEvent`, includes today/month-to-date/all-time windows, excludes diagnostic usage, and handles timezone/DST boundaries without fixed-offset historical classification.

## Security Review Subagent
Result: PASS
Findings: Retrospective fresh security-review subagent returned PASS for Task 3. No credential or provider-native payload path was found; aggregation stays tenant-scoped and fails closed on runtime or event-store read failures.

## Notes
- `UsageAccumulatedEvent.diagnostic` added with `#[serde(default)]` for backward-compatible journal replay.
- Docs updated in `backend-engineering.md` for `get_model_usage_summary`.
