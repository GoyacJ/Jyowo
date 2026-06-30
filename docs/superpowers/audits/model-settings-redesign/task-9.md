# Model Settings Redesign Task 9 Audit

## Current Audit Status

Code Review: PASS
Security Review: PASS
Last Updated: 2026-06-30T16:23:35Z

## Task Analysis

Task 9 analysis:
- Objective: 移除把 metadata validation 表述成 connectivity check 的旧 UX，清理旧 ProviderSettingsForm/route 技术债，并更新活跃文档。
- Current code facts: `ModelSettingsPage` 已用 `probeProviderConfig` 做模型矩阵检查；`ProviderSettingsForm` 已在 Task 7 降为兼容导出，但旧文案、测试或 docs 可能仍引用旧表单/旧 test 语义。
- Files to touch: Task 9 列出的 model page/matrix/details、ProviderSettingsForm/test、i18n、frontend/backend docs、计划与 Task 9 audit；若搜索发现孤儿引用，只做必要删除。
- Tests that must fail before implementation: 回归测试应证明没有可见 UI 把 metadata validation 当 connectivity check，主检查动作走 `probeProviderConfig`，旧 route section 不再出现，storybook/tests 使用新组件名。
- Security and privacy constraints: 不改变 credential reveal 流程；不把 provider-native 错误或 API key 放入 UI、测试快照或 docs；backend 仍是 probe/route/validation 权威。
- Destructive refactor decision: 删除/改写旧路径，不保留旧 UI flag。
- What will not be changed: 不改 provider runtime、route persistence、quota adapter、usage aggregation或 IPC contract。

## Exit Analysis

Task 9 exit analysis:
- Implemented behavior: 设置页回归测试确认主检查动作调用 `probeProviderConfig`，且不会调用 `validateProviderSettings`。旧 `ProviderSettingsForm` 组件、测试和 story 已删除。活跃 Storybook 只保留 `ModelSettingsPage` stories。活跃 docs 已更新为 ModelSettingsPage、ModelMatrix、ModelDetailsDrawer、CapabilityRoutesPanel 和 backend probe/usage/quota/route 命令边界。
- Removed old behavior: 删除旧 ProviderSettingsForm 兼容导出、旧 story、旧测试、旧 provider capability routing 文案和 metadata check toast 文案。设置页不再有把 metadata validation 伪装成 connectivity check 的可见 UI。
- Tests added or changed: `ModelSettingsPage.test.tsx` 增加无旧 Check/metadata 文案和 probe-vs-validate 回归断言。
- Gates run with exit code 0: `pnpm -C apps/desktop test -- ModelSettingsPage.test.tsx ModelDetailsDrawer.test.tsx CapabilityRoutesPanel.test.tsx`; `pnpm check:frontend-docs`; `pnpm check:backend-docs`; `pnpm check:docs`; `pnpm check:desktop`; `git diff --check`.
- Secret / provider payload / private path leakage check: Task 9 只删除旧 UI 路径和更新文档，没有新增 credential 或 provider payload 流。API key reveal 仍只在现有 explicit reveal path。
- Remaining unsupported cases and why they fail closed: `validate_provider_settings` 仍作为 metadata-only command/wrapper 存在，不再驱动健康检查。真实 probe、quota、usage 和 route 状态继续由 backend 命令返回；失败在现有 safe error/partial unavailable 状态中显示。

## Code Review Subagent

Result: PASS
Findings:

Code-review subagent returned PASS after running the Task 9 targeted UI tests, docs gates, `pnpm check:desktop`, and `git diff --check HEAD`.

## Security Review Subagent

Result: PASS
Findings:

Security-review subagent returned PASS. It verified `validateProviderSettings` is not used by production settings UI, the probe regression asserts `probeProviderConfig`, legacy metadata-check copy was removed, docs clarify metadata validation versus provider network actions, and no new secret or provider-native payload exposure was introduced.

## Gates

- `pnpm -C apps/desktop test -- ModelSettingsPage.test.tsx ModelDetailsDrawer.test.tsx CapabilityRoutesPanel.test.tsx`: exit 0
- `pnpm check:frontend-docs`: exit 0
- `pnpm check:backend-docs`: exit 0
- `pnpm check:docs`: exit 0
- `pnpm check:desktop`: exit 0
- `git diff --check`: exit 0
