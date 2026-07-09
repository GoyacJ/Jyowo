# 工具模块欠缺文档

本文用于指导后续工具模块开发与完善。

项目当前采用 AI 辅助开发。本文刻意写得较细，目标是让执行者不要漏掉工具、文件、契约、权限、schema、测试和宿主集成点。

## 适用范围

本文覆盖当前已梳理到的工具相关模块：

- `crates/jyowo-harness-contracts/src/tool.rs`
- `crates/jyowo-harness-contracts/src/enums.rs`
- `crates/jyowo-harness-contracts/src/capability.rs`
- `crates/jyowo-harness-contracts/src/deferred_tools.rs`
- `crates/jyowo-harness-contracts/src/events/tool.rs`
- `crates/jyowo-harness-contracts/src/runtime_execution_status.rs`
- `crates/jyowo-harness-contracts/src/tool_profile.rs`
- `crates/jyowo-harness-tool/src/tool.rs`
- `crates/jyowo-harness-tool/src/context.rs`
- `crates/jyowo-harness-tool/src/registry.rs`
- `crates/jyowo-harness-tool/src/builder.rs`
- `crates/jyowo-harness-tool/src/orchestrator.rs`
- `crates/jyowo-harness-tool/src/pool.rs`
- `crates/jyowo-harness-tool/src/result_budget.rs`
- `crates/jyowo-harness-tool/src/network_broker.rs`
- `crates/jyowo-harness-tool/src/process_registry.rs`
- `crates/jyowo-harness-tool/src/provider_media.rs`
- `crates/jyowo-harness-tool/src/skill_script.rs`
- `crates/jyowo-harness-tool/src/builtin/*.rs`
- `crates/jyowo-harness-tool/src/builtin/workspace_path.rs`
- `crates/jyowo-harness-tool-search/src/*.rs`
- `crates/jyowo-harness-tool-search/src/backends/*.rs`
- `crates/jyowo-harness-mcp/src/wrapper.rs`
- `crates/jyowo-harness-mcp/src/registry.rs`
- `crates/jyowo-harness-plugin/src/cargo_extension.rs`
- `crates/jyowo-harness-plugin/src/capability.rs`
- `crates/jyowo-harness-subagent/src/lib.rs`
- `crates/jyowo-harness-sdk/src/harness.rs`
- `crates/jyowo-harness-sdk/src/harness/tool_pool.rs`
- `crates/jyowo-harness-sdk/src/lib.rs`
- `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.tsx`
- `apps/desktop/src/features/conversation/timeline/*tool*`
- `apps/desktop/src/features/conversation/timeline/pending-tool-permission.ts`
- `scripts/check-tool-network-broker-boundary.mjs`
- `scripts/check-tool-network-broker-boundary.test.mjs`
- `crates/jyowo-harness-tool/tests/*.rs`
- `crates/jyowo-harness-tool-search/tests/*.rs`
- `crates/jyowo-harness-contracts/tests/tool_profile_contract.rs`
- `crates/jyowo-harness-sdk/tests/*tool*`

本文不是最终设计文档。它是欠缺清单和执行索引。

## 当前工具系统主线

### 核心执行链路

当前工具执行不是“拿到输入就执行”。主线是：

```text
model tool call input
  -> Tool::validate
  -> Tool::plan
  -> ToolActionPlan
  -> permission decision
  -> authorization ticket
  -> AuthorizedToolInput
  -> Tool::execute_authorized
  -> ToolStream
  -> ToolOrchestrator collect_stream
  -> ToolResultEnvelope
```

相关核心文件：

- `crates/jyowo-harness-tool/src/tool.rs`
- `crates/jyowo-harness-tool/src/orchestrator.rs`
- `crates/jyowo-harness-tool/src/context.rs`
- `crates/jyowo-harness-contracts/src/tool.rs`

关键约束：

- `Tool::descriptor()` 声明工具契约。
- `Tool::validate()` 校验输入。
- `Tool::plan()` 生成权限计划。
- `Tool::execute_authorized()` 只接受已授权输入。
- `AuthorizationTicketClaims` 绑定 `tenant_id`、`session_id`、`run_id`、`tool_use_id`、`tool_name`、`action_plan_hash`。
- `TicketLedger::consume()` 检查 ticket 是否存在、是否过期、是否已消费、claims 是否匹配。
- `AuthorizedToolInput::new()` 再校验 `tool_use_id`、`tool_name`、canonical action plan hash。

这条链路的设计意图是：批准的是某个具体 action plan，执行时不能偷偷换工具或换资源。

### 注册和工具发现

相关文件：

- `crates/jyowo-harness-tool/src/registry.rs`
- `crates/jyowo-harness-tool/src/builder.rs`
- `crates/jyowo-harness-tool/src/pool.rs`
- `crates/jyowo-harness-tool-search/src/search_tool.rs`
- `crates/jyowo-harness-tool-search/src/runtime.rs`

当前结构：

- `ToolRegistry` 持有 `BTreeMap<String, RegisteredTool>`。
- `ToolRegistryBuilder` 按 `BuiltinToolset` 注册默认工具。
- `ToolRegistrySnapshot` 提供只读快照、descriptor 列表、group 查询、generation。
- `ToolPool` 基于 registry 暴露给 orchestrator 和 SDK。
- `jyowo-harness-tool-search` 是工具搜索和 deferred tools 扩展，不替代 registry。
- SDK、MCP、plugin、subagent、team runtime 还会向 `ToolPool` 追加 runtime tools。

shadow 规则已经存在：

- builtin 与 builtin 重名时保留已有。
- builtin 与 plugin / MCP / skill 重名时，builtin 保留或替换为最终可见工具。
- 非 builtin 重名时，`AdminTrusted` 高于 user-controlled / 其他 trust。
- 非 builtin 且同 trust 重名时保留已有。
- 每次 shadow 会记录 `ShadowedRegistration`，包含 kept、rejected、reason、时间。

仍缺：

- shadow 规则缺完整生命周期文档。
- shadowed 记录是否进入 UI / telemetry / event stream 未定。
- plugin、MCP、skill、builtin、runtime appended tool 的重名矩阵还需要快照测试。

### ToolPool 装配过滤

registry 中存在的工具不等于本轮会进入 prompt 或可执行工具池。

SDK session 装配当前按以下顺序收窄工具：

1. `filter_unavailable_tools()`：按 descriptor 的 `required_capabilities` 过滤 capability 缺失工具。
2. `filter_unrouted_service_tools()`：按 descriptor 的 `service_binding` 和 provider capability routes 过滤没有路由的 provider service 工具。
3. `apply_tenant_tool_filter()`：按 tenant policy 的 allowed tools 过滤。
4. `ToolPoolFilter::from_profile()`：按 `ToolProfile` 过滤 group、MCP、plugin、allowlist、denylist。
5. `ToolPool::assemble()`：解析 dynamic schema，并按 tool search mode 拆分 always-loaded / deferred。

相关文件：

- `crates/jyowo-harness-sdk/src/harness/session_runtime.rs`
- `crates/jyowo-harness-sdk/src/harness/tool_pool.rs`
- `crates/jyowo-harness-tool/src/pool.rs`

runtime appended tools 还会在 `ToolPool::assemble()` 之后按 run policy 注入，例如 `tool_search`、`background_agent`、`agent_team`、subagent `agent` 和 team control tools。因此审计时要区分：

- registry-visible
- pool-visible
- prompt-visible
- deferred but materializable
- runtime-appended

### 工具 journal authority

工具不能任意写 journal 事件。orchestrator 会按 `ToolJournalAuthority` gate `ToolEvent::Journal`：

- `Bash`、`Diagnostics`、`ProcessStart` 使用 `ToolJournalAuthority::Sandbox`。
- `Clarify` 使用 `ToolJournalAuthority::Clarification`。
- `ExecuteCode` 使用 `ToolJournalAuthority::ExecuteCode`。
- 其他工具默认 `ToolJournalAuthority::None`。

未授权的 journal event 会返回 `PermissionDenied`，不会 emit 到 event stream。

已有覆盖：

- `crates/jyowo-harness-tool/tests/orchestrator.rs` 覆盖非 owner clarification / sandbox / execute_code event 被拒绝。
- `crates/jyowo-harness-tool/tests/registry_pool.rs` 覆盖 default builtin sandbox tools 的 authority 和 runtime same-name tool 不覆盖已有 authority。

仍缺：

- 按默认工具集输出完整 authority 快照。
- 对新增 runtime appended 工具明确 authority 策略。

### feature flags

相关文件：

- `crates/jyowo-harness-tool/Cargo.toml`
- `crates/jyowo-harness-tool/src/builder.rs`
- `crates/jyowo-harness-tool/src/builtin/mod.rs`

当前 feature：

```toml
default = ["builtin-toolset"]
skill-tools = []
builtin-toolset = ["jyowo-harness-permission/dangerous"]
programmatic-tool-calling = ["builtin-toolset", "jyowo-harness-sandbox/code-runtime"]
minimax-tools = ["builtin-toolset", ...]
seedance-tools = ["builtin-toolset", ...]
```

基础内置工具默认启用。消费者只有在显式使用 `default-features = false` 时，才会关闭 `builtin-toolset`。

需要注意：`programmatic-tool-calling`、`minimax-tools`、`seedance-tools` 仍然是可选 feature。默认工具集包含基础内置工具，不包含这些 provider 或程序化执行扩展。

技能工具有一个单独边界：

- `BuiltinToolset::Default` 下注册 `SkillsList`、`SkillsView`、`SkillsInvoke` 依赖 `builtin-toolset`。
- `BuiltinToolset::Skills` 会调用 `register_skill_tools()`，只要启用 `builtin-toolset` 或 `skill-tools` 任一 feature，就能注册这三个 skill 工具。
- 因此“默认工具集包含 skill 工具”和“只启用 skill 工具”是两条不同编译路径，测试要分别覆盖。

## 当前已存在的工具清单

### 基础内置工具

这些工具默认由 `BuiltinToolset::Default` 注册。前提是消费者没有显式设置 `default-features = false`。

文件和搜索：

- `FileRead` -> `crates/jyowo-harness-tool/src/builtin/read.rs`
- `FileEdit` -> `crates/jyowo-harness-tool/src/builtin/edit.rs`
- `FileWrite` -> `crates/jyowo-harness-tool/src/builtin/write.rs`
- `ListDir` -> `crates/jyowo-harness-tool/src/builtin/list_dir.rs`
- `Grep` -> `crates/jyowo-harness-tool/src/builtin/grep.rs`
- `Glob` -> `crates/jyowo-harness-tool/src/builtin/glob.rs`
- `ReadBlob` -> `crates/jyowo-harness-tool/src/builtin/read_blob.rs`

Git：

- `GitStatus` -> `crates/jyowo-harness-tool/src/builtin/git.rs`
- `GitDiff` -> `crates/jyowo-harness-tool/src/builtin/git.rs`
- `GitShow` -> `crates/jyowo-harness-tool/src/builtin/git.rs`
- `GitLog` -> `crates/jyowo-harness-tool/src/builtin/git.rs`
- `GitStage` -> `crates/jyowo-harness-tool/src/builtin/git.rs`
- `GitCommit` -> `crates/jyowo-harness-tool/src/builtin/git.rs`
- `GitBranch` -> `crates/jyowo-harness-tool/src/builtin/git.rs`
- `GitPull` -> `crates/jyowo-harness-tool/src/builtin/git.rs`
- `GitPush` -> `crates/jyowo-harness-tool/src/builtin/git.rs`

执行、诊断、进程：

- `Bash` -> `crates/jyowo-harness-tool/src/builtin/bash.rs`
- `Diagnostics` -> `crates/jyowo-harness-tool/src/builtin/diagnostics.rs`
- `ProcessStart` -> `crates/jyowo-harness-tool/src/builtin/process_monitor.rs`
- `ProcessRead` -> `crates/jyowo-harness-tool/src/builtin/process_monitor.rs`
- `ProcessStop` -> `crates/jyowo-harness-tool/src/builtin/process_monitor.rs`

网络：

- `WebFetch` -> `crates/jyowo-harness-tool/src/builtin/web_fetch.rs`
- `WebSearch` -> `crates/jyowo-harness-tool/src/builtin/web_search.rs`

对话、任务、记忆、技能：

- `Clarify` -> `crates/jyowo-harness-tool/src/builtin/clarify.rs`
- `SendMessage` -> `crates/jyowo-harness-tool/src/builtin/send_message.rs`
- `Todo` -> `crates/jyowo-harness-tool/src/builtin/todo.rs`
- `Memory` (`memory`) -> `crates/jyowo-harness-tool/src/builtin/memory.rs`
- `TaskStop` -> `crates/jyowo-harness-tool/src/builtin/task_stop.rs`
- `SkillsList` (`skills_list`) -> `crates/jyowo-harness-tool/src/builtin/skills.rs`
- `SkillsView` (`skills_view`) -> `crates/jyowo-harness-tool/src/builtin/skills.rs`
- `SkillsInvoke` (`skills_invoke`) -> `crates/jyowo-harness-tool/src/builtin/skills.rs`

宿主平台代理工具：

- `Worktree` -> `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`
- `Session` -> `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`
- `Artifact` -> `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`
- `BrowserUse` -> `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`
- `ComputerUse` -> `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`
- `ImageGeneration` -> `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`
- `NotebookEdit` -> `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`
- `LSP` -> `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`
- `Automation` -> `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`
- `Workflow` -> `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`

### 可选程序化执行工具

这些工具需要 `programmatic-tool-calling` feature。

- `ExecuteCode` -> `crates/jyowo-harness-tool/src/builtin/execute_code.rs`

### MiniMax 工具

这些工具需要 `minimax-tools` feature。

- `MiniMaxTextToImageTool`
- `MiniMaxImageToImageTool`
- `MiniMaxTextToVideoTool`
- `MiniMaxImageToVideoTool`
- `MiniMaxFirstLastFrameToVideoTool`

### Zhipu 工具

这些工具需要 `zhipu-tools` feature，并经由 `ToolNetworkBrokerCap` 访问智谱官方 API。

- `ZhipuImageGeneration` -> `crates/jyowo-harness-tool/src/builtin/zhipu.rs`
- `ZhipuImageGenerationAsync` -> `crates/jyowo-harness-tool/src/builtin/zhipu.rs`
- `ZhipuImageGenerationQuery` -> `crates/jyowo-harness-tool/src/builtin/zhipu.rs`
- `ZhipuVideoGeneration` -> `crates/jyowo-harness-tool/src/builtin/zhipu.rs`
- `ZhipuVideoGenerationQuery` -> `crates/jyowo-harness-tool/src/builtin/zhipu.rs`
- `ZhipuTextToSpeech` -> `crates/jyowo-harness-tool/src/builtin/zhipu.rs`
- `ZhipuSpeechToText` -> `crates/jyowo-harness-tool/src/builtin/zhipu.rs`
- `MiniMaxSubjectReferenceVideoTool`
- `MiniMaxVideoGenerationQueryTool`
- `MiniMaxVideoTemplateTool`
- `MiniMaxVideoTemplateQueryTool`
- `MiniMaxTextToSpeechTool`
- `MiniMaxTextToSpeechAsyncTool`
- `MiniMaxTextToSpeechAsyncQueryTool`
- `MiniMaxVoiceCloneTool`
- `MiniMaxVoiceDesignTool`
- `MiniMaxListVoicesTool`
- `MiniMaxDeleteVoiceTool`
- `MiniMaxLyricsGenerationTool`
- `MiniMaxMusicGenerationTool`
- `MiniMaxMusicCoverPreprocessTool`
- `MiniMaxFileUploadTool`
- `MiniMaxFileListTool`
- `MiniMaxFileRetrieveTool`
- `MiniMaxFileDeleteTool`
- `MiniMaxModelsListTool`
- `MiniMaxModelRetrieveTool`
- `MiniMaxResponsesTool`
- `MiniMaxResponsesInputTokensTool`
- `MiniMaxAnthropicMessagesTool`
- `MiniMaxAnthropicCountTokensTool`
- `MiniMaxAnthropicModelsListTool`
- `MiniMaxAnthropicModelRetrieveTool`

实现文件：

- `crates/jyowo-harness-tool/src/builtin/minimax.rs`
- `crates/jyowo-harness-tool/src/provider_media.rs`
- `crates/jyowo-harness-tool/src/provider_minimax.rs`

### Seedance 工具

这些工具需要 `seedance-tools` feature。

- `SeedanceTextToVideo`
- `SeedanceImageToVideo`
- `SeedanceVideoGenerationQueryTool`

实现文件：

- `crates/jyowo-harness-tool/src/builtin/seedance.rs`
- `crates/jyowo-harness-tool/src/provider_media.rs`

### 工具搜索和 deferred tools

这部分不是普通工具实现，而是工具池扩展能力。

相关文件：

- `crates/jyowo-harness-tool-search/src/search_tool.rs`
- `crates/jyowo-harness-tool-search/src/runtime.rs`
- `crates/jyowo-harness-tool-search/src/backend.rs`
- `crates/jyowo-harness-tool-search/src/backends/anthropic.rs`
- `crates/jyowo-harness-tool-search/src/coalescer.rs`
- `crates/jyowo-harness-tool-search/src/delta.rs`
- `crates/jyowo-harness-tool-search/src/scorer.rs`
- `crates/jyowo-harness-contracts/src/deferred_tools.rs`

当前已看到的能力：

- `tool_search` 本身是 runtime tool，descriptor 包含 output schema，预算 metric 是 `Bytes`。
- deferred tools delta attachment。
- tool search backend。
- policy / scorer / coalescer 测试。
- `force defer` 在 tool search disabled 时应失败的测试。

### SDK / runtime 注入工具

这些工具不在 `jyowo-harness-tool` 的默认 builder 注册列表中，但会进入实际 `ToolPool`，因此也属于工具系统审计范围。

- `tool_search` -> `crates/jyowo-harness-tool-search/src/search_tool.rs`
- `background_agent` -> `crates/jyowo-harness-sdk/src/harness/tool_pool.rs`
- `agent_team` -> `crates/jyowo-harness-sdk/src/harness/tool_pool.rs`
- `agent` -> `crates/jyowo-harness-subagent/src/lib.rs`
- `dispatch` -> `crates/jyowo-harness-sdk/src/lib.rs`
- `message` -> `crates/jyowo-harness-sdk/src/lib.rs`
- `pause_worker` -> `crates/jyowo-harness-sdk/src/lib.rs`
- `resume_worker` -> `crates/jyowo-harness-sdk/src/lib.rs`
- `spawn_worker` -> `crates/jyowo-harness-sdk/src/lib.rs`
- `stop_team` -> `crates/jyowo-harness-sdk/src/lib.rs`
- `team_status` -> `crates/jyowo-harness-sdk/src/lib.rs`

### MCP / plugin 工具包装层

这些工具由外部注册源生成 descriptor，并进入同一套 registry / pool / orchestrator 流程。

- MCP wrapper 保留上游 `input_schema` 和 `output_schema`，并设置 `metadata.integration_source = Mcp`。
- MCP authorization 会建模 `ActionResource::McpTool`、`McpResource`、`McpPrompt`、`McpTransport`、`McpSampling`。
- plugin sidecar tool 使用 manifest 中的 `input_schema` 做 JSON Schema validate，并设置 `metadata.integration_source = Plugin`。
- plugin sidecar tool 当前 `output_schema` 仍为 `None`。

## 能力依赖清单

工具注册成功不代表能执行成功。很多工具依赖 `ToolContext.cap_registry` 中的 capability。

| Capability | 影响工具 | 需要补齐的开发工作 |
|---|---|---|
| `BrokeredPlatformRuntimeCap` | `Worktree`、`Session`、`Artifact`、`BrowserUse`、`ComputerUse`、`ImageGeneration`、`NotebookEdit`、`LSP`、`Automation`、`Workflow` | 宿主 runtime 必须实现并注册。工具列表展示前应能提示 capability 是否可用。 |
| `ToolNetworkBrokerCap` | `WebFetch`、MiniMax、Seedance | 所有外部网络访问必须通过 broker。MiniMax / Seedance 的 descriptor 不把它列入 `required_capabilities`，但执行时会从 `ToolContext` 获取 broker。需要持续用边界测试防回归。 |
| `WebSearchBackend` | `WebSearch` | 需要确认 backend 是否执行等价的网络权限、redaction、审计。 |
| `DiagnosticsRunnerCap` | `Diagnostics` | 需要确认 runner 的工作目录、命令来源、输出清洗、超时策略。 |
| `ClarifyChannelCap` | `Clarify` | 需要确认 UI 通道存在时才展示工具，或执行前给出清晰错误。 |
| `MemoryToolRuntimeCap` | `Memory` | 需要确认记忆写入、可见性、线程设置、敏感内容过滤。 |
| `UserMessengerCap` | `SendMessage` | 需要确认 outbound message 通道、目标、审计和失败语义。 |
| `TodoStoreCap` | `Todo` | 需要确认 todo 合并、状态约束、持久化和并发更新语义。 |
| `RunCancellerCap` | `TaskStop` | 需要确认 stop 作用域、重复停止和当前 run 终止语义。 |
| `SkillRegistryCap` | `SkillsList`、`SkillsView`、`SkillsInvoke` | 需要确认 skill 可见性、渲染、参数校验和来源边界。 |
| `ContextPatchSinkCap` | `SkillsInvoke` | 需要确认 skill 注入后的 context patch 生命周期、去重和撤销语义。 |
| `RunScopedProcessRegistryCap` | `ProcessStart`、`ProcessRead`、`ProcessStop` | 需要确认进程 id 作用域、输出读取、停止语义和 redaction。 |
| `ProviderCredentialResolverCap` | MiniMax、Seedance | descriptor 会用它做 `required_capabilities`。需要确认凭证解析、provider 限制、错误脱敏和 route 选择。 |
| `BlobWriterCap` | MiniMax / Seedance 的 media 或 async query 工具 | descriptor 对产出 media/blob 的工具会声明它。需要确认写入、retention、content-type 和 artifact 引用。 |
| `BlobReaderCap` | `ReadBlob`、orchestrator offload 读取 | 需要确认 blob 读取权限、offset/limit、过期和二进制输出策略。 |
| `OffloadedBlobAuthorizerCap` | `ReadBlob` | 需要确认 offloaded blob 只允许当前租户、会话、run 或授权范围读取。 |
| `CodeRuntimeCap` | `execute_code` | 需要确认程序化执行 runtime、资源限制和语言隔离。 |
| `EmbeddedToolDispatcherCap` | `execute_code` | 需要确认代码内嵌工具调用的权限继承、审计和循环限制。 |
| `ToolSearchRuntimeCap` | `tool_search` | descriptor 当前 `required_capabilities` 为空，但执行时会读取该 runtime capability。需要确认 SDK 注入和 disabled 模式行为。 |
| `ToolCapability::SubagentRunner` | `agent` | 需要确认 subagent lifecycle、输出契约、权限继承和失败恢复。 |
| `jyowo.background_agent.starter` | `background_agent` | 需要确认后台 agent capability readiness、权限模式、session snapshot 和输出契约。 |
| `jyowo.agent_team.runner` | `agent_team` | 需要确认 team runner readiness、成员生命周期、结果契约和停止语义。 |
| sandbox backend | `Bash`、`ProcessStart`、`ExecuteCode`、部分文件/Git 操作 | 需要确认 sandbox policy、workspace scope、超时、interrupt 均生效。 |
| blob store | 大输出、offload、`ReadBlob` | 需要确认 offload 后可读、权限、retention、redaction。 |

另有一条不是 capability registry 的过滤路径：

- MiniMax / Seedance 部分 descriptor 会设置 `service_binding`。
- SDK session assembly 会用 provider capability routes 过滤没有启用 route 的 service-bound tools。
- 这意味着 `ProviderCredentialResolverCap` 存在也不保证 provider 工具会出现在 prompt 中。
- 需要把 service binding / route gating 写入 provider 工具文档和测试矩阵。

## 欠缺项总览

### G1. 默认工具集已默认开启，剩余问题是测试和下游配置对齐

原问题：

- 之前 `crates/jyowo-harness-tool/Cargo.toml` 使用 `default = []`。
- `BuiltinToolset::Default` 的基础内置工具注册都在 `#[cfg(feature = "builtin-toolset")]` 下。
- 这会导致普通消费者调用 `ToolRegistry::builder().build()` 时，看起来使用默认工具集，实际可能得到空 registry。
- 这个风险已经通过默认开启 `builtin-toolset` 处理。

当前状态：

- `crates/jyowo-harness-tool/Cargo.toml` 已改为 `default = ["builtin-toolset"]`。
- 默认编译 `jyowo-harness-tool` 时会启用基础内置工具。
- `ToolRegistry::builder().build()` 在默认 feature 下应返回包含基础内置工具的 registry。
- `default-features = false` 现在应被视为显式 opt-out，而不是普通默认路径。

边界：

- 下游 crate 如果继续使用 `default-features = false`，仍然需要手动声明 `builtin-toolset`。
- `apps/desktop/src-tauri/Cargo.toml` 当前仍显式使用 `default-features = false`，并手动启用 `builtin-toolset`、`minimax-tools`、`seedance-tools`。这是可行配置，但不会受工具 crate 默认 feature 自动影响。
- `programmatic-tool-calling`、`minimax-tools`、`seedance-tools` 仍不是默认 feature。
- 工具默认注册不等于运行时 capability 已存在。`BrokeredPlatformRuntimeCap`、`ToolNetworkBrokerCap`、`WebSearchBackend` 等仍需要宿主注册。

后续 checklist：

- [x] 将 `builtin-toolset` 加入 `jyowo-harness-tool` 默认 feature。
- [x] 已有测试覆盖默认 feature 下 builder 会注册关键基础内置工具。
- [x] 新增完整快照测试：默认工具集名称和数量稳定。
- [x] 新增编译路径测试：`default-features = false` 时 builder 行为符合预期。
- [x] 新增编译路径测试：只启用 `skill-tools` 时 `BuiltinToolset::Skills` 可注册 `SkillsList`、`SkillsView`、`SkillsInvoke`。
- [ ] 检查下游 crate 是否仍需要 `default-features = false`。
- [ ] 如果下游保留 `default-features = false`，确认它们都显式声明了需要的工具 feature。

### G2. brokered platform 工具不是完整实现，只是宿主转发层

相关文件：

- `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`

当前状态：

- `BrokeredPlatformTool` 统一实现 validate / plan / execute。
- `execute_authorized()` 把 `BrokeredPlatformRuntimeRequest` 交给宿主 runtime。
- 工具 crate 内没有真实执行 worktree、browser、computer、image、notebook、LSP、automation、workflow。
- descriptor 已通过 `required_capabilities` 标记 `BrokeredPlatformRuntimeCap`。
- SDK tool pool 已能按 `required_capabilities` 过滤不可用工具。
- SDK runtime status 当前只报告 `Bash`、`Diagnostics`、`WebFetch`、`WebSearch`、`MiniMaxTextToImage`、`SeedanceTextToVideo`、`SendMessage` 的 readiness，不报告任一 brokered platform 工具。

影响：

- 工具注册成功不代表可用。
- capability 缺失时执行失败。
- 工具 schema 和权限 plan 很难覆盖每个具体 action。

处理方向：

- [x] 在 descriptor 中标记 brokered 工具需要宿主 capability。
- [x] SDK tool pool 按 capability 缺失过滤不可用工具。
- [ ] UI 层展示 capability missing 的可操作错误，而不是普通失败。
- [ ] 扩展 SDK runtime status，覆盖所有 brokered platform 工具。
- [ ] 为每个 brokered 工具补宿主 runtime contract 文档。
- [ ] 为每个 brokered 工具补 host-side integration test 或 mock runtime test。

### G3. brokered platform 工具 input schema 太宽

当前例子：

- `Worktree` 只有 `action` required，`action` 是自由字符串。
- `Session` 只有 `action` required，字段依赖 action 但 schema 不表达。
- `BrowserUse`、`ComputerUse`、`Automation` 同样依赖自由字符串 action。
- `NotebookEdit` 只要求 `path`，可选 `operation` 是自由字符串；它当前不是 `action` 字段。
- `Workflow` required 是 `name`，但 `params` 没有按 workflow schema 约束。

影响：

- 模型容易传错 action。
- validate 只检查 object，不能早发现错误。
- permission review 只能按整个 tool invocation 展示，很难细化到资源。
- 宿主 runtime 承担过多校验责任。

处理方向：

- [ ] 为每个 brokered 工具定义 action / operation enum。
- [ ] 使用 `oneOf` / `anyOf` 表达不同 action / operation 的 required 字段。
- [ ] 为高风险 action / operation 增加确认文案。
- [ ] 为不同 action / operation 生成不同 `ActionResource`。
- [ ] 为 host runtime 增加 schema compatibility test。
- [ ] 为错误 action、缺字段、多字段、类型错误补 validate 测试。

### G4. 多数工具缺 output schema

相关文件：

- `crates/jyowo-harness-tool/src/builtin/mod.rs`

当前 helper 默认：

```rust
output_schema: None,
dynamic_schema: false,
```

例外：

- `tool_search` 已有 output schema。
- MCP wrapper 会保留上游 MCP tool 的 `output_schema`。
- 测试用 tool 可自定义 output schema。

仍缺：

- 基础 builtin helper 注册的多数工具仍为 `output_schema: None`。
- SDK runtime 注入的 `agent`、`background_agent`、`agent_team`、team control tools 当前也未声明 output schema。
- plugin sidecar tool 当前未从 manifest 填充 output schema。

影响：

- UI 只能猜测结构化结果。
- SDK 消费者缺少稳定输出契约。
- provider adapter 难以理解输出 artifact。
- regression test 不容易覆盖输出字段变化。

优先级：

1. `FileRead`
2. `FileWrite`
3. `FileEdit`
4. `ListDir`
5. `Grep`
6. `Glob`
7. `ReadBlob`
8. `GitStatus`
9. `GitDiff`
10. `GitShow`
11. `GitLog`
12. `ProcessStart`
13. `ProcessRead`
14. `ProcessStop`
15. `Diagnostics`
16. `WebFetch`
17. `WebSearch`
18. `Memory`
19. `Todo`
20. `SkillsList`
21. `SkillsView`
22. `SkillsInvoke`

处理方向：

- [x] 给 `ToolDescriptor` 增加或填充 output schema。
- [ ] 对结构化输出建立 Rust 类型。
- [ ] 对每个 output schema 增加 contract test。
- [x] UI 只依赖 schema 中声明的字段。
- [x] 对 Mixed / Blob / Offloaded 输出写清楚 schema 表达方式。

### G5. input schema 默认允许未知字段

相关文件：

- `crates/jyowo-harness-tool/src/builtin/mod.rs`

当前 `object_schema()`：

```rust
json!({
    "type": "object",
    "required": required,
    "properties": properties
})
```

没有统一设置：

```json
"additionalProperties": false
```

需要区分两层契约：

- `input_schema` 是 descriptor contract，主要影响 prompt、SDK、UI 和 contract test。
- orchestrator 不会对 builtin 自动跑通用 JSON Schema validator。
- 实际执行路径是 `ToolOrchestrator -> tool.validate()`，由每个工具自己的 `validate()` 决定是否拒绝未知字段。
- MCP wrapper 和 plugin sidecar tool 会使用 JSON Schema validator；多数 builtin 不是这条路径。

已知例外：

- SDK runtime 注入的 `background_agent`、`agent_team` 已设置 `additionalProperties: false`。
- MCP wrapper 和 plugin sidecar tool 会按 descriptor schema 做 JSON Schema validate。

仍缺：

- `agent` input schema 没有 `additionalProperties: false`。
- team control tools 当前 input schema 只有 `{ "type": "object" }`。
- 基础 builtin 的 `additionalProperties: false` 即使补在 descriptor，也不会自动保证运行时拒绝未知字段，除非同步补统一 validator 或 per-tool `validate()`。

影响：

- 未知字段可能被接受。
- 模型输入拼错字段时不一定失败。
- permission plan 可能没有覆盖未知字段里的意图。

处理方向：

- [ ] 决定默认严格还是按工具定制。
- [ ] 决定运行时校验责任：统一 JSON Schema validator，还是每个工具的 `validate()` 明确拒绝未知字段。
- [ ] 如果默认严格，在 `object_schema()` 中加入 `additionalProperties: false`，并同步更新对应 `validate()`。
- [ ] 如果个别工具需要透传对象，显式允许并在文档中说明。
- [ ] 所有工具补 descriptor schema strictness 测试。
- [ ] 所有工具补 validate 行为测试，覆盖“未知字段被拒绝 / 被允许”。
- [ ] 检查 brokered platform、MiniMax、Seedance 是否依赖透传字段。

### G6. permission plan 的资源建模需要逐工具审计

相关文件：

- `crates/jyowo-harness-tool/src/builtin/mod.rs`
- `crates/jyowo-harness-tool/src/tool.rs`
- `crates/jyowo-harness-contracts/src/tool.rs`
- 所有 `builtin/*.rs`

当前 helper `generic_action_plan()` 使用空资源：

```rust
Vec::new(),
WorkspaceAccess::None,
NetworkAccess::None,
```

已有建模：

- 文件读类工具使用 `ActionResource::FileRead`。
- 文件写类工具使用 `ActionResource::FileWrite`。
- `Bash`、`ProcessStart`、`Diagnostics`、Git 工具使用 `ActionResource::Command`。
- `WebFetch`、`WebSearch`、MiniMax、Seedance 使用 `ActionResource::Network` 或网络访问计划。
- `SendMessage` 使用 `ActionResource::Network`、`ToolCapability::UserMessenger` 和 `ExternalCapability` execution channel；它的 `NetworkAccess` 是空 allowlist，用来表达宿主消息通道，不代表直接外网 broker。
- MCP authorization 已建模 MCP transport/tool/resource/prompt/sampling 等资源。

仍缺或需细化：

- `generic_action_plan()` 使用空资源，`Todo`、`TaskStop`、`ReadBlob`、`Skills*`、`Memory`、`ProcessRead`、`ProcessStop` 等需要逐个确认。
- 当前 `ActionResource` enum 没有 Memory / Process / TeamControl 资源变体。`Memory`、`ProcessRead`、`ProcessStop`、runtime 注入工具和 team control tools 如果要补真实资源，可能需要先扩展 contracts。
- brokered platform 当前按整个 tool invocation 授权，未按 action 拆资源。
- Git pull/push 的网络语义当前主要通过 command 表达，是否需要显式 network resource 仍需确认。
- permission review details 和 snapshot test 覆盖不完整。

影响：

- 权限 UI 可能看不到具体文件、命令、网络 host、MCP 资源。
- persisted decision scope 可能过宽。
- action plan hash 可以防篡改，但如果资源本身没建模，审计粒度仍不足。

处理方向：

- [x] 文件读写工具已建模 `ActionResource::FileRead` / `FileWrite`。
- [x] `Bash`、`ProcessStart`、`Diagnostics`、Git 工具已建模 `ActionResource::Command`。
- [x] `WebFetch`、`WebSearch`、MiniMax、Seedance 已建模网络资源或网络访问计划。
- [x] `SendMessage` 已建模网络资源和 `UserMessenger` 外部能力通道。
- [ ] 审计 `generic_action_plan()` 使用者，确认哪些必须补资源。
- [ ] 为 Memory / Process / TeamControl 等缺失资源先补 `ActionResource` contract，再补工具 plan。
- [ ] 为 brokered platform 高风险 action / operation 建模对应资源。
- [ ] 为 `Memory` 写入建模记忆 subject。
- [ ] 为 `ComputerUse` / `BrowserUse` 增加外部交互提示。
- [ ] 为每个高风险工具补 permission review snapshot test。

### G7. `ToolDescriptorMetadata` 使用不均匀

相关文件：

- `crates/jyowo-harness-contracts/src/tool.rs`
- `crates/jyowo-harness-tool/src/builtin/mod.rs`
- `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`

当前情况：

- `ToolDescriptorMetadata` 已有 aliases、families、platforms、examples、risk_level、effects、modalities、integration_source。
- brokered platform 工具有较完整 metadata。
- Git 等少量普通内置工具已有 searchable metadata。
- 其余普通内置工具大多通过 helper 得到默认 metadata。
- `search_hint` 默认为 `None`。

影响：

- tool search 排序信号不足。
- deferred tools 无法充分解释为什么加载某个工具。
- UI 无法稳定展示风险、类型、别名、适用场景。

处理方向：

- [ ] 为所有基础工具补 `aliases`。
- [ ] 为所有基础工具补 `families`。
- [ ] 为所有基础工具补 `risk_level`。
- [ ] 为所有基础工具补 `effects`。
- [ ] 为所有基础工具补 `modalities`。
- [ ] 为所有基础工具补 `integration_source`。
- [ ] 为所有基础工具补 `search_hint`。
- [ ] 更新 `tool_profile_contract` 测试，防止 metadata 回退。

### G8. 网络边界存在两套路径

相关文件：

- `crates/jyowo-harness-tool/src/builtin/web_fetch.rs`
- `crates/jyowo-harness-tool/src/builtin/web_search.rs`
- `crates/jyowo-harness-tool/src/builtin/minimax.rs`
- `crates/jyowo-harness-tool/src/builtin/seedance.rs`
- `crates/jyowo-harness-tool/src/network_broker.rs`
- `scripts/check-tool-network-broker-boundary.mjs`

当前情况：

- `WebFetch` 走 `ToolNetworkBrokerCap`。
- MiniMax 和 Seedance 走 `ToolNetworkBrokerCap`。
- `WebSearch` 走 `WebSearchBackend`。

影响：

- 如果 `WebSearchBackend` 内部没有走同等权限和 redaction 策略，网络边界会不一致。
- 静态边界测试可能覆盖不到 backend 内部实现。

处理方向：

- [x] 明确 `WebSearchBackend` 是否必须通过 network broker。
- [ ] 如果必须，调整 trait 或 runtime 实现。
- [x] 如果不必须，补安全说明和单独 permission policy。
- [x] 扩展 `check-tool-network-broker-boundary` 覆盖 WebSearch backend。
- [ ] 增加 host、scheme、redirect、content-type、body-size 测试。

### G9. long-running、heartbeat、timeout 策略需要逐工具确认

相关文件：

- `crates/jyowo-harness-tool/src/orchestrator.rs`
- `crates/jyowo-harness-contracts/src/tool.rs`
- `crates/jyowo-harness-tool/src/builtin/process_monitor.rs`
- `crates/jyowo-harness-tool/src/builtin/bash.rs`
- `crates/jyowo-harness-tool/src/builtin/execute_code.rs`
- `crates/jyowo-harness-tool/src/builtin/minimax.rs`
- `crates/jyowo-harness-tool/src/builtin/seedance.rs`

当前 descriptor helper 默认：

```rust
long_running: None,
```

`ToolOrchestrator` 已支持：

- `long_running_policy.stall_threshold`
- heartbeat
- `hard_timeout`
- interrupt

欠缺点：

- 需要确认哪些工具应该声明 long-running。
- 需要确认 async query 型 provider 工具是否应该使用 heartbeat。
- 需要确认外部 brokered 工具是否要 host-side timeout。

处理方向：

- [x] 为 `Bash` 定义 timeout 和 heartbeat 策略。
- [x] 为 `ProcessStart` / `ProcessRead` / `ProcessStop` 定义 timeout 和 heartbeat 策略。
- [x] 为 `ExecuteCode` 定义 timeout 和 sandbox interrupt 策略。
- [x] 为 MiniMax 视频、音乐、TTS async 工具定义长任务策略。
- [x] 为 Seedance 视频任务定义长任务策略。
- [ ] 为 brokered `BrowserUse` / `ComputerUse` / `Workflow` 定义宿主超时。
- [ ] 增加超时、interrupt、partial progress 测试。

### G10. result budget 和 offload 需要按工具校准

相关文件：

- `crates/jyowo-harness-tool/src/result_budget.rs`
- `crates/jyowo-harness-tool/src/orchestrator.rs`
- `crates/jyowo-harness-tool/src/builtin/read_blob.rs`
- `crates/jyowo-harness-tool/tests/result_budget.rs`

当前 helper 默认：

- metric: chars
- overflow: offload
- preview head: 2000 chars
- preview tail: 2000 chars

例外：

- `tool_search` 使用 `BudgetMetric::Bytes`，limit 为 32 KiB，并且已有 output schema。
- result budget / offload 已有核心测试。
- `ReadBlob` 已有 capability、授权、offset/limit 等测试。

欠缺点：

- 文件读、grep、git diff、web fetch、process output 的预算不应完全相同。
- 二进制、图片、音频、视频、blob 输出需要更明确的预算表达。
- provider 工具的 artifact 输出要明确是 inline、blob、URL 还是 mixed。

处理方向：

- [x] 给文本型大输出工具单独设置预算。
- [x] 给二进制 / provider media 工具设置 blob-first 策略。
- [x] 给 process output 设置 stdout/stderr 分离预算。
- [ ] 给 Git diff 设置 patch-aware preview。
- [x] 给 WebFetch 设置 content-type aware budget。
- [x] 补 offload 写入与 `ReadBlob` 恢复读取之间的端到端集成测试。

### G11. dynamic schema 能力尚未充分使用

相关文件：

- `crates/jyowo-harness-tool/src/tool.rs`
- `crates/jyowo-harness-tool/src/builtin/skills.rs`
- `crates/jyowo-harness-tool/src/builtin/brokered_platform.rs`

当前 `Tool::resolve_schema()` 默认返回静态 schema。descriptor helper 默认 `dynamic_schema: false`。

潜在需要 dynamic schema 的工具：

- `Workflow`：不同 workflow 的参数不同。
- `Automation`：不同 schedule/action 的参数不同。
- `SkillsInvoke`：不同 skill 的参数不同。
- `MiniMax*`：不同模型支持的参数可能不同。
- `Seedance*`：不同模型支持的参数可能不同。
- `LSP`：不同 action 的参数不同。

处理方向：

- [ ] 明确哪些工具需要 dynamic schema。
- [ ] 为这些工具实现 `resolve_schema()`。
- [ ] 给 schema resolver 加 context-aware cache。
- [x] 给动态 schema 加 contract test。
- [ ] UI 和 SDK 支持刷新动态 schema。

### G12. deferred tools 依赖 metadata，但当前信号不足

相关文件：

- `crates/jyowo-harness-tool-search/src/scorer.rs`
- `crates/jyowo-harness-tool-search/src/search_tool.rs`
- `crates/jyowo-harness-tool-search/src/coalescer.rs`
- `crates/jyowo-harness-contracts/src/deferred_tools.rs`

影响：

- 工具检索可能漏掉工具。
- 工具检索可能加载过多工具。
- 工具池变更 delta 难以给用户解释。

当前已实现：

- scorer 已使用 descriptor metadata、group filter、search hint、description、name parts、risk/source 等 filter。
- scorer 已有 metadata、search hint、description、discovered penalty 相关测试。
- `tool_search` 已有 output schema contract 测试。

仍缺：

- required capabilities 没有进入 scorer 加权。
- 搜索结果还不能解释具体命中字段。
- deferred delta 缺少面向用户的 reason。

处理方向：

- [ ] 所有工具补 metadata。
- [x] scorer 已使用 descriptor metadata、group、risk/source 等 filter。
- [x] 评估 required capabilities 是否应进入 scorer 加权或过滤。
- [x] 搜索结果解释包含命中字段。
- [x] deferred delta 中加入可读 reason。
- [ ] 增加“文件任务命中文件工具”、“Git 任务命中 Git 工具”、“图片任务命中图片工具”的检索测试。

### G13. 插件和 MCP 工具生命周期需要补完整文档和测试

相关文件：

- `crates/jyowo-harness-tool/src/registry.rs`
- `crates/jyowo-harness-contracts/src/tool.rs`
- `crates/jyowo-harness-mcp/src/wrapper.rs`
- `crates/jyowo-harness-mcp/src/registry.rs`
- `crates/jyowo-harness-plugin/src/cargo_extension.rs`
- `crates/jyowo-harness-plugin/src/capability.rs`

当前已有：

- `register_from_plugin()`。
- `deregister_from_plugin()`。
- `deregister_mcp_tool()`。
- MCP canonical name 相关函数。
- shadowed registration 记录。
- shadow resolution：builtin wins；非 builtin 高 trust wins；同 trust 保留已有。
- MCP wrapper 保留上游 `input_schema` / `output_schema`。
- MCP wrapper 设置 `metadata.integration_source = Mcp`。
- plugin sidecar tool 使用 JSON Schema validate。
- plugin sidecar descriptor 设置 `metadata.integration_source = Plugin`。

欠缺点：

- 插件工具 trust level 与 capability 许可边界需要更完整测试。
- MCP 工具命名、注销、shadow 行为需要完整生命周期矩阵。
- plugin sidecar tool 当前没有 output schema。
- shadowed 记录是否需要暴露给 UI / telemetry 未明确。

处理方向：

- [x] 写插件工具注册生命周期文档。
- [x] 写 MCP 工具注册生命周期文档。
- [x] 补 builtin / plugin / MCP / skill / runtime appended 重名优先级快照。
- [x] 补 trust level 与 destructive 工具测试。
- [x] 补 capability not permitted 测试。
- [x] 明确 plugin manifest 是否需要支持 output schema。
- [x] 决定 shadowed 是否进入事件流。

### G14. UI 工具调用展示需要对齐后端契约

相关文件：

- `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.tsx`
- `apps/desktop/src/features/conversation/evidence/ToolInvocationCard.test.tsx`
- `apps/desktop/src/features/conversation/timeline/tool-group-segment-view.tsx`
- `apps/desktop/src/features/conversation/timeline/tool-attempt-row.tsx`
- `apps/desktop/src/features/conversation/timeline/pending-tool-permission.ts`

欠缺点：

- output schema 缺失会让 UI 只能按 ad-hoc 字段渲染。
- permission review 的资源如果不完整，UI 无法展示风险。
- brokered 工具 capability missing 需要单独状态。
- deferred tools delta 需要用户可理解的展示。

处理方向：

- [x] UI 渲染基于 output schema 和 ToolResult 类型。
- [x] pending permission 展示 resources、scope、severity、confirmation。
- [ ] capability missing 展示“宿主能力未注册”。
- [ ] brokered 工具展示真实 action，而不是只展示工具名。
- [x] deferred tools 展示新增/移除和原因。
- [x] 为每种 ToolResult 增加 UI 测试。

### G15. 测试矩阵需要从“有测试”升级为“按工具覆盖”

当前已有测试文件包括：

- `crates/jyowo-harness-tool/tests/api_contract.rs`
- `crates/jyowo-harness-tool/tests/builtin_diagnostics.rs`
- `crates/jyowo-harness-tool/tests/builtin_exec.rs`
- `crates/jyowo-harness-tool/tests/builtin_io.rs`
- `crates/jyowo-harness-tool/tests/builtin_process_monitor.rs`
- `crates/jyowo-harness-tool/tests/builtin_skills.rs`
- `crates/jyowo-harness-tool/tests/builtin_tools.rs`
- `crates/jyowo-harness-tool/tests/capabilities.rs`
- `crates/jyowo-harness-tool/tests/capability_policy.rs`
- `crates/jyowo-harness-tool/tests/contract.rs`
- `crates/jyowo-harness-tool/tests/execute_code.rs`
- `crates/jyowo-harness-tool/tests/memory_tool.rs`
- `crates/jyowo-harness-tool/tests/minimax_tools.rs`
- `crates/jyowo-harness-tool/tests/orchestrator.rs`
- `crates/jyowo-harness-tool/tests/permission_fingerprint.rs`
- `crates/jyowo-harness-tool/tests/registry.rs`
- `crates/jyowo-harness-tool/tests/registry_pool.rs`
- `crates/jyowo-harness-tool/tests/result_budget.rs`
- `crates/jyowo-harness-tool/tests/seedance_tools.rs`
- `crates/jyowo-harness-tool/tests/skill_script.rs`
- `crates/jyowo-harness-tool-search/tests/*.rs`
- `crates/jyowo-harness-contracts/tests/tool_contracts.rs`
- `crates/jyowo-harness-contracts/tests/tool_profile_contract.rs`
- `crates/jyowo-harness-contracts/tests/tool_search_mode.rs`
- `crates/jyowo-harness-contracts/tests/authorization_contracts.rs`
- `crates/jyowo-harness-contracts/tests/capability_testing.rs`
- `crates/jyowo-harness-contracts/tests/diagnostics_contract.rs`
- `crates/jyowo-harness-contracts/tests/process_monitor_contract.rs`
- `crates/jyowo-harness-contracts/tests/provider_capability_routes.rs`
- `crates/jyowo-harness-sdk/tests/tool_search.rs`
- `crates/jyowo-harness-sdk/tests/sdk_tool_search_flow.rs`
- `crates/jyowo-harness-sdk/tests/runtime_execution_status.rs`
- `crates/jyowo-harness-sdk/tests/runtime_assembly_tools.rs`
- `crates/jyowo-harness-sdk/tests/runtime_assembly_authorization.rs`
- `crates/jyowo-harness-sdk/tests/runtime_assembly_capability_routes.rs`
- `crates/jyowo-harness-sdk/tests/runtime_assembly_agents.rs`
- `crates/jyowo-harness-sdk/tests/runtime_assembly_agent_policy.rs`
- `crates/jyowo-harness-sdk/tests/runtime_assembly_agent_profiles.rs`
- `crates/jyowo-harness-sdk/tests/runtime_assembly_contract.rs`
- `crates/jyowo-harness-sdk/tests/agents_team.rs`
- `crates/jyowo-harness-sdk/tests/agents_team_facade.rs`
- `crates/jyowo-harness-sdk/tests/agents_team_support.rs`

已有部分覆盖：

- 默认工具注册关键名称。
- descriptor metadata 的部分覆盖。
- ticket mismatch / plan hash mismatch 防护。
- capability missing 的部分覆盖。
- result budget / offload 核心行为。
- `ReadBlob` capability、授权和窗口读取。
- `tool_search` output schema、scorer metadata、search hint。
- MiniMax / Seedance 的部分 provider、network、authorization 测试。
- ToolPool profile filter、dynamic schema assembly、runtime same-name 去重的部分覆盖。
- provider capability route filter 的部分覆盖。
- journal authority 的非 owner event 拒绝和默认 sandbox authority 的部分覆盖。

仍需补齐的测试维度：

- [ ] descriptor contract。
- [ ] input schema strictness。
- [ ] descriptor schema 与运行时 `validate()` 行为一致性。
- [ ] output schema contract。
- [ ] validation error。
- [ ] permission plan resources。
- [x] ToolPool 装配过滤链快照：capability missing、service route missing、tenant allowlist、profile filter、deferred partition。
- [x] journal authority 快照：默认工具集每个工具的 authority，runtime appended same-name 不覆盖 authority。
- [ ] 按工具覆盖 ticket mismatch / plan hash mismatch 防护。
- [ ] capability missing。
- [ ] successful execution。
- [ ] redaction。
- [ ] result budget/offload。
- [ ] interrupt/timeout。
- [x] UI rendering。

## 逐工具补全 checklist

每个工具至少完成以下共同项：

- [ ] descriptor name、display name、description、group 正确。
- [ ] `is_read_only`、`is_destructive`、`is_concurrency_safe` 正确。
- [ ] input schema 完整。
- [ ] input schema 是否允许未知字段有明确决定。
- [ ] validate 覆盖必填字段、类型错误、非法组合、未知字段。
- [ ] output schema 完整。
- [ ] plan 中 resources、workspace access、network access、execution channel 正确。
- [ ] permission review summary 和 details 可读。
- [ ] required capabilities 完整。
- [ ] capability missing 错误可读。
- [ ] execute_authorized 不绕过授权输入。
- [ ] redaction 覆盖命令、路径、URL、token、响应体。
- [ ] result budget 合理。
- [ ] long-running / timeout / interrupt 策略明确。
- [ ] unit test 覆盖 validate / plan / execute。
- [ ] contract test 覆盖 descriptor / schema。
- [ ] UI test 覆盖展示结果和错误。
- [ ] 文档说明用途、输入、输出、权限和限制。

### 文件和搜索工具

- [ ] `FileRead`
  - [x] 路径通过 workspace scope 和授权文件资源校验。
  - [ ] 输出 schema：content、path、size、truncated/offloaded、encoding。
  - [x] permission resource：`ActionResource::FileRead`。
  - [ ] 测试：正常读、缺文件、目录、越界路径、大文件 offload、redaction。

- [ ] `FileEdit`
  - [ ] 确认 patch / old-new 字符串语义。
  - [x] 输出 schema：path、changed、diff、old_hash/new_hash。
  - [x] permission resource：`ActionResource::FileWrite`。
  - [ ] 测试：唯一匹配、多重匹配、无匹配、越界路径、并发修改。

- [ ] `FileWrite`
  - [x] 当前实现使用 `std::fs::write`，语义是创建或覆盖。
  - [x] 输出 schema：path、bytes_written、content_hash。
  - [x] permission resource：`ActionResource::FileWrite`。
  - [ ] 测试：创建、覆盖、父目录不存在、越界路径、内容 hash。

- [ ] `ListDir`
  - [x] 输出 schema：entries、entry_type、size、modified。
  - [x] permission resource：`ActionResource::FileRead`。
  - [ ] 测试：空目录、大目录、隐藏文件、越界路径、排序。

- [ ] `Grep`
  - [ ] 明确 regex 语法、大小写、glob、limit。
  - [x] 输出 schema：matches、file、line、column、preview、truncated。
  - [x] permission resource：`ActionResource::FileRead`。
  - [ ] 测试：无匹配、多匹配、非法 regex、大输出 offload。

- [ ] `Glob`
  - [ ] 明确 glob 语法、忽略规则、排序。
  - [x] 输出 schema：paths、truncated。
  - [x] permission resource：`ActionResource::FileRead`。
  - [ ] 测试：递归、ignore、空结果、大结果。

- [x] `ReadBlob`
  - [ ] 明确 blob id、retention、权限。
  - [x] 输出 schema：blob_id、content、content_type、size。
  - [x] 已有 capability、授权、offset/limit 相关测试。
  - [ ] 测试：过期、二进制、offload 后端到端恢复读取。

### Git 工具

- [ ] `GitStatus`
  - [ ] 输出 schema：branch、ahead/behind、changed files、untracked。
  - [x] permission resource：`ActionResource::Command`，workspace access 为 read-only。
  - [ ] 测试：干净仓库、dirty、untracked、非 git 目录。

- [ ] `GitDiff`
  - [ ] 输出 schema：files、patch、stats、truncated/offloaded。
  - [x] permission resource：`ActionResource::Command`，workspace access 为 read-only。
  - [ ] 测试：staged、unstaged、path filter、大 diff。

- [ ] `GitShow`
  - [ ] 输出 schema：commit、files、patch/content。
  - [x] permission resource：`ActionResource::Command`，workspace access 为 read-only。
  - [ ] 测试：commit、path、非法 rev。

- [ ] `GitLog`
  - [ ] 输出 schema：commits、hash、author、date、subject。
  - [x] permission resource：`ActionResource::Command`，workspace access 为 read-only。
  - [ ] 测试：limit、path filter、空历史。

- [ ] `GitStage`
  - [ ] 明确禁止 stage secrets 的策略是否在工具层处理。
  - [ ] 输出 schema：staged_paths、status_after。
  - [x] permission resource：`ActionResource::Command`，workspace access 为 read-write。
  - [ ] 测试：单文件、多文件、不存在文件、越界路径。

- [ ] `GitCommit`
  - [ ] 明确 commit message 规则、hook 行为、失败回滚。
  - [ ] 输出 schema：commit_hash、summary、status_after。
  - [x] permission resource：`ActionResource::Command`，workspace access 为 read-write。
  - [ ] 测试：无 staged、hook fail、成功 commit、author 配置缺失。

- [ ] `GitBranch`
  - [ ] 明确 list/create/delete/switch 行为。
  - [ ] 输出 schema：branch、action、branches。
  - [x] permission resource：`ActionResource::Command`。
  - [ ] 按 branch action 区分 workspace access 和风险。
  - [ ] 测试：创建、切换、删除保护、dirty worktree。

- [ ] `GitPull`
  - [ ] network access 必须明确。
  - [ ] 输出 schema：summary、changed files、conflicts。
  - [x] 当前 permission resource：`ActionResource::Command`。
  - [ ] 确认是否需要显式 `ActionResource::Network`。
  - [ ] 测试：up-to-date、conflict、auth fail、network fail。

- [ ] `GitPush`
  - [ ] network access 必须明确。
  - [ ] 明确禁止 force push 的策略。
  - [ ] 输出 schema：remote、branch、pushed。
  - [x] 当前 permission resource：`ActionResource::Command`。
  - [ ] 确认是否需要显式 `ActionResource::Network`。
  - [ ] 测试：normal push、auth fail、rejected、force push blocked。

### 执行、诊断、进程工具

- [ ] `Bash`
  - [x] 当前 input schema：`command`、`cwd`。
  - [ ] 明确是否需要扩展 env、timeout。
  - [x] permission resource：`ActionResource::Command`。
  - [ ] sandbox policy 明确。
  - [x] output schema：exit_code、stdout、stderr、timed_out、truncated。
  - [ ] 测试：成功、非零退出、timeout、interrupt、redaction、cwd 越界。

- [ ] `Diagnostics`
  - [ ] runner capability 缺失错误可读。
  - [x] output schema：diagnostics、path、line、severity、source。
  - [x] permission resource：`ActionResource::Command`。
  - [ ] 测试：TypeScript、Cargo、空诊断、解析失败。

- [ ] `ProcessStart`
  - [x] 当前 input schema：`command`、`args`、`cwd`、`buffer_bytes`。
  - [ ] 明确前台/后台、terminal id、env、timeout。
  - [x] permission resource：`ActionResource::Command`。
  - [x] output schema：process_id、started、stdout_preview、stderr_preview。
  - [ ] 测试：成功启动、启动失败、重复启动、redaction。

- [x] `ProcessRead`
  - [x] 当前 input schema：`process_id`、`max_bytes`。
  - [x] permission resource：当前走 `generic_action_plan()`，需要确认是否补 process resource。
  - [x] output schema：stdout、stderr、running、exit_code、offset。
  - [ ] 测试：运行中、已退出、未知进程、大输出、redaction。

- [x] `ProcessStop`
  - [x] 当前 input schema：`process_id`。
  - [ ] 明确 graceful/kill 策略。
  - [x] permission resource：当前走 `generic_action_plan()`，需要确认是否补 process resource。
  - [x] output schema：stopped、signal、exit_code。
  - [ ] 测试：停止运行中进程、停止未知进程、重复停止。

- [ ] `ExecuteCode`
  - [x] 只在 `programmatic-tool-calling` 下注册。
  - [ ] sandbox code runtime 权限明确。
  - [x] output schema：result、stdout、stderr、artifacts、timeout。
  - [ ] 测试：成功、异常、timeout、资源限制、危险代码隔离。

### 网络工具

- [ ] `WebFetch`
  - [x] 执行路径通过 `ToolNetworkBrokerCap`。
  - [x] 当前 input schema：`url`、`max_bytes`。
  - [ ] 明确是否需要扩展 method、headers、body、timeout。
  - [x] output schema：status、headers、body、content_type、truncated、final_url。
  - [x] permission resource：`ActionResource::Network`。
  - [ ] 测试：http/https、redirect、blocked host、large body、unsupported content type。

- [ ] `WebSearch`
  - [x] 明确 `WebSearchBackend` 的网络和权限边界。
  - [x] 当前 input schema：`query`、`max_results`、`region`、`recency`。
  - [x] output schema：results、title、url、snippet、source。
  - [x] permission resource：`ActionResource::Network`，execution channel 为 `web_search_backend` external capability。
  - [ ] 测试：backend missing、empty result、redaction、limit。

### 对话、任务、记忆、技能工具

- [x] `Clarify`
  - [x] output schema：question、answers、selected。
  - [ ] permission channel：clarification authority。
  - [ ] 测试：capability missing、single answer、multi answer、cancel。

- [ ] `SendMessage`
  - [ ] 明确 parent/subagent message 行为。
  - [x] output schema：sent、target、message_id。
  - [ ] 测试：正常发送、目标不存在、interrupt。

- [x] `Todo`
  - [x] output schema：todos、changed、merge mode。
  - [ ] 测试：新增、更新、取消、非法状态、多个 in_progress。

- [ ] `Memory`
  - [ ] 明确 visibility、thread settings、敏感内容过滤。
  - [x] output schema：drafts、saved、visibility、thread。
  - [x] permission resource：memory mutation。
  - [ ] 测试：draft、save、reject、redaction、capability missing。

- [x] `TaskStop`
  - [ ] 明确 stop reason、scope、是否终止当前 run。
  - [x] output schema：stopped、reason。
  - [ ] 测试：正常 stop、重复 stop、无 reason。

- [ ] `SkillsList`
  - [x] output schema：skills、name、description、source。
  - [ ] 测试：无 skill、有 skill、过滤。

- [ ] `SkillsView`
  - [x] output schema：skill、content、metadata。
  - [ ] 测试：存在、不存在、路径越界。

- [ ] `SkillsInvoke`
  - [ ] 明确是否需要 dynamic schema。
  - [x] output schema：invocation、result、events。
  - [ ] 测试：成功、skill 不存在、参数错误、脚本失败。

### brokered platform 工具

这些工具共享 `BrokeredPlatformRuntimeCap`。

- [ ] `Worktree`
  - [ ] action enum：list/create/switch/delete 等。
  - [ ] 每个 action 的 required 字段。
  - [ ] 高风险 action 的 permission review。
  - [ ] host runtime contract test。

- [ ] `Session`
  - [ ] action enum：list/open/send/rename/delete 等。
  - [ ] 本地、worktree、cloud thread 的边界。
  - [ ] message 内容 redaction。
  - [ ] host runtime contract test。

- [ ] `Artifact`
  - [ ] action enum：create/update/read/export/delete。
  - [ ] artifact content schema。
  - [ ] output artifact 类型。
  - [ ] host runtime contract test。

- [ ] `BrowserUse`
  - [ ] action enum：navigate/click/type/screenshot/evaluate 等。
  - [ ] network 和外部交互提示。
  - [ ] sensitive input redaction。
  - [ ] host runtime contract test。

- [ ] `ComputerUse`
  - [ ] action enum：click/type/key/screenshot 等。
  - [ ] 坐标、目标、文本字段严格校验。
  - [ ] 高风险确认。
  - [ ] host runtime contract test。

- [ ] `ImageGeneration`
  - [ ] 区分 generate/edit/variation。
  - [ ] image input 类型和 size enum。
  - [ ] output schema：image artifact/blob/url。
  - [ ] host runtime contract test。

- [ ] `NotebookEdit`
  - [ ] 将当前自由字符串 `operation` 收敛为 enum：read/update/insert/delete/run。
  - [ ] cell id / index / source 约束。
  - [ ] notebook 文件 resource。
  - [ ] host runtime contract test。

- [ ] `LSP`
  - [ ] action enum：diagnostics/symbols/definition/references/hover。
  - [ ] output schema 按 action 区分。
  - [ ] read-only 标记确认。
  - [ ] host runtime contract test。

- [ ] `Automation`
  - [ ] action enum：create/update/list/delete/run。
  - [ ] schedule schema。
  - [ ] prompt redaction。
  - [ ] host runtime contract test。

- [ ] `Workflow`
  - [ ] workflow discovery。
  - [ ] dynamic params schema。
  - [ ] output schema：workflow result/events。
  - [ ] host runtime contract test。

### MiniMax 工具

共享要求：

- [ ] 所有网络请求通过 `ToolNetworkBrokerCap`。
- [ ] descriptor 的 `required_capabilities` 当前是 `ProviderCredentialResolver`；media / artifact 输出相关工具还会声明 `BlobWriter`。`ToolNetworkBrokerCap` 是执行路径依赖，不在 descriptor capability 过滤中表达。
- [ ] service-bound 工具还受 provider capability routes 过滤；需要测试 route enabled / missing 时 prompt 工具集变化。
- [x] API key 和 token 不进入错误消息。
- [ ] input schema 对模型、尺寸、时长、format、voice、file_id 严格约束。
- [x] output schema 区分 text、image、video、audio、file、model、tokens。
- [x] provider media 下载走安全 content-type 校验。
- [x] async query 工具明确轮询和任务状态。
- [ ] 所有工具有 capability missing 测试。
- [ ] 所有工具有 provider error redaction 测试。

逐工具：

- [ ] `MiniMaxTextToImageTool`
- [ ] `MiniMaxImageToImageTool`
- [ ] `MiniMaxTextToVideoTool`
- [ ] `MiniMaxImageToVideoTool`
- [ ] `MiniMaxFirstLastFrameToVideoTool`
- [ ] `MiniMaxSubjectReferenceVideoTool`
- [ ] `MiniMaxVideoGenerationQueryTool`
- [ ] `MiniMaxVideoTemplateTool`
- [ ] `MiniMaxVideoTemplateQueryTool`
- [ ] `MiniMaxTextToSpeechTool`
- [ ] `MiniMaxTextToSpeechAsyncTool`
- [ ] `MiniMaxTextToSpeechAsyncQueryTool`
- [ ] `MiniMaxVoiceCloneTool`
- [ ] `MiniMaxVoiceDesignTool`
- [ ] `MiniMaxListVoicesTool`
- [ ] `MiniMaxDeleteVoiceTool`
- [ ] `MiniMaxLyricsGenerationTool`
- [ ] `MiniMaxMusicGenerationTool`
- [ ] `MiniMaxMusicCoverPreprocessTool`
- [ ] `MiniMaxFileUploadTool`
- [ ] `MiniMaxFileListTool`
- [ ] `MiniMaxFileRetrieveTool`
- [ ] `MiniMaxFileDeleteTool`
- [ ] `MiniMaxModelsListTool`
- [ ] `MiniMaxModelRetrieveTool`
- [ ] `MiniMaxResponsesTool`
- [ ] `MiniMaxResponsesInputTokensTool`
- [ ] `MiniMaxAnthropicMessagesTool`
- [ ] `MiniMaxAnthropicCountTokensTool`
- [ ] `MiniMaxAnthropicModelsListTool`
- [ ] `MiniMaxAnthropicModelRetrieveTool`

### Seedance 工具

共享要求：

- [ ] 所有网络请求通过 `ToolNetworkBrokerCap`。
- [ ] descriptor 的 `required_capabilities` 当前是 `ProviderCredentialResolver`；query 工具还会声明 `BlobWriter`。`ToolNetworkBrokerCap` 是执行路径依赖，不在 descriptor capability 过滤中表达。
- [ ] service-bound 工具还受 provider capability routes 过滤；需要测试 route enabled / missing 时 prompt 工具集变化。
- [x] data URL 只接受安全 mime 和 base64。
- [x] output schema 区分 task id、status、video artifact/blob/url。
- [x] async query 状态机明确。
- [ ] provider error redaction 测试。

逐工具：

- [ ] `SeedanceTextToVideo`
- [ ] `SeedanceImageToVideo`
- [ ] `SeedanceVideoGenerationQueryTool`

### runtime 注入工具

- [x] `tool_search`
  - [x] output schema 已存在。
  - [x] result budget 使用 `BudgetMetric::Bytes`。
  - [x] input schema 仍需决定是否拒绝未知字段。
  - [x] 搜索结果解释命中字段和 materialization reason。

- [x] `background_agent`
  - [x] input schema 已设置 `additionalProperties: false`。
  - [x] output schema：thread/session id、status、title、error。
  - [x] capability readiness：`jyowo.background_agent.starter`。
  - [x] permission mode、session snapshot、model config 行为文档。
  - [x] 测试：capability missing、启动成功、策略拒绝、输出契约。

- [ ] `agent_team`
  - [x] input schema 已设置 `additionalProperties: false`。
  - [x] output schema：team id、members、status、message count、errors。
  - [x] capability readiness：`jyowo.agent_team.runner`。
  - [ ] topology、max turns、停止语义文档。
  - [ ] 测试：capability missing、启动成功、重复启动、stop/report。

- [x] `agent`
  - [x] output schema：subagent id、status、summary、transcript ref、usage。
  - [x] capability readiness：`ToolCapability::SubagentRunner`。
  - [x] 明确 parent/subagent 权限转发和取消语义。
  - [ ] input schema 是否需要 `additionalProperties: false`。

- [x] team control tools：`dispatch`、`message`、`pause_worker`、`resume_worker`、`spawn_worker`、`stop_team`、`team_status`
  - [ ] 当前 input schema 只有 `{ "type": "object" }`，需要按工具拆字段。
  - [ ] output schema 按工具声明。
  - [ ] plan 当前无 resources，需确认 team control resource 是否建模。
  - [x] 测试：非法 agent id、目标不存在、重复 pause/resume、stop 后操作、status。

### MCP / plugin 生成工具

- [ ] MCP wrapper
  - [x] 保留上游 `input_schema` 和 `output_schema`。
  - [x] descriptor 标记 `metadata.integration_source = Mcp`。
  - [x] authorization 已有 MCP resource 建模。
  - [x] 命名、注销、shadow、cancel/interrupt ack 的生命周期矩阵测试。

- [ ] plugin sidecar tool
  - [x] 使用 JSON Schema validate input。
  - [x] descriptor 标记 `metadata.integration_source = Plugin`。
  - [x] 明确 manifest 是否支持 output schema。
  - [x] trust level、destructive、capability 许可边界测试。

## 分阶段执行计划

### Phase 0：建立基线，防止 AI 漏工具

- [x] 已有默认工具集关键名称注册测试。
- [x] 新增完整工具清单快照：默认工具集名称和数量。
- [x] 新增 feature-gated 工具清单测试：`programmatic-tool-calling`、`minimax-tools`、`seedance-tools`。
- [ ] 新增 descriptor 完整性测试：所有工具必须有 name、group、budget、risk metadata。
- [ ] 新增 capability 映射测试：required capabilities 与 execute 中实际 capability 调用一致。
- [x] 新增文档检查：本文中的 builder 工具名必须与注册列表一致。
- [x] 新增运行时工具清单检查：SDK、MCP、plugin、subagent、team runtime 注入工具不能漏。
- [x] 新增 ToolPool session assembly 快照：capability、service route、tenant policy、profile、tool search partition。

### Phase 1：契约和 schema

- [x] 严格化 input schema。
- [x] 明确 descriptor schema 与 `Tool::validate()` 的职责边界。
- [x] 补 output schema。
- [ ] 补 metadata 和 search_hint。
- [x] 更新 `ToolDescriptor` contract 测试。
- [x] 增加 schema descriptor 与 validate 行为一致性测试。
- [x] 更新 UI 渲染类型。

### Phase 2：权限和资源建模

- [x] 文件、命令、网络类基础资源已有部分 `ActionResource` 建模。
- [ ] 审计所有 `generic_action_plan()` 使用者。
- [ ] 扩展 `ActionResource` contract，覆盖 Memory / Process / TeamControl 等当前缺失资源。
- [ ] 为 brokered action、memory、process read/stop、runtime 注入工具补或确认 `ActionResource`。
- [x] 固化默认工具集 journal authority 快照。
- [x] 补 permission review details。
- [x] 补 persisted decision scope 测试。
- [x] 已有 ticket mismatch 和 hash mismatch 测试。
- [ ] 按工具补 permission review snapshot test。

### Phase 3：执行可靠性

- [x] 补 long-running policy。
- [ ] 补 timeout。
- [ ] 补 interrupt。
- [ ] 补 result budget。
- [x] 补 offload / blob 恢复测试。
- [x] 补 capability missing UX。

### Phase 4：brokered platform 完整化

- [ ] 拆 action / operation enum。
- [ ] 拆每个 action / operation 的 schema。
- [ ] 定义 host runtime contract。
- [ ] 补 host-side mock tests。
- [ ] UI 展示 brokered action 和 risk。

### Phase 5：provider 工具完整化

- [ ] MiniMax schema / output / network / redaction 全覆盖。
- [ ] Seedance schema / output / network / redaction 全覆盖。
- [x] provider media 类型安全测试。
- [x] async query 状态测试。

### Phase 6：tool search 和 deferred tools

- [x] scorer 已覆盖 metadata、group filter、search hint、description、risk/source 等信号。
- [ ] 所有工具 metadata 可用于检索。
- [x] required capabilities 是否进入 scorer 规则需定案。
- [x] scorer 加权规则继续补端到端检索测试。
- [x] search backend 错误降级。
- [x] deferred delta 增加 reason。
- [x] UI 展示 deferred tools 变化。

## 开发时的固定检查命令

按改动范围选择最小命令。

Rust 工具模块：

```sh
cargo test -p jyowo-harness-tool
```

工具搜索模块：

```sh
cargo test -p jyowo-harness-tool-search
```

contracts：

```sh
cargo test -p jyowo-harness-contracts
```

SDK tool pool：

```sh
cargo test -p jyowo-harness-sdk tool
```

前端工具展示：

```sh
pnpm check
```

网络 broker 边界：

```sh
node scripts/check-tool-network-broker-boundary.mjs
node scripts/check-tool-network-broker-boundary.test.mjs
```

全量检查：

```sh
pnpm check
```

## 完成定义

工具模块不能只以“能执行”为完成标准。每个工具完成时必须满足：

- [ ] 工具出现在正确 feature 的 registry 中。
- [ ] descriptor 完整。
- [ ] input schema 严格且有测试。
- [ ] output schema 存在且有测试。
- [ ] validate 覆盖错误输入。
- [ ] plan 建模真实资源、workspace access、network access、execution channel。
- [ ] permission review 可读。
- [ ] capability 缺失时错误明确。
- [ ] execute 只使用 `AuthorizedToolInput`。
- [ ] 网络工具不绕过 broker 或有明确例外文档。
- [ ] 高风险工具有确认策略。
- [ ] 长任务有 timeout / heartbeat / interrupt。
- [ ] 大输出有 budget / offload。
- [ ] 敏感内容被 redaction。
- [x] UI 能正确展示 pending、running、success、failure、offload。
- [ ] 单元测试、contract test、集成测试按工具覆盖。
- [ ] 文档更新。

## 当前最高优先级 checklist

- [ ] 补 `BuiltinToolset::Default` 在无 `builtin-toolset` feature 时的编译/行为测试。
- [x] 给基础工具补 output schema。
- [x] 明确 descriptor schema 与 `Tool::validate()` 的校验职责，并补未知字段策略测试。
- [ ] 审计 `generic_action_plan()` 使用者和 runtime 注入工具的 `ActionResource`。
- [ ] 扩展 `ActionResource`，覆盖 Memory / Process / TeamControl 等当前无枚举变体的资源。
- [x] 建立 ToolPool session assembly 过滤链快照测试。
- [x] 建立默认工具集 journal authority 快照测试。
- [ ] 给 brokered platform 工具拆 action / operation schema。
- [x] 补 provider service binding / route gating 文档和测试。
- [x] 明确 `WebSearchBackend` 的网络边界。
- [ ] 给缺失的基础工具补 metadata / search_hint。
- [x] 建立 builder 工具清单和 runtime 注入工具清单快照测试。
