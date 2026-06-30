# Model Settings Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign Settings > Models from a provider configuration form into a model health, usage, quota, and capability-routing control center backed by real backend data.

**Architecture:** Rust remains the policy authority. Provider connectivity, usage aggregation, account quota retrieval, route validation, credential handling, redaction, and persistence live in the backend. React renders model asset state from typed Tauri commands, sends user intent, and never infers security, quota, provider support, or secret availability on its own.

**Tech Stack:** Rust 1.96, Tauri 2, React 19, TypeScript 6, Zod, TanStack Query, React Hook Form, Testing Library, Storybook, Vitest, `jyowo-harness-contracts`, `jyowo-harness-model`, `jyowo-harness-observability`, `jyowo-harness-journal`, `cargo test`, `pnpm check:rust`, `pnpm check:desktop`, `pnpm check:docs`, `pnpm check`.

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
  local usage for today / month / total
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
  month usage
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
- The plan file must exist on `main` before the implementation worktree is created.
- No production mock data, fake provider status, hardcoded quota, hardcoded usage, UI-only result, fake latency, fake official package state, or placeholder adapter.
- Tests may use deterministic local fixtures only for parser, schema, and error-classification tests. Fixtures must not be rendered as product data and must not replace integration paths.
- Connectivity probing must call the real provider runtime path for the selected saved provider config.
- Official quota/package rows must be backed by a real provider account API when supported. If the provider exposes no usable official account API, return `unsupported` with source and reason.
- React must not read or persist API keys except through the existing explicit reveal-token flow.
- Raw provider credentials, provider-native error payloads, account identifiers, authorization headers, signed URLs, private absolute paths, and provider request bodies must not enter prompts, events, logs, traces, screenshots, snapshots, frontend state, or support payloads.
- Tauri commands remain thin IPC adapters. Business logic belongs in harness crates or focused backend service modules.
- Breaking refactors are allowed when they remove ambiguous ownership, duplicate state, or compatibility debt. Do not keep old UI or old command semantics as parallel authoritative paths.
- Every task must start with a written implementation analysis.
- Every task must end with a fresh subagent audit before commit. Tasks touching provider credentials, network calls, IPC commands, usage events, quota APIs, or route policy also require a security-review subagent.
- No task is complete until its task-specific tests, task-specific gate, `git diff --check`, and required audits pass.

## Required Worktree Setup

Implementation starts here. Run these commands from the original repository:

```bash
cd /Users/goya/Repo/Git/Jyowo
git status --short docs/superpowers/plans/2026-06-30-model-settings-redesign-implementation.md
git status --short
git branch --list goya/model-settings-redesign
git worktree add ../Jyowo-model-settings-redesign -b goya/model-settings-redesign
cd ../Jyowo-model-settings-redesign
```

Expected:

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
    NeverChecked,
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
    pub checked_at: Option<DateTime<Utc>>,
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

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelUsageSummary {
    pub total: UsageSnapshot,
    pub by_model: Vec<ModelUsageBucket>,
    pub generated_at: DateTime<Utc>,
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
    pub model_id: String,
    pub status: OfficialQuotaStatus,
    pub period_start: Option<DateTime<Utc>>,
    pub period_end: Option<DateTime<Utc>>,
    pub quota_used: Option<u64>,
    pub quota_total: Option<u64>,
    pub quota_remaining: Option<u64>,
    pub unit: Option<String>,
    pub billing_label: Option<String>,
    pub source_url: Option<String>,
    pub fetched_at: Option<DateTime<Utc>>,
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
- Probe and quota actions operate on `configId`.
- React query hooks fetch backend data through `CommandClient` only.
- No feature component imports Tauri `invoke` directly.
- No feature component stores provider keys in React state except the existing reveal UI path.

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
apps/desktop/src-tauri/src/commands/runtime.rs
apps/desktop/src-tauri/src/commands/stores/mod.rs
apps/desktop/src-tauri/src/commands/tests.rs
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

Expected wire names:

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

- [ ] **Step 2: Add contract types**

Add the target backend contract types from the "Target Backend Contracts" section. Use Rust snake_case serde. Frontend conversion to camelCase remains owned by Tauri command payloads and Zod schemas.

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

- [ ] **Step 3: Add desktop probe command**

Add Tauri command:

```text
probe_provider_config(config_id: String, timeout_ms?: u64) -> Result<ProbeProviderConfigResponse, CommandErrorPayload>
```

Rules:

- Default timeout: 10000 ms.
- Minimum timeout: 1000 ms.
- Maximum timeout: 60000 ms.
- Load config from `ProviderSettingsStore`.
- Use existing provider construction path. If helper functions must be refactored out of `providers.rs`, keep them backend-only and covered by tests.
- Persist last snapshot after every completed probe attempt, including failure.
- Do not update provider settings or route settings from a probe.
- Keep `validate_provider_settings` as metadata validation only unless all call sites are migrated and tests prove no misleading UI remains.

- [ ] **Step 4: Add frontend command schema**

In `apps/desktop/src/shared/tauri/commands.ts` add:

```text
probeProviderConfig({ configId, timeoutMs? })
listProviderProbeSnapshots()
```

Zod must validate exact camelCase IPC payloads. Tests must reject malformed status, negative latency, empty config id, and unknown error kind.

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
- Modify: `apps/desktop/src-tauri/src/commands/tests.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`

- [ ] **Step 1: Write failing usage aggregation tests**

Use real `UsageAccumulatedEvent` values. Tests must prove:

- Global totals aggregate `inputTokens`, `outputTokens`, `cacheReadTokens`, `cacheWriteTokens`, `toolCalls`, and `costMicros`.
- `byModel` groups by `providerId/modelId`.
- `lastUsedAt` is the latest usage event time for the model.
- Events without `modelRef` count toward total but not a model row.
- Zero usage events do not create empty rows.

Expected: tests fail because `model_usage` does not exist.

- [ ] **Step 2: Add observability aggregation**

Implement a pure accumulator that accepts an iterator of `EventEnvelope` or `Event` and returns `ModelUsageSummary`.

Rules:

- Do not read files directly inside the accumulator.
- Do not depend on desktop shell types.
- Do not include prompt, tool output, raw event JSON, or private paths in the summary.

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

- [ ] **Step 4: Add frontend schema**

Add `getModelUsageSummary()` to `CommandClient`. Zod schema must validate `total`, `byModel`, and `generatedAt`.

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
- Modify: `apps/desktop/src-tauri/src/commands/tests.rs`
- Modify: `apps/desktop/src/shared/tauri/commands.ts`
- Modify: `apps/desktop/src/shared/tauri/commands.test.ts`

- [ ] **Step 1: Verify official provider APIs before coding**

For each provider in the current catalog, read official provider documentation on the implementation day. Record the result in the Task 4 analysis:

```text
provider id:
official account usage/quota API:
official source URL:
required credential scope:
supported in this task: yes/no
reason:
```

Rules:

- Use only official provider documentation or official API references.
- Do not use blog posts, community snippets, dashboard scraping, browser automation against account dashboards, or inferred private endpoints.
- If an official API is absent or requires unavailable account-scope credentials, return `unsupported`.

- [ ] **Step 2: Write failing quota framework tests**

Tests must prove:

- Provider with no official adapter returns `status = unsupported` and safe reason.
- Provider config with missing API key returns `status = not_configured`.
- Adapter auth failure returns `status = auth_required`.
- Adapter network/provider failure returns `status = failed`.
- Supported adapter response maps into `OfficialQuotaSnapshot` without provider-native payload.
- Cache records include `fetchedAt` and source URL.

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
- The framework must not hardcode quota values.

- [ ] **Step 4: Add desktop commands**

Add:

```text
refresh_official_quota(config_id: String) -> Result<RefreshOfficialQuotaResponse, CommandErrorPayload>
list_official_quota_snapshots() -> Result<ListOfficialQuotaSnapshotsResponse, CommandErrorPayload>
```

Rules:

- Refresh is explicit user action or controlled query action. Do not refresh every render.
- Persist safe quota snapshots under `.jyowo/runtime/provider-quota-cache.json`.
- Cache must not contain API keys, account ids, provider-native payloads, headers, or request bodies.
- If an adapter is unsupported, persist the unsupported result so the UI can show a stable state.

- [ ] **Step 5: Add frontend schema**

Add `refreshOfficialQuota` and `listOfficialQuotaSnapshots` to `CommandClient`.

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
  apps/desktop/src-tauri/src/commands/tests.rs \
  apps/desktop/src/shared/tauri/commands.ts \
  apps/desktop/src/shared/tauri/commands.test.ts \
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
- Quota display is keyed by `configId`.
- Missing probe becomes `never_checked`.
- Unsupported quota displays unsupported with safe message.
- Default model is derived from backend `isDefault`, not frontend guesswork.
- Route rows group by `CapabilityRouteKind` and surface backend-provided unavailable reasons.

Expected: tests fail because the module does not exist.

- [ ] **Step 2: Implement pure view-model builders**

Rules:

- Pure functions only; no React, no query client, no Tauri invoke.
- No generated sample rows.
- No fallback fake latency or fake usage.
- Empty backend state returns empty rows and explicit empty summary.

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
```

Rules:

- Hooks live in the feature directory.
- Query keys are stable constants.
- Mutations invalidate or update only affected queries.
- Feature leaf components do not import `CommandClient`.

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
- Modify: `apps/desktop/src/features/settings/SettingsPage.tsx`
- Modify: `apps/desktop/src/features/settings/SettingsPage.test.tsx`
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
- No API key or raw provider payload appears in the rendered DOM.

Expected: tests fail because the new page does not exist and Settings still renders `ProviderSettingsForm`.

- [ ] **Step 2: Build the matrix-centered page**

Rules:

- First viewport is the summary band, filters, and model matrix.
- Create/edit provider configuration is not a permanent right-side form.
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

- [ ] **Step 4: Add Storybook states**

Stories must cover loading, empty, ready with mixed statuses, error, unsupported quota, and narrow layout.

- [ ] **Step 5: Run tests and gates**

Run:

```bash
pnpm -C apps/desktop test -- ModelSettingsPage.test.tsx SettingsPage.test.tsx
pnpm -C apps/desktop build-storybook
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
  apps/desktop/src/features/settings/SettingsPage.tsx \
  apps/desktop/src/features/settings/SettingsPage.test.tsx \
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
- Usage tab shows model usage totals and labels them as model-level usage unless config-level usage exists.
- Official quota tab shows supported, unsupported, failed, auth required, and not configured states.
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
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 2: Run targeted searches**

Run:

```bash
rg -n "validateProviderSettings\\(|validate_provider_settings" apps/desktop/src apps/desktop/src-tauri/src crates -g '!**/*.test.*'
rg -n "mock|fake|hardcoded|placeholder|sample quota|sample usage|TO[D]O|T[B]D" apps/desktop/src/features/settings apps/desktop/src-tauri/src/commands crates/jyowo-harness-model crates/jyowo-harness-observability
rg -n "apiKey|api_key|Authorization|Bearer|provider-native|raw provider" apps/desktop/src/features/settings apps/desktop/src-tauri/src/commands crates/jyowo-harness-model crates/jyowo-harness-observability
```

Expected:

- `validate_provider_settings` appears only in backend command definition, metadata-validation tests, and save/create validation paths.
- Search hits for banned words are reviewed and either removed or documented as comments/tests that do not violate the plan.
- No secret leakage path is found.

- [ ] **Step 3: Manual product verification**

Start desktop dev environment through the repo's existing desktop workflow. Verify:

- Settings > Models opens with summary and matrix as the first useful content.
- Empty provider state is usable.
- A saved provider profile row can be selected.
- Probe action calls backend probe command and row updates with last snapshot.
- Details drawer shows overview, connectivity, usage, quota, configuration, and capabilities.
- API key reveal requires explicit reveal action and clears on selection change.
- Capability Routes sub-tab edits routes without exposing route editor inside model details.
- Unsupported official quota is visibly distinct from failed quota fetch.

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
- Usage summary is backend-owned and restart-stable.
- Official quota framework uses only official APIs or returns explicit unsupported states.
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
git add .
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
- Each model row displays last connectivity status, latency, and timeout threshold.
- Usage summary is read from backend-owned usage data and survives restart.
- Official quota/package state is either fetched from official provider account APIs or shown as unsupported with source and safe reason.
- No production mock data, fake values, hardcoded quota, or placeholder adapter exists.
- API keys remain hidden except through the existing explicit reveal flow.
- `pnpm check` exits 0.
- Final review and security-review subagents return PASS.
