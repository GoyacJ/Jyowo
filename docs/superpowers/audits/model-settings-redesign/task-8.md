# Model Settings Redesign Task 8 Audit

## Current Audit Status

Code Review: PASS
Security Review: PASS
Last Updated: 2026-06-30T16:09:47Z

## Task Analysis

Task 8 analysis:
- Objective: 把能力路由从模型详情/旧表单中拆成独立 `Capability Routes` 产品面。
- Current code facts: 当前 diff 已新增路由面板和编辑抽屉，模型详情只显示 route bindings。
- Files to touch: Task 8 列出的四个组件/测试、view-model、query hooks、i18n、计划与 audit 文件。
- Tests that must fail before implementation: 之前已写 route UI/view-model 测试并验证红灯，当前进入绿灯复验。
- Security and privacy constraints: 路由验证和候选目标来自 backend；React 不推断 runtime support，不接触 API key，不展示 provider-native payload。
- Destructive refactor decision: 不保留完整路由编辑器在详情抽屉内。
- What will not be changed: 不改 backend route contract，不加 fallback/priority routing。

## Exit Analysis

Task 8 exit analysis:
- Implemented behavior: Added `Models` and `Capability Routes` sub-tabs in `ModelSettingsPage`. Added `CapabilityRoutesPanel` as the full route table and `CapabilityRouteEditorDrawer` as the only full route editor. Route rows are built only from backend routes and `listProviderCapabilityRouteOptions`, show selected profile, execution mode, cost risk, probe health, eligible targets, unavailable targets, and selected operation IDs. Route loading, empty, error, unavailable, and ready states are distinct.
- Removed old behavior: Model details no longer contain a full route editor table. The capabilities tab only shows read-only route bindings and route shortcuts.
- Tests added or changed: Added route panel/editor tests for route kinds, configured/unconfigured rows, loading/empty/error states, eligible/unavailable targets, selected operation IDs, save, clear, and detail drawer read-only bindings. Added view-model coverage for route kind order, backend unavailable reasons, route bindings, and saved routes that become unavailable.
- Gates run with exit code 0: `pnpm -C apps/desktop test -- CapabilityRoutesPanel.test.tsx CapabilityRouteEditorDrawer.test.tsx model-settings-view-model.test.ts`; `pnpm check:desktop`; `git diff --check`.
- Secret / provider payload / private path leakage check: Task 8 adds no provider credential reads and no provider-network payload handling. Route save/delete uses existing backend-owned route validation through command wrappers. Rendered data is limited to backend-safe route options, saved route metadata, display names, operation IDs, probe health, and safe messages.
- Remaining unsupported cases and why they fail closed: If route queries are loading, failed, or unavailable, the route surface stays local to loading/error/unavailable states and does not invent route support. Unsupported target options remain disabled with backend reasons. Saved routes whose targets later become unavailable stay visible but are not made eligible for saving unless backend options report runtime support.

## Code Review Subagent

Result: PASS
Findings:

Initial code-review failed on missing selected operation ID display and missing empty-state coverage. Second review failed on collapsed loading/error states and saved routes becoming hidden when their target became unavailable. Fixes added operation ID display/tests, loading/error/empty/unavailable/ready route states, and saved-unavailable route visibility. Final code-review subagent returned PASS with no line-level findings.

## Security Review Subagent

Result: PASS
Findings:

Security review returned PASS. Re-review after route-state and saved-unavailable fixes also returned PASS with no line-level findings.

## Gates

- `pnpm -C apps/desktop test -- CapabilityRoutesPanel.test.tsx CapabilityRouteEditorDrawer.test.tsx model-settings-view-model.test.ts`: exit 0
- `pnpm check:desktop`: exit 0
- `git diff --check`: exit 0
