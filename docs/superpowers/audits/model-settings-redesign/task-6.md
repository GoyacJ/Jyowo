# Model Settings Redesign Task 6 Audit

## Current Audit Status
Code Review: PASS
Security Review: PASS
Last Updated: 2026-06-30T13:23:51Z

## Task Analysis
Task 6 analysis:
- Objective: Replace the active Settings > Models provider form with a model-matrix control surface backed by the Task 5 frontend query and view-model layer.
- Current code facts: `SettingsPage.tsx` still rendered `ProviderSettingsForm`; Task 5 added `model-settings-view-model.ts` and `model-settings-queries.ts`; provider settings, probe snapshots, usage summary, quota snapshots, and pending mutation state were already exposed through `CommandClient`.
- Files to touch: `apps/desktop/src/features/settings/models/ModelSettingsPage.tsx`, `ModelSettingsPage.test.tsx`, `ModelSettingsPage.stories.tsx`, `ModelSummaryBand.tsx`, `ModelMatrix.tsx`, `apps/desktop/e2e/model-settings-storybook.spec.ts`, `apps/desktop/src/features/settings/SettingsPage.tsx`, `SettingsPage.test.tsx`, `apps/desktop/src/app/App.test.tsx`, `apps/desktop/playwright.storybook.config.ts`, `apps/desktop/src/features/settings/models/model-settings-view-model.ts`, and locale files.
- Tests that must fail before implementation: Model settings page and Settings tab tests should fail because the new page did not exist and Models still rendered the provider form.
- Security and privacy constraints: React must not render API keys, provider credentials, provider-native payloads, account identifiers, authorization headers, signed URLs, private paths, request bodies, or unsafe backend errors. Actions must pass only `configId` and safe parameters through `CommandClient`.
- Destructive refactor decision: The old `ProviderSettingsForm` remains in the codebase for now, but is removed from the active Models tab. No route editor, drawer, or provider edit form is added in Task 6.
- What will not be changed: No backend command, IPC contract, quota adapter, route policy, credential resolution, persistence, or provider runtime behavior changes are made in this task.

## Exit Analysis
Task 6 exit analysis:
- Implemented behavior: Settings > Models now renders a matrix-centered `ModelSettingsPage` with summary metrics, filters, desktop table, narrow dense list layout, row probe action, row quota refresh action, per-config pending state, loading, empty, error, ready, and partial-data states.
- Removed old behavior: The active Models tab no longer renders the permanent `ProviderSettingsForm`.
- Tests added or changed: Added `ModelSettingsPage.test.tsx`, `ModelSettingsPage.stories.tsx`, `model-settings-storybook.spec.ts`; updated Settings/App tests, Storybook Playwright match config, exported view-model page types, and model settings locale strings.
- Gates run with exit code 0: `pnpm -C apps/desktop test -- ModelSettingsPage.test.tsx SettingsPage.test.tsx`; `pnpm -C apps/desktop build-storybook`; `pnpm -C apps/desktop test:e2e:storybook`; `pnpm check:desktop`; `pnpm check:docs`; `git diff --check`.
- Secret / provider payload / private path leakage check: UI tests assert API keys and raw provider payload text are absent from DOM. Task 6 only consumes existing safe frontend command responses and does not add credential reveal, provider-native payload rendering, logging, tracing, support export, screenshots with secrets, or backend exposure.
- Remaining unsupported cases and why they fail closed: Failed probe, usage, and quota query fragments degrade affected UI columns or metrics to unavailable states. Provider settings or catalog failures block the page with a safe error state.

## Code Review Subagent
Result: PASS
Findings:

Checked Task 6 diff only.

Verified:
- Model page is matrix-centered.
- `ProviderSettingsForm` is no longer active in Models tab.
- No production mock/fake provider data added.
- React uses existing command/query layer and sends intent by `configId`.
- Pending state blocks repeat probe/quota clicks per config.
- Loading, empty, error, ready, partial-data states are covered.
- Storybook and Storybook e2e cover the new matrix surface.

Commands run:
- `pnpm -C apps/desktop test -- ModelSettingsPage.test.tsx SettingsPage.test.tsx --runInBand`
- `pnpm -C apps/desktop test -- model-settings-view-model.test.ts --runInBand`
- `pnpm -C apps/desktop build-storybook`
- `pnpm -C apps/desktop test:e2e:storybook`
- `pnpm check:desktop`
- `git diff --check`

## Security Review Subagent
Result: PASS
Findings:

No line-level security findings in Task 6 diff.

## Gates
- `pnpm -C apps/desktop test -- ModelSettingsPage.test.tsx SettingsPage.test.tsx`: exit 0
- `pnpm -C apps/desktop build-storybook`: exit 0
- `pnpm -C apps/desktop test:e2e:storybook`: exit 0
- `pnpm check:desktop`: exit 0
- `pnpm check:docs`: exit 0
- `git diff --check`: exit 0
