# Jyowo Test Inventory

## Totals by Layer

| Layer | Count |
|---|---|
| Frontend Vitest files | 74 |
| Frontend Vitest test cases | 689 |
| Storybook files | 19 |
| Playwright spec files | 4 |
| Rust test files | 332 |
| Rust `#[test]` / `#[tokio::test]` count | 2476 |
| Script policy test files | 9 |

## Largest Test Files by Line Count

| File | Lines | Kind |
|---|---|---|
| apps/desktop/src/shared/tauri/commands.test.ts | 6463 | frontend |
| crates/jyowo-harness-journal/tests/conversation_read_model.rs | 3683 | rust |
| crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs | 3317 | rust |
| crates/jyowo-harness-plugin/tests/registry.rs | 3168 | rust |
| crates/jyowo-harness-engine/tests/subagent_tool_feature.rs | 2373 | rust |
| crates/jyowo-harness-engine/tests/main_loop.rs | 1895 | rust |
| crates/jyowo-harness-engine/tests/hook_pipeline.rs | 1729 | rust |
| crates/jyowo-harness-mcp/tests/server_protocol.rs | 1595 | rust |
| apps/desktop/src/shared/events/run-event-schema.test.ts | 1594 | frontend |
| crates/jyowo-harness-team/tests/team_e2e.rs | 1388 | rust |
| crates/jyowo-harness-plugin/tests/sources.rs | 1274 | rust |
| crates/jyowo-harness-subagent/tests/default_runner.rs | 1199 | rust |
| crates/jyowo-harness-session/tests/run_turn.rs | 1198 | rust |
| crates/jyowo-harness-contracts/tests/core_contracts.rs | 1196 | rust |
| crates/jyowo-harness-journal/tests/evidence_ref_store.rs | 1195 | rust |
| crates/jyowo-harness-sdk/tests/runtime_assembly_support/mod.rs | 1184 | rust |
| apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx | 1176 | frontend |
| apps/desktop/src-tauri/tests/commands/provider_routes.rs | 1174 | rust |
| apps/desktop/src-tauri/tests/commands/permissions.rs | 1163 | rust |
| crates/jyowo-harness-sdk/tests/runtime_assembly_agents.rs | 1161 | rust |
| crates/jyowo-harness-tool/tests/builtin_exec.rs | 1152 | rust |
| crates/jyowo-harness-contracts/tests/memory_platform_contracts.rs | 1146 | rust |
| crates/jyowo-harness-mcp/tests/http.rs | 1145 | rust |
| apps/desktop/src-tauri/tests/agent_orchestration_e2e.rs | 1133 | rust |
| apps/desktop/src/features/workbench/WorkbenchInspector.artifacts.test.tsx | 1126 | frontend |
| crates/jyowo-harness-sdk/tests/agents_team.rs | 1125 | rust |
| apps/desktop/src-tauri/tests/commands/provider_settings.rs | 1092 | rust |
| apps/desktop/src-tauri/tests/commands/mcp.rs | 1078 | rust |
| crates/jyowo-harness-sdk/tests/facade.rs | 1074 | rust |
| crates/jyowo-harness-sdk/tests/runtime_assembly_context.rs | 1071 | rust |

## Files Over 1200 Lines (hard fail)

- apps/desktop/src/shared/tauri/commands.test.ts (6463 lines)
- crates/jyowo-harness-journal/tests/conversation_read_model.rs (3683 lines)
- crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs (3317 lines)
- crates/jyowo-harness-plugin/tests/registry.rs (3168 lines)
- crates/jyowo-harness-engine/tests/subagent_tool_feature.rs (2373 lines)
- crates/jyowo-harness-engine/tests/main_loop.rs (1895 lines)
- crates/jyowo-harness-engine/tests/hook_pipeline.rs (1729 lines)
- crates/jyowo-harness-mcp/tests/server_protocol.rs (1595 lines)
- apps/desktop/src/shared/events/run-event-schema.test.ts (1594 lines)
- crates/jyowo-harness-team/tests/team_e2e.rs (1388 lines)
- crates/jyowo-harness-plugin/tests/sources.rs (1274 lines)

## Files Over 800 Lines (warning)

- crates/jyowo-harness-subagent/tests/default_runner.rs (1199 lines)
- crates/jyowo-harness-session/tests/run_turn.rs (1198 lines)
- crates/jyowo-harness-contracts/tests/core_contracts.rs (1196 lines)
- crates/jyowo-harness-journal/tests/evidence_ref_store.rs (1195 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_support/mod.rs (1184 lines)
- apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx (1176 lines)
- apps/desktop/src-tauri/tests/commands/provider_routes.rs (1174 lines)
- apps/desktop/src-tauri/tests/commands/permissions.rs (1163 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_agents.rs (1161 lines)
- crates/jyowo-harness-tool/tests/builtin_exec.rs (1152 lines)
- crates/jyowo-harness-contracts/tests/memory_platform_contracts.rs (1146 lines)
- crates/jyowo-harness-mcp/tests/http.rs (1145 lines)
- apps/desktop/src-tauri/tests/agent_orchestration_e2e.rs (1133 lines)
- apps/desktop/src/features/workbench/WorkbenchInspector.artifacts.test.tsx (1126 lines)
- crates/jyowo-harness-sdk/tests/agents_team.rs (1125 lines)
- apps/desktop/src-tauri/tests/commands/provider_settings.rs (1092 lines)
- apps/desktop/src-tauri/tests/commands/mcp.rs (1078 lines)
- crates/jyowo-harness-sdk/tests/facade.rs (1074 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_context.rs (1071 lines)
- crates/jyowo-harness-agent-runtime/tests/agent_orchestration_background.rs (1062 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_tools.rs (1059 lines)
- crates/jyowo-harness-subagent/tests/permission_bridge.rs (1059 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_contract.rs (1055 lines)
- apps/desktop/src-tauri/tests/commands/support.rs (1044 lines)
- crates/jyowo-harness-memory/tests/local_provider.rs (1041 lines)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx (1021 lines)
- apps/desktop/src-tauri/tests/commands/conversations.rs (1016 lines)
- crates/jyowo-harness-journal/tests/conversation_workbench_projection.rs (977 lines)
- crates/jyowo-harness-team/tests/routing.rs (977 lines)
- crates/jyowo-harness-tool/tests/registry_pool.rs (940 lines)
- crates/jyowo-harness-sandbox/tests/local.rs (933 lines)
- crates/jyowo-harness-engine/tests/interrupt.rs (931 lines)
- apps/desktop/src-tauri/tests/commands/activity_redaction.rs (930 lines)
- crates/jyowo-harness-tool/tests/orchestrator.rs (905 lines)
- crates/jyowo-harness-execution/tests/authorization_flow.rs (901 lines)
- crates/jyowo-harness-memory/tests/extraction.rs (898 lines)
- apps/desktop/src-tauri/tests/commands/artifact_listing.rs (894 lines)
- crates/jyowo-harness-memory/tests/recall.rs (893 lines)
- crates/jyowo-harness-sandbox/tests/docker.rs (891 lines)
- crates/jyowo-harness-tool/tests/minimax_tools.rs (865 lines)
- crates/jyowo-harness-memory/tests/store_audit.rs (849 lines)
- crates/jyowo-harness-mcp/tests/core.rs (823 lines)

## Disallowed or Suspect Names

- crates/jyowo-harness-team/tests/team_e2e.rs

## Ignored / Manual / Live / Stress Tests

### Ignored tests

### manual_live_*.rs
None.

### stress_*.rs
None.

## createTestCommandClient Usage by File

- apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx (25)
- apps/desktop/src/features/settings/MCPManager.test.tsx (19)
- apps/desktop/src/features/settings/ExecutionSettings.test.tsx (15)
- apps/desktop/src/features/skills/SkillsPage.test.tsx (15)
- apps/desktop/src/features/workbench/WorkbenchInspector.artifacts.test.tsx (14)
- apps/desktop/src/features/settings/PluginsManager.test.tsx (13)
- apps/desktop/src/features/workbench/WorkbenchInspector.test.tsx (13)
- apps/desktop/src/features/workspace/SidebarNav.test.tsx (10)
- apps/desktop/src/features/artifacts/ArtifactsPage.test.tsx (9)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.artifacts.test.tsx (9)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.large-output.test.tsx (9)
- apps/desktop/src/features/background-agents/BackgroundAgentsPanel.test.tsx (8)
- apps/desktop/src/features/memory/MemoryBrowser.test.tsx (8)
- apps/desktop/src/features/settings/AutomationSettings.test.tsx (8)
- apps/desktop/src/app/App.test.tsx (7)
- apps/desktop/src/features/settings/models/ModelConfigDialog.test.tsx (7)
- apps/desktop/src/features/settings/models/ModelDetailsDrawer.test.tsx (7)
- apps/desktop/src/shared/tauri/commands.test.ts (7)
- apps/desktop/src/app/shell/AppShell.test.tsx (5)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.render.test.tsx (5)
- apps/desktop/src/features/settings/models/ModelSettingsPage.stories.tsx (5)
- apps/desktop/src/features/settings/models/model-settings-view-model.test.ts (5)
- apps/desktop/src/features/context/use-context-snapshot.test.tsx (4)
- apps/desktop/src/features/conversation/evidence/DiffPane.test.tsx (4)
- apps/desktop/src/features/evals/EvalLabPage.test.tsx (4)
- apps/desktop/src/features/conversation/evidence/CommandExecutionView.test.tsx (3)
- apps/desktop/src/features/conversation/timeline/use-conversation-timeline.test.tsx (3)
- apps/desktop/src/features/settings/models/ModelSettingsPage.test.tsx (3)
- apps/desktop/src/features/system-status/SystemStatusPage.test.tsx (3)
- apps/desktop/src/features/workbench/WorkbenchInspector.artifact-media.test.tsx (3)
- apps/desktop/src/testing/command-client/index.ts (3)
- apps/desktop/src/features/conversation/ConversationWorkspace.stories.tsx (2)
- apps/desktop/src/features/conversation/timeline/conversation-timeline-source.test.ts (2)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx (2)
- apps/desktop/src/features/settings/AboutSettings.test.tsx (2)
- apps/desktop/src/features/settings/SettingsPage.test.tsx (2)
- apps/desktop/src/features/settings/SkillSettings.test.tsx (2)
- apps/desktop/src/features/workbench/WorkbenchInspector.test-support.tsx (2)
- apps/desktop/src/main.tsx (2)
- apps/desktop/src/testing/command-client/conversation-handlers.test.ts (2)
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
- apps/desktop/src/features/conversation/evidence/ChangeSetSummary.stories.tsx
- apps/desktop/src/features/conversation/evidence/CommandExecutionView.stories.tsx
- apps/desktop/src/features/conversation/evidence/DecisionPanel.stories.tsx
- apps/desktop/src/features/conversation/evidence/DiffPane.stories.tsx
- apps/desktop/src/features/conversation/evidence/ToolInvocationCard.stories.tsx
- apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx

### memory
- apps/desktop/src/features/memory/MemoryItemCard.stories.tsx

### settings
- apps/desktop/src/features/settings/MCPServerCard.stories.tsx
- apps/desktop/src/features/settings/models/CapabilityRoutesPanel.stories.tsx
- apps/desktop/src/features/settings/models/ModelSettingsPage.stories.tsx

### workbench
- apps/desktop/src/features/workbench/WorkbenchInspector.stories.tsx

## Duplicate contract.rs / api_contract.rs Pairs

- crates/jyowo-harness-hook/tests
- crates/jyowo-harness-memory/tests
- crates/jyowo-harness-sandbox/tests
- crates/jyowo-harness-tool/tests
