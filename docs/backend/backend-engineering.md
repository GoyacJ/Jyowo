# Jyowo Backend Engineering

This document defines Rust backend implementation rules.

## Stack

Runtime stack:

```text
Rust 1.96
Tauri 2
tauri-plugin-store
Tokio
serde
serde_json
schemars JsonSchema
thiserror
tracing
tracing-subscriber
tracing-appender
rusqlite
SQLite FTS5
refinery
reqwest
tokio::process
portable-pty
```

Tooling:

```text
Node 24 LTS
pnpm 11.7
cargo fmt
cargo check
cargo test
cargo update --dry-run
insta
proptest
GitHub Actions
```

Root Rust policy:

```toml
unsafe_code = "forbid"
```

All Rust code MUST preserve the workspace lint policy. Do not add `unsafe` to application or harness code.

## Library Boundaries

Backend libraries are selected by runtime ownership. Do not add a parallel library
when the existing stack already owns the capability.

Persistence:

- `rusqlite` owns local SQLite access.
- SQLite FTS5 owns local search for conversations, artifacts, Memory, and project
  metadata.
- `refinery` owns SQLite schema migrations.
- Migration definitions belong beside the crate that owns the persisted schema.
- Schema changes require migration tests and restart-stable compatibility coverage.

Secrets:

- `secrecy` owns in-memory secret handling.
- `zeroize` owns explicit memory clearing where needed.
- Provider API keys are stored directly in the workspace provider settings file
  because the product supports explicit user reveal.
- List/save IPC payloads do not return raw provider API keys.
- `get_provider_config_api_key` is the only provider key reveal command.
- Prompt, Journal, Replay, logs, traces, screenshots, and support bundles must
  not include raw provider API keys.

Observability:

- `tracing` owns structured instrumentation.
- `tracing-subscriber` owns local subscriber setup.
- `tracing-appender` owns local rolling file sinks.
- OpenTelemetry crates own optional external telemetry export.
- Telemetry failures must not bypass policy or reveal secrets.

Contracts:

- `serde` and `serde_json` own serialized payload shape.
- `schemars` owns JsonSchema export.
- Contract schema export must be generated from Rust types, not hand-written.

Execution:

- `tokio::process` owns non-interactive command execution.
- `portable-pty` owns interactive terminal sessions only when a real PTY is needed.
- Command execution remains behind Tool, Sandbox, PermissionBroker, and Redactor
  boundaries.

Testing:

- `cargo test` owns Rust test execution.
- `insta` owns contract and event snapshot tests.
- `proptest` owns property tests for permission, redaction, budget, ordering, and
  migration invariants.

Forbidden:

- adding an ORM on top of `rusqlite`
- adding an external search service for local workspace search
- using provider API key values as prompt, event, log, trace, screenshot, or snapshot data
- using `anyhow` across public crate, IPC, or contract boundaries
- using `portable-pty` for simple non-interactive commands

## Workspace Layers

Dependency direction:

```text
Tauri shell -> L4 -> L3 -> L2 -> L1 -> L0
```

Lower layers MUST NOT depend on higher layers.

| Package | Path | Layer | Rule |
|---|---|---|---|
| `jyowo-desktop-shell` | `apps/desktop/src-tauri` | Tauri shell | Exposes desktop IPC and starts the in-process harness facade. |
| `jyowo-harness-contracts` | `crates/jyowo-harness-contracts` | L0 | Owns public IDs, messages, events, errors, serde shape, and JsonSchema exports. |
| `jyowo-harness-budget` | `crates/jyowo-harness-budget` | L1 | Owns shared quota and token budget carriers. |
| `jyowo-harness-journal` | `crates/jyowo-harness-journal` | L1 | Owns event stores, snapshots, audit projections, blobs, and Replay cursors. |
| `jyowo-harness-memory` | `crates/jyowo-harness-memory` | L1 | Owns Memory primitives, recall, consolidation, and visibility rules. |
| `jyowo-harness-model` | `crates/jyowo-harness-model` | L1 | Owns provider abstractions, model errors, and usage reporting. |
| `jyowo-harness-permission` | `crates/jyowo-harness-permission` | L1 | Owns PermissionBroker, rule providers, deduplication, fingerprints, and persistence. |
| `jyowo-harness-sandbox` | `crates/jyowo-harness-sandbox` | L1 | Owns sandbox policies, execution isolation, resource limits, and backend errors. |
| `jyowo-harness-context` | `crates/jyowo-harness-context` | L2 | Owns context assembly, compaction, token budget behavior, and context events. |
| `jyowo-harness-hook` | `crates/jyowo-harness-hook` | L2 | Owns hook execution, hook outcomes, and hook event contracts. |
| `jyowo-harness-mcp` | `crates/jyowo-harness-mcp` | L2 | Owns MCP connection state, tool injection, resource updates, sampling, and elicitation. |
| `jyowo-harness-session` | `crates/jyowo-harness-session` | L2 | Owns sessions, workspace bootstrap, stream handles, and session lifecycle. |
| `jyowo-harness-skill` | `crates/jyowo-harness-skill` | L2 | Owns skill loading, validation, threat detection, and invocation contracts. |
| `jyowo-harness-tool` | `crates/jyowo-harness-tool` | L2 | Owns Tool traits, registry, orchestration, built-ins, result budget, and permission checks. |
| `jyowo-harness-tool-search` | `crates/jyowo-harness-tool-search` | L2 | Owns on-demand tool search and schema materialization. |
| `jyowo-harness-engine` | `crates/jyowo-harness-engine` | L3 | Owns run orchestration, model/tool loop, budgets, and runtime event emission. |
| `jyowo-harness-observability` | `crates/jyowo-harness-observability` | L3 | Owns tracing, usage accounting, Replay helpers, and Redactor implementations. |
| `jyowo-harness-plugin` | `crates/jyowo-harness-plugin` | L3 | Owns plugin loading, manifest validation, and plugin rejection. |
| `jyowo-harness-subagent` | `crates/jyowo-harness-subagent` | L3 | Owns subagent lifecycle, permission forwarding, and stalled-worker behavior. |
| `jyowo-harness-agent-runtime` | `crates/jyowo-harness-agent-runtime` | L3 | Owns cross-domain agent orchestration storage, profile registry, capability policy inputs, background registry, team persistence, and workspace isolation leases. |
| `jyowo-harness-team` | `crates/jyowo-harness-team` | L3 | Owns multi-agent teams, member routing, topology, quotas, and team termination. |
| `jyowo-harness-sdk` | `crates/jyowo-harness-sdk` | L4 | Owns the business-facing facade, builder, prelude, builtins, and testing adapters. |

Rules:

- New public contract types belong in `jyowo-harness-contracts`.
- New primitive runtime capability crates belong in L1.
- New composite domains belong in L2.
- New orchestration across domains belongs in L3.
- Application-facing assembly belongs in `jyowo-harness-sdk`.
- Tauri command code must not reach around the SDK into lower layers unless the command is only exposing shell metadata.

## Agent Orchestration Engineering

Agent orchestration spans subagents, run-scoped teams, and durable background
agents. Ownership stays in Rust.

Layer placement:

- public agent option, capability, permission, team, and background event shapes
  belong in `jyowo-harness-contracts`.
- agent profile storage, capability policy inputs, durable background registry,
  task/mailbox tables, and workspace isolation leases belong in
  `jyowo-harness-agent-runtime`.
- child agent delegation and child permission forwarding belong in
  `jyowo-harness-subagent`.
- run-scoped team routing, member sessions, cancellation, and quotas belong in
  `jyowo-harness-team`.
- application composition and Tauri-facing facades belong in
  `jyowo-harness-sdk`.
- desktop command handlers in `jyowo-desktop-shell` stay thin and call the SDK.

Capability resolver rules:

- `resolve_agent_capabilities` is the source for `subagents`, `agentTeams`, and
  `backgroundAgents` availability.
- the resolver must include feature flags, runtime support, profile/model
  support, workspace state, and write-isolation requirements in its decision.
- disabled or unavailable capabilities must return backend-authored reason
  payloads.
- frontend settings may store requested defaults, but final availability is
  recomputed by Rust.

Worktree isolation rules:

- write-capable child, team, or background work must acquire a backend write
  lease before it runs.
- duplicate write leases for the same checkout fail closed.
- no Tauri command may mark a run as isolated unless the runtime acquired the
  lease.

Durable background agent rules:

- public background start uses `start_run` with `agentOptions.background`.
- separate background commands may list, read, pause, resume, cancel, send input,
  archive, and delete durable records.
- registry mutation must happen through `jyowo-harness-agent-runtime` and SDK
  facades.
- supervisor sidecar wakeups must revalidate queued payloads before execution.
- restart recovery must use durable registry and journal state, not live task
  handles.

Permission source attribution rules:

- foreground permissions use `parentRun`.
- child agent permissions use `subagent`.
- run-scoped team member permissions use `teamMember`.
- durable background agent permissions use `backgroundAgent`.
- command handlers and event projection must preserve actor source tags through
  the Rust contract and frontend Zod schema.

## Contracts

`harness-contracts` is the source of truth for backend-to-frontend and backend-to-backend public contracts.

Rules:

- Public payloads use `serde` derives.
- Stable schemas use `JsonSchema`.
- Event enums use explicit serde tags.
- Contract enums that can grow externally SHOULD be `#[non_exhaustive]`.
- Field renames require migration or compatibility handling.
- Error enums exposed across crate or IPC boundaries are contract surface.
- Tests must cover serialization shape, deserialization, and representative error variants.

Forbidden:

```text
ad hoc JSON assembled with string concatenation
frontend-only event names without Rust contract mapping
renaming serialized fields without tests
placing public contract structs in application crates
```

## Tauri Commands

Tauri command is an IPC boundary. It is not a place for business logic.

Rules:

- Command names use `snake_case`.
- Command payload structs use explicit `serde` shape.
- Command handlers stay thin.
- Validation happens at the Rust boundary before touching runtime state.
- New command output shape must be documented in backend and frontend docs.
- New command exposure must be registered in `generate_handler!`.
- Commands that touch files, network, tools, model providers, permissions, MCP, Memory, Journal, Replay, Audit, or Secret data require security review.

Current Tauri commands:

```text
add_project
archive_background_agent
cancel_background_agent
cancel_run
clear_mcp_diagnostics
create_attachment_from_path
create_conversation
delete_automation
delete_agent_profile
delete_background_agent
delete_conversation
delete_mcp_server
delete_memory_item
delete_project
delete_provider_capability_route
delete_skill
export_memory_items
export_support_bundle
list_artifacts
list_eval_cases
get_context_snapshot
get_artifact_media_preview
get_attachment_media_preview
get_app_info
get_background_agent
get_conversation
get_execution_settings
get_memory_item
get_mcp_server_config
get_plugin_detail
get_provider_config_api_key
get_replay_timeline
get_skill_catalog_entry
get_skill_catalog_file
get_skill_detail
get_skill_file
harness_healthcheck
install_plugin_from_path
list_activity
list_agent_profiles
list_background_agents
list_automation_runs
list_automations
list_conversations
list_browser_mcp_presets
list_skill_catalog_entries
list_skill_catalog_install_tasks
list_skill_catalog_sources
list_reference_candidates
list_model_provider_catalog
list_mcp_diagnostics
list_mcp_servers
list_memory_items
list_plugins
list_provider_capability_route_options
list_provider_capability_routes
list_provider_settings
list_projects
list_skills
page_conversation_timeline
page_conversation_worktree
pause_background_agent
resolve_permission
request_provider_config_api_key_reveal
resume_background_agent
restart_mcp_server
run_automation_now
run_eval_case
save_automation
save_agent_profile
save_browser_mcp_preset
save_mcp_server
save_provider_capability_route
save_provider_settings
send_background_agent_input
import_skill
install_skill_from_catalog
set_execution_settings
set_automation_enabled
set_conversation_model_config
set_mcp_server_enabled
set_skill_enabled
set_project_plugins_enabled
start_run
subscribe_conversation_events
subscribe_mcp_diagnostics
switch_project
unsubscribe_conversation_events
unsubscribe_mcp_diagnostics
uninstall_plugin
update_memory_item
reload_plugin
set_plugin_enabled
update_plugin_config
validate_plugin_from_path
validate_provider_settings
```

Command payloads:

```rust
add_project(path: String) -> Result<SwitchProjectResponse, CommandErrorPayload>
archive_background_agent(
  background_agent_id: String,
  conversation_id: Option<String>
) -> Result<BackgroundAgentActionResponse, CommandErrorPayload>
cancel_background_agent(
  background_agent_id: String,
  conversation_id: Option<String>
) -> Result<BackgroundAgentActionResponse, CommandErrorPayload>
cancel_run(run_id: String) -> Result<CancelRunResponse, CommandErrorPayload>
clear_mcp_diagnostics(
  server_id: Option<String>
) -> Result<ClearMcpDiagnosticsResponse, CommandErrorPayload>
create_attachment_from_path(
  path: String
) -> Result<CreateAttachmentFromPathResponse, CommandErrorPayload>
create_conversation() -> Result<CreateConversationResponse, CommandErrorPayload>
delete_automation(id: String) -> Result<DeleteAutomationResponse, CommandErrorPayload>
delete_agent_profile(id: String) -> Result<DeleteAgentProfileResponse, CommandErrorPayload>
delete_background_agent(
  background_agent_id: String,
  conversation_id: Option<String>
) -> Result<BackgroundAgentDeleteResponse, CommandErrorPayload>
delete_conversation(conversation_id: String) -> Result<DeleteConversationResponse, CommandErrorPayload>
delete_mcp_server(id: String) -> Result<DeleteMcpServerResponse, CommandErrorPayload>
delete_memory_item(id: String) -> Result<DeleteMemoryItemResponse, CommandErrorPayload>
delete_project(path: String) -> Result<DeleteProjectResponse, CommandErrorPayload>
delete_provider_capability_route(
  kind: CapabilityRouteKind,
  config_id: String,
  provider_id: String
) -> Result<DeleteProviderCapabilityRouteResponse, CommandErrorPayload>
delete_skill(id: String) -> Result<DeleteSkillResponse, CommandErrorPayload>
export_memory_items() -> Result<ExportMemoryItemsResponse, CommandErrorPayload>
export_support_bundle(
  conversation_id: Option<String>,
  run_id: Option<String>
) -> Result<ExportSupportBundleResponse, CommandErrorPayload>
list_artifacts(
  conversation_id: String
) -> Result<ListArtifactsResponse, CommandErrorPayload>
get_artifact_media_preview(
  conversation_id: String,
  artifact_id: String
) -> Result<GetArtifactMediaPreviewResponse, CommandErrorPayload>
get_attachment_media_preview(
  conversation_id: String,
  attachment_id: String
) -> Result<GetAttachmentMediaPreviewResponse, CommandErrorPayload>
list_eval_cases() -> Result<ListEvalCasesResponse, CommandErrorPayload>
get_context_snapshot(
  conversation_id: Option<String>,
  run_id: Option<String>
) -> Result<GetContextSnapshotResponse, CommandErrorPayload>
get_app_info() -> AppInfoPayload
get_background_agent(
  background_agent_id: String,
  conversation_id: Option<String>
) -> Result<GetBackgroundAgentResponse, CommandErrorPayload>
get_conversation(conversation_id: String) -> Result<GetConversationResponse, CommandErrorPayload>
get_execution_settings(workspace_path?: string) -> Result<GetExecutionSettingsResponse, CommandErrorPayload>
get_memory_item(id: String) -> Result<GetMemoryItemResponse, CommandErrorPayload>
get_mcp_server_config(id: String) -> Result<GetMcpServerConfigResponse, CommandErrorPayload>
get_plugin_detail(plugin_id: PluginId) -> Result<GetPluginDetailResponse, CommandErrorPayload>
get_provider_config_api_key(
  config_id: String,
  reveal_token: String
) -> Result<GetProviderConfigApiKeyResponse, CommandErrorPayload>
get_replay_timeline(
  conversation_id: Option<String>,
  run_id: Option<String>
) -> Result<ReplayTimelineResponse, CommandErrorPayload>
get_skill_detail(id: String) -> Result<GetSkillDetailResponse, CommandErrorPayload>
get_skill_file(
  id: String,
  path: String
) -> Result<GetSkillFileResponse, CommandErrorPayload>
get_skill_catalog_entry(
  source_id: String,
  entry_id: String,
  version: Option<String>
) -> Result<GetSkillCatalogEntryResponse, CommandErrorPayload>
get_skill_catalog_file(
  source_id: String,
  entry_id: String,
  version: Option<String>,
  path: String
) -> Result<GetSkillCatalogFileResponse, CommandErrorPayload>
harness_healthcheck() -> HarnessHealthcheckPayload
install_plugin_from_path(
  source_path: String
) -> Result<PluginOperationResult, CommandErrorPayload>
list_activity(
  conversation_id: Option<String>,
  run_id: Option<String>
) -> Result<ListActivityResponse, CommandErrorPayload>
list_agent_profiles() -> Result<ListAgentProfilesResponse, CommandErrorPayload>
list_background_agents(
  conversation_id: Option<String>,
  include_archived: bool
) -> Result<ListBackgroundAgentsResponse, CommandErrorPayload>
list_automation_runs(
  automation_id: Option<String>
) -> Result<ListAutomationRunsResponse, CommandErrorPayload>
list_automations() -> Result<ListAutomationsResponse, CommandErrorPayload>
list_browser_mcp_presets() -> Result<ListBrowserMcpPresetsResponse, CommandErrorPayload>
list_conversations() -> Result<ListConversationsResponse, CommandErrorPayload>
list_skill_catalog_entries(
  source_id: String,
  query: Option<String>,
  cursor: Option<String>,
  limit: Option<u32>,
  sort: Option<String>
) -> Result<ListSkillCatalogEntriesResponse, CommandErrorPayload>
list_skill_catalog_install_tasks() -> Result<ListSkillCatalogInstallTasksResponse, CommandErrorPayload>
list_skill_catalog_sources() -> Result<ListSkillCatalogSourcesResponse, CommandErrorPayload>
list_reference_candidates(
  conversation_id: String
) -> Result<ListReferenceCandidatesResponse, CommandErrorPayload>
list_model_provider_catalog() -> ModelProviderCatalogResponse
list_mcp_diagnostics(
  server_id: Option<String>
) -> Result<ListMcpDiagnosticsResponse, CommandErrorPayload>
list_mcp_servers() -> Result<ListMcpServersResponse, CommandErrorPayload>
list_memory_items() -> Result<ListMemoryItemsResponse, CommandErrorPayload>
list_plugins() -> Result<ListPluginsResponse, CommandErrorPayload>
list_provider_capability_route_options() -> Result<ListProviderCapabilityRouteOptionsResponse, CommandErrorPayload>
list_provider_capability_routes() -> Result<ListProviderCapabilityRoutesResponse, CommandErrorPayload>
list_provider_settings() -> Result<ListProviderSettingsResponse, CommandErrorPayload>
list_projects() -> ListProjectsResponse
list_skills() -> Result<ListSkillsResponse, CommandErrorPayload>
page_conversation_timeline(
  conversation_id: String,
  after_cursor: Option<ConversationCursor>,
  limit: Option<u32>
) -> Result<ConversationTimelinePage, CommandErrorPayload>
page_conversation_worktree(
  conversation_id: String,
  page_cursor: Option<ConversationTurnCursor>,
  direction: Option<PageConversationWorktreeDirection>,
  limit: Option<u32>
) -> Result<ConversationWorktreePage, CommandErrorPayload>
pause_background_agent(
  background_agent_id: String,
  conversation_id: Option<String>
) -> Result<BackgroundAgentActionResponse, CommandErrorPayload>
resolve_permission(
  decision: PermissionDecision,
  request_id: String
) -> Result<ResolvePermissionResponse, CommandErrorPayload>
request_provider_config_api_key_reveal(
  config_id: String
) -> Result<RequestProviderConfigApiKeyRevealResponse, CommandErrorPayload>
resume_background_agent(
  background_agent_id: String,
  conversation_id: Option<String>
) -> Result<BackgroundAgentActionResponse, CommandErrorPayload>
restart_mcp_server(id: String) -> Result<RestartMcpServerResponse, CommandErrorPayload>
run_automation_now(id: String) -> Result<RunAutomationNowResponse, CommandErrorPayload>
run_eval_case(case_id: String) -> Result<RunEvalCaseResponse, CommandErrorPayload>
save_automation(
  automation: AutomationSpec
) -> Result<SaveAutomationResponse, CommandErrorPayload>
save_agent_profile(
  profile: AgentProfile
) -> Result<SaveAgentProfileResponse, CommandErrorPayload>
save_browser_mcp_preset(
  preset_id: BrowserMcpPresetId,
  enabled: Option<bool>
) -> Result<SaveBrowserMcpPresetResponse, CommandErrorPayload>
save_mcp_server(
  enabled: Option<bool>,
  display_name: String,
  id: String,
  scope: String,
  transport: McpServerTransportConfig
) -> Result<SaveMcpServerResponse, CommandErrorPayload>
save_provider_capability_route(
  route: ProviderCapabilityRoute
) -> Result<SaveProviderCapabilityRouteResponse, CommandErrorPayload>
save_provider_settings(
  api_key: Option<String>,
  base_url: Option<String>,
  config_id: Option<String>,
  display_name: Option<String>,
  model_id: String,
  provider_id: String,
  set_default: Option<bool>
) -> Result<SaveProviderSettingsResponse, CommandErrorPayload>
send_background_agent_input(
  background_agent_id: String,
  conversation_id: Option<String>,
  input: String,
  request_id: String
) -> Result<BackgroundAgentActionResponse, CommandErrorPayload>
import_skill(source_path: String) -> Result<ImportSkillResponse, CommandErrorPayload>
install_skill_from_catalog(
  source_id: String,
  entry_id: String,
  version: Option<String>,
  operation_id: Option<String>
) -> Result<InstallSkillFromCatalogResponse, CommandErrorPayload>
// Starts an in-process background install task and returns task state immediately.
// Task payload: operationId, sourceId, entryId, version?, status, stage, percent,
// startedAt, updatedAt, message?.
// The shell also emits skill_catalog_install_progress while the task runs.
// Event delivery failure is telemetry-only and must not alter install policy.
set_execution_settings(
  permission_mode: PermissionMode,
  context_compression_trigger_ratio: f32
) -> Result<SetExecutionSettingsResponse, CommandErrorPayload>
set_automation_enabled(
  id: String,
  enabled: bool
) -> Result<SetAutomationEnabledResponse, CommandErrorPayload>
set_conversation_model_config(
  conversation_id: String,
  model_config_id: String
) -> Result<SetConversationModelConfigResponse, CommandErrorPayload>
set_mcp_server_enabled(
  id: String,
  enabled: bool
) -> Result<SetMcpServerEnabledResponse, CommandErrorPayload>
set_skill_enabled(
  id: String,
  enabled: bool
) -> Result<SetSkillEnabledResponse, CommandErrorPayload>
switch_project(path: String) -> Result<SwitchProjectResponse, CommandErrorPayload>
start_run(
  agent_options: Option<AgentRunOptions>,
  client_message_id: Option<String>,
  attachments: Option<Vec<AttachmentReferencePayload>>,
  context_references: Option<Vec<ContextReferencePayload>>,
  conversation_id: String,
  permission_mode: Option<PermissionMode>,
  prompt: String
) -> Result<StartRunResponse, CommandErrorPayload>
subscribe_conversation_events(
  conversation_id: String,
  after_cursor: Option<ConversationCursor>
) -> Result<SubscribeConversationEventsResponse, CommandErrorPayload>
subscribe_mcp_diagnostics(
  server_id: Option<String>
) -> Result<SubscribeMcpDiagnosticsResponse, CommandErrorPayload>
unsubscribe_conversation_events(
  subscription_id: String
) -> Result<UnsubscribeConversationEventsResponse, CommandErrorPayload>
unsubscribe_mcp_diagnostics(
  subscription_id: String
) -> Result<UnsubscribeMcpDiagnosticsResponse, CommandErrorPayload>
uninstall_plugin(plugin_id: PluginId) -> Result<PluginOperationResult, CommandErrorPayload>
update_memory_item(
  content: String,
  id: String
) -> Result<UpdateMemoryItemResponse, CommandErrorPayload>
reload_plugin(plugin_id: PluginId) -> Result<PluginOperationResult, CommandErrorPayload>
set_plugin_enabled(
  plugin_id: PluginId,
  enabled: bool
) -> Result<PluginOperationResult, CommandErrorPayload>
set_project_plugins_enabled(
  enabled: bool
) -> Result<SetProjectPluginsEnabledResponse, CommandErrorPayload>
update_plugin_config(
  plugin_id: PluginId,
  values: Value
) -> Result<PluginOperationResult, CommandErrorPayload>
validate_plugin_from_path(
  source_path: String
) -> Result<PluginInstallReport, CommandErrorPayload>
validate_provider_settings(
  model_id: String,
  provider_id: String
) -> Result<ValidateProviderSettingsResponse, CommandErrorPayload>
```

`validate_provider_settings` validates payload shape, provider id, and model
metadata support. It must not claim remote API availability unless the runtime
provider implements a policy-governed network health check.

`save_provider_settings` stores provider credentials in the workspace provider
settings record. `api_key` is required for new provider configs and optional
when saving an existing config without changing provider or base URL. The save
and list payloads must not return the raw key. `request_provider_config_api_key_reveal`
issues a short-lived one-use reveal token; `get_provider_config_api_key` requires
that token and is the explicit reveal path.

`list_provider_capability_routes`, `list_provider_capability_route_options`,
`save_provider_capability_route`, and `delete_provider_capability_route` manage
workspace capability routes stored in `.jyowo/runtime/provider-capability-routes.json`.
These commands must not return API keys, signed URLs, or provider-native payloads.
`list_provider_capability_route_options` is UX metadata only. It reports
`runtimeSupported` from descriptor-derived `ProviderServiceAdapterAvailability`,
not from provider catalog declarations alone. Save and delete validation remain
backend authority and must reject unknown configs, provider mismatches, missing
API keys, unsupported operations, and duplicate enabled route kinds.
`save_provider_capability_route` reloads runtime route settings for newly built
conversation harnesses. `start_run` must not carry route decisions.

`list_mcp_servers`, `get_mcp_server_config`, `save_mcp_server`,
`set_mcp_server_enabled`, `restart_mcp_server`, and `delete_mcp_server` expose
only sanitized MCP server management payloads. Workspace-managed config
supports `stdio` and `http`. `get_mcp_server_config` only returns
workspace-managed persisted records and must not expose plugin, policy, managed,
or runtime-only server internals.
`stdio` may persist non-sensitive inline env values and inherited env var
names. `http` may persist static non-sensitive headers and env var names for
bearer tokens or header values. It must not serialize raw env values,
authorization headers, bearer tokens, OAuth secrets, private absolute paths, or
tool-call arguments. Runtime tool exposure remains owned by the MCP registry
and `PermissionBroker`; Tauri only lists summaries and persists structured
config.

`list_mcp_diagnostics`, `clear_mcp_diagnostics`,
`subscribe_mcp_diagnostics`, and `unsubscribe_mcp_diagnostics` expose only
sanitized diagnostic records. Persisted diagnostics live under
`.jyowo/runtime/mcp-diagnostics.jsonl` and are bounded by a ring buffer. They
must contain severity, time, server id, event type, and summary only. They must
not contain raw MCP event payloads, raw OAuth data, raw `Authorization` or
`Cookie` headers, secret-like values, or private absolute paths.

`list_memory_items`, `get_memory_item`, `update_memory_item`,
`delete_memory_item`, and `export_memory_items` must go through the SDK Memory
facade. They must enforce tenant and actor visibility before returning,
editing, deleting, or exporting records. Delete and export operations must emit
audit events that contain hashes and counts, not raw memory content.
`export_memory_items` writes the JSON export under `.jyowo/runtime/exports` and
returns only the relative path, item count, format, and timestamp over IPC; raw
export content must not cross into frontend state.

`list_agent_profiles`, `save_agent_profile`, and `delete_agent_profile` must go
through the SDK agent-runtime facade and `jyowo-harness-agent-runtime` profile
registry. User and project profiles persist in
`.jyowo/runtime/agent-profiles.json`; profile metadata cache and validation state
persist in `.jyowo/runtime/agent-runtime.sqlite`. Builtin profiles are read-only.
Invalid profile files are quarantined before any list or save succeeds.

`list_skills`, `get_skill_detail`, `get_skill_file`, `import_skill`,
`set_skill_enabled`, and `delete_skill` must go through the SDK skill facade.
Tauri commands may manage the workspace skill store under
`.jyowo/runtime/skills`, but runtime registry reload, validation, and hook
replacement stay behind the SDK boundary. `list_skills` must return only
summaries. `get_skill_detail` may return manifest metadata and a relative file
index, but must not read file bodies. `get_skill_file` is the only command that
reads a selected package file. Imported source paths must not be returned over
IPC. `import_skill(source_path)` accepts only a local skill package directory
containing `SKILL.md`; single Markdown files are rejected. Workspace packages
are persisted as
`.jyowo/runtime/skills/enabled/<id>/SKILL.md` or
`.jyowo/runtime/skills/disabled/<id>/SKILL.md`, with package resources copied
under the same `<id>` directory. Disabled workspace skills remain in the store
index but must not be loaded into the runtime registry.
Skill catalog commands expose only the fixed official source set. Remote catalog
content must be fetched, scanned, materialized into a temporary package, and
then installed through the same managed skill store pipeline as local imports.
Remote source paths, package temp paths, and rejected scan payloads must not be
returned over IPC. Catalog install records may store source identity and
homepage metadata as `origin`, but that metadata does not upgrade the skill's
runtime trust.
Catalog detail may return `validation.status = "blocked"` for malformed or
non-installable remote entries, including entries without `SKILL.md`; that is a
displayable validation state and should not be converted into a command error.
`get_skill_catalog_file` reads one relative preview path from the selected
catalog entry, rejects empty, absolute, or parent-traversal paths, returns only
UTF-8 text, and truncates content using the same preview limit as catalog
detail.

`start_run` and `cancel_run` must go through the runtime conversation facade.
`resolve_permission` must go through `PermissionBroker`. These shell commands
return `RUNTIME_UNAVAILABLE` when those runtime paths are not available.

`set_execution_settings` stores the workspace default permission mode only.
It must not change conversation identity, session option hashes, or authorize
later runs by itself.

`start_run` accepts an optional `client_message_id` generated by the frontend
and an optional per-run `permission_mode`. The request permission mode wins
over the saved workspace default for that run only. If the request omits it,
Rust reads the saved default. `Auto` must still be validated by the Rust shell
and fail closed when the desktop build does not support auto mode.

The conversation event projection must echo `client_message_id` on
`user.message.appended` when it is present. Optimistic user message
confirmation depends on that id, not message body text. `RunStartedEvent` and
the `run.started` projection payload include the resolved permission mode so
Replay and Activity show the run snapshot.
`PermissionRequestedEvent` includes `auto_resolved` when a run authorization
mode automatically allowed a request; the projection exposes it as
`permission.requested.payload.autoResolved` so Activity can show an approved
audit record instead of a pending approval.

`BypassPermissions` / `DontAsk` skip interactive permission approval prompts,
but they do not bypass tenant scope, workspace scope, sandbox policy, Secret
redaction, payload validation, or hard policy deny rules.

`page_conversation_worktree` is the conversation canvas data source. It returns
`ConversationWorktreePage`, whose top-level items are complete conversation
turns. The projection is owned by Rust and exposed through the SDK facade.
The current SQLite read-model path replays the complete session timeline into
`ConversationTurn[]` before slicing by turn cursor. It does not read from
materialized worktree tables. `After` cursors point at the last returned turn;
`Before` cursors point at the first returned turn. Both directions return turns
in ascending conversation order.
Assistant work process is projected as `ProcessSegment` with UI-safe
`ProcessStep` entries for reasoning summaries, activity, command, file, diff,
tool, artifact, and synthesis states. Raw chain-of-thought remains private and
must not enter `ProcessSegment`, `TextSegment`, artifact metadata, or command
payloads. Legacy `ThinkingSegment` may be deserialized for compatibility, but
new conversation projection should use `ProcessSegment`.
`page_conversation_timeline` remains a raw execution surface for Activity,
Replay, and details views. `get_conversation.messages` must not drive
`ConversationCanvas`.

`subscribe_conversation_events` and `unsubscribe_conversation_events` expose the
conversation timeline event stream to the desktop shell. Subscription handlers
are thin Tauri boundaries around the runtime projection. The subscribe response
returns replay events first. Live `conversation_event_batch` emissions for the
same `subscription_id` start only after replay has been read. Emitted batches
are scoped to the calling Tauri window and selected conversation id.

Conversation event payloads must include:

```text
id
conversationSequence
runId
sequence
timestamp
type
source
visibility
payload
```

`conversationSequence` is the monotonic conversation order key derived from the
durable conversation event page order. `sequence` remains run-local validation
data and must not be used as the global timeline order.
`assistant.delta` payloads must include `messageId` and UI-safe `text`.
`assistant.thinking.delta` payloads may include `status`, `safeSummary`, or
`safeSummaryDelta`; they must not include raw thought `text`, provider-native
thinking payloads, signatures, tool arguments, or tool output.

Live subscription delivery is a single-process guarantee. The durable replay
and snapshot paths are the restart-stable guarantee. The desktop shell may poll
the runtime journal tail on a documented interval for live delivery, but
overflow, unknown ordering, or cursor mismatch must surface `gap: true` instead
of silently dropping events.

`list_artifacts` must require an explicit conversation id and read through that
runtime conversation projection, not a static demo payload. It must project only explicit artifact lifecycle events.
Artifact events whose `session_id` does not match the requested conversation are ignored.
Assistant replies, assistant deltas, and reasoning summaries are conversation
content, not artifacts.
Optional fields must be omitted instead of serialized as `null`.

`get_artifact_media_preview` must require an explicit conversation id and
artifact id. It may read the owned blob through runtime state only after
verifying the artifact belongs to that conversation. It returns only image
preview data as a data URL plus MIME type and byte count. Non-image artifacts,
missing artifacts, oversized previews, private paths, blob paths, remote URLs,
signed URLs, and provider-native payloads must fail closed and must not cross
IPC.

`get_attachment_media_preview` must require an explicit conversation id and
attachment id. It may read an attachment blob through runtime state only after
finding that attachment in `UserMessageAppended.attachments` for the requested
conversation. It returns only image preview data as a data URL plus MIME type
and byte count. PNG previews must strip nonessential PNG chunks. JPEG, GIF, and
WebP inputs must be decoded and re-encoded as PNG preview data. AVIF inputs
must pass Rust-side AVIF container validation and unsafe metadata checks before
returning AVIF preview data; AVIF files with descriptive or unsafe metadata must
fail closed. Non-image attachments, missing attachments, mismatched MIME
metadata, oversized previews, private paths, blob paths, remote URLs, signed
URLs, provider-native payloads, and blobs outside tenant scope or the current
session scope must fail closed and must not cross IPC.

`get_context_snapshot` must read through the runtime conversation projection and
workspace root. It may project current files, latest explicit artifact event,
pending runtime decisions, and next actions until a dedicated context snapshot
store exists. UI-visible workspace display fields must pass through Redactor
before IPC. Runtime read failures must return a fixed safe error message over
IPC.

`get_replay_timeline` and `export_support_bundle` must read through the Replay
and Journal projection path after Redactor has run. They require a conversation
scope and may optionally narrow by run id. Support bundle export writes under
`.jyowo/runtime/exports` and returns only redacted file metadata, counts, and
relative paths over IPC.

Forbidden:

```text
generic execute command
command string built from frontend input
command returning untyped serde_json::Value as the stable API
command reading or writing Secret values without a policy check
command bypassing PermissionBroker for tool or filesystem operations
```

## Provider Capability Routing

Capability routes are workspace-level policies that bind a `CapabilityRouteKind`
to a provider profile and provider operation ids.

Contracts live in `harness-contracts`:

```text
CapabilityRouteKind
ProviderCapabilityRoute
ProviderCapabilityRouteSettings
ProviderCapabilityRouteOption
ListProviderCapabilityRouteOptionsResponse
ToolServiceBinding
ProviderServiceAdapterAvailability
ProviderCredentialResolveContext.operation_id
ProviderCredentialResolveContext.route_kind
```

Persistence:

```text
.jyowo/runtime/provider-capability-routes.json
```

Route validation rules:

- `version == 1`
- missing route file normalizes to empty version 1 settings
- each route passes `validate_provider_capability_route`
- each enabled route references an existing provider config with an API key
- `route.provider_id == config.provider_id`
- every operation id is declared by the provider catalog for that provider
- every enabled operation has a registered runtime adapter
- the same enabled `CapabilityRouteKind` cannot point to multiple configs in one settings file

Service tool visibility:

- `ToolDescriptor.service_binding` identifies the provider service operation.
- `jyowo-harness-sdk` owns route-based service tool filtering during ToolPool assembly.
- Descriptors without `service_binding` are unaffected.
- Descriptors with `service_binding` are denylisted when no enabled matching route exists.
- Match by `route_kind`, `provider_id`, and `operation_id`.
- `jyowo-harness-engine` keeps only the existing tool-calling visibility gate.

Credential resolution:

- `DesktopProviderCredentialResolver` must resolve routed service operations from route settings and `route.config_id`.
- Routed service operations must not fall back to the default provider profile.
- Non-service tools may keep provider-only resolution where allowed.

Typed service artifacts:

- Completed provider service output uses `ToolResultPart::Artifact`.
- Async provider jobs use structured tool output with schema ref `provider_service_async_job.v1`.
- Engine artifact creation must read typed artifact output only.
- Provider adapters must not send raw artifact blob metadata to model providers.

Provider media download:

- Shared provider media download policy lives in `jyowo-harness-tool`.
- Only http/https URLs on an explicit provider allowlist or trusted signed CDN host are accepted.
- Redirects must be disabled or revalidated at every hop.
- Content length, response MIME, and sniffed MIME must match the expected artifact kind.
- Raw provider URLs must not become trusted artifact metadata.

Provider service onboarding checklist:

```text
[ ] official API docs verified
[ ] provider catalog service capability added
[ ] runtime adapter implemented
[ ] descriptor service binding added
[ ] route validation recognizes adapter through descriptor-derived service binding
[ ] backend route option command exposes only backend-evaluated runtime support
[ ] credential resolver passes operation id and route kind
[ ] artifact output typed
[ ] provider media download uses shared fail-closed URL/MIME policy
[ ] tests cover success and fail-closed errors
[ ] frontend eligibility shows only runnable options
```

## Runtime Bypass Rules

Backend code MUST NOT bypass:

- `PermissionBroker` for Tool, filesystem, network, sandbox, MCP, or destructive operations.
- `Redactor` before Journal persistence, Replay, logs, traces, or export.
- `Journal` for product trace events.
- tenant and workspace scope checks for Memory, Replay, and Audit reads.
- result budget handling for large Tool output.

Bypass code is allowed only for tests that explicitly use test adapters.

## Naming

Rust crate package names use `jyowo-harness-*`.

Rust library crate names use `harness_*` for harness crates and `jyowo_desktop_shell` for the Tauri shell.

Domain nouns should match contract names:

```text
Run
Event
Tool
Permission
MCP
Memory
Model
Journal
Replay
Audit
Secret
```

Avoid generic names:

```text
Manager
Processor
Handler
Data
Payload
```

`Payload` is allowed only at IPC edges where the type is an explicit command payload.
