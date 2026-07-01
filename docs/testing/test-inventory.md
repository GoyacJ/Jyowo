# Jyowo Test Inventory

## Totals by Layer

| Layer | Count |
|---|---|
| Frontend Vitest files | 56 |
| Frontend Vitest test cases | 467 |
| Storybook files | 13 |
| Playwright spec files | 3 |
| Rust test files | 264 |
| Rust `#[test]` / `#[tokio::test]` count | 1804 |
| Script policy test files | 6 |

## Largest Test Files by Line Count

| File | Lines | Kind |
|---|---|---|
| apps/desktop/src/shared/tauri/commands.test.ts | 4272 | frontend |
| crates/jyowo-harness-plugin/tests/registry.rs | 3077 | rust |
| crates/jyowo-harness-engine/tests/subagent_tool_feature.rs | 2310 | rust |
| crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs | 2281 | rust |
| crates/jyowo-harness-journal/tests/conversation_read_model.rs | 2119 | rust |
| crates/jyowo-harness-engine/tests/main_loop.rs | 1861 | rust |
| crates/jyowo-harness-engine/tests/hook_pipeline.rs | 1644 | rust |
| apps/desktop/src/features/settings/ProviderSettingsForm.test.tsx | 1572 | frontend |
| crates/jyowo-harness-mcp/tests/server_protocol.rs | 1380 | rust |
| crates/jyowo-harness-team/tests/team_e2e.rs | 1374 | rust |
| apps/desktop/src/shared/events/run-event-schema.test.ts | 1302 | frontend |
| crates/jyowo-harness-plugin/tests/sources.rs | 1261 | rust |
| crates/jyowo-harness-session/tests/run_turn.rs | 1198 | rust |
| apps/desktop/src-tauri/tests/commands/provider_settings.rs | 1191 | rust |
| apps/desktop/src-tauri/tests/commands/provider_routes.rs | 1176 | rust |
| crates/jyowo-harness-subagent/tests/default_runner.rs | 1153 | rust |
| crates/jyowo-harness-sdk/tests/agents_team.rs | 1134 | rust |
| crates/jyowo-harness-mcp/tests/http.rs | 1133 | rust |
| crates/jyowo-harness-sdk/tests/runtime_assembly_support/mod.rs | 1133 | rust |
| crates/jyowo-harness-tool/tests/builtin_exec.rs | 1115 | rust |
| crates/jyowo-harness-tool/tests/orchestrator.rs | 1085 | rust |
| crates/jyowo-harness-sdk/tests/runtime_assembly_tools.rs | 1038 | rust |
| crates/jyowo-harness-team/tests/routing.rs | 974 | rust |
| crates/jyowo-harness-contracts/tests/core_contracts.rs | 956 | rust |
| crates/jyowo-harness-tool/tests/registry_pool.rs | 931 | rust |
| crates/jyowo-harness-sdk/tests/facade.rs | 921 | rust |
| crates/jyowo-harness-engine/tests/interrupt.rs | 913 | rust |
| apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx | 894 | storybook |
| crates/jyowo-harness-sandbox/tests/local.rs | 869 | rust |
| apps/desktop/src-tauri/tests/commands/activity_redaction.rs | 867 | rust |

## Files Over 1200 Lines (hard fail)

- apps/desktop/src/shared/tauri/commands.test.ts (4272 lines)
- crates/jyowo-harness-plugin/tests/registry.rs (3077 lines)
- crates/jyowo-harness-engine/tests/subagent_tool_feature.rs (2310 lines)
- crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs (2281 lines)
- crates/jyowo-harness-journal/tests/conversation_read_model.rs (2119 lines)
- crates/jyowo-harness-engine/tests/main_loop.rs (1861 lines)
- crates/jyowo-harness-engine/tests/hook_pipeline.rs (1644 lines)
- apps/desktop/src/features/settings/ProviderSettingsForm.test.tsx (1572 lines)
- crates/jyowo-harness-mcp/tests/server_protocol.rs (1380 lines)
- crates/jyowo-harness-team/tests/team_e2e.rs (1374 lines)
- apps/desktop/src/shared/events/run-event-schema.test.ts (1302 lines)
- crates/jyowo-harness-plugin/tests/sources.rs (1261 lines)

## Files Over 800 Lines (warning)

- crates/jyowo-harness-session/tests/run_turn.rs (1198 lines)
- apps/desktop/src-tauri/tests/commands/provider_settings.rs (1191 lines)
- apps/desktop/src-tauri/tests/commands/provider_routes.rs (1176 lines)
- crates/jyowo-harness-subagent/tests/default_runner.rs (1153 lines)
- crates/jyowo-harness-sdk/tests/agents_team.rs (1134 lines)
- crates/jyowo-harness-mcp/tests/http.rs (1133 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_support/mod.rs (1133 lines)
- crates/jyowo-harness-tool/tests/builtin_exec.rs (1115 lines)
- crates/jyowo-harness-tool/tests/orchestrator.rs (1085 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_tools.rs (1038 lines)
- crates/jyowo-harness-team/tests/routing.rs (974 lines)
- crates/jyowo-harness-contracts/tests/core_contracts.rs (956 lines)
- crates/jyowo-harness-tool/tests/registry_pool.rs (931 lines)
- crates/jyowo-harness-sdk/tests/facade.rs (921 lines)
- crates/jyowo-harness-engine/tests/interrupt.rs (913 lines)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx (894 lines)
- crates/jyowo-harness-sandbox/tests/local.rs (869 lines)
- apps/desktop/src-tauri/tests/commands/activity_redaction.rs (867 lines)
- apps/desktop/src-tauri/tests/commands/permissions.rs (867 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_contract.rs (856 lines)
- crates/jyowo-harness-tool/tests/minimax_tools.rs (856 lines)
- apps/desktop/src-tauri/tests/commands/mcp.rs (813 lines)
- crates/jyowo-harness-mcp/tests/core.rs (810 lines)

## Disallowed or Suspect Names

- crates/jyowo-harness-team/tests/team_e2e.rs

## Ignored / Manual / Live / Stress Tests

### Ignored tests

### manual_live_*.rs
None.

### stress_*.rs
None.

## createTestCommandClient Usage by File

- apps/desktop/src/features/settings/ProviderSettingsForm.test.tsx (20)
- apps/desktop/src/features/settings/MCPManager.test.tsx (18)
- apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx (17)
- apps/desktop/src/features/skills/SkillsPage.test.tsx (15)
- apps/desktop/src/features/settings/PluginsManager.test.tsx (13)
- apps/desktop/src/features/workspace/SidebarNav.test.tsx (10)
- apps/desktop/src/app/shell/AppShell.test.tsx (9)
- apps/desktop/src/features/memory/MemoryBrowser.test.tsx (8)
- apps/desktop/src/features/settings/AutomationSettings.test.tsx (8)
- apps/desktop/src/app/App.test.tsx (7)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.artifacts.test.tsx (7)
- apps/desktop/src/features/settings/ExecutionSettings.test.tsx (7)
- apps/desktop/src/shared/tauri/commands.test.ts (7)
- apps/desktop/src/features/artifacts/ArtifactsPage.test.tsx (6)
- apps/desktop/src/features/context/use-context-snapshot.test.tsx (4)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.large-output.test.tsx (4)
- apps/desktop/src/features/evals/EvalLabPage.test.tsx (4)
- apps/desktop/src/features/settings/ProviderSettingsForm.stories.tsx (4)
- apps/desktop/src/features/settings/CapabilityRoutesPanel.stories.tsx (3)
- apps/desktop/src/features/system-status/SystemStatusPage.test.tsx (3)
- apps/desktop/src/testing/command-client/index.ts (3)
- apps/desktop/src/features/conversation/ConversationWorkspace.stories.tsx (2)
- apps/desktop/src/features/conversation/timeline/conversation-timeline-source.test.ts (2)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.render.test.tsx (2)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx (2)
- apps/desktop/src/features/conversation/timeline/use-conversation-timeline.test.tsx (2)
- apps/desktop/src/features/settings/AboutSettings.test.tsx (2)
- apps/desktop/src/features/settings/SettingsPage.test.tsx (2)
- apps/desktop/src/features/settings/SkillSettings.test.tsx (2)
- apps/desktop/src/main.tsx (2)
- apps/desktop/src/testing/command-client/state.ts (1)

## Storybook Files by Feature

### activity
- apps/desktop/src/features/activity/ActivityRail.stories.tsx
- apps/desktop/src/features/activity/RunEventDetails.stories.tsx

### app
- apps/desktop/src/app/Foundation.stories.tsx
- apps/desktop/src/app/shell/AppShell.stories.tsx

### artifacts
- apps/desktop/src/features/artifacts/ArtifactPreview.stories.tsx

### context
- apps/desktop/src/features/context/ContextPanel.stories.tsx

### conversation
- apps/desktop/src/features/conversation/Composer.stories.tsx
- apps/desktop/src/features/conversation/ConversationWorkspace.stories.tsx
- apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx

### memory
- apps/desktop/src/features/memory/MemoryItemCard.stories.tsx

### settings
- apps/desktop/src/features/settings/CapabilityRoutesPanel.stories.tsx
- apps/desktop/src/features/settings/MCPServerCard.stories.tsx
- apps/desktop/src/features/settings/ProviderSettingsForm.stories.tsx

## Duplicate contract.rs / api_contract.rs Pairs

- crates/jyowo-harness-hook/tests
- crates/jyowo-harness-memory/tests
- crates/jyowo-harness-sandbox/tests
- crates/jyowo-harness-tool/tests
