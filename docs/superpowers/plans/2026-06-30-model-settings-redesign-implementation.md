# Model Settings Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign Settings > Models from a provider configuration form into a model health, usage, quota, and capability-routing control center backed by real backend data.

**Architecture:** Rust remains the policy authority. Provider connectivity, usage aggregation, account quota retrieval, route validation, credential handling, redaction, and persistence live in the backend. React renders model asset state from typed Tauri commands, sends user intent, and never infers security, quota, provider support, or secret availability on its own.

**Tech Stack:** Rust 1.96, Tauri 2, React 19, TypeScript 6, Zod, TanStack Query, React Hook Form, Testing Library, Storybook, Playwright, Vitest, `jyowo-harness-contracts`, `jyowo-harness-model`, `jyowo-harness-observability`, `jyowo-harness-journal`, `cargo test`, `pnpm check:rust`, `pnpm check:desktop`, `pnpm check:desktop:full`, `pnpm check:docs`, `pnpm check`.

---

## Product Design

The current Models settings surface is provider-config centered. The target product surface is model-asset centered.

Primary navigation inside Settings > Models:

```text
Settings / Models
  Models
  Capability Routes
```

`Models` owns configured model assets:

```text
Summary band
  default model
  available / failing configured models
  local usage for today / month-to-date / all-time total
  official quota state

Filter bar
  provider
  health status
  default only
  failing only
  text search

Model matrix
  model
  provider
  default
  connectivity status
  latency
  timeout threshold
  today usage
  month-to-date usage
  total usage
  official quota
  actions

Model detail drawer
  overview
  connectivity
  usage
  official quota
  configuration
  capabilities
```

Required `Models` layout specification:

- The first viewport is a working control surface, not a provider edit form and not a marketing page.
- Desktop layout, width >= 1100 px:
  - full-width summary band at the top, maximum two visual rows
  - one compact filter/action toolbar below the summary
  - full-width matrix below the toolbar
  - no permanent right-side form, side panel, or nested card shell
- Matrix column order:
  1. model/profile identity
  2. provider
  3. default marker
  4. connectivity status
  5. latency
  6. timeout threshold
  7. today usage
  8. month-to-date usage
  9. total usage
  10. official quota/package state
  11. actions
- Matrix column priority:
  - Never hide: identity, provider, connectivity status, timeout threshold, primary action.
  - May compact: default marker, latency, quota/package state.
  - May move into row details on narrow screens: today, month-to-date, total usage.
- Narrow layout, width < 900 px:
  - summary band becomes a two-column or one-column metric grid
  - filters wrap into a toolbar without horizontal overflow
  - matrix becomes a dense list/table hybrid where identity, health, timeout, and actions remain visible
  - detail drawer uses `min(720px, 92vw)` and must not cover the only visible close action
- Detail drawer:
  - opens from row selection or explicit details action
  - right-side drawer on desktop
  - full-width sheet on narrow screens
  - tabs use the exact product grouping: overview, connectivity, usage, official quota, configuration, capabilities
- Visual rules:
  - use existing `shared/ui` primitives and semantic tokens
  - no nested cards, decorative gradients, hero treatment, or explanatory marketing copy
  - cards are allowed only for repeated row/detail subsections, with radius <= 8 px unless the existing component requires otherwise
  - icon-only actions need accessible names and tooltips
  - loading, empty, error, partial-data, and ready states must keep stable dimensions and avoid layout shift

`Capability Routes` owns global service routing policy:

```text
Route table
  capability kind
  current routed provider profile
  route health
  execution mode
  cost risk
  actions

Route editor drawer
  eligible targets
  unavailable targets with backend reason
  selected operation ids
  save / clear
```

Model details may show which routes currently target the selected profile, but the main editor for routing stays in Capability Routes. This preserves the backend design rule that the main conversation model and provider capability routes are separate runtime policies.

## Current Code Facts

- Settings > Models currently renders one large component: `apps/desktop/src/features/settings/ProviderSettingsForm.tsx`.
- `ProviderSettingsForm` mixes provider profile list, create/edit forms, default model selection, API key reveal, model capability display, and capability route editing.
- Current provider "test" is metadata validation only. Frontend calls `validate_provider_settings` with `providerId` and `modelId`; backend only calls `ensure_provider_model_supported` and returns `accepted`.
- Saved provider settings live in `.jyowo/runtime/provider-settings.json`. List/save payloads expose `hasApiKey` only; raw keys are returned only by the existing short-lived reveal-token flow.
- Provider capability routes live in `.jyowo/runtime/provider-capability-routes.json` and are already backend-validated.
- Real provider construction already exists through `model_from_provider_settings` and `build_provider`.
- Usage events already exist through `UsageAccumulatedEvent`. Current usage aggregation is available in `UsageAccumulator`, but Settings has no command for model usage summary.
- `ModelRef` currently identifies only `providerId/modelId`. It does not identify a provider config profile.
- Official provider account quota/package usage is not implemented. Catalog source URLs are documentation sources, not account quota APIs.

## Non-Negotiable Rules

- Implement in an isolated git worktree. Do not implement in `/Users/goya/Repo/Git/Jyowo`.
- The implementation branch must be created from `main`, not from an arbitrary current HEAD.
- The plan file must exist on `main` before the implementation worktree is created.
- No production mock data, fake provider status, hardcoded quota, hardcoded usage, UI-only result, fake latency, fake official package state, or placeholder adapter.
- Tests may use deterministic local fixtures only for parser, schema, and error-classification tests. Fixtures must not be rendered as product data and must not replace integration paths.
- Connectivity probing must call the real provider runtime path for the selected saved provider config.
- Connectivity probing is diagnostic. It may consume provider quota, but it must not be merged into normal model usage totals. If provider usage is returned during probe, store or display it only as diagnostic probe usage.
- Connectivity probing and official quota refresh are real provider-network actions. They must be per-`configId` single-flight operations in the backend, expose pending state in React, and prevent repeated clicks from launching concurrent provider calls for the same config.
- Official quota/package rows must be backed by a real provider account API when supported. If the provider exposes no usable official account API, return `unsupported` with source and reason.
- For every catalog provider, if an official account usage/quota API exists and is usable with credentials this product already stores or can explicitly request, Task 4 must implement a real adapter. If every provider is unsupported, Task 4 must record official evidence for each provider in a committed evidence file and lock those unsupported states with tests.
- Local usage summaries must include today, month-to-date, and all-time totals. Period boundaries are computed in the backend from workspace-local time and returned as UTC instants. Do not classify historical usage with one fixed current timezone offset across daylight-saving transitions.
- React must not read or persist API keys except through the existing explicit reveal-token flow.
- Raw provider credentials, provider-native error payloads, account identifiers, authorization headers, signed URLs, private absolute paths, and provider request bodies must not enter prompts, events, logs, traces, screenshots, snapshots, frontend state, or support payloads.
- Shared Rust contracts may use snake_case. Tauri IPC payloads consumed by React must use explicit camelCase command wrapper structs and tested conversions. Do not expose snake_case shared structs directly to React.
- Runtime JSON files added by this plan must follow the existing provider settings store safety pattern: no symlink components, create parent safely, write a 0600 temp file, `sync_all`, rename, and deterministic invalid JSON handling.
- Tauri commands remain thin IPC adapters. Business logic belongs in harness crates or focused backend service modules.
- Breaking refactors are allowed when they remove ambiguous ownership, duplicate state, or compatibility debt. Do not keep old UI or old command semantics as parallel authoritative paths.
- Do not use `git add .` in this plan. Stage explicit files after reviewing `git status --short` and `git diff --stat`.
- Every task must start with a written implementation analysis.
- Every task must end with a fresh subagent audit before commit. Tasks touching provider credentials, network calls, IPC commands, usage events, quota APIs, or route policy also require a security-review subagent.
- No task is complete until its task-specific tests, task-specific gate, `git diff --check`, and required audits pass.

## Required Worktree Setup

Implementation starts here. Run these commands from the original repository:

```bash
cd /Users/goya/Repo/Git/Jyowo
git branch --show-current
git status --short docs/superpowers/plans/2026-06-30-model-settings-redesign-implementation.md
git status --short
git branch --list goya/model-settings-redesign
git worktree add ../Jyowo-model-settings-redesign -b goya/model-settings-redesign main
cd ../Jyowo-model-settings-redesign
```

Expected:

- `git branch --show-current` prints `main`.
- The plan file exists on `main`.
- If `git status --short docs/superpowers/plans/2026-06-30-model-settings-redesign-implementation.md` prints output, stop and commit this plan on `main` first.
- `git branch --list goya/model-settings-redesign` prints nothing. If it prints a branch, run `git worktree list` and ask before reusing it.
- All implementation commands after setup run in `/Users/goya/Repo/Git/Jyowo-model-settings-redesign`.
- Do not run `git reset --hard`, `git checkout --`, or equivalent destructive commands against user changes.

## Mandatory Reading

Before Task 1, read these files in the implementation worktree:

```text
AGENTS.md
docs/frontend/agent-harness-frontend-development-guidelines.md
docs/frontend/frontend-product-ux.md
docs/frontend/frontend-engineering.md
docs/frontend/frontend-quality.md
docs/backend/agent-harness-backend-development-guidelines.md
docs/backend/backend-runtime.md
docs/backend/backend-engineering.md
docs/backend/backend-quality.md
docs/superpowers/plans/2026-06-30-model-settings-redesign-implementation.md
```

If any deeper `AGENTS.md` exists under a touched directory, read it before editing files in that directory.

## Per-Task Protocol

Before editing files for each task, write this analysis in the agent response:

```text
Task N analysis:
- Objective:
- Current code facts:
- Files to touch:
- Tests that must fail before implementation:
- Security and privacy constraints:
- Destructive refactor decision:
- What will not be changed:
```

Before marking a task complete:

1. Run the task-specific tests.
2. Run the task-specific gate.
3. Run `git diff --check`.
4. Write the exit analysis:

```text
Task N exit analysis:
- Implemented behavior:
- Removed old behavior:
- Tests added or changed:
- Gates run with exit code 0:
- Secret / provider payload / private path leakage check:
- Remaining unsupported cases and why they fail closed:
```

5. Dispatch a fresh code-review subagent with this exact prompt:

```text
Audit Task N in docs/superpowers/plans/2026-06-30-model-settings-redesign-implementation.md.

Review only this task's diff.
Check:
- The implementation fully satisfies Task N and does not invent extra scope.
- The product design remains model-matrix centered and route editing remains in Capability Routes.
- No production mock data, fake provider status, fake latency, hardcoded quota, hardcoded usage, or UI-only implementation was added.
- Rust remains the policy authority for connectivity, usage, quota, routing, credentials, and validation.
- React only renders backend state and sends user intent.
- Public payloads have Rust serde, frontend Zod schemas, and tests when IPC changed.
- Real provider-network actions are per-config single-flight operations and expose user-visible pending state where applicable.
- Official quota evidence covers the exact current catalog provider id set when Task 4 changed quota behavior.
- Official quota snapshots preserve non-empty official `sourceUrl` for every status except `notConfigured`, and unsupported/auth-required/failed states include safe messages.
- Tests cover loading, empty, error, and ready states when UI changed.
- No unrelated refactor, orphan import, unused state, or compatibility dead path remains.

Return PASS or FAIL.
For FAIL, include file path and line-level findings.
```

6. For tasks touching provider credentials, network calls, IPC commands, usage events, quota APIs, or route policy, dispatch a security-review subagent with this exact prompt:

```text
Security-audit Task N in docs/superpowers/plans/2026-06-30-model-settings-redesign-implementation.md.

Review only this task's diff.
Check:
- API keys and provider credentials cannot enter prompts, events, logs, traces, screenshots, snapshots, frontend state, or support payloads.
- Provider-native request and response payloads are not exposed to React.
- Error messages returned to the UI are safe summaries.
- Network and provider failures fail closed.
- Tauri command handlers validate payloads before touching runtime state.
- Capability route validation and credential resolution remain backend-owned.
- Usage and quota aggregation cannot cross workspace, tenant, or session scope.
- Official quota adapters do not use hardcoded account data or unofficial scraping.
- Official quota refresh and provider probing cannot be spammed into concurrent calls for the same saved config.
- Providers with official quota APIs are not marked unsupported merely because an extra safe credential field was missing before this plan.

Return PASS or FAIL.
For FAIL, include file path and line-level findings.
```

7. Persist the task audit before commit:

```text
docs/superpowers/audits/model-settings-redesign/task-N.md
```

Use this format:

```text
# Model Settings Redesign Task N Audit

## Current Audit Status
Code Review: PASS|FAIL
Security Review: PASS|FAIL|NOT_REQUIRED
Last Updated: YYYY-MM-DDTHH:MM:SSZ

## Task Analysis
<copy the Task N analysis>

## Exit Analysis
<copy the Task N exit analysis>

## Code Review Subagent
Result: PASS|FAIL
Findings:

## Security Review Subagent
Result: PASS|FAIL|NOT_REQUIRED
Findings:

## Gates
- <command>: exit 0
```

If an audit fails, fix the findings, rerun tests and gates, then rerun the same audit. Do not commit until required audits pass.

## IPC And Store Implementation Rules

Tauri command registration:

- Every new command must be exported from `apps/desktop/src-tauri/src/commands/mod.rs`.
- Every new command must be added to the existing `tauri::generate_handler!` list in `apps/desktop/src-tauri/src/lib.rs`.
- Every new command must have a camelCase request/response type in `apps/desktop/src-tauri/src/commands/contracts.rs`.
- Every new frontend command must go through `apps/desktop/src/shared/tauri/commands.ts` and `CommandClient`.
- Backend docs changed by this plan must name each new command and state whether it is metadata validation, provider network I/O, usage read, quota read, or route policy.

IPC naming:

- Shared `jyowo-harness-contracts` types may serialize with snake_case for Rust schema stability.
- IPC payloads consumed by React must serialize with camelCase.
- Add explicit `From` or conversion helpers between shared contract types and IPC payload types.
- Tests must cover both shapes: shared contract serde tests for snake_case and Tauri command serde/Zod tests for camelCase.

Runtime JSON store safety:

- `provider-diagnostics.json` and `provider-quota-cache.json` must use dedicated desktop store types.
- Store types must block symlink components before read, before parent creation, before temp creation, and before final rename.
- Store types must write to a unique temp file with mode `0o600` on Unix, call `sync_all`, then rename.
- Store types must define invalid JSON behavior. Use the existing provider settings pattern unless the task analysis gives a safer reason to fail closed.
- Store tests must cover missing file, invalid JSON, symlink path rejection, and successful atomic write.

## Target Backend Contracts

Create stable shared types in `crates/jyowo-harness-contracts/src/model_settings.rs` and re-export them from `crates/jyowo-harness-contracts/src/lib.rs`.

Target shape:

```rust
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CapabilityRouteKind, ModelRef, UsageSnapshot};

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProbeStatus {
    Online,
    Timeout,
    Unauthenticated,
    RateLimited,
    Unsupported,
    Failed,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProbeErrorKind {
    Timeout,
    Auth,
    RateLimit,
    Network,
    Provider,
    Unsupported,
    InvalidConfig,
    Unknown,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderProbeSnapshot {
    pub config_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub status: ProviderProbeStatus,
    pub timeout_ms: u64,
    pub latency_ms: Option<u64>,
    pub checked_at: DateTime<Utc>,
    pub error_kind: Option<ProviderProbeErrorKind>,
    pub safe_message: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelUsageBucket {
    pub key: String,
    pub provider_id: String,
    pub model_id: String,
    pub usage: UsageSnapshot,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelUsagePeriod {
    Today,
    MonthToDate,
    AllTime,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelUsageWindow {
    pub period: ModelUsagePeriod,
    pub period_start: Option<DateTime<Utc>>,
    pub period_end: Option<DateTime<Utc>>,
    pub total: UsageSnapshot,
    pub by_model: Vec<ModelUsageBucket>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelUsageSummary {
    pub timezone_id: Option<String>,
    pub timezone_offset_minutes: i32,
    pub today: ModelUsageWindow,
    pub month_to_date: ModelUsageWindow,
    pub all_time: ModelUsageWindow,
    pub generated_at: DateTime<Utc>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OfficialQuotaScope {
    Account,
    Project,
    Provider,
    Model,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OfficialQuotaStatus {
    Supported,
    Unsupported,
    NotConfigured,
    AuthRequired,
    Failed,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OfficialQuotaSnapshot {
    pub config_id: String,
    pub provider_id: String,
    pub model_id: Option<String>,
    pub scope: OfficialQuotaScope,
    pub status: OfficialQuotaStatus,
    pub period_start: Option<DateTime<Utc>>,
    pub period_end: Option<DateTime<Utc>>,
    pub quota_used: Option<u64>,
    pub quota_total: Option<u64>,
    pub quota_remaining: Option<u64>,
    pub unit: Option<String>,
    pub billing_label: Option<String>,
    pub source_url: String,
    pub fetched_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub is_stale: bool,
    pub safe_message: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityRouteHealth {
    pub kind: CapabilityRouteKind,
    pub config_id: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub probe: Option<ProviderProbeSnapshot>,
}
```

Field naming may be adjusted only if both Rust serde tests and frontend Zod tests prove the exact final wire shape.

Backend contract invariants:

- `ProviderProbeSnapshot` represents a completed probe attempt only. `checked_at` is required. A never-checked row is a frontend view-model state derived from absence of a snapshot, not a persisted backend snapshot.
- `OfficialQuotaSnapshot.fetched_at` records when the official provider state was checked. `expires_at` records when the backend cache entry stops being current. `is_stale` is computed by the backend response from `fetched_at`, `expires_at`, and command time.
- `OfficialQuotaSnapshot.source_url` is required and must be a non-empty official source URL for every status except `not_configured`. Unsupported, auth-required, failed, and supported snapshots must retain the official source that justifies the state.
- `OfficialQuotaSnapshot.safe_message` is required for `unsupported`, `auth_required`, and `failed` states. The message must be safe and must not include account ids, raw provider payloads, headers, or request bodies.
- `ModelUsageSummary.timezone_id` carries an IANA timezone id when the platform can resolve one. `timezone_offset_minutes` is the offset at `generated_at` for display only. Period membership must be computed with backend local-time conversion for each event or boundary, not by applying the current offset to all historical events.

## Target Frontend Data Model

Frontend domain view models live under `apps/desktop/src/features/settings/models/`.

Target view-model boundaries:

```text
provider config payload
  + provider catalog
  + probe snapshots
  + usage summary
  + quota snapshots
  + capability routes
    -> ModelAssetRow[]
    -> ModelSettingsSummaryView
    -> CapabilityRouteRow[]
```

Rules:

- `ModelAssetRow` represents a configured provider profile row and uses `configId` for actions.
- Usage buckets are keyed by `providerId/modelId` until backend usage events carry `configId`.
- If `configId`-accurate usage is added in this plan, the UI must display profile usage; otherwise labels must say "model usage" rather than "profile usage".
- If multiple configured profiles share the same `providerId/modelId`, they display the same model-level usage with a visible "shared model usage" label in the details drawer. Do not split or estimate profile usage.
- Probe and quota actions operate on `configId`.
- Probe and quota row actions display a pending state for the affected `configId` and do not dispatch another mutation for that `configId` until the current mutation settles.
- Quota/package rows are keyed by `configId`, but the displayed label must honor `OfficialQuotaScope`. Account/project/provider scoped quota must not be labeled as model-specific quota.
- React query hooks fetch backend data through `CommandClient` only.
- No feature component imports Tauri `invoke` directly.
- No feature component stores provider keys in React state except the existing reveal UI path.
- Provider settings and provider catalog failures block the page with a safe error state.
- Probe snapshot, usage summary, quota snapshot, or route option failures degrade only the affected summary metric, matrix column, drawer tab, or route section. Do not collapse the whole page unless the backend reports corrupted or unsafe state.

## Planned File Structure

Backend:

```text
crates/jyowo-harness-contracts/src/model_settings.rs
crates/jyowo-harness-contracts/tests/model_settings.rs
crates/jyowo-harness-model/src/diagnostics.rs
crates/jyowo-harness-model/src/account_usage.rs
crates/jyowo-harness-model/tests/diagnostics.rs
crates/jyowo-harness-model/tests/account_usage.rs
crates/jyowo-harness-observability/src/model_usage.rs
crates/jyowo-harness-observability/tests/model_usage.rs
apps/desktop/src-tauri/src/commands/model_settings.rs
apps/desktop/src-tauri/src/commands/providers.rs
apps/desktop/src-tauri/src/commands/contracts.rs
apps/desktop/src-tauri/src/commands/mod.rs
apps/desktop/src-tauri/src/lib.rs
apps/desktop/src-tauri/src/commands/runtime.rs
apps/desktop/src-tauri/src/commands/stores/mod.rs
apps/desktop/src-tauri/src/commands/tests.rs
docs/superpowers/audits/model-settings-redesign/official-quota-evidence.md
```

Frontend:

```text
apps/desktop/src/shared/tauri/commands.ts
apps/desktop/src/shared/tauri/commands.test.ts
apps/desktop/src/features/settings/SettingsPage.tsx
apps/desktop/src/features/settings/SettingsPage.test.tsx
apps/desktop/src/features/settings/ProviderSettingsForm.tsx
apps/desktop/src/features/settings/ProviderSettingsForm.test.tsx
apps/desktop/src/features/settings/models/ModelSettingsPage.tsx
apps/desktop/src/features/settings/models/ModelSettingsPage.test.tsx
apps/desktop/src/features/settings/models/ModelSettingsPage.stories.tsx
apps/desktop/src/features/settings/models/model-settings-view-model.ts
apps/desktop/src/features/settings/models/model-settings-view-model.test.ts
apps/desktop/src/features/settings/models/model-settings-queries.ts
apps/desktop/src/features/settings/models/ModelSummaryBand.tsx
apps/desktop/src/features/settings/models/ModelMatrix.tsx
apps/desktop/src/features/settings/models/ModelDetailsDrawer.tsx
apps/desktop/src/features/settings/models/ModelConfigDialog.tsx
apps/desktop/src/features/settings/models/CapabilityRoutesPanel.tsx
apps/desktop/src/features/settings/models/CapabilityRouteEditorDrawer.tsx
apps/desktop/e2e/model-settings-storybook.spec.ts
apps/desktop/playwright.storybook.config.ts
apps/desktop/src/shared/i18n/locales/en-US.ts
apps/desktop/src/shared/i18n/locales/zh-CN.ts
```

Docs:

```text
docs/frontend/frontend-quality.md
docs/backend/backend-engineering.md
docs/backend/backend-quality.md
docs/superpowers/audits/model-settings-redesign/task-N.md
```

Do not keep `ProviderSettingsForm` as the active model page after the redesign. It may be deleted or reduced to compatibility-free form subcomponents if doing so makes ownership clearer.

## Task 1: Shared Contracts For Model Settings State

**Files:**

- Create: `crates/jyowo-harness-contracts/src/model_settings.rs`
- Create: `crates/jyowo-harness-contracts/tests/model_settings.rs`
- Modify: `crates/jyowo-harness-contracts/src/lib.rs`
- Modify: `crates/jyowo-harness-contracts/src/schema_export.rs`

- [ ] **Step 1: Write failing contract tests**

Add tests that serialize and deserialize:

```text
ProviderProbeSnapshot
ModelUsageSummary
OfficialQuotaSnapshot
CapabilityRouteHealth
```

Expected shared contract wire names:

```json
{
  "config_id": "cfg-openai",
  "provider_id": "openai",
  "model_id": "gpt-4.1",
  "status": "online",
  "timeout_ms": 10000,
  "latency_ms": 812,
  "checked_at": "2026-06-30T00:00:00Z",
  "error_kind": null,
  "safe_message": null
}
```

Expected: `cargo test -p jyowo-harness-contracts model_settings -- --nocapture` fails because the module does not exist.

Additional contract test requirements:

- `ProviderProbeSnapshot.checked_at` is required during deserialize. Missing `checked_at` fails.
- `ProviderProbeStatus` does not include a persisted `never_checked` variant.
- `ModelUsageSummary` serializes `timezone_id` and `timezone_offset_minutes`.
- `OfficialQuotaSnapshot` serializes non-empty `source_url`, `fetched_at`, `expires_at`, and `is_stale`.
- Missing `fetched_at`, `expires_at`, or `is_stale` fails for `OfficialQuotaSnapshot`.
- Missing or empty `source_url` fails for `OfficialQuotaSnapshot` statuses other than `not_configured`.
- Missing `safe_message` fails for `OfficialQuotaSnapshot` statuses `unsupported`, `auth_required`, and `failed`.

- [ ] **Step 2: Add contract types**

Add the target backend contract types from the "Target Backend Contracts" section.

Rules:

- Shared contract serde remains snake_case.
- `ModelUsageSummary` must include `today`, `month_to_date`, `all_time`, `timezone_id`, `timezone_offset_minutes`, and `generated_at`.
- `OfficialQuotaSnapshot` must include `scope` so account/project/provider quota cannot be mislabeled as model quota.
- `OfficialQuotaSnapshot` must include freshness fields so cached quota cannot be shown as current without backend staleness classification.
- `OfficialQuotaSnapshot` must include a non-empty official `source_url` except when status is `not_configured`.
- `OfficialQuotaSnapshot` must include a safe message for `unsupported`, `auth_required`, and `failed`.
- Do not expose these shared structs directly as Tauri IPC payloads unless the command wrapper explicitly converts to camelCase and tests prove the final IPC shape.

- [ ] **Step 3: Export schemas**

Register the new contract types in `schema_export.rs` so docs/schema gates cover them.

- [ ] **Step 4: Run tests and gates**

Run:

```bash
cargo test -p jyowo-harness-contracts model_settings -- --nocapture
cargo test -p jyowo-harness-contracts schema -- --nocapture
pnpm check:backend-docs
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 5: Audit and commit**

Security review is not required unless the task also changes IPC command exposure.

```bash
git add crates/jyowo-harness-contracts/src/model_settings.rs \
  crates/jyowo-harness-contracts/tests/model_settings.rs \
  crates/jyowo-harness-contracts/src/lib.rs \
  crates/jyowo-harness-contracts/src/schema_export.rs \
  docs/superpowers/audits/model-settings-redesign/task-1.md
git commit -m "feat: add model settings contracts"
```

## Task 2: Real Provider Connectivity Probe

**Files:**

- Create: `crates/jyowo-harness-model/src/diagnostics.rs`
- Create: `crates/jyowo-harness-model/tests/diagnostics.rs`
- Create: `apps/desktop/src-tauri/src/commands/model_settings.rs`
- Modify: `crates/jyowo-harness-model/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands/providers.rs`
- Modify: `apps/desktop/src-tauri/src/commands/stores/mod.rs`
- Modify: `apps/desktop/src-tauri/src/commands/tests.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`

- [ ] **Step 1: Write failing backend tests**

Test requirements:

- `probe_provider_config` rejects an unknown `configId`.
- `probe_provider_config` rejects a config without API key.
- Probe uses saved `apiKey`, `baseUrl`, `providerId`, and `modelId`, not only catalog metadata.
- Probe maps timeout to `status = timeout`, `errorKind = timeout`, and includes `timeoutMs`.
- Probe maps auth failure to `status = unauthenticated` without returning provider-native body text.
- Probe persists the last safe snapshot under `.jyowo/runtime/provider-diagnostics.json`.
- Completed probe snapshots always include `checkedAt`; a never-checked state is represented by no snapshot.
- Probe persistence follows the runtime JSON store safety rules for missing file, invalid JSON, symlink rejection, and atomic write.
- Concurrent `probe_provider_config` calls for the same `configId` are single-flight and perform at most one provider runtime request.
- Concurrent probes for different `configId` values may run independently.
- Probe does not emit or persist normal `UsageAccumulatedEvent` records and does not change `ModelUsageSummary`.
- If the provider returns token usage during the probe, that value is classified as diagnostic probe usage only.

Expected: tests fail because the command and store do not exist.

- [ ] **Step 2: Add probe runner in model crate**

Add `ProviderProbeRunner` to `crates/jyowo-harness-model/src/diagnostics.rs`.

Required behavior:

- Build a `ModelRequest` with:
  - one user message
  - short harmless content such as `Respond with OK.`
  - `max_tokens = Some(8)`
  - `temperature = Some(0.0)`
  - `stream = true`
  - empty tools
- Use `InferContext` with `deadline = Some(Instant::now() + timeout)`.
- Drain the returned stream until `ModelStreamEvent::MessageStop` or `ModelStreamEvent::StreamError`.
- Treat `MessageStop` as success.
- Treat `StreamError` as provider failure and classify through `ErrorClass`.
- Use `tokio::time::timeout`.
- Return only safe error classification and safe message.
- Do not include prompt text, API key, headers, raw provider body, or provider request id in the returned snapshot.
- Run the request in a diagnostic context. The diagnostic context must suppress normal product usage accounting.
- Expose a safe boolean or safe usage summary only if needed to tell the UI that the probe consumed provider quota. Do not merge this into usage totals.

- [ ] **Step 3: Add desktop probe command**

Add Tauri command:

```text
probe_provider_config(config_id: String, timeout_ms?: u64) -> Result<ProbeProviderConfigResponse, CommandErrorPayload>
```

Rules:

- Default timeout: 10000 ms.
- Minimum timeout: 1000 ms.
- Maximum timeout: 60000 ms.
- Use a backend per-`configId` single-flight guard before constructing the provider request. Duplicate calls while a probe is in flight must await the same result or return a safe already-running error; they must not start a second provider request.
- Load config from `ProviderSettingsStore`.
- Use existing provider construction path. If helper functions must be refactored out of `providers.rs`, keep them backend-only and covered by tests.
- Persist last snapshot after every completed probe attempt, including failure.
- Persist snapshots only for completed attempts and always set `checked_at`.
- Persist through a dedicated `DesktopProviderDiagnosticsStore` that follows the runtime JSON store safety rules.
- Do not update provider settings or route settings from a probe.
- Keep `validate_provider_settings` as metadata validation only unless all call sites are migrated and tests prove no misleading UI remains.
- Add `probe_provider_config` and `list_provider_probe_snapshots` to `commands/mod.rs` and the `tauri::generate_handler!` list in `apps/desktop/src-tauri/src/lib.rs`.
- The command response structs live in `commands/contracts.rs`, use camelCase IPC serde, and convert from the shared snake_case contract types.

- [ ] **Step 4: Add frontend command schema**

In `apps/desktop/src/shared/tauri/commands.ts` add:

```text
probeProviderConfig({ configId, timeoutMs? })
listProviderProbeSnapshots()
```

Zod must validate exact camelCase IPC payloads. Tests must reject malformed status, negative latency, empty config id, missing `checkedAt`, unknown error kind, persisted `never_checked`, and snake_case backend-only field names.

- [ ] **Step 5: Run tests and gates**

Run:

```bash
cargo test -p jyowo-harness-model diagnostics -- --nocapture
cargo test -p jyowo-desktop-shell provider_probe -- --nocapture
pnpm -C apps/desktop test -- commands.test.ts
pnpm check:backend-docs
pnpm check:desktop
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 6: Audit and commit**

Code-review and security-review subagents are required.

```bash
git add crates/jyowo-harness-model/src/diagnostics.rs \
  crates/jyowo-harness-model/tests/diagnostics.rs \
  crates/jyowo-harness-model/src/lib.rs \
  apps/desktop/src-tauri/src/commands/model_settings.rs \
  apps/desktop/src-tauri/src/commands/contracts.rs \
  apps/desktop/src-tauri/src/commands/mod.rs \
  apps/desktop/src-tauri/src/lib.rs \
  apps/desktop/src-tauri/src/commands/providers.rs \
  apps/desktop/src-tauri/src/commands/stores/mod.rs \
  apps/desktop/src-tauri/src/commands/tests.rs \
  apps/desktop/src/shared/tauri/commands.ts \
  apps/desktop/src/shared/tauri/commands.test.ts \
  docs/superpowers/audits/model-settings-redesign/task-2.md
git commit -m "feat: add real provider connectivity probes"
```

## Task 3: Restart-Stable Model Usage Summary

**Files:**

- Create: `crates/jyowo-harness-observability/src/model_usage.rs`
- Create: `crates/jyowo-harness-observability/tests/model_usage.rs`
- Modify: `crates/jyowo-harness-observability/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands/model_settings.rs`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands/tests.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`

- [ ] **Step 1: Write failing usage aggregation tests**

Use real `UsageAccumulatedEvent` values. Tests must prove:

- All-time global totals aggregate `inputTokens`, `outputTokens`, `cacheReadTokens`, `cacheWriteTokens`, `toolCalls`, and `costMicros`.
- `byModel` groups by `providerId/modelId`.
- `lastUsedAt` is the latest usage event time for the model.
- Events without `modelRef` count toward total but not a model row.
- Zero usage events do not create empty rows.
- `today`, `monthToDate`, and `allTime` windows are all returned.
- Today and month-to-date boundaries use backend workspace-local time converted to UTC instants.
- Fixed-clock tests cover events just before and just after day and month boundaries.
- Fixed-clock tests cover a daylight-saving transition in a timezone with DST. The test must prove historical events are not classified by applying the current offset to every timestamp.
- Diagnostic probe usage is excluded from all three windows.

Expected: tests fail because `model_usage` does not exist.

- [ ] **Step 2: Add observability aggregation**

Implement a pure accumulator that accepts an iterator of `EventEnvelope` or `Event`, a fixed `now_utc`, and a backend-owned workspace timezone resolver, then returns `ModelUsageSummary`.

Rules:

- Do not read files directly inside the accumulator.
- Do not depend on desktop shell types.
- Do not include prompt, tool output, raw event JSON, or private paths in the summary.
- `today.period_start` and `today.period_end` must bound the current local day.
- `month_to_date.period_start` must be local month start and `month_to_date.period_end` must match the same end as today.
- `all_time.period_start` and `all_time.period_end` are `None`.
- The timezone resolver must use an IANA timezone id when available. If the platform cannot provide an IANA id, it must use backend local-time conversion for each event and boundary; it must not reuse the current offset for all historical timestamps.
- If duplicate provider configs share a `providerId/modelId`, aggregation remains model-level and does not estimate per-config usage.

- [ ] **Step 3: Add desktop command**

Add:

```text
get_model_usage_summary() -> Result<GetModelUsageSummaryResponse, CommandErrorPayload>
```

Rules:

- Read from the workspace event store through backend-owned journal APIs.
- Use tenant `TenantId::SINGLE` unless the desktop runtime has introduced explicit tenant selection.
- Aggregate persisted `UsageAccumulated` events so the result survives restart.
- If event reading fails, return a safe command error. Do not fall back to empty totals unless the workspace has no events.
- Do not aggregate from frontend run-ended events.
- Resolve workspace-local timezone in the backend at command execution time and include `timezoneId` plus `timezoneOffsetMinutes` in the IPC response. `timezoneOffsetMinutes` is the offset at `generatedAt` only.
- Add `get_model_usage_summary` to `commands/mod.rs` and the `tauri::generate_handler!` list in `apps/desktop/src-tauri/src/lib.rs`.
- The command response struct lives in `commands/contracts.rs`, uses camelCase IPC serde, and converts from the shared snake_case contract type.

- [ ] **Step 4: Add frontend schema**

Add `getModelUsageSummary()` to `CommandClient`. Zod schema must validate `today`, `monthToDate`, `allTime`, `timezoneId`, `timezoneOffsetMinutes`, `generatedAt`, and nested `total` / `byModel` values. Tests must reject the old all-time-only shape and summaries without timezone identity fields.

- [ ] **Step 5: Run tests and gates**

Run:

```bash
cargo test -p jyowo-harness-observability model_usage -- --nocapture
cargo test -p jyowo-desktop-shell model_usage_summary -- --nocapture
pnpm -C apps/desktop test -- commands.test.ts
pnpm check:backend-docs
pnpm check:desktop
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 6: Audit and commit**

Code-review and security-review subagents are required.

```bash
git add crates/jyowo-harness-observability/src/model_usage.rs \
  crates/jyowo-harness-observability/tests/model_usage.rs \
  crates/jyowo-harness-observability/src/lib.rs \
  apps/desktop/src-tauri/src/commands/model_settings.rs \
  apps/desktop/src-tauri/src/commands/contracts.rs \
  apps/desktop/src-tauri/src/commands/mod.rs \
  apps/desktop/src-tauri/src/lib.rs \
  apps/desktop/src-tauri/src/commands/tests.rs \
  apps/desktop/src/shared/tauri/commands.ts \
  apps/desktop/src/shared/tauri/commands.test.ts \
  docs/superpowers/audits/model-settings-redesign/task-3.md
git commit -m "feat: add model usage summary"
```

## Task 4: Official Provider Quota Framework

**Files:**

- Create: `crates/jyowo-harness-model/src/account_usage.rs`
- Create: `crates/jyowo-harness-model/tests/account_usage.rs`
- Modify: `crates/jyowo-harness-model/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands/model_settings.rs`
- Modify: `apps/desktop/src-tauri/src/commands/contracts.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands/stores/mod.rs`
- Modify: `apps/desktop/src-tauri/src/commands/tests.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`
- Create: `docs/superpowers/audits/model-settings-redesign/official-quota-evidence.md`

- [ ] **Step 1: Verify official provider APIs before coding**

Enumerate the current provider catalog from repository code before reading official provider documentation. Record the exact source used to enumerate provider ids.

For each provider id in that catalog, read official provider documentation on the implementation day. Record the result in both the Task 4 analysis and `docs/superpowers/audits/model-settings-redesign/official-quota-evidence.md`:

```text
catalog provider ids source:
catalog provider ids:

provider id:
official account usage/quota API:
official source URL:
accessed at:
required credential scope:
credential storage decision: existing/extend/not_safe
supported in this task: yes/no
reason:
```

Rules:

- Use only official provider documentation or official API references.
- Do not use blog posts, community snippets, dashboard scraping, browser automation against account dashboards, or inferred private endpoints.
- The evidence file must contain exactly one evidence row for every current catalog provider id and no stale rows for provider ids no longer in the catalog.
- If an official API exists and is usable with a credential type the product can store explicitly, implement the adapter in this task.
- If an official API exists but requires a separate organization/admin/project credential, first classify whether that credential can be safely requested, stored, redacted, and scoped through the existing provider settings security model.
- If the separate credential can be safely requested and stored, this task must extend the provider settings contract, command validation, redaction tests, UI configuration dialog, and account usage adapter to support it. Do not mark the provider unsupported only because the current settings form lacks that field.
- If Task 4 extends provider settings for extra safe quota credentials, its Task 4 analysis must list the exact additional files before editing, and its commit step must stage those exact files explicitly. Do not use `git add .`.
- If the separate credential cannot be safely requested or stored under the current security model, return `auth_required` or `unsupported` with the exact safe reason and source URL, and record the security-model blocker in the evidence file.
- If an official API is absent, return `unsupported` with the exact safe reason and source URL.
- If every provider is marked unsupported, the Task 4 audit must include the official evidence table and must fail if any provider had a usable official API.
- The Task 4 audit must compare the evidence file provider id set with the current catalog provider id set and fail on any missing, extra, or duplicate provider row.
- The Task 4 audit must fail if a provider was marked unsupported while its evidence row says `credential storage decision: extend`.

- [ ] **Step 2: Write failing quota framework tests**

Tests must prove:

- Provider with no official adapter returns `status = unsupported` and safe reason.
- Provider config with missing API key returns `status = not_configured`.
- Adapter auth failure returns `status = auth_required`.
- Adapter network/provider failure returns `status = failed`.
- Supported adapter response maps into `OfficialQuotaSnapshot` without provider-native payload.
- Unsupported, auth-required, failed, and supported snapshots all include non-empty official `sourceUrl`.
- Unsupported, auth-required, and failed snapshots include a safe non-empty message.
- Cache records include `fetchedAt`, `expiresAt`, `isStale`, and source URL.
- Cached quota older than the configured TTL or `expiresAt` returns `isStale = true`.
- Snapshot scope is preserved as `account`, `project`, `provider`, or `model`.
- Account/project/provider scoped quota is not serialized as model-scoped quota.
- Cache persistence follows the runtime JSON store safety rules for missing file, invalid JSON, symlink rejection, and atomic write.
- Concurrent `refresh_official_quota` calls for the same `configId` are single-flight and perform at most one provider account API request.
- Concurrent official quota refreshes for different `configId` values may run independently.

Expected: tests fail because account usage framework does not exist.

- [ ] **Step 3: Add account usage trait and adapter registry**

Add:

```rust
pub trait ProviderAccountUsageClient: Send + Sync {
    fn provider_id(&self) -> &str;
    fn source_url(&self) -> &'static str;
    async fn fetch_quota(
        &self,
        request: ProviderAccountUsageRequest,
    ) -> ProviderAccountUsageResult;
}
```

Rules:

- The trait returns normalized `OfficialQuotaSnapshot` data only.
- No provider-native JSON leaves the model crate.
- Unsupported providers must be explicit registry entries or explicit registry misses mapped to `unsupported`.
- Registry entries for providers with official APIs that require extra safe credentials must expose `auth_required` until the credential is configured, not `unsupported`.
- The framework must not hardcode quota values.
- Each adapter must set a cache freshness policy. If the provider response has a provider-native expiry, map it to `expires_at`; otherwise use a conservative backend TTL documented in code and tests.
- No placeholder adapter is allowed. An adapter is either real and documented, or absent and mapped to explicit unsupported/auth-required state.
- At least one real adapter is required when the official API verification proves a current catalog provider supports account usage/quota with available credentials.

- [ ] **Step 4: Add desktop commands**

Add:

```text
refresh_official_quota(config_id: String) -> Result<RefreshOfficialQuotaResponse, CommandErrorPayload>
list_official_quota_snapshots() -> Result<ListOfficialQuotaSnapshotsResponse, CommandErrorPayload>
```

Rules:

- Refresh is explicit user action or controlled query action. Do not refresh every render.
- Use a backend per-`configId` single-flight guard before calling an account usage adapter. Duplicate calls while refresh is in flight must await the same result or return a safe already-running error; they must not start a second provider account API request.
- Persist safe quota snapshots under `.jyowo/runtime/provider-quota-cache.json`.
- Cache must not contain API keys, account ids, provider-native payloads, headers, or request bodies.
- If an adapter is unsupported, persist the unsupported result so the UI can show a stable state.
- `list_official_quota_snapshots` must recompute `isStale` from command time and cached freshness fields before returning IPC data.
- Persist through a dedicated `DesktopProviderQuotaCacheStore` that follows the runtime JSON store safety rules.
- Add `refresh_official_quota` and `list_official_quota_snapshots` to `commands/mod.rs` and the `tauri::generate_handler!` list in `apps/desktop/src-tauri/src/lib.rs`.
- The command response structs live in `commands/contracts.rs`, use camelCase IPC serde, and convert from the shared snake_case contract types.

- [ ] **Step 5: Add frontend schema**

Add `refreshOfficialQuota` and `listOfficialQuotaSnapshots` to `CommandClient`. Zod must validate `scope`, non-empty `sourceUrl` for every status except `notConfigured`, `fetchedAt`, `expiresAt`, `isStale`, distinguish `unsupported` from `failed`, and reject missing source/freshness fields and snake_case backend-only field names.

- [ ] **Step 6: Run tests and gates**

Run:

```bash
cargo test -p jyowo-harness-model account_usage -- --nocapture
cargo test -p jyowo-desktop-shell official_quota -- --nocapture
pnpm -C apps/desktop test -- commands.test.ts
pnpm check:backend-docs
pnpm check:desktop
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 7: Audit and commit**

Code-review and security-review subagents are required.

```bash
git add crates/jyowo-harness-model/src/account_usage.rs \
  crates/jyowo-harness-model/tests/account_usage.rs \
  crates/jyowo-harness-model/src/lib.rs \
  apps/desktop/src-tauri/src/commands/model_settings.rs \
  apps/desktop/src-tauri/src/commands/contracts.rs \
  apps/desktop/src-tauri/src/commands/mod.rs \
  apps/desktop/src-tauri/src/lib.rs \
  apps/desktop/src-tauri/src/commands/stores/mod.rs \
  apps/desktop/src-tauri/src/commands/tests.rs \
  apps/desktop/src/shared/tauri/commands.ts \
  apps/desktop/src/shared/tauri/commands.test.ts \
  docs/superpowers/audits/model-settings-redesign/official-quota-evidence.md \
  docs/superpowers/audits/model-settings-redesign/task-4.md
git commit -m "feat: add official quota framework"
```

## Task 5: Model Settings View Model And Query Layer

**Files:**

- Create: `apps/desktop/src/features/settings/models/model-settings-view-model.ts`
- Create: `apps/desktop/src/features/settings/models/model-settings-view-model.test.ts`
- Create: `apps/desktop/src/features/settings/models/model-settings-queries.ts`

- [ ] **Step 1: Write failing view-model tests**

Tests must prove:

- Provider settings, catalog, probe snapshots, usage summary, quota snapshots, and routes merge into `ModelAssetRow[]`.
- Probe actions are keyed by `configId`.
- Usage display is keyed by `providerId/modelId`.
- Usage display reads `today`, `monthToDate`, and `allTime`; it never derives today/month from all-time totals in React.
- Duplicate configured profiles with the same `providerId/modelId` show shared model-level usage and do not invent per-profile usage.
- Quota display is keyed by `configId`.
- Quota display preserves `OfficialQuotaScope` and labels account/project/provider scope separately from model scope.
- Missing probe becomes `never_checked`.
- Unsupported quota displays unsupported with safe message.
- Default model is derived from backend `isDefault`, not frontend guesswork.
- Route rows group by `CapabilityRouteKind` and surface backend-provided unavailable reasons.
- Provider settings/catalog query failure returns a page-blocking error view model.
- Usage/probe/quota/route option query failure returns partial unavailable state for the affected section only.
- Probe and quota mutation state is tracked by `configId` so only the affected row/action becomes pending.
- Duplicate probe or quota mutation requests for the same `configId` are blocked in the query layer while the previous mutation is pending.

Expected: tests fail because the module does not exist.

- [ ] **Step 2: Implement pure view-model builders**

Rules:

- Pure functions only; no React, no query client, no Tauri invoke.
- No generated sample rows.
- No fallback fake latency or fake usage.
- Empty backend state returns empty rows and explicit empty summary.
- Empty usage summary must preserve the three real windows with zero totals.
- Partial query errors must be represented as typed view-model states, not thrown from render components.

- [ ] **Step 3: Implement query hooks**

Use TanStack Query wrappers for:

```text
listModelProviderCatalog
listProviderSettings
listProviderProbeSnapshots
getModelUsageSummary
listOfficialQuotaSnapshots
listProviderCapabilityRoutes
listProviderCapabilityRouteOptions
probeProviderConfig
refreshOfficialQuota
```

Rules:

- Hooks live in the feature directory.
- Query keys are stable constants.
- Mutations invalidate or update only affected queries.
- Probe and quota mutations expose per-`configId` pending state and block duplicate mutation dispatches for the same `configId` while pending.
- Feature leaf components do not import `CommandClient`.
- Query composition must preserve partial-data behavior from the pure view-model builder.

- [ ] **Step 4: Run tests and gates**

Run:

```bash
pnpm -C apps/desktop test -- model-settings-view-model.test.ts
pnpm check:desktop
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 5: Audit and commit**

Security review is required because the view model handles credential-adjacent provider state.

```bash
git add apps/desktop/src/features/settings/models/model-settings-view-model.ts \
  apps/desktop/src/features/settings/models/model-settings-view-model.test.ts \
  apps/desktop/src/features/settings/models/model-settings-queries.ts \
  docs/superpowers/audits/model-settings-redesign/task-5.md
git commit -m "feat: add model settings view model"
```

## Task 6: Model Matrix Page

**Files:**

- Create: `apps/desktop/src/features/settings/models/ModelSettingsPage.tsx`
- Create: `apps/desktop/src/features/settings/models/ModelSettingsPage.test.tsx`
- Create: `apps/desktop/src/features/settings/models/ModelSettingsPage.stories.tsx`
- Create: `apps/desktop/src/features/settings/models/ModelSummaryBand.tsx`
- Create: `apps/desktop/src/features/settings/models/ModelMatrix.tsx`
- Create: `apps/desktop/e2e/model-settings-storybook.spec.ts`
- Modify: `apps/desktop/src/features/settings/SettingsPage.tsx`
- Modify: `apps/desktop/src/features/settings/SettingsPage.test.tsx`
- Modify: `apps/desktop/playwright.storybook.config.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`

- [ ] **Step 1: Write failing UI tests**

Tests must cover:

- Loading state for combined model settings queries.
- Empty state when no provider configs exist.
- Error state with safe backend error message.
- Ready state with summary band and matrix rows.
- Filtering by provider, health status, default only, failing only, and search.
- Matrix row action calls probe mutation with `configId`.
- Probe and quota row actions show pending state for the affected `configId` and ignore repeat clicks while pending.
- Matrix displays today, month-to-date, and total usage from backend view-model state.
- Matrix distinguishes unsupported official quota from failed official quota.
- Partial usage/probe/quota failures show unavailable state only in the affected metric, column, or row action.
- No API key or raw provider payload appears in the rendered DOM.

Expected: tests fail because the new page does not exist and Settings still renders `ProviderSettingsForm`.

- [ ] **Step 2: Build the matrix-centered page**

Rules:

- First viewport is the summary band, filters, and model matrix.
- Create/edit provider configuration is not a permanent right-side form.
- Implement the required `Models` layout specification from the Product Design section exactly.
- Desktop matrix columns follow the specified order and priority.
- Narrow layout keeps identity, provider, health, timeout threshold, and primary action visible without horizontal overflow.
- Use existing `shared/ui` primitives.
- Use lucide icons where buttons need icons.
- Cover loading, empty, error, and ready states.
- Use restrained settings-page styling. No landing-page hero, decorative gradients, nested cards, or explanatory marketing copy.

- [ ] **Step 3: Wire Settings tab**

Replace the active Models tab content with `ModelSettingsPage`.

Rules:

- Keep Settings page tab structure.
- Do not retain `ProviderSettingsForm` as an active page.
- If `ProviderSettingsForm` is partially reused, rename/split it into focused components in `features/settings/models`.

- [ ] **Step 4: Add Storybook states and visual checks**

Stories must cover loading, empty, ready with mixed statuses, error, unsupported quota, and narrow layout.

Add a Playwright Storybook spec that:

- opens the ready, partial-data, empty, and narrow stories
- checks screenshots are nonblank at 1440x900, 1024x768, and 390x844
- asserts no visible text overlaps or horizontal page overflow
- asserts the first viewport contains summary, filters, and matrix/list content
- asserts the old permanent provider form is not visible

Update `apps/desktop/playwright.storybook.config.ts` so `testMatch` includes both the existing `conversation-evidence-storybook.spec.ts` and the new `model-settings-storybook.spec.ts`. `pnpm -C apps/desktop test:e2e:storybook` must execute the new spec; do not rely on a file that is outside the configured match pattern.

- [ ] **Step 5: Run tests and gates**

Run:

```bash
pnpm -C apps/desktop test -- ModelSettingsPage.test.tsx SettingsPage.test.tsx
pnpm -C apps/desktop build-storybook
pnpm -C apps/desktop test:e2e:storybook
pnpm check:desktop
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 6: Audit and commit**

Code-review subagent is required. Security-review subagent is required because the page renders provider configuration state.

```bash
git add apps/desktop/src/features/settings/models/ModelSettingsPage.tsx \
  apps/desktop/src/features/settings/models/ModelSettingsPage.test.tsx \
  apps/desktop/src/features/settings/models/ModelSettingsPage.stories.tsx \
  apps/desktop/src/features/settings/models/ModelSummaryBand.tsx \
  apps/desktop/src/features/settings/models/ModelMatrix.tsx \
  apps/desktop/e2e/model-settings-storybook.spec.ts \
  apps/desktop/src/features/settings/SettingsPage.tsx \
  apps/desktop/src/features/settings/SettingsPage.test.tsx \
  apps/desktop/playwright.storybook.config.ts \
  apps/desktop/src/shared/i18n/locales/en-US.ts \
  apps/desktop/src/shared/i18n/locales/zh-CN.ts \
  docs/superpowers/audits/model-settings-redesign/task-6.md
git commit -m "feat: add model matrix settings page"
```

## Task 7: Model Details Drawer And Configuration Dialog

**Files:**

- Create: `apps/desktop/src/features/settings/models/ModelDetailsDrawer.tsx`
- Create: `apps/desktop/src/features/settings/models/ModelConfigDialog.tsx`
- Create: `apps/desktop/src/features/settings/models/ModelDetailsDrawer.test.tsx`
- Create: `apps/desktop/src/features/settings/models/ModelConfigDialog.test.tsx`
- Modify: `apps/desktop/src/features/settings/models/ModelSettingsPage.tsx`
- Modify: `apps/desktop/src/features/settings/models/ModelSettingsPage.stories.tsx`
- Modify: `apps/desktop/src/features/settings/ProviderSettingsForm.tsx`
- Modify: `apps/desktop/src/features/settings/ProviderSettingsForm.test.tsx`
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`

- [ ] **Step 1: Write failing drawer/dialog tests**

Tests must cover:

- Drawer tabs: overview, connectivity, usage, official quota, configuration, capabilities.
- Connectivity tab shows last probe result, timeout threshold, latency, checked time, and safe error.
- Usage tab shows today, month-to-date, and all-time model usage totals and labels them as model-level usage unless config-level usage exists.
- Usage tab shows a shared model-usage note when multiple configured profiles share `providerId/modelId`.
- Official quota tab shows supported, unsupported, failed, auth required, and not configured states with the correct quota scope label.
- Configuration tab shows API key presence only and preserves explicit reveal flow.
- Config dialog saves through `saveProviderSettings`.
- Config dialog never displays raw API key unless the existing reveal-token flow is used.

Expected: tests fail because components do not exist.

- [ ] **Step 2: Build drawer and dialog**

Rules:

- Details are secondary. The matrix remains the default page focus.
- Editing provider, model id, base URL, display name, and API key happens in a dialog or drawer edit mode.
- Reveal API key flow must reuse existing token command behavior.
- Do not store revealed key outside the reveal component state.
- Clearing or switching selected model must clear revealed key state.

- [ ] **Step 3: Remove or reduce old form**

Delete `ProviderSettingsForm.tsx` if all behavior moved into focused components. If a smaller form component remains, it must not own data fetching, matrix layout, capability routing, or query orchestration.

- [ ] **Step 4: Run tests and gates**

Run:

```bash
pnpm -C apps/desktop test -- ModelDetailsDrawer.test.tsx ModelConfigDialog.test.tsx ProviderSettingsForm.test.tsx
pnpm check:desktop
git diff --check
```

Expected: all commands exit 0. If `ProviderSettingsForm.test.tsx` is deleted because the file is deleted, run the replacement tests and document that deletion in Task 7 exit analysis.

- [ ] **Step 5: Audit and commit**

Code-review and security-review subagents are required.

```bash
git add apps/desktop/src/features/settings/models/ModelDetailsDrawer.tsx \
  apps/desktop/src/features/settings/models/ModelConfigDialog.tsx \
  apps/desktop/src/features/settings/models/ModelDetailsDrawer.test.tsx \
  apps/desktop/src/features/settings/models/ModelConfigDialog.test.tsx \
  apps/desktop/src/features/settings/models/ModelSettingsPage.tsx \
  apps/desktop/src/features/settings/models/ModelSettingsPage.stories.tsx \
  apps/desktop/src/features/settings/ProviderSettingsForm.tsx \
  apps/desktop/src/features/settings/ProviderSettingsForm.test.tsx \
  apps/desktop/src/shared/i18n/locales/en-US.ts \
  apps/desktop/src/shared/i18n/locales/zh-CN.ts \
  docs/superpowers/audits/model-settings-redesign/task-7.md
git commit -m "feat: add model details and config editing"
```

## Task 8: Capability Routes As A Separate Product Surface

**Files:**

- Create: `apps/desktop/src/features/settings/models/CapabilityRoutesPanel.tsx`
- Create: `apps/desktop/src/features/settings/models/CapabilityRouteEditorDrawer.tsx`
- Create: `apps/desktop/src/features/settings/models/CapabilityRoutesPanel.test.tsx`
- Create: `apps/desktop/src/features/settings/models/CapabilityRouteEditorDrawer.test.tsx`
- Modify: `apps/desktop/src/features/settings/models/ModelSettingsPage.tsx`
- Modify: `apps/desktop/src/features/settings/models/ModelDetailsDrawer.tsx`
- Modify: `apps/desktop/src/features/settings/models/model-settings-view-model.ts`
- Modify: `apps/desktop/src/features/settings/models/model-settings-view-model.test.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`

- [ ] **Step 1: Write failing route UI tests**

Tests must cover:

- Settings > Models has `Models` and `Capability Routes` sub-tabs.
- Capability route table lists image generation, video generation, speech to text, text to speech, and music generation route kinds when available from backend options.
- Existing configured routes show selected profile, execution mode, cost risk, and health.
- Unconfigured route kind shows configure action.
- Editor drawer lists eligible targets and disabled unavailable targets with backend reason.
- Save calls `saveProviderCapabilityRoute`.
- Clear calls `deleteProviderCapabilityRoute`.
- Model detail drawer only displays route bindings and shortcuts; it does not contain the full route editor table.

Expected: tests fail because route UI is still embedded in the old provider form or missing.

- [ ] **Step 2: Move route editing into `CapabilityRoutesPanel`**

Rules:

- Use existing route commands and backend route validation.
- Do not let React infer runtime support from catalog alone.
- Use `listProviderCapabilityRouteOptions` for candidate targets and unavailable reasons.
- Show probe health if a selected route target has a probe snapshot.
- Do not add fallback chains or priority routing. Current backend contract allows one enabled target per `CapabilityRouteKind`.

- [ ] **Step 3: Add model detail read-only route binding**

Rules:

- The model detail capabilities tab may show which route kinds target the selected config.
- A shortcut such as "Use for image generation" may open the route editor drawer.
- The route editor remains the only full editor.

- [ ] **Step 4: Run tests and gates**

Run:

```bash
pnpm -C apps/desktop test -- CapabilityRoutesPanel.test.tsx CapabilityRouteEditorDrawer.test.tsx model-settings-view-model.test.ts
pnpm check:desktop
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 5: Audit and commit**

Code-review and security-review subagents are required.

```bash
git add apps/desktop/src/features/settings/models/CapabilityRoutesPanel.tsx \
  apps/desktop/src/features/settings/models/CapabilityRouteEditorDrawer.tsx \
  apps/desktop/src/features/settings/models/CapabilityRoutesPanel.test.tsx \
  apps/desktop/src/features/settings/models/CapabilityRouteEditorDrawer.test.tsx \
  apps/desktop/src/features/settings/models/ModelSettingsPage.tsx \
  apps/desktop/src/features/settings/models/ModelDetailsDrawer.tsx \
  apps/desktop/src/features/settings/models/model-settings-view-model.ts \
  apps/desktop/src/features/settings/models/model-settings-view-model.test.ts \
  apps/desktop/src/shared/i18n/locales/en-US.ts \
  apps/desktop/src/shared/i18n/locales/zh-CN.ts \
  docs/superpowers/audits/model-settings-redesign/task-8.md
git commit -m "feat: separate capability route management"
```

## Task 9: Remove Misleading Validation UX And Old Technical Debt

**Files:**

- Modify: `apps/desktop/src/features/settings/models/ModelSettingsPage.tsx`
- Modify: `apps/desktop/src/features/settings/models/ModelMatrix.tsx`
- Modify: `apps/desktop/src/features/settings/models/ModelDetailsDrawer.tsx`
- Modify: `apps/desktop/src/features/settings/ProviderSettingsForm.tsx`
- Modify: `apps/desktop/src/features/settings/ProviderSettingsForm.test.tsx`
- Modify: `apps/desktop/src/shared/i18n/locales/en-US.ts`
- Modify: `apps/desktop/src/shared/i18n/locales/zh-CN.ts`
- Modify: `docs/frontend/frontend-quality.md`
- Modify: `docs/backend/backend-engineering.md`
- Modify: `docs/backend/backend-quality.md`

- [ ] **Step 1: Write failing regression tests**

Tests must prove:

- No visible UI label claims metadata validation is a connectivity check.
- The primary check action uses `probeProviderConfig`.
- `validateProviderSettings` is used only for metadata validation during save/create flows if still needed.
- No old route section remains below the model matrix.
- Storybook and tests reference the new component names.

- [ ] **Step 2: Delete or rewrite old paths**

Rules:

- Remove old "test" button behavior that calls `validateProviderSettings`.
- Remove dead route rows from `ProviderSettingsForm`.
- Remove unused imports, old query keys, stale translation strings, and orphan tests.
- Do not keep old UI behind a flag.

- [ ] **Step 3: Update active docs**

Update docs only where the project docs require it:

- `docs/frontend/frontend-quality.md`: replace ProviderSettingsForm coverage with ModelSettingsPage, ModelMatrix, ModelDetailsDrawer, and CapabilityRoutesPanel coverage.
- `docs/backend/backend-engineering.md`: document new commands and clarify `validate_provider_settings` remains metadata-only.
- `docs/backend/backend-quality.md`: document tests for provider probe, model usage summary, and official quota command behavior.

- [ ] **Step 4: Run tests and gates**

Run:

```bash
pnpm -C apps/desktop test -- ModelSettingsPage.test.tsx ModelDetailsDrawer.test.tsx CapabilityRoutesPanel.test.tsx
pnpm check:frontend-docs
pnpm check:backend-docs
pnpm check:docs
pnpm check:desktop
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 5: Audit and commit**

Code-review and security-review subagents are required.

```bash
git add apps/desktop/src/features/settings/models/ModelSettingsPage.tsx \
  apps/desktop/src/features/settings/models/ModelMatrix.tsx \
  apps/desktop/src/features/settings/models/ModelDetailsDrawer.tsx \
  apps/desktop/src/features/settings/ProviderSettingsForm.tsx \
  apps/desktop/src/features/settings/ProviderSettingsForm.test.tsx \
  apps/desktop/src/shared/i18n/locales/en-US.ts \
  apps/desktop/src/shared/i18n/locales/zh-CN.ts \
  docs/frontend/frontend-quality.md \
  docs/backend/backend-engineering.md \
  docs/backend/backend-quality.md \
  docs/superpowers/audits/model-settings-redesign/task-9.md
git commit -m "refactor: remove legacy model settings form flow"
```

## Task 10: Full Verification And Final Review

**Files:**

- Modify: only files needed to fix findings from full verification.

- [ ] **Step 1: Run full gates**

Run:

```bash
pnpm check:docs
pnpm check:agent-docs
pnpm check:frontend-docs
pnpm check:backend-docs
pnpm check:desktop
pnpm check:rust
pnpm check
pnpm -C apps/desktop test:e2e:storybook
pnpm check:desktop:full
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 2: Run targeted searches**

Run:

```bash
rg -n "validateProviderSettings\\(|validate_provider_settings" apps/desktop/src apps/desktop/src-tauri/src crates -g '!**/*.test.*'
rg -n "mock|fake|hardcoded|placeholder|sample quota|sample usage|TO[D]O|T[B]D" apps/desktop/src/features/settings apps/desktop/src-tauri/src/commands crates/jyowo-harness-model crates/jyowo-harness-observability
rg -n "apiKey|api_key|Authorization|Bearer|provider-native|raw provider" apps/desktop/src/features/settings apps/desktop/src-tauri/src/commands crates/jyowo-harness-model crates/jyowo-harness-observability
rg -n "config_id|provider_id|model_id|timeout_ms|checked_at|month_to_date|all_time" apps/desktop/src/features/settings apps/desktop/src/shared/tauri -g '!**/*.test.*'
rg -n "provider-diagnostics.json|provider-quota-cache.json|ensure_no_symlink_components|sync_all|rename\\(" apps/desktop/src-tauri/src/commands apps/desktop/src-tauri/src/lib.rs
rg -n "testMatch|model-settings-storybook|conversation-evidence-storybook" apps/desktop/playwright.storybook.config.ts apps/desktop/e2e
rg -n "single-flight|singleflight|in_flight|isPending|pendingByConfigId|sourceUrl|source_url|expiresAt|isStale|timezoneId" apps/desktop/src apps/desktop/src-tauri/src crates/jyowo-harness-model crates/jyowo-harness-observability
```

Expected:

- `validate_provider_settings` appears only in backend command definition, metadata-validation tests, and save/create validation paths.
- Search hits for banned words are reviewed and either removed or documented as comments/tests that do not violate the plan.
- No secret leakage path is found.
- React-facing code does not consume snake_case model settings IPC fields.
- Diagnostics and quota cache stores use symlink checks and atomic writes.
- Storybook Playwright `testMatch` includes both existing and model-settings storybook specs.
- Single-flight, pending-state, quota source URL, quota staleness, and timezone identity code paths are present and covered by tests.

- [ ] **Step 3: Manual product verification**

Start desktop dev environment through the repo's existing desktop workflow. Verify:

- Settings > Models opens with summary and matrix as the first useful content.
- Desktop first viewport shows summary, filters, and matrix in the required order.
- Narrow viewport has no horizontal page overflow and keeps identity, health, timeout, and primary actions visible.
- Empty provider state is usable.
- A saved provider profile row can be selected.
- Probe action calls backend probe command and row updates with last snapshot.
- Repeated probe or quota clicks on the same row while pending do not launch duplicate provider calls.
- Probe usage does not change normal today/month/all-time usage totals.
- Details drawer shows overview, connectivity, usage, quota, configuration, and capabilities.
- Usage shows today, month-to-date, and all-time data from backend-owned summary.
- Usage period boundaries are correct for the workspace timezone, including daylight-saving transitions covered by tests.
- API key reveal requires explicit reveal action and clears on selection change.
- Capability Routes sub-tab edits routes without exposing route editor inside model details.
- Unsupported official quota is visibly distinct from failed quota fetch.
- Stale official quota cache is visibly distinct from current fetched quota.
- Account/project/provider scoped quota is not labeled as model-specific quota.

- [ ] **Step 4: Final subagent audit**

Dispatch a fresh review subagent with this exact prompt:

```text
Final audit for docs/superpowers/plans/2026-06-30-model-settings-redesign-implementation.md.

Review the full branch diff.
Check:
- The final product matches the model matrix + capability route design.
- Every task audit file exists and records PASS for required audits.
- There is no production mock data, fake status, fake quota, fake usage, or UI-only implementation.
- Connectivity probes call real provider runtime paths by configId.
- Connectivity probes and official quota refreshes are per-config single-flight operations with row-level pending UI.
- Connectivity probe usage is diagnostic and does not pollute normal usage summary.
- Usage summary is backend-owned, restart-stable, includes today/month-to-date/all-time windows, and does not classify historical events with one fixed current timezone offset.
- Official quota framework uses real official APIs when available or returns explicit unsupported/auth-required states with committed source evidence covering the exact current catalog provider id set.
- Official quota framework extends safe credential storage when official APIs require a separately storable credential; it does not downgrade those providers to unsupported.
- Official quota cache exposes freshness and stale state.
- Official quota snapshots carry non-empty official source URLs and safe messages for unsupported/auth-required/failed states.
- Tauri IPC payloads consumed by React are camelCase wrappers, not direct snake_case shared contracts.
- Runtime JSON stores added by this plan use symlink checks and atomic writes.
- Playwright/Storybook visual checks prove the layout works at desktop and narrow viewports.
- React does not make final provider, route, secret, quota, or usage decisions.
- API keys and provider-native payloads cannot leak to frontend state, logs, traces, snapshots, or docs.
- Docs and gates are updated.

Return PASS or FAIL.
For FAIL, include file path and line-level findings.
```

Dispatch security-review subagent with the Task security prompt, scoped to the full branch.

- [ ] **Step 5: Final commit**

If full verification required fixes after Task 9, commit them:

```bash
git status --short
git diff --stat
git add <explicit files changed by full verification fixes>
git commit -m "chore: verify model settings redesign"
```

If no changes remain after full verification:

```bash
git status --short
```

Expected: clean working tree.

## Completion Criteria

The work is complete only when all of these are true:

- Implementation happened in `/Users/goya/Repo/Git/Jyowo-model-settings-redesign`.
- Each task has a corresponding audit file under `docs/superpowers/audits/model-settings-redesign/`.
- Required code-review audits passed.
- Required security-review audits passed.
- Settings > Models defaults to the model matrix, not a provider form.
- Capability Routes is a separate sub-tab and owns route editing.
- Connectivity check calls real provider runtime paths using saved `configId`.
- Connectivity probe and official quota refresh are per-config single-flight operations and expose row-level pending state.
- Each model row displays last connectivity status, latency, and timeout threshold.
- Completed connectivity snapshots always include checked time; never-checked is derived from absence of a snapshot.
- Probe calls are marked diagnostic and do not pollute normal usage summary.
- Usage summary is read from backend-owned usage data, includes today/month-to-date/all-time windows, handles workspace timezone boundaries without fixed-offset DST errors, and survives restart.
- Official quota/package state is fetched from real official provider account APIs when available, or shown as unsupported/auth-required with committed source evidence covering the exact current catalog provider set.
- Official quota/package support extends safe credential storage when official APIs require separately storable credentials; providers are not marked unsupported only because the old config lacked that field.
- Official quota/package snapshots include non-empty official source URLs and safe messages for unsupported/auth-required/failed states.
- Official quota/package cache exposes fetched time, expiry, and stale state.
- Account/project/provider scoped quota is never mislabeled as model-scoped quota.
- Tauri IPC payloads consumed by React use tested camelCase wrappers.
- `provider-diagnostics.json` and `provider-quota-cache.json` stores pass symlink and atomic-write tests.
- Storybook Playwright checks pass for desktop, tablet, and narrow viewports.
- No production mock data, fake values, hardcoded quota, or placeholder adapter exists.
- API keys remain hidden except through the existing explicit reveal flow.
- `pnpm check`, `pnpm -C apps/desktop test:e2e:storybook`, and `pnpm check:desktop:full` exit 0.
- Final review and security-review subagents return PASS.
