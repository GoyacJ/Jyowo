# Jyowo Test Inventory

## Totals by Layer

| Layer | Count |
|---|---|
| Frontend Vitest files | 64 |
| Frontend Vitest test cases | 557 |
| Storybook files | 13 |
| Playwright spec files | 4 |
| Rust test files | 295 |
| Rust `#[test]` / `#[tokio::test]` count | 2089 |
| Script policy test files | 8 |

## Largest Test Files by Line Count

| File | Lines | Kind |
|---|---|---|
| apps/desktop/src/shared/tauri/commands.test.ts | 5364 | frontend |
| crates/jyowo-harness-plugin/tests/registry.rs | 3086 | rust |
| crates/jyowo-harness-journal/tests/conversation_read_model.rs | 2862 | rust |
| crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs | 2831 | rust |
| crates/jyowo-harness-engine/tests/subagent_tool_feature.rs | 2344 | rust |
| crates/jyowo-harness-engine/tests/main_loop.rs | 1879 | rust |
| crates/jyowo-harness-engine/tests/hook_pipeline.rs | 1692 | rust |
| apps/desktop/src/shared/events/run-event-schema.test.ts | 1594 | frontend |
| crates/jyowo-harness-team/tests/team_e2e.rs | 1388 | rust |
| crates/jyowo-harness-mcp/tests/server_protocol.rs | 1378 | rust |
| crates/jyowo-harness-plugin/tests/sources.rs | 1273 | rust |
| crates/jyowo-harness-sdk/tests/runtime_assembly_support/mod.rs | 1200 | rust |
| crates/jyowo-harness-session/tests/run_turn.rs | 1196 | rust |
| crates/jyowo-harness-sdk/tests/runtime_assembly_tools.rs | 1179 | rust |
| apps/desktop/src-tauri/tests/commands/provider_routes.rs | 1171 | rust |
| crates/jyowo-harness-subagent/tests/default_runner.rs | 1168 | rust |
| crates/jyowo-harness-sdk/tests/runtime_assembly_agents.rs | 1156 | rust |
| crates/jyowo-harness-tool/tests/builtin_exec.rs | 1150 | rust |
| crates/jyowo-harness-mcp/tests/http.rs | 1133 | rust |
| crates/jyowo-harness-sdk/tests/agents_team.rs | 1119 | rust |
| apps/desktop/src-tauri/tests/commands/permissions.rs | 1103 | rust |
| apps/desktop/src-tauri/tests/agent_orchestration_e2e.rs | 1084 | rust |
| crates/jyowo-harness-agent-runtime/tests/agent_orchestration_background.rs | 1062 | rust |
| crates/jyowo-harness-sdk/tests/runtime_assembly_contract.rs | 1055 | rust |
| crates/jyowo-harness-contracts/tests/core_contracts.rs | 1053 | rust |
| apps/desktop/src-tauri/tests/commands/provider_settings.rs | 1024 | rust |
| apps/desktop/src-tauri/tests/commands/conversations.rs | 1016 | rust |
| crates/jyowo-harness-subagent/tests/permission_bridge.rs | 1009 | rust |
| crates/jyowo-harness-team/tests/routing.rs | 974 | rust |
| apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx | 957 | frontend |

## Files Over 1200 Lines (hard fail)

- apps/desktop/src/shared/tauri/commands.test.ts (5364 lines)
- crates/jyowo-harness-plugin/tests/registry.rs (3086 lines)
- crates/jyowo-harness-journal/tests/conversation_read_model.rs (2862 lines)
- crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs (2831 lines)
- crates/jyowo-harness-engine/tests/subagent_tool_feature.rs (2344 lines)
- crates/jyowo-harness-engine/tests/main_loop.rs (1879 lines)
- crates/jyowo-harness-engine/tests/hook_pipeline.rs (1692 lines)
- apps/desktop/src/shared/events/run-event-schema.test.ts (1594 lines)
- crates/jyowo-harness-team/tests/team_e2e.rs (1388 lines)
- crates/jyowo-harness-mcp/tests/server_protocol.rs (1378 lines)
- crates/jyowo-harness-plugin/tests/sources.rs (1273 lines)

## Files Over 800 Lines (warning)

- crates/jyowo-harness-sdk/tests/runtime_assembly_support/mod.rs (1200 lines)
- crates/jyowo-harness-session/tests/run_turn.rs (1196 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_tools.rs (1179 lines)
- apps/desktop/src-tauri/tests/commands/provider_routes.rs (1171 lines)
- crates/jyowo-harness-subagent/tests/default_runner.rs (1168 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_agents.rs (1156 lines)
- crates/jyowo-harness-tool/tests/builtin_exec.rs (1150 lines)
- crates/jyowo-harness-mcp/tests/http.rs (1133 lines)
- crates/jyowo-harness-sdk/tests/agents_team.rs (1119 lines)
- apps/desktop/src-tauri/tests/commands/permissions.rs (1103 lines)
- apps/desktop/src-tauri/tests/agent_orchestration_e2e.rs (1084 lines)
- crates/jyowo-harness-agent-runtime/tests/agent_orchestration_background.rs (1062 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_contract.rs (1055 lines)
- crates/jyowo-harness-contracts/tests/core_contracts.rs (1053 lines)
- apps/desktop/src-tauri/tests/commands/provider_settings.rs (1024 lines)
- apps/desktop/src-tauri/tests/commands/conversations.rs (1016 lines)
- crates/jyowo-harness-subagent/tests/permission_bridge.rs (1009 lines)
- crates/jyowo-harness-team/tests/routing.rs (974 lines)
- apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx (957 lines)
- crates/jyowo-harness-tool/tests/registry_pool.rs (940 lines)
- crates/jyowo-harness-sandbox/tests/local.rs (933 lines)
- crates/jyowo-harness-sdk/tests/facade.rs (928 lines)
- crates/jyowo-harness-engine/tests/interrupt.rs (919 lines)
- crates/jyowo-harness-tool/tests/orchestrator.rs (904 lines)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx (894 lines)
- crates/jyowo-harness-sandbox/tests/docker.rs (891 lines)
- apps/desktop/src-tauri/tests/commands/activity_redaction.rs (888 lines)
- crates/jyowo-harness-tool/tests/minimax_tools.rs (862 lines)
- crates/jyowo-harness-mcp/tests/core.rs (822 lines)
- apps/desktop/src-tauri/tests/commands/mcp.rs (806 lines)

## Disallowed or Suspect Names

- crates/jyowo-harness-team/tests/team_e2e.rs

## Ignored / Manual / Live / Stress Tests

### Ignored tests

### manual_live_*.rs
None.

### stress_*.rs
None.

## createTestCommandClient Usage by File

- apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx (22)
- apps/desktop/src/features/settings/MCPManager.test.tsx (18)
- apps/desktop/src/features/settings/ExecutionSettings.test.tsx (15)
- apps/desktop/src/features/skills/SkillsPage.test.tsx (15)
- apps/desktop/src/features/settings/PluginsManager.test.tsx (13)
- apps/desktop/src/features/workspace/SidebarNav.test.tsx (10)
- apps/desktop/src/app/shell/AppShell.test.tsx (9)
- apps/desktop/src/features/settings/models/ModelDetailsDrawer.test.tsx (9)
- apps/desktop/src/features/background-agents/BackgroundAgentsPanel.test.tsx (8)
- apps/desktop/src/features/memory/MemoryBrowser.test.tsx (8)
- apps/desktop/src/features/settings/AutomationSettings.test.tsx (8)
- apps/desktop/src/app/App.test.tsx (7)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.artifacts.test.tsx (7)
- apps/desktop/src/features/settings/models/ModelConfigDialog.test.tsx (7)
- apps/desktop/src/shared/tauri/commands.test.ts (7)
- apps/desktop/src/features/artifacts/ArtifactsPage.test.tsx (6)
- apps/desktop/src/features/settings/models/ModelSettingsPage.stories.tsx (5)
- apps/desktop/src/features/settings/models/model-settings-view-model.test.ts (5)
- apps/desktop/src/features/context/use-context-snapshot.test.tsx (4)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.large-output.test.tsx (4)
- apps/desktop/src/features/evals/EvalLabPage.test.tsx (4)
- apps/desktop/src/features/settings/models/ModelSettingsPage.test.tsx (3)
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
- apps/desktop/src/features/settings/MCPServerCard.stories.tsx
- apps/desktop/src/features/settings/models/CapabilityRoutesPanel.stories.tsx
- apps/desktop/src/features/settings/models/ModelSettingsPage.stories.tsx

## Duplicate contract.rs / api_contract.rs Pairs

- crates/jyowo-harness-hook/tests
- crates/jyowo-harness-memory/tests
- crates/jyowo-harness-sandbox/tests
- crates/jyowo-harness-tool/tests
