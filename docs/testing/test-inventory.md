# Jyowo Test Inventory

## Totals by Layer

| Layer | Count |
|---|---|
| Frontend Vitest files | 77 |
| Frontend Vitest test cases | 731 |
| Storybook files | 19 |
| Playwright spec files | 4 |
| Rust test files | 340 |
| Rust `#[test]` / `#[tokio::test]` count | 2587 |
| Script policy test files | 10 |

## Largest Test Files by Line Count

| File | Lines | Kind |
|---|---|---|
| apps/desktop/src/shared/tauri/commands.test.ts | 6696 | frontend |
| crates/jyowo-harness-journal/tests/conversation_read_model.rs | 3723 | rust |
| crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs | 3503 | rust |
| crates/jyowo-harness-plugin/tests/registry.rs | 3169 | rust |
| crates/jyowo-harness-engine/tests/subagent_tool_feature.rs | 2384 | rust |
| crates/jyowo-harness-engine/tests/main_loop.rs | 1909 | rust |
| crates/jyowo-harness-engine/tests/hook_pipeline.rs | 1732 | rust |
| apps/desktop/src/shared/events/run-event-schema.test.ts | 1615 | frontend |
| crates/jyowo-harness-mcp/tests/server_protocol.rs | 1597 | rust |
| crates/jyowo-harness-team/tests/team_e2e.rs | 1388 | rust |
| crates/jyowo-harness-plugin/tests/sources.rs | 1289 | rust |
| crates/jyowo-harness-session/tests/run_turn.rs | 1199 | rust |
| crates/jyowo-harness-subagent/tests/default_runner.rs | 1199 | rust |
| crates/jyowo-harness-journal/tests/evidence_ref_store.rs | 1195 | rust |
| crates/jyowo-harness-contracts/tests/core_contracts.rs | 1188 | rust |
| crates/jyowo-harness-sdk/tests/runtime_assembly_support/mod.rs | 1186 | rust |
| crates/jyowo-harness-tool/tests/builtin_exec.rs | 1185 | rust |
| apps/desktop/src-tauri/tests/commands/support.rs | 1183 | rust |
| apps/desktop/src-tauri/tests/agent_orchestration_e2e.rs | 1181 | rust |
| apps/desktop/src-tauri/tests/commands/mcp.rs | 1178 | rust |
| crates/jyowo-harness-sdk/tests/runtime_assembly_agents.rs | 1176 | rust |
| crates/jyowo-harness-sdk/tests/facade.rs | 1173 | rust |
| apps/desktop/src-tauri/tests/commands/permissions.rs | 1165 | rust |
| crates/jyowo-harness-contracts/tests/memory_platform_contracts.rs | 1146 | rust |
| crates/jyowo-harness-mcp/tests/http.rs | 1145 | rust |
| crates/jyowo-harness-sdk/tests/runtime_assembly_context.rs | 1139 | rust |
| apps/desktop/src/features/workbench/WorkbenchInspector.artifacts.test.tsx | 1126 | frontend |
| crates/jyowo-harness-sdk/tests/agents_team.rs | 1126 | rust |
| crates/jyowo-harness-agent-runtime/tests/agent_orchestration_background.rs | 1106 | rust |
| apps/desktop/src-tauri/tests/commands/provider_settings.rs | 1099 | rust |

## Files Over 1200 Lines (hard fail)

- apps/desktop/src/shared/tauri/commands.test.ts (6696 lines)
- crates/jyowo-harness-journal/tests/conversation_read_model.rs (3723 lines)
- crates/jyowo-harness-journal/tests/conversation_worktree_projector.rs (3503 lines)
- crates/jyowo-harness-plugin/tests/registry.rs (3169 lines)
- crates/jyowo-harness-engine/tests/subagent_tool_feature.rs (2384 lines)
- crates/jyowo-harness-engine/tests/main_loop.rs (1909 lines)
- crates/jyowo-harness-engine/tests/hook_pipeline.rs (1732 lines)
- apps/desktop/src/shared/events/run-event-schema.test.ts (1615 lines)
- crates/jyowo-harness-mcp/tests/server_protocol.rs (1597 lines)
- crates/jyowo-harness-team/tests/team_e2e.rs (1388 lines)
- crates/jyowo-harness-plugin/tests/sources.rs (1289 lines)

## Files Over 800 Lines (warning)

- crates/jyowo-harness-session/tests/run_turn.rs (1199 lines)
- crates/jyowo-harness-subagent/tests/default_runner.rs (1199 lines)
- crates/jyowo-harness-journal/tests/evidence_ref_store.rs (1195 lines)
- crates/jyowo-harness-contracts/tests/core_contracts.rs (1188 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_support/mod.rs (1186 lines)
- crates/jyowo-harness-tool/tests/builtin_exec.rs (1185 lines)
- apps/desktop/src-tauri/tests/commands/support.rs (1183 lines)
- apps/desktop/src-tauri/tests/agent_orchestration_e2e.rs (1181 lines)
- apps/desktop/src-tauri/tests/commands/mcp.rs (1178 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_agents.rs (1176 lines)
- crates/jyowo-harness-sdk/tests/facade.rs (1173 lines)
- apps/desktop/src-tauri/tests/commands/permissions.rs (1165 lines)
- crates/jyowo-harness-contracts/tests/memory_platform_contracts.rs (1146 lines)
- crates/jyowo-harness-mcp/tests/http.rs (1145 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_context.rs (1139 lines)
- apps/desktop/src/features/workbench/WorkbenchInspector.artifacts.test.tsx (1126 lines)
- crates/jyowo-harness-sdk/tests/agents_team.rs (1126 lines)
- crates/jyowo-harness-agent-runtime/tests/agent_orchestration_background.rs (1106 lines)
- apps/desktop/src-tauri/tests/commands/provider_settings.rs (1099 lines)
- apps/desktop/src/features/conversation/ConversationWorkspace.test.tsx (1075 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_tools.rs (1072 lines)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.stories.tsx (1071 lines)
- crates/jyowo-harness-tool/tests/minimax_tools.rs (1069 lines)
- crates/jyowo-harness-subagent/tests/permission_bridge.rs (1059 lines)
- crates/jyowo-harness-sdk/tests/runtime_assembly_contract.rs (1055 lines)
- apps/desktop/src-tauri/tests/commands/conversations.rs (1028 lines)
- apps/desktop/src-tauri/tests/commands/provider_routes.rs (1021 lines)
- crates/jyowo-harness-memory/tests/local_provider.rs (1005 lines)
- crates/jyowo-harness-journal/tests/conversation_workbench_projection.rs (977 lines)
- crates/jyowo-harness-team/tests/routing.rs (977 lines)
- crates/jyowo-harness-engine/tests/interrupt.rs (943 lines)
- crates/jyowo-harness-sandbox/tests/docker.rs (943 lines)
- crates/jyowo-harness-tool/tests/registry_pool.rs (941 lines)
- crates/jyowo-harness-sandbox/tests/local.rs (933 lines)
- apps/desktop/src-tauri/tests/commands/activity_redaction.rs (930 lines)
- crates/jyowo-harness-tool/tests/orchestrator.rs (917 lines)
- crates/jyowo-harness-memory/tests/extraction.rs (898 lines)
- apps/desktop/src-tauri/tests/commands/artifact_listing.rs (894 lines)
- crates/jyowo-harness-memory/tests/recall.rs (893 lines)
- crates/jyowo-harness-memory/tests/store_audit.rs (849 lines)
- crates/jyowo-harness-mcp/tests/core.rs (830 lines)
- apps/desktop/src-tauri/tests/commands/activity.rs (802 lines)

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
- apps/desktop/src/features/settings/MCPManager.test.tsx (21)
- apps/desktop/src/features/settings/ExecutionSettings.test.tsx (16)
- apps/desktop/src/features/skills/SkillsPage.test.tsx (15)
- apps/desktop/src/features/workbench/WorkbenchInspector.artifacts.test.tsx (14)
- apps/desktop/src/features/settings/PluginsManager.test.tsx (13)
- apps/desktop/src/features/workbench/WorkbenchInspector.test.tsx (13)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.large-output.test.tsx (11)
- apps/desktop/src/features/workspace/SidebarNav.test.tsx (11)
- apps/desktop/src/features/artifacts/ArtifactsPage.test.tsx (9)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.artifacts.test.tsx (9)
- apps/desktop/src/features/settings/AutomationSettings.test.tsx (9)
- apps/desktop/src/features/background-agents/BackgroundAgentsPanel.test.tsx (8)
- apps/desktop/src/features/conversation/evidence/CommandExecutionView.test.tsx (8)
- apps/desktop/src/features/conversation/evidence/DiffPane.test.tsx (8)
- apps/desktop/src/features/memory/MemoryBrowser.test.tsx (8)
- apps/desktop/src/app/App.test.tsx (7)
- apps/desktop/src/features/settings/models/ModelConfigDialog.test.tsx (7)
- apps/desktop/src/features/settings/models/ModelDetailsDrawer.test.tsx (7)
- apps/desktop/src/shared/tauri/commands.test.ts (7)
- apps/desktop/src/features/conversation/ConversationWorkspace.model-config.test.tsx (6)
- apps/desktop/src/app/shell/AppShell.test.tsx (5)
- apps/desktop/src/features/conversation/timeline/conversation-timeline.render.test.tsx (5)
- apps/desktop/src/features/conversation/timeline/use-conversation-timeline.test.tsx (5)
- apps/desktop/src/features/settings/models/ModelSettingsPage.stories.tsx (5)
- apps/desktop/src/features/settings/models/model-settings-view-model.test.ts (5)
- apps/desktop/src/features/context/use-context-snapshot.test.tsx (4)
- apps/desktop/src/features/evals/EvalLabPage.test.tsx (4)
- apps/desktop/src/features/settings/models/ModelSettingsPage.test.tsx (3)
- apps/desktop/src/features/system-status/SystemStatusPage.test.tsx (3)
- apps/desktop/src/features/workbench/WorkbenchInspector.artifact-media.test.tsx (3)
- apps/desktop/src/testing/command-client/index.ts (3)
- apps/desktop/src/features/conversation/ConversationWorkspace.stories.tsx (2)
- apps/desktop/src/features/conversation/WelcomeWorkspace.test.tsx (2)
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
