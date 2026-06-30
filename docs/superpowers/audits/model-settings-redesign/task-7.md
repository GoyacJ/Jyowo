# Model Settings Redesign Task 7 Audit

## Current Audit Status

Code Review: PASS
Security Review: PASS
Last Updated: 2026-06-30T15:16:10Z

## Task Analysis

Task 7 analysis:
- Objective: Add a secondary model detail drawer and a focused provider configuration dialog while keeping Settings > Models centered on the model matrix.
- Current code facts: Task 6 already renders `ModelSettingsPage`, `ModelSummaryBand`, and `ModelMatrix`. Provider settings, catalog, probe, usage, quota, and route state are already merged by the model settings view-model/query layer. `ProviderSettingsForm` still exists as the old provider-form surface.
- Files to touch: `ModelDetailsDrawer.tsx`, `ModelConfigDialog.tsx`, their tests, `ModelSettingsPage.tsx`, `ModelMatrix.tsx`, `model-settings-view-model.ts`, `ProviderSettingsForm.tsx`, `ProviderSettingsForm.test.tsx`, `ProviderSettingsForm.stories.tsx`, i18n locale files, the Task 7 checklist, and `workspace-scope.ts` for an orphan export cleanup exposed by reducing the old form.
- Tests that must fail before implementation: drawer/dialog component tests for required tabs, usage/quota/configuration/capability display, API key reveal behavior, and config save behavior; compatibility coverage for reduced `ProviderSettingsForm`.
- Security and privacy constraints: Raw provider keys may only appear through the existing explicit reveal-token flow. Revealed keys must stay bound to the currently selected config/generation, must clear when selection/open state changes, and typed keys must be cleared on save failure or close. React must not infer provider support, connectivity, quota, or credential validity.
- Destructive refactor decision: Reduce the old `ProviderSettingsForm` to a compatibility export of `ModelSettingsPage` so the old form no longer owns data fetching, matrix layout, capability routing, or query orchestration.
- What will not be changed: No backend IPC, provider runtime, quota adapter, probe single-flight, usage aggregation, or capability route policy changes.

## Exit Analysis

Task 7 exit analysis:
- Implemented behavior: Added `ModelDetailsDrawer` with overview, connectivity, usage, official quota, configuration, and capabilities tabs. Added `ModelConfigDialog` for provider/model/base URL/display name/API key edits through `saveProviderSettings`. Wired row details/edit actions into `ModelMatrix` and `ModelSettingsPage`, including add/edit dialog state. Drawer capabilities render backend catalog model descriptor data when present.
- Removed old behavior: `ProviderSettingsForm` no longer renders or owns the old provider form. It is reduced to a compatibility export of `ModelSettingsPage`; the old large provider-form test was replaced by a focused compatibility assertion. The old story was reduced to a model page wrapper with required providers.
- Tests added or changed: Added drawer tests for required tabs, connectivity, usage, quota states, API key reveal flow, stale reveal races, and capabilities. Added dialog tests for saving edits, not resubmitting unchanged keys, clearing typed keys on failure/close, provider/model switching, and preserving legacy model ids. Updated page tests for add dialog wiring and old form compatibility coverage.
- Gates run with exit code 0: `pnpm -C apps/desktop test -- ModelDetailsDrawer.test.tsx ModelConfigDialog.test.tsx ProviderSettingsForm.test.tsx`; `pnpm check:desktop`; `git diff --check`.
- Secret / provider payload / private path leakage check: The reveal path uses `requestProviderConfigApiKeyReveal` and `getProviderConfigApiKey` only. Revealed keys are stored with config id and generation, rendered only for the active config/generation, and stale async responses cannot fetch or render raw keys after row switch/close. Typed API keys are cleared on save success, save failure, and dialog close/cancel.
- Remaining unsupported cases and why they fail closed: Official quota support remains whatever Task 4 backend state reports. Unsupported/auth-required/failed quota states are rendered from backend view-model safe fields; Task 7 adds no quota adapter or UI-only quota state.

## Code Review Subagent

Result: PASS
Findings:

Initial code-review subagent returned PASS for Task 7.

## Security Review Subagent

Result: PASS
Findings:

Security review initially failed on stale reveal races and typed API key retention. Fixes added config/generation-bound reveal state, pre-key-fetch stale checks, render-time active config invalidation, and password clearing on save failure and close/cancel. Final security-review subagent returned PASS.

## Gates

- `pnpm -C apps/desktop test -- ModelDetailsDrawer.test.tsx ModelConfigDialog.test.tsx ProviderSettingsForm.test.tsx`: exit 0
- `pnpm check:desktop`: exit 0
- `git diff --check`: exit 0
